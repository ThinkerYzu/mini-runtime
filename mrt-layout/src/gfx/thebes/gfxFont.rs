// Gecko: gfx/thebes/gfxFont.h, gfx/thebes/gfxFont.cpp (REDUCED), gfx/thebes/gfxHarfBuzzShaper.cpp
// (SetGlyphsFromRun only), gfx/thebes/gfxRect.h / gfx/2d/Point.h (the few members used).
//
// What this file has: what gfxTextRun needs from gfxFont.h -- ShapedTextFlags, gfxShapedText with
// CompressedGlyph / DetailedGlyph (the per-character glyph records), gfxTextRunFactory::Parameters,
// gfxFont::Spacing / RunMetrics / BoundingBoxType, FontMatchType -- and, as the trait `gfxFont_Ext`,
// the members of gfxFont.cpp that fill or measure a text run (Measure, ShapeText, SplitAndInitTextRun,
// SetGlyphsFromRun).
//
// `gfxFont` itself (the face at a size: metrics, the HarfBuzz font) is mrt-text's translation of
// gfx/thebes/gfxFont.{h,cpp} and lives in another crate. A Rust crate cannot add inherent methods to a
// type from another crate, so the members that need gfxTextRun are the trait `gfxFont_Ext`, implemented
// for mrt_text::gfxFont; `font.Measure(..)` reads as in C++ because lib.rs re-exports the trait.
//
// The shaped-word cache (gfxShapedWord, ProcessShapedWordInternal, CopyGlyphDataFrom) is ported since
// stage 4 (a wake shapes the same words again); its expiration timer is not (see gfxShapedWord below).
// NOT PORTED: refcount logging; Graphite and the platform shapers; glyph extents (gfxGlyphExtents) and tight ink bounding boxes (every
// bounding box is the loose one: the font box); synthetic bold and oblique; tracking tables; vertical
// text (the orientation parameters stay, only horizontal metrics exist); 8-bit text storage (an 8-bit
// string is widened and TEXT_IS_8BIT is only recorded); gfxFontMissingGlyphs (a missing glyph is as wide
// as the font's average character); font features and variations; the rounding flags (mrt-text shapes
// with unrounded advances, Firefox's default on Linux).
use crate::*;
use icu_segmenter::GraphemeClusterSegmenter;
use std::cell::Cell;
use std::ops::{Add, BitOr};
use std::rc::Rc;

pub use mrt_text::{gfxFont, gfxFontEntry, gfxFontStyle, gfxFont_Metrics, ShapedClump, ShapedRun};

// Gecko: gfx/thebes/gfxTypes.h gfxFloat
pub type gfxFloat = f64;

// Gecko: gfx/2d/Point.h mozilla::gfx::Point (REDUCED to the members the glyph records need)
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

// Gecko: gfx/thebes/gfxPoint.h gfxPoint (REDUCED)
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct gfxPoint {
    pub x: f64,
    pub y: f64,
}

impl gfxPoint {
    pub fn new(aX: f64, aY: f64) -> gfxPoint {
        gfxPoint { x: aX, y: aY }
    }
}

// Gecko: gfx/thebes/gfxRect.h gfxRect (REDUCED: the members gfxFont::Measure and gfxTextRun use;
// BaseRect<double, ...> semantics)
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct gfxRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl gfxRect {
    pub fn new(aX: f64, aY: f64, aWidth: f64, aHeight: f64) -> gfxRect {
        gfxRect { x: aX, y: aY, width: aWidth, height: aHeight }
    }
    pub fn X(&self) -> f64 {
        self.x
    }
    pub fn Y(&self) -> f64 {
        self.y
    }
    pub fn Width(&self) -> f64 {
        self.width
    }
    pub fn Height(&self) -> f64 {
        self.height
    }
    pub fn XMost(&self) -> f64 {
        self.x + self.width
    }
    pub fn YMost(&self) -> f64 {
        self.y + self.height
    }
    pub fn IsEmpty(&self) -> bool {
        self.height <= 0.0 || self.width <= 0.0
    }
    pub fn SetEmpty(&mut self) {
        self.width = 0.0;
        self.height = 0.0;
    }
    pub fn SetWidth(&mut self, aWidth: f64) {
        self.width = aWidth;
    }
    pub fn MoveByX(&mut self, aDx: f64) {
        self.x += aDx;
    }
    pub fn MoveByY(&mut self, aDy: f64) {
        self.y += aDy;
    }
    pub fn MoveToX(&mut self, aX: f64) {
        self.x = aX;
    }
    pub fn SetBoxX(&mut self, aX: f64, aXMost: f64) {
        self.x = aX;
        self.width = aXMost - aX;
    }
    // Gecko: BaseRect::Union
    pub fn Union(&self, aRect: &gfxRect) -> gfxRect {
        if self.IsEmpty() {
            *aRect
        } else if aRect.IsEmpty() {
            *self
        } else {
            self.UnionEdges(aRect)
        }
    }
    // Gecko: BaseRect::UnionEdges
    pub fn UnionEdges(&self, aRect: &gfxRect) -> gfxRect {
        let x = self.x.min(aRect.x);
        let y = self.y.min(aRect.y);
        gfxRect {
            x,
            y,
            width: self.XMost().max(aRect.XMost()) - x,
            height: self.YMost().max(aRect.YMost()) - y,
        }
    }
}

impl Add<gfxPoint> for gfxRect {
    type Output = gfxRect;
    fn add(self, aPoint: gfxPoint) -> gfxRect {
        gfxRect { x: self.x + aPoint.x, y: self.y + aPoint.y, width: self.width, height: self.height }
    }
}

// Gecko: gfx/2d/2D.h mozilla::gfx::DrawTarget
// NOT PORTED: drawing. The parameter stays where Gecko's signatures have it; nothing reads it.
#[derive(Clone, Copy, Debug, Default)]
pub struct DrawTarget;

// Gecko: gfx/thebes/gfxMissingFontRecorder (gfxTextRun.h) gfxMissingFontRecorder
// NOT PORTED: reporting scripts that have no font (telemetry).
#[derive(Default)]
pub struct gfxMissingFontRecorder;

// Gecko: gfx/thebes/gfxTypes.h FontMatchType::Kind
MOZ_ENUM_CLASS_BITS! {
    pub struct FontMatchType_Kind: u8 {
        kUnspecified = 0,
        kFontGroup = 1 << 0,
        kPrefsFallback = 1 << 1,
        kSystemFallback = 1 << 2,
    }
}

// ADAPTER (generated by cbindgen from Stylo in Gecko, no header): mozilla::StyleGenericFontFamily
pub type StyleGenericFontFamily = mrt_style::style::values::computed::font::GenericFontFamily;

// Gecko: gfx/thebes/gfxTypes.h FontMatchType
#[derive(Clone, Copy, Debug)]
pub struct FontMatchType {
    pub kind: FontMatchType_Kind,
    pub generic: StyleGenericFontFamily,
}

impl FontMatchType {
    pub fn new(aKind: FontMatchType_Kind) -> FontMatchType {
        FontMatchType { kind: aKind, generic: StyleGenericFontFamily::None }
    }
}

// Flags that live in the gfxShapedText::mFlags field.
// (Note that gfxTextRun has an additional mFlags2 field for use
// by textrun clients like nsTextFrame.)
//
// If you add a flag, please add support for it in gfxTextRun::Dump.
// Gecko: gfx/thebes/gfxFont.h mozilla::gfx::ShapedTextFlags
MOZ_ENUM_CLASS_BITS! {
    pub struct ShapedTextFlags: u16 {
        /*
         * When set, the text is RTL.
         */
        TEXT_IS_RTL = 0x0001,
        /*
         * When set, spacing is enabled and the textrun needs to call GetSpacing
         * on the spacing provider.
         */
        TEXT_ENABLE_SPACING = 0x0002,
        /*
         * When set, the text has no characters above 255 and it is stored
         * in the textrun in 8-bit format.
         */
        TEXT_IS_8BIT = 0x0004,
        /*
         * When set, GetHyphenationBreaks may return true for some character
         * positions, otherwise it will always return false for all characters.
         */
        TEXT_ENABLE_HYPHEN_BREAKS = 0x0008,
        /*
         * When set, the RunMetrics::mBoundingBox field will be initialized
         * properly based on glyph extents, in particular, glyph extents that
         * overflow the standard font-box (the box defined by the ascent, descent
         * and advance width of the glyph). When not set, it may just be the
         * standard font-box even if glyphs overflow.
         */
        TEXT_NEED_BOUNDING_BOX = 0x0010,
        /*
         * When set, optional ligatures are disabled. Ligatures that are
         * required for legible text should still be enabled.
         */
        TEXT_DISABLE_OPTIONAL_LIGATURES = 0x0020,
        /*
         * When set, the textrun should favour speed of construction over
         * quality. This may involve disabling ligatures and/or kerning or
         * other effects.
         */
        TEXT_OPTIMIZE_SPEED = 0x0040,
        /*
         * When set, the textrun should discard control characters instead of
         * turning them into hexboxes.
         */
        TEXT_HIDE_CONTROL_CHARACTERS = 0x0080,

        /*
         * nsTextFrameThebes sets these, but they're defined here rather than
         * in nsTextFrameUtils.h because ShapedWord creation/caching also needs
         * to check the _INCOMING flag
         */
        TEXT_TRAILING_ARABICCHAR = 0x0100,
        /*
         * When set, the previous character for this textrun was an Arabic
         * character.  This is used for the context detection necessary for
         * bidi.numeral implementation.
         */
        TEXT_INCOMING_ARABICCHAR = 0x0200,

        /*
         * Set if the textrun should use the OpenType 'math' script.
         */
        TEXT_USE_MATH_SCRIPT = 0x0400,

        /*
         * Bit 0x0800 is currently unused.
         */

        /*
         * Field for orientation of the textrun and glyphs within it.
         * Possible values of the TEXT_ORIENT_MASK field:
         *   TEXT_ORIENT_HORIZONTAL
         *   TEXT_ORIENT_VERTICAL_UPRIGHT
         *   TEXT_ORIENT_VERTICAL_SIDEWAYS_RIGHT
         *   TEXT_ORIENT_VERTICAL_SIDEWAYS_LEFT
         *   TEXT_ORIENT_VERTICAL_MIXED
         * For all VERTICAL settings, the x and y coordinates of glyph
         * positions are exchanged, so that simple advances are vertical.
         *
         * The MIXED value indicates vertical textRuns for which the CSS
         * text-orientation property is 'mixed', but is never used for
         * individual glyphRuns; it will be resolved to either UPRIGHT
         * or SIDEWAYS_RIGHT according to the UTR50 properties of the
         * characters, and separate glyphRuns created for the resulting
         * glyph orientations.
         */
        TEXT_ORIENT_MASK = 0x7000,
        TEXT_ORIENT_HORIZONTAL = 0x0000,
        TEXT_ORIENT_VERTICAL_UPRIGHT = 0x1000,
        TEXT_ORIENT_VERTICAL_SIDEWAYS_RIGHT = 0x2000,
        TEXT_ORIENT_VERTICAL_MIXED = 0x3000,
        TEXT_ORIENT_VERTICAL_SIDEWAYS_LEFT = 0x4000,
    }
}

// Gecko: gfx/thebes/gfxFont.h gfxFontShaper::RoundingFlags
MOZ_ENUM_CLASS_BITS! {
    pub struct gfxFontShaper_RoundingFlags: u8 {
        kRoundX = 0x01,
        kRoundY = 0x02,
    }
}

// Gecko: gfx/thebes/gfxFont.h gfxFont::BoundingBoxType
// NOT PORTED: tight bounding boxes (glyph extents); every type measures the loose box.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum gfxFont_BoundingBoxType {
    // Includes glyph ink extents; the box is extended to the font box.
    LOOSE_INK_EXTENTS,
    // Tight bounds around the ink.
    TIGHT_INK_EXTENTS,
    // Hinted outline extents.
    TIGHT_HINTED_OUTLINE_EXTENTS,
}

/**
 * We let layout specify spacing on either side of any
 * character. We need to specify both before and after
 * spacing so that substring measurement can do the right things.
 * These values are in appunits.
 */
// Gecko: gfx/thebes/gfxFont.h gfxFont::Spacing
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct gfxFont_Spacing {
    pub mBefore: nscoord,
    pub mAfter: nscoord,
}

/**
 * Metrics for a particular string
 */
// Gecko: gfx/thebes/gfxFont.h gfxFont::RunMetrics
#[derive(Clone, Copy, Debug, Default)]
pub struct gfxFont_RunMetrics {
    // can be negative (partly due to negative spacing).
    // Advance widths should be additive: the advance width of the
    // (offset1, length1) plus the advance width of (offset1 + length1,
    // length2) should be the advance width of (offset1, length1 + length2)
    pub mAdvanceWidth: gfxFloat,

    // For zero-width substrings, these must be zero!
    pub mAscent: gfxFloat,  // always non-negative
    pub mDescent: gfxFloat, // always non-negative

    // Bounding box that is guaranteed to include everything drawn.
    // If a tight boundingBox was requested when these metrics were
    // generated, this will tightly wrap the glyphs, otherwise it is
    // "loose" and may be larger than the true bounding box.
    // Coordinates are relative to the baseline left origin, so typically
    // mBoundingBox.y == -mAscent
    pub mBoundingBox: gfxRect,
}

impl gfxFont_RunMetrics {
    // Gecko: gfx/thebes/gfxFont.h gfxFont::RunMetrics::RunMetrics
    pub fn new() -> gfxFont_RunMetrics {
        gfxFont_RunMetrics { mAdvanceWidth: 0.0, mAscent: 0.0, mDescent: 0.0, mBoundingBox: gfxRect::default() }
    }

    // Gecko: gfx/thebes/gfxFont.cpp gfxFont::RunMetrics::CombineWith
    pub fn CombineWith(&mut self, aOther: &gfxFont_RunMetrics, aOtherIsOnLeft: bool) {
        self.mAscent = self.mAscent.max(aOther.mAscent);
        self.mDescent = self.mDescent.max(aOther.mDescent);
        if aOtherIsOnLeft {
            self.mBoundingBox = (self.mBoundingBox + gfxPoint::new(aOther.mAdvanceWidth, 0.0)).Union(&aOther.mBoundingBox);
        } else {
            self.mBoundingBox = self.mBoundingBox.Union(&(aOther.mBoundingBox + gfxPoint::new(self.mAdvanceWidth, 0.0)));
        }
        self.mAdvanceWidth += aOther.mAdvanceWidth;
    }
}

// Gecko: gfx/thebes/gfxFont.h gfxTextRunFactory::Parameters
/**
 * This record contains all the parameters needed to initialize a textrun.
 */
pub struct gfxTextRunFactory_Parameters<'a> {
    // Shape text params suggesting where the textrun will be rendered
    pub mDrawTarget: Option<&'a DrawTarget>,
    // ADAPTER: Gecko's `void* mUserData` (pointer to arbitrary user data) is not here: the text run
    // keeps its user data in an enum field (gfxTextRun_UserData) that nsTextFrame sets after
    // creating the run, as it does in Gecko (the parameter is always nullptr there).
    // A description of which characters have been stripped from the original
    // DOM string to produce the characters in the textrun. May be null
    // if that information is not relevant.
    pub mSkipChars: Option<&'a mut gfxSkipChars>,
    // A list of where linebreaks are currently placed in the textrun. May
    // be null if mInitialBreakCount is zero.
    pub mInitialBreaks: Option<&'a [u32]>,
    pub mInitialBreakCount: u32,
    // The ratio to use to convert device pixels to application layout units
    pub mAppUnitsPerDevUnit: i32,
}

// Gecko: gfx/thebes/gfxFont.h gfxTextRunFactory (the class; the members are Parameters above)
pub struct gfxTextRunFactory;

// ---------------------------------------------------------------------------------------------
// Unicode helpers the glyph records need (nsContentUtils.cpp, nsUnicodeProperties.h)
// ---------------------------------------------------------------------------------------------

// Gecko: dom/base/nsContentUtils.cpp nsContentUtils::IsAlphanumeric
fn nsContentUtils_IsAlphanumeric(aChar: u32) -> bool {
    use GeneralCategory::*;
    matches!(
        UnicodeProperties::CharType(aChar),
        Uppercase_Letter
            | Lowercase_Letter
            | Titlecase_Letter
            | Modifier_Letter
            | Other_Letter
            | Decimal_Number
            | Letter_Number
            | Other_Number
    )
}

// Gecko: dom/base/nsContentUtils.cpp nsContentUtils::IsHyphen
fn nsContentUtils_IsHyphen(aChar: u32) -> bool {
    // Characters treated as hyphens for the purpose of "emergency" breaking
    // when the content would otherwise overflow.
    aChar == '-' as u32 ||  // HYPHEN-MINUS
    aChar == 0x2010 ||      // HYPHEN
    aChar == 0x2012 ||      // FIGURE DASH
    aChar == 0x2013 ||      // EN DASH
    aChar == 0x058A // ARMENIAN HYPHEN
}

// Gecko: intl/unicharutil/util/nsUnicodeProperties.h mozilla::unicode::IsClusterExtender
pub fn IsClusterExtender(aCh: u32) -> bool {
    IsClusterExtenderExcludingJoiners__uint32_t(aCh) || aCh == 0x200C || aCh == 0x200D
}

// Gecko: gfx/thebes/gfxFont.cpp IsIgnorable
fn IsIgnorable(aChar: u32) -> bool {
    const ZWNJ: u32 = 0x200C;
    const ZWJ: u32 = 0x200D;
    IsDefaultIgnorable(aChar) || aChar == ZWNJ || aChar == ZWJ
}

// ---------------------------------------------------------------------------------------------
// gfxShapedText
// ---------------------------------------------------------------------------------------------

/**
 * This class records the information associated with a character in the
 * input string. It's optimized for the case where there is one glyph
 * representing that character alone.
 *
 * A character can have zero or more associated glyphs. Each glyph
 * has an advance width and an x and y offset.
 * A character may be the start of a cluster.
 * A character may be the start of a ligature group.
 * A character can be "missing", indicating that the system is unable
 * to render the character.
 *
 * All characters in a ligature group conceptually share all the glyphs
 * associated with the characters in a group.
 */
// Gecko: gfx/thebes/gfxFont.h gfxShapedText::CompressedGlyph
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct gfxShapedText_CompressedGlyph {
    mValue: u32,
}

// Gecko: gfx/thebes/gfxFont.h gfxShapedText::CompressedGlyph::Flags
// (the C++ enum is uint32_t; the FLAG_BREAK_TYPE_* values are u8 here, as in the
// gfxTextRun_CompressedGlyph module of nsLineBreaker.rs, because CanBreakBefore() is a uint8_t)
impl gfxShapedText_CompressedGlyph {
    // Indicates that a cluster and ligature group starts at this
    // character; this character has a single glyph with a reasonable
    // advance and zero offsets. A "reasonable" advance
    // is one that fits in the available bits (currently 12) (specified
    // in appunits).
    pub const FLAG_IS_SIMPLE_GLYPH: u32 = 0x80000000;

    // These flags are applicable to both "simple" and "complex" records.
    pub const COMMON_FLAGS_MASK: u32 = 0x70000000;

    // Indicates whether a linebreak is allowed before this character;
    // this is a two-bit field that holds a FLAG_BREAK_TYPE_xxx value
    // indicating the kind of linebreak (if any) allowed here.
    pub const FLAGS_CAN_BREAK_BEFORE: u32 = 0x60000000;

    pub const FLAGS_CAN_BREAK_SHIFT: u32 = 29;
    pub const FLAG_BREAK_TYPE_NONE: u8 = 0;
    pub const FLAG_BREAK_TYPE_NORMAL: u8 = 1;
    pub const FLAG_BREAK_TYPE_HYPHEN: u8 = 2;
    // Allow break before this position if needed to avoid overflow:
    pub const FLAG_BREAK_TYPE_EMERGENCY_WRAP: u8 = 3;

    pub const FLAG_CHAR_IS_SPACE: u32 = 0x10000000;

    // Fields present only when FLAG_IS_SIMPLE_GLYPH is /true/.
    // The advance is stored in appunits as a 12-bit field:
    pub const ADVANCE_MASK: u32 = 0x0FFF0000;
    pub const ADVANCE_SHIFT: u32 = 16;
    // and the glyph ID is stored in the low 16 bits.
    pub const GLYPH_MASK: u32 = 0x0000FFFF;

    // Fields present only when FLAG_IS_SIMPLE_GLYPH is /false/.
    // Non-simple glyphs may or may not have glyph data in the
    // corresponding mDetailedGlyphs entry. They have a glyph count
    // stored in the low 16 bits, and the following flag bits:
    pub const GLYPH_COUNT_MASK: u32 = 0x0000FFFF;

    // When NOT set, indicates that this character corresponds to a
    // missing glyph and should be skipped (or possibly, render the character
    // Unicode value in some special way). If there are glyphs,
    // the mGlyphID is actually the UTF16 character code. The bit is
    // inverted so we can memset the array to zero to indicate all missing.
    pub const FLAG_NOT_MISSING: u32 = 0x010000;
    pub const FLAG_NOT_CLUSTER_START: u32 = 0x020000;
    pub const FLAG_NOT_LIGATURE_GROUP_START: u32 = 0x040000;

    // This is a "merged glyph" representing multiple characters that result
    // from text-run transformation; if there is letter-spacing, it should be
    // applied between the detailed-glyph components.
    // Currently the only use of this is when es-zet is uppercased as SS.
    //
    // Note that in general, letter-spacing does not depend on this flag; it
    // is applied *between* CompressedGlyph records by textrun drawing and
    // measuring code. This flag only refers to letter-spacing between the
    // individual DetailedGlyph components of a single CompressedGlyph.
    pub const FLAG_APPLY_LETTER_SPACING_BETWEEN_DETAILED_GLYPHS: u32 = 0x080000;

    // Certain types of characters are marked so that they can be given
    // special treatment in rendering. This may require use of a "complex"
    // CompressedGlyph record even for a character that would otherwise be
    // treated as "simple".
    pub const CHAR_TYPE_FLAGS_MASK: u32 = 0xF00000;
    pub const FLAG_CHAR_IS_TAB: u32 = 0x100000;
    pub const FLAG_CHAR_IS_NEWLINE: u32 = 0x200000;
    // Per CSS Text Decoration Module Level 3, emphasis marks are not
    // drawn for any character in Unicode categories Z*, Cc, Cf, and Cn
    // which is not combined with any combining characters. This flag is
    // set for all those characters except 0x20 whitespace.
    pub const FLAG_CHAR_NO_EMPHASIS_MARK: u32 = 0x400000;
    // Per CSS Text, letter-spacing is not applied to formatting chars
    // (category Cf). We mark those in the textrun so as to be able to
    // skip them when setting up spacing in nsTextFrame.
    pub const FLAG_CHAR_IS_FORMATTING_CONTROL: u32 = 0x800000;

    // The bits 0x0F000000 are currently unused in non-simple glyphs.
}

impl gfxShapedText_CompressedGlyph {
    // "Simple glyphs" have a simple glyph ID, simple advance and their
    // x and y offsets are zero. Also the glyph extents do not overflow
    // the font-box defined by the font ascent, descent and glyph advance width.
    // These case is optimized to avoid storing DetailedGlyphs.

    // Returns true if the glyph ID aGlyph fits into the compressed
    // representation
    pub fn IsSimpleGlyphID(aGlyph: u32) -> bool {
        (aGlyph & Self::GLYPH_MASK) == aGlyph
    }
    // Returns true if the advance aAdvance fits into the compressed
    // representation. aAdvance is in appunits.
    pub fn IsSimpleAdvance(aAdvance: u32) -> bool {
        (aAdvance & (Self::ADVANCE_MASK >> Self::ADVANCE_SHIFT)) == aAdvance
    }

    pub fn IsSimpleGlyph(&self) -> bool {
        self.mValue & Self::FLAG_IS_SIMPLE_GLYPH != 0
    }
    pub fn GetSimpleAdvance(&self) -> u32 {
        MOZ_ASSERT!(self.IsSimpleGlyph());
        (self.mValue & Self::ADVANCE_MASK) >> Self::ADVANCE_SHIFT
    }
    pub fn GetSimpleGlyph(&self) -> u32 {
        MOZ_ASSERT!(self.IsSimpleGlyph());
        self.mValue & Self::GLYPH_MASK
    }

    pub fn IsSimpleGlyphNoBreakBefore(&self) -> bool {
        (self.mValue & (Self::FLAG_IS_SIMPLE_GLYPH | Self::FLAGS_CAN_BREAK_BEFORE)) == Self::FLAG_IS_SIMPLE_GLYPH
    }

    pub fn IsMissing(&self) -> bool {
        self.mValue & (Self::FLAG_NOT_MISSING | Self::FLAG_IS_SIMPLE_GLYPH) == 0
    }
    pub fn IsClusterStart(&self) -> bool {
        self.IsSimpleGlyph() || self.mValue & Self::FLAG_NOT_CLUSTER_START == 0
    }
    pub fn IsLigatureGroupStart(&self) -> bool {
        self.IsSimpleGlyph() || self.mValue & Self::FLAG_NOT_LIGATURE_GROUP_START == 0
    }
    pub fn IsLigatureContinuation(&self) -> bool {
        !self.IsSimpleGlyph()
            && (self.mValue & (Self::FLAG_NOT_LIGATURE_GROUP_START | Self::FLAG_NOT_MISSING))
                == (Self::FLAG_NOT_LIGATURE_GROUP_START | Self::FLAG_NOT_MISSING)
    }

    // Return true if the original character was a normal (breakable,
    // trimmable) space (U+0020). Not true for other characters that
    // may happen to map to the space glyph (U+00A0).
    pub fn CharIsSpace(&self) -> bool {
        self.mValue & Self::FLAG_CHAR_IS_SPACE != 0
    }

    pub fn CharIsTab(&self) -> bool {
        !self.IsSimpleGlyph() && self.mValue & Self::FLAG_CHAR_IS_TAB != 0
    }
    pub fn CharIsNewline(&self) -> bool {
        !self.IsSimpleGlyph() && self.mValue & Self::FLAG_CHAR_IS_NEWLINE != 0
    }
    pub fn CharMayHaveEmphasisMark(&self) -> bool {
        !self.CharIsSpace() && (self.IsSimpleGlyph() || self.mValue & Self::FLAG_CHAR_NO_EMPHASIS_MARK == 0)
    }
    pub fn CharIsFormattingControl(&self) -> bool {
        !self.IsSimpleGlyph() && self.mValue & Self::FLAG_CHAR_IS_FORMATTING_CONTROL != 0
    }

    pub fn CharTypeFlags(&self) -> u32 {
        if self.IsSimpleGlyph() { 0 } else { self.mValue & Self::CHAR_TYPE_FLAGS_MASK }
    }

    pub fn SetClusterStart(&mut self, aIsClusterStart: bool) {
        MOZ_ASSERT!(!self.IsSimpleGlyph());
        if aIsClusterStart {
            self.mValue &= !Self::FLAG_NOT_CLUSTER_START;
        } else {
            self.mValue |= Self::FLAG_NOT_CLUSTER_START;
        }
    }

    pub fn CanBreakBefore(&self) -> u8 {
        ((self.mValue & Self::FLAGS_CAN_BREAK_BEFORE) >> Self::FLAGS_CAN_BREAK_SHIFT) as u8
    }
    // Returns FLAGS_CAN_BREAK_BEFORE if the setting changed, 0 otherwise
    pub fn SetCanBreakBefore(&mut self, aCanBreakBefore: u8) -> u32 {
        MOZ_ASSERT!(aCanBreakBefore <= 3, "Bogus break-flags value!");
        let breakMask: u32 = (aCanBreakBefore as u32) << Self::FLAGS_CAN_BREAK_SHIFT;
        let toggle: u32 = breakMask ^ (self.mValue & Self::FLAGS_CAN_BREAK_BEFORE);
        self.mValue ^= toggle;
        toggle
    }

    // Create a CompressedGlyph value representing a simple glyph with
    // no extra flags (line-break or is_space) set.
    pub fn MakeSimpleGlyph(aAdvanceAppUnits: u32, aGlyph: u32) -> gfxShapedText_CompressedGlyph {
        MOZ_ASSERT!(Self::IsSimpleAdvance(aAdvanceAppUnits));
        MOZ_ASSERT!(Self::IsSimpleGlyphID(aGlyph));
        gfxShapedText_CompressedGlyph {
            mValue: Self::FLAG_IS_SIMPLE_GLYPH | (aAdvanceAppUnits << Self::ADVANCE_SHIFT) | aGlyph,
        }
    }

    // Assign a simple glyph value to an existing CompressedGlyph record,
    // preserving line-break/is-space flags if present.
    pub fn SetSimpleGlyph(&mut self, aAdvanceAppUnits: u32, aGlyph: u32) -> &mut Self {
        MOZ_ASSERT!(self.CharTypeFlags() == 0, "Char type flags lost");
        self.mValue = (self.mValue & Self::COMMON_FLAGS_MASK) | Self::MakeSimpleGlyph(aAdvanceAppUnits, aGlyph).mValue;
        self
    }

    // Create a CompressedGlyph value representing a complex glyph record,
    // without any line-break or char-type flags.
    pub fn MakeComplex(aClusterStart: bool, aLigatureStart: bool) -> gfxShapedText_CompressedGlyph {
        gfxShapedText_CompressedGlyph {
            mValue: Self::FLAG_NOT_MISSING
                | (if aClusterStart { 0 } else { Self::FLAG_NOT_CLUSTER_START })
                | (if aLigatureStart { 0 } else { Self::FLAG_NOT_LIGATURE_GROUP_START }),
        }
    }

    // Assign a complex glyph value to an existing CompressedGlyph record,
    // preserving line-break/char-type flags if present.
    // This sets the glyphCount to zero; it will be updated when we call
    // gfxShapedText::SetDetailedGlyphs.
    pub fn SetComplex(&mut self, aClusterStart: bool, aLigatureStart: bool) -> &mut Self {
        self.mValue = (self.mValue & Self::COMMON_FLAGS_MASK)
            | self.CharTypeFlags()
            | Self::MakeComplex(aClusterStart, aLigatureStart).mValue;
        self
    }

    /**
     * Mark a glyph record as being a missing-glyph.
     * Missing glyphs are treated as ligature group starts; don't mess with
     * the cluster-start flag (see bugs 618870 and 619286).
     * We also preserve the glyph count here, as this is used after any
     * required DetailedGlyphs (to store the char code for a hexbox) has been
     * set up.
     * This must be called *after* SetDetailedGlyphs is used for the relevant
     * offset in the shaped-word, because that will mark it as not-missing.
     */
    pub fn SetMissing(&mut self) -> &mut Self {
        MOZ_ASSERT!(!self.IsSimpleGlyph());
        self.mValue &= !(Self::FLAG_NOT_MISSING | Self::FLAG_NOT_LIGATURE_GROUP_START);
        self
    }

    pub fn GetGlyphCount(&self) -> u32 {
        MOZ_ASSERT!(!self.IsSimpleGlyph());
        self.mValue & Self::GLYPH_COUNT_MASK
    }
    pub fn SetGlyphCount(&mut self, aGlyphCount: u32) {
        MOZ_ASSERT!(!self.IsSimpleGlyph());
        MOZ_ASSERT!(self.GetGlyphCount() == 0, "Glyph count already set");
        MOZ_ASSERT!(aGlyphCount <= 0xffff, "Glyph count out of range");
        self.mValue |= Self::FLAG_NOT_MISSING | aGlyphCount;
    }

    pub fn SetIsSpace(&mut self) {
        self.mValue |= Self::FLAG_CHAR_IS_SPACE;
    }
    pub fn SetIsTab(&mut self) {
        MOZ_ASSERT!(!self.IsSimpleGlyph());
        self.mValue |= Self::FLAG_CHAR_IS_TAB;
    }
    pub fn SetIsNewline(&mut self) {
        MOZ_ASSERT!(!self.IsSimpleGlyph());
        self.mValue |= Self::FLAG_CHAR_IS_NEWLINE;
    }
    pub fn SetNoEmphasisMark(&mut self) {
        MOZ_ASSERT!(!self.IsSimpleGlyph());
        self.mValue |= Self::FLAG_CHAR_NO_EMPHASIS_MARK;
    }
    pub fn SetIsFormattingControl(&mut self) {
        MOZ_ASSERT!(!self.IsSimpleGlyph());
        self.mValue |= Self::FLAG_CHAR_IS_FORMATTING_CONTROL;
    }

    pub fn SetApplyLetterSpacingBetweenDetailedGlyphs(&mut self) {
        MOZ_ASSERT!(!self.IsSimpleGlyph());
        self.mValue |= Self::FLAG_APPLY_LETTER_SPACING_BETWEEN_DETAILED_GLYPHS;
    }
    pub fn ApplyLetterSpacingBetweenDetailedGlyphs(&self) -> bool {
        !self.IsSimpleGlyph() && self.mValue & Self::FLAG_APPLY_LETTER_SPACING_BETWEEN_DETAILED_GLYPHS != 0
    }
}

/**
 * When the glyphs for a character don't fit into a CompressedGlyph record
 * in SimpleGlyph format, we use an array of DetailedGlyphs instead.
 */
// Gecko: gfx/thebes/gfxFont.h gfxShapedText::DetailedGlyph
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct gfxShapedText_DetailedGlyph {
    // The glyphID, or the Unicode character if this is a missing glyph
    pub mGlyphID: u32,
    // The advance of the glyph, in appunits.
    // mAdvance is in the text direction (RTL or LTR),
    // and will normally be non-negative (although this is not guaranteed)
    pub mAdvance: i32,
    // The offset from the glyph's default position, in line-relative
    // coordinates (so mOffset.x is an offset in the line-right direction,
    // and mOffset.y is an offset in line-downwards direction).
    // These values are in floating-point appUnits.
    pub mOffset: Point,
}

// Gecko: gfx/thebes/gfxFont.h gfxShapedText::DetailedGlyphStore::DGRec
#[derive(Clone, Copy, Debug)]
struct gfxShapedText_DetailedGlyphStore_DGRec {
    mOffset: u32, // source character offset in the textrun
    mIndex: u32,  // index where this char's DetailedGlyphs begin
}

// For characters whose glyph data does not fit the "simple" glyph criteria
// in CompressedGlyph, we use a sorted array to store the association
// between the source character offset and an index into an array
// DetailedGlyphs. The CompressedGlyph record includes a count of
// the number of DetailedGlyph records that belong to the character,
// starting at the given index.
// Gecko: gfx/thebes/gfxFont.h gfxShapedText::DetailedGlyphStore
#[derive(Default)]
pub struct gfxShapedText_DetailedGlyphStore {
    // Concatenated array of all the DetailedGlyph records needed for the
    // textRun; individual character offsets are associated with indexes
    // into this array via the mOffsetToIndex table.
    mDetails: Vec<gfxShapedText_DetailedGlyph>,

    // For each character offset that needs DetailedGlyphs, we record the
    // index in mDetails where the list of glyphs begins. This array is
    // sorted by mOffset.
    mOffsetToIndex: Vec<gfxShapedText_DetailedGlyphStore_DGRec>,

    // Records the most recently used index into mOffsetToIndex, so that
    // we can support sequential access more quickly than just doing
    // a binary search each time.
    mLastUsed: Cell<usize>,
}

impl gfxShapedText_DetailedGlyphStore {
    // This is optimized for the most common calling patterns:
    // we rarely need random access to the records, access is most commonly
    // sequential through the textRun, so we record the last-used index
    // and check whether the caller wants the same record again, or the
    // next; if not, it's most likely we're starting over from the start
    // of the run, so we check the first entry before resorting to binary
    // search as a last resort.
    // NOTE that this must not be called for a character offset that does
    // not have any DetailedGlyph records; callers must have verified that
    // mCharacterGlyphs[aOffset].GetGlyphCount() is greater than zero
    // before calling this, otherwise the assertions here will fire (in a
    // debug build), and we'll probably crash.
    // ADAPTER: returns the glyphs from the character's first one to the end of the store; the caller
    // reads GetGlyphCount() of them.
    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::DetailedGlyphStore::Get
    pub fn Get(&self, aOffset: u32) -> &[gfxShapedText_DetailedGlyph] {
        NS_ASSERTION!(self.mOffsetToIndex.len() > 0, "no detailed glyph records!");
        let mut lastUsed = self.mLastUsed.get();
        // check common cases (fwd iteration, initial entry, etc) first
        if lastUsed + 1 < self.mOffsetToIndex.len() && aOffset == self.mOffsetToIndex[lastUsed + 1].mOffset {
            lastUsed += 1;
        } else if aOffset == self.mOffsetToIndex[0].mOffset {
            lastUsed = 0;
        } else if aOffset == self.mOffsetToIndex[lastUsed].mOffset {
            // do nothing
        } else if lastUsed > 0 && aOffset == self.mOffsetToIndex[lastUsed - 1].mOffset {
            lastUsed -= 1;
        } else {
            lastUsed = self
                .mOffsetToIndex
                .binary_search_by(|r| r.mOffset.cmp(&aOffset))
                .expect("detailed glyph record missing!");
        }
        self.mLastUsed.set(lastUsed);
        &self.mDetails[self.mOffsetToIndex[lastUsed].mIndex as usize..]
    }

    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::DetailedGlyphStore::Allocate
    pub fn Allocate(&mut self, aOffset: u32, aCount: u32) -> &mut [gfxShapedText_DetailedGlyph] {
        let detailIndex: u32 = self.mDetails.len() as u32;
        self.mDetails.resize(self.mDetails.len() + aCount as usize, gfxShapedText_DetailedGlyph::default());
        // We normally set up glyph records sequentially, so the common case
        // here is to append new records to the mOffsetToIndex array;
        // test for that before falling back to the InsertElementSorted
        // method.
        let rec = gfxShapedText_DetailedGlyphStore_DGRec { mOffset: aOffset, mIndex: detailIndex };
        if self.mOffsetToIndex.is_empty() || aOffset > self.mOffsetToIndex[self.mOffsetToIndex.len() - 1].mOffset {
            self.mOffsetToIndex.push(rec);
        } else {
            // InsertElementSorted: after the records that are not greater
            let at = self.mOffsetToIndex.partition_point(|r| r.mOffset <= aOffset);
            self.mOffsetToIndex.insert(at, rec);
        }
        &mut self.mDetails[detailIndex as usize..]
    }
}

/*
 * gfxShapedText is an abstract superclass for gfxShapedWord and gfxTextRun.
 * These are objects that store a list of zero or more glyphs for each
 * character. For each glyph we store the glyph ID, the advance, and possibly
 * x/y-offsets. The idea is that a string is rendered by a loop that draws each
 * glyph at its designated offset from the current point, then advances the
 * current point by the glyph's advance in the direction of the textrun (LTR or
 * RTL). Each glyph advance is always rounded to the nearest appunit; this
 * ensures consistent results when dividing the text in a textrun into multiple
 * text frames (frame boundaries are always aligned to appunits). We optimize
 * for the case where a character has a single glyph and zero xoffset and
 * yoffset, and the glyph ID and advance are in a reasonable range so we can
 * pack all necessary data into 32 bits.
 *
 * gfxFontShaper can shape text into either a gfxShapedWord (cached by a
 * gfxFont) or directly into a gfxTextRun (for cases where we want to shape
 * textruns in their entirety rather than using cached words, because there may
 * be layout features that depend on the inter-word spaces).
 */
// Gecko: gfx/thebes/gfxFont.h gfxShapedText
// ADAPTER: there is no gfxShapedWord here, so the base class owns the glyph array (Gecko's
// gfxTextRun::mCharacterGlyphs, reached through the virtual GetCharacterGlyphs()). gfxTextRun holds
// this struct as its base and Derefs to it.
pub struct gfxShapedText {
    // ADAPTER: an exact-size array (Gecko: a pointer to storage allocated after the run, whose size is
    // mLength records); the length of the array is the mLength of Gecko, which is not stored.
    pub(crate) mCharacterGlyphs: Box<[gfxShapedText_CompressedGlyph]>,

    pub(crate) mDetailedGlyphs: Option<Box<gfxShapedText_DetailedGlyphStore>>,

    // Shaping flags (direction, ligature-suppression)
    pub(crate) mFlags: ShapedTextFlags,

    mAppUnitsPerDevUnit: u16,
}

impl gfxShapedText {
    // Instrumentation (bench/textrun_profile.py): [glyph len, glyph capacity, has detailed store,
    // details len, details capacity, offset records len, offset records capacity, simple glyph count,
    // size_of CompressedGlyph, size_of DetailedGlyph, size_of DGRec]
    pub fn MemStats(&self) -> [usize; 11] {
        let simple = self.mCharacterGlyphs.iter().filter(|g| g.IsSimpleGlyph()).count();
        let (has, dl, dc, ol, oc) = match &self.mDetailedGlyphs {
            Some(d) => (1, d.mDetails.len(), d.mDetails.capacity(), d.mOffsetToIndex.len(), d.mOffsetToIndex.capacity()),
            None => (0, 0, 0, 0, 0),
        };
        [
            self.mCharacterGlyphs.len(), self.mCharacterGlyphs.len(), has, dl, dc, ol, oc, simple,
            std::mem::size_of::<gfxShapedText_CompressedGlyph>(), std::mem::size_of::<gfxShapedText_DetailedGlyph>(),
            std::mem::size_of::<gfxShapedText_DetailedGlyphStore_DGRec>(),
        ]
    }

    // ADAPTER: make the detailed glyph arrays exact-size once shaping is done (Vec growth leaves spare
    // capacity; Gecko's nsTArray does too, and keeps it).
    pub fn ShrinkDetailedGlyphs(&mut self) {
        if let Some(d) = &mut self.mDetailedGlyphs {
            d.mDetails.shrink_to_fit();
            d.mOffsetToIndex.shrink_to_fit();
        }
    }

    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::gfxShapedText
    pub fn new(aLength: u32, aFlags: ShapedTextFlags, aAppUnitsPerDevUnit: u16) -> gfxShapedText {
        gfxShapedText {
            // Gecko: the glyph storage is zeroed (AllocateStorageForTextRun), i.e. every character
            // starts as a missing glyph.
            mCharacterGlyphs: vec![gfxShapedText_CompressedGlyph::default(); aLength as usize].into_boxed_slice(),
            mDetailedGlyphs: None,
            mFlags: aFlags,
            mAppUnitsPerDevUnit: aAppUnitsPerDevUnit,
        }
    }

    // Accessor for the array of CompressedGlyph records, which will be in
    // a different place in gfxShapedWord vs gfxTextRun
    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::GetCharacterGlyphs
    pub fn GetCharacterGlyphs(&self) -> &[gfxShapedText_CompressedGlyph] {
        &self.mCharacterGlyphs
    }
    pub fn GetCharacterGlyphs_mut(&mut self) -> &mut [gfxShapedText_CompressedGlyph] {
        &mut self.mCharacterGlyphs
    }

    // Store DetailedGlyph records for the given index. (This does not modify
    // the associated CompressedGlyph character-type or break flags.)
    // Gecko: gfx/thebes/gfxFont.cpp gfxShapedText::SetDetailedGlyphs
    pub fn SetDetailedGlyphs(&mut self, aIndex: u32, aGlyphCount: u32, aGlyphs: &[gfxShapedText_DetailedGlyph]) {
        MOZ_ASSERT!(aIndex > 0 || self.mCharacterGlyphs[aIndex as usize].IsLigatureGroupStart(),
            "First character can't be a ligature continuation!");

        if aGlyphCount > 0 {
            let details = self.AllocateDetailedGlyphs(aIndex, aGlyphCount);
            details.copy_from_slice(&aGlyphs[..aGlyphCount as usize]);
        }

        self.mCharacterGlyphs[aIndex as usize].SetGlyphCount(aGlyphCount);
    }

    // Gecko: gfx/thebes/gfxFont.cpp gfxShapedText::SetMissingGlyph
    // NOT PORTED: gfxFontMissingGlyphs::GetDesiredMinWidth (the width of a hexbox); a missing glyph
    // is as wide as the font's average character.
    pub fn SetMissingGlyph(&mut self, aIndex: u32, aChar: u32, aFont: &gfxFont) {
        let category = UnicodeProperties::CharType(aChar);
        if category == GeneralCategory::Spacing_Mark
            || category == GeneralCategory::Enclosing_Mark
            || category == GeneralCategory::Nonspacing_Mark
        {
            self.mCharacterGlyphs[aIndex as usize].SetComplex(false, true);
        }

        // Leaving advance as zero will prevent drawing the hexbox for ignorables.
        let mut advance: i32 = 0;
        if !IsIgnorable(aChar) {
            let width: gfxFloat = aFont.GetMetrics().aveCharWidth;
            advance = (width * self.mAppUnitsPerDevUnit as f64) as i32;
        }
        let detail = gfxShapedText_DetailedGlyph { mGlyphID: aChar, mAdvance: advance, mOffset: Point::default() };
        self.SetDetailedGlyphs(aIndex, 1, &[detail]);
        self.mCharacterGlyphs[aIndex as usize].SetMissing();
    }

    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::SetIsSpace
    pub fn SetIsSpace(&mut self, aIndex: u32) {
        self.mCharacterGlyphs[aIndex as usize].SetIsSpace();
    }

    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::HasDetailedGlyphs
    pub fn HasDetailedGlyphs(&self) -> bool {
        self.mDetailedGlyphs.is_some()
    }

    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::IsLigatureGroupStart
    pub fn IsLigatureGroupStart(&self, aPos: u32) -> bool {
        NS_ASSERTION!(aPos < self.GetLength(), "aPos out of range");
        self.mCharacterGlyphs[aPos as usize].IsLigatureGroupStart()
    }

    // NOTE that this must not be called for a character offset that does
    // not have any DetailedGlyph records; callers must have verified that
    // GetCharacterGlyphs()[aCharIndex].GetGlyphCount() is greater than zero.
    // ADAPTER: the slice runs from the character's first glyph to the end of the store.
    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::GetDetailedGlyphs
    pub fn GetDetailedGlyphs(&self, aCharIndex: u32) -> &[gfxShapedText_DetailedGlyph] {
        NS_ASSERTION!(
            self.HasDetailedGlyphs()
                && !self.mCharacterGlyphs[aCharIndex as usize].IsSimpleGlyph()
                && self.mCharacterGlyphs[aCharIndex as usize].GetGlyphCount() > 0,
            "invalid use of GetDetailedGlyphs; check the caller!"
        );
        self.mDetailedGlyphs.as_ref().unwrap().Get(aCharIndex)
    }

    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::ApplyTrackingToClusters
    // NOT PORTED: tracking tables (gfxFontEntry::HasTrackingTable); no font here has one.
    pub fn ApplyTrackingToClusters(&mut self, aTrackingAdjustment: gfxFloat, aOffset: u32, aLength: u32) {
        unimplemented!("NOT PORTED: tracking tables")
    }

    // Mark clusters in the CompressedGlyph records, starting at aOffset,
    // based on the Unicode properties of the text in aString.
    // This is also responsible to set the IsSpace flag for space characters.
    // Gecko: gfx/thebes/gfxFont.cpp gfxShapedText::SetupClusterBoundaries(uint32_t, const char16_t*, uint32_t)
    pub fn SetupClusterBoundaries__uint32_t_char16_t_ptr_uint32_t(&mut self, aOffset: u32, aString: &[u16], aLength: u32) {
        if aLength == 0 {
            return;
        }

        let glyphs = &mut self.mCharacterGlyphs[aOffset as usize..];
        let extendCluster = gfxShapedText_CompressedGlyph::MakeComplex(false, true);

        // GraphemeClusterBreakIteratorUtf16 won't be able to tell us if the string
        // _begins_ with a cluster-extender, so we handle that here
        let mut ch: u32 = aString[0] as u32;
        if aLength > 1 && IsSurrogatePair(ch, aString[1] as u32) {
            ch = SurrogateToUCS4(aString[0], aString[1]);
        }
        if IsClusterExtender(ch) {
            glyphs[0] = extendCluster;
        }

        // icu_segmenter: intl::GraphemeClusterBreakIteratorUtf16 is the crate's iterator; the break
        // positions are collected (they include 0 and the length) and walked with `iter`.
        let breaks: Vec<usize> = GraphemeClusterSegmenter::new().segment_utf16(&aString[..aLength as usize]).collect();
        let mut iter = breaks.iter().skip(1);
        let mut pos: u32 = 0;

        const kIdeographicSpace: u16 = 0x3000;
        // Special case for Bengali: although Virama normally clusters with the
        // preceding letter, we *also* want to cluster it with a following Ya
        // so that when the Virama+Ya form ya-phala, this is not separated from the
        // preceding letter by any letter-spacing or justification.
        const kBengaliVirama: u16 = 0x09CD;
        const kBengaliYa: u16 = 0x09AF;
        let mut prevWasHyphen = false;
        while pos < aLength {
            let ch: u16 = aString[pos as usize];
            if prevWasHyphen {
                if nsContentUtils_IsAlphanumeric(ch as u32) {
                    glyphs[pos as usize].SetCanBreakBefore(gfxShapedText_CompressedGlyph::FLAG_BREAK_TYPE_EMERGENCY_WRAP);
                }
                prevWasHyphen = false;
            }
            if ch == ' ' as u16 || ch == kIdeographicSpace {
                glyphs[pos as usize].SetIsSpace();
            } else if nsContentUtils_IsHyphen(ch as u32) && pos != 0 && nsContentUtils_IsAlphanumeric(aString[pos as usize - 1] as u32) {
                prevWasHyphen = true;
            } else if ch == kBengaliYa {
                // Unless we're at the start, check for a preceding virama.
                if pos > 0 && aString[pos as usize - 1] == kBengaliVirama {
                    glyphs[pos as usize] = extendCluster;
                }
            }
            // advance iter to the next cluster-start (or end of text)
            let nextPos: u32 = *iter.next().unwrap_or(&(aLength as usize)) as u32;
            // step past the first char of the cluster
            pos += 1;
            // mark all the rest as cluster-continuations
            while pos < nextPos {
                glyphs[pos as usize] = extendCluster;
                pos += 1;
            }
        }
    }

    // In 8-bit text, there won't actually be any clusters, but we still need
    // the space-marking functionality.
    // Gecko: gfx/thebes/gfxFont.cpp gfxShapedText::SetupClusterBoundaries(uint32_t, const uint8_t*, uint32_t)
    pub fn SetupClusterBoundaries__uint32_t_uint8_t_ptr_uint32_t(&mut self, aOffset: u32, aString: &[u8], aLength: u32) {
        let glyphs = &mut self.mCharacterGlyphs[aOffset as usize..];
        let mut pos: u32 = 0;
        let mut prevWasHyphen = false;
        while pos < aLength {
            let ch: u8 = aString[pos as usize];
            if prevWasHyphen {
                if nsContentUtils_IsAlphanumeric(ch as u32) {
                    glyphs[pos as usize].SetCanBreakBefore(gfxShapedText_CompressedGlyph::FLAG_BREAK_TYPE_EMERGENCY_WRAP);
                }
                prevWasHyphen = false;
            }
            if ch == b' ' {
                glyphs[pos as usize].SetIsSpace();
            } else if ch == b'-' && pos != 0 && nsContentUtils_IsAlphanumeric(aString[pos as usize - 1] as u32) {
                prevWasHyphen = true;
            }
            pos += 1;
        }
    }

    // ADAPTER: the template call `SetupClusterBoundaries(aOffset, aString, aLength)` over T.
    pub fn SetupClusterBoundaries<T: TextFrameChar>(&mut self, aOffset: u32, aString: &[T], aLength: u32) {
        if T::IS_WIDE {
            let s: Vec<u16> = aString[..aLength as usize].iter().map(|c| c.to_u32() as u16).collect();
            self.SetupClusterBoundaries__uint32_t_char16_t_ptr_uint32_t(aOffset, &s, aLength);
        } else {
            let s: Vec<u8> = aString[..aLength as usize].iter().map(|c| c.to_u32() as u8).collect();
            self.SetupClusterBoundaries__uint32_t_uint8_t_ptr_uint32_t(aOffset, &s, aLength);
        }
    }

    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::GetFlags
    pub fn GetFlags(&self) -> ShapedTextFlags {
        self.mFlags
    }

    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::IsVertical
    pub fn IsVertical(&self) -> bool {
        (self.GetFlags() & ShapedTextFlags::TEXT_ORIENT_MASK) != ShapedTextFlags::TEXT_ORIENT_HORIZONTAL
    }

    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::UseCenterBaseline
    pub fn UseCenterBaseline(&self) -> bool {
        let orient = self.GetFlags() & ShapedTextFlags::TEXT_ORIENT_MASK;
        orient == ShapedTextFlags::TEXT_ORIENT_VERTICAL_MIXED || orient == ShapedTextFlags::TEXT_ORIENT_VERTICAL_UPRIGHT
    }

    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::IsRightToLeft
    pub fn IsRightToLeft(&self) -> bool {
        (self.GetFlags() & ShapedTextFlags::TEXT_IS_RTL) == ShapedTextFlags::TEXT_IS_RTL
    }

    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::IsSidewaysLeft
    pub fn IsSidewaysLeft(&self) -> bool {
        (self.GetFlags() & ShapedTextFlags::TEXT_ORIENT_MASK) == ShapedTextFlags::TEXT_ORIENT_VERTICAL_SIDEWAYS_LEFT
    }

    // Return true if the logical inline direction is reversed compared to
    // normal physical coordinates (i.e. if it is leftwards or upwards)
    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::IsInlineReversed
    pub fn IsInlineReversed(&self) -> bool {
        self.IsSidewaysLeft() != self.IsRightToLeft()
    }

    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::GetDirection
    pub fn GetDirection(&self) -> gfxFloat {
        if self.IsInlineReversed() { -1.0 } else { 1.0 }
    }

    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::DisableLigatures
    pub fn DisableLigatures(&self) -> bool {
        (self.GetFlags() & ShapedTextFlags::TEXT_DISABLE_OPTIONAL_LIGATURES) == ShapedTextFlags::TEXT_DISABLE_OPTIONAL_LIGATURES
    }

    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::TextIs8Bit
    pub fn TextIs8Bit(&self) -> bool {
        (self.GetFlags() & ShapedTextFlags::TEXT_IS_8BIT) == ShapedTextFlags::TEXT_IS_8BIT
    }

    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::GetAppUnitsPerDevUnit
    pub fn GetAppUnitsPerDevUnit(&self) -> i32 {
        self.mAppUnitsPerDevUnit as i32
    }

    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::GetLength
    // (the number of char16_t characters and CompressedGlyph glyph records: the array length)
    pub fn GetLength(&self) -> u32 {
        self.mCharacterGlyphs.len() as u32
    }

    // Gecko: gfx/thebes/gfxFont.cpp gfxShapedText::FilterIfIgnorable
    pub fn FilterIfIgnorable(&mut self, aIndex: u32, aCh: u32) -> bool {
        if !IsIgnorable(aCh) {
            return false;
        }
        // There are a few default-ignorables of Letter category (currently,
        // just the Hangul filler characters) that we'd better not discard
        // if they're followed by additional characters in the same cluster.
        // Some fonts use them to carry the width of a whole cluster of
        // combining jamos; see bug 1238243.
        let length = self.GetLength();
        let charGlyphs = &mut self.mCharacterGlyphs;
        let category = UnicodeProperties::CharType(aCh);
        if category == GeneralCategory::Other_Letter && aIndex + 1 < length && !charGlyphs[aIndex as usize + 1].IsClusterStart() {
            return false;
        }
        let g = &mut charGlyphs[aIndex as usize];
        // For Format-Control characters, we avoid filtering if the glyph is _not_
        // .notdef and has a positive advance width (e.g. Mongolian Vowel Separator
        // in some fonts).
        if category == GeneralCategory::Format {
            if !g.IsSimpleGlyph() || (g.GetSimpleGlyph() != 0 && g.GetSimpleAdvance() > 0) {
                return false;
            }
        }
        // A compressedGlyph that is set to MISSING but has no DetailedGlyphs list
        // will be zero-width/invisible, which is what we want here.
        let (cs, lgs) = (g.IsClusterStart(), g.IsLigatureGroupStart());
        g.SetComplex(cs, lgs).SetMissing();
        true
    }

    // Allocate aCount DetailedGlyphs for the given index
    // Gecko: gfx/thebes/gfxFont.cpp gfxShapedText::AllocateDetailedGlyphs
    pub fn AllocateDetailedGlyphs(&mut self, aIndex: u32, aCount: u32) -> &mut [gfxShapedText_DetailedGlyph] {
        NS_ASSERTION!(aIndex < self.GetLength(), "Index out of range");

        if self.mDetailedGlyphs.is_none() {
            self.mDetailedGlyphs = Some(Box::new(gfxShapedText_DetailedGlyphStore::default()));
        }

        &mut self.mDetailedGlyphs.as_mut().unwrap().Allocate(aIndex, aCount)[..aCount as usize]
    }

    // Ensure the glyph on the given index is complex glyph so that we can use
    // it to record specific characters that layout may need to detect.
    // Gecko: gfx/thebes/gfxFont.h gfxShapedText::EnsureComplexGlyph
    pub fn EnsureComplexGlyph(&mut self, aIndex: u32) -> &mut gfxShapedText_CompressedGlyph {
        let g = self.mCharacterGlyphs[aIndex as usize];
        if g.IsSimpleGlyph() {
            let details = gfxShapedText_DetailedGlyph {
                mGlyphID: g.GetSimpleGlyph(),
                mAdvance: g.GetSimpleAdvance() as i32,
                mOffset: Point::default(),
            };
            self.mCharacterGlyphs[aIndex as usize].SetComplex(true, true);
            self.SetDetailedGlyphs(aIndex, 1, &[details]);
        }
        &mut self.mCharacterGlyphs[aIndex as usize]
    }
}

// ---------------------------------------------------------------------------------------------
// gfxFont members that work on text runs
// ---------------------------------------------------------------------------------------------

// limit the length of text we pass to shapers in a single call
// slightly less than 32K, trying to avoid over-stressing platform shapers
const MAX_SHAPING_LENGTH: u32 = 32760;
// backtrack this far looking for a good place to split into fragments for separate shaping
const BACKTRACK_LIMIT: u32 = 16;

// Check if aCh is an unhandled control character that should be displayed
// as a hexbox rather than rendered by some random font on the system.
// We exclude \r as stray &#13;s are rather common (bug 941940).
// Note that \n and \t don't come through here, as they have specific
// meanings that have already been handled.
// Gecko: gfx/thebes/gfxFont.cpp IsInvalidControlChar
fn IsInvalidControlChar(aCh: u32) -> bool {
    aCh != '\r' as u32 && ((aCh & 0x7f) < 0x20 || aCh == 0x7f)
}

// If aChar is a "word boundary" for shaped-word caching purposes, return it;
// else return 0.
// Gecko: gfx/thebes/gfxFont.cpp IsBoundarySpace(char16_t, char16_t)
fn IsBoundarySpace<T: TextFrameChar>(aChar: T, aNextChar: T) -> u32 {
    let c = aChar.to_u32();
    // In 8-bit text, there cannot be any cluster-extenders.
    if (c == ' ' as u32 || c == 0x00A0) && (!T::IS_WIDE || !IsClusterExtender(aNextChar.to_u32())) {
        return c;
    }
    0
}

// Gecko: gfx/thebes/gfxFont.h gfxFontGroup::IsInvalidChar(char16_t) / (uint8_t)
// (declared in gfxTextRun.h; here because ShapeTextWithoutWordCache calls it)
pub fn gfxFontGroup_IsInvalidChar<T: TextFrameChar>(ch: T) -> bool {
    let ch = ch.to_u32();
    if !T::IS_WIDE {
        return (ch & 0x7f) < 0x20 || ch == 0x7f;
    }
    // All printable 7-bit ASCII values are OK.
    if ch.wrapping_sub(0x20) < 0x7f - 0x20 {
        return false;
    }
    // No point in sending non-printing control chars through font shaping.
    if ch <= 0x9f {
        return true;
    }
    // Word-separating format/bidi control characters are not shaped as part
    // of words.
    ((ch & 0xFF00) == 0x2000
        && (ch == 0x200B /* zero-width space */
            || ch == 0x2028 /* line separator */
            || ch == 0x2029 /* paragraph separator */
            || ch == 0x2060 /* word joiner */))
        || ch == 0xfeff /* zero-width no-break space */
        || IsBidiControl(ch)
}

// Gecko: gfx/thebes/gfxFont.h gfxShapedWord
// A word shaped once and kept in its font's word cache (below), so that the next text run with the same
// word copies the glyph records instead of shaping it again (gfxTextRun::CopyGlyphDataFrom).
// ADAPTER: the text is a boxed slice (Gecko stores it after the glyph records); 8-bit words are kept
// widened, as the port shapes all text as 16-bit; the age counter is not kept (no expiration timer).
pub struct gfxShapedWord {
    pub mShapedText: gfxShapedText,
    mText: Box<[u16]>,
    mLanguage: Option<nsAtom>,
    mScript: Script,
    mRounding: gfxFontShaper_RoundingFlags,
}

impl gfxShapedWord {
    // Gecko: gfx/thebes/gfxFont.h gfxShapedWord::Create / gfxShapedWord::gfxShapedWord
    pub fn Create(aText: &[u16], aRunScript: Script, aLanguage: Option<nsAtom>, aAppUnitsPerDevUnit: u16, aFlags: ShapedTextFlags, aRounding: gfxFontShaper_RoundingFlags) -> gfxShapedWord {
        let aLength = aText.len() as u32;
        let mut word = gfxShapedWord {
            mShapedText: gfxShapedText::new(aLength, aFlags, aAppUnitsPerDevUnit),
            mText: aText.into(),
            mLanguage: aLanguage,
            mScript: aRunScript,
            mRounding: aRounding,
        };
        word.mShapedText.SetupClusterBoundaries__uint32_t_char16_t_ptr_uint32_t(0, aText, aLength);
        word
    }

    // Gecko: gfx/thebes/gfxFont.h gfxShapedWord::GetLength
    pub fn GetLength(&self) -> u32 {
        self.mText.len() as u32
    }

    // Gecko: gfx/thebes/gfxFont.h gfxShapedWord::HashMix
    pub fn HashMix(aHash: u32, aCh: u32) -> u32 {
        (aHash >> 28) ^ (aHash << 4) ^ aCh
    }

    // Gecko: gfx/thebes/gfxFont.cpp gfxFont::WordCacheKey::HashPolicy::match (the members compared)
    fn Matches(&self, aText: &[u16], aScript: Script, aLanguage: Option<nsAtom>, aAppUnitsPerDevUnit: u16, aFlags: ShapedTextFlags, aRounding: gfxFontShaper_RoundingFlags) -> bool {
        self.mShapedText.GetFlags() == aFlags
            && self.mRounding == aRounding
            && self.mShapedText.GetAppUnitsPerDevUnit() as u16 == aAppUnitsPerDevUnit
            && self.mScript == aScript
            && self.mLanguage == aLanguage
            && *self.mText == *aText
    }

    // ADAPTER: bytes held (for the memory report; Gecko: gfxShapedWord::SizeOfIncludingThis).
    pub fn SizeOfIncludingThis(&self) -> usize {
        let details = self.mShapedText.mDetailedGlyphs.as_ref().map_or(0, |d| {
            fn bytes<T>(v: &Vec<T>) -> usize {
                v.capacity() * size_of::<T>()
            }
            bytes(&d.mDetails) + bytes(&d.mOffsetToIndex)
        });
        size_of::<gfxShapedWord>() + self.mText.len() * 2 + self.mShapedText.mCharacterGlyphs.len() * size_of::<gfxShapedText_CompressedGlyph>() + details
    }
}

// Gecko: gfx/thebes/gfxFont.h gfxFont::mWordCache (a HashMap<WordCacheKey, UniquePtr<gfxShapedWord>>)
// ADAPTER: gfxFont is mrt-text's type, which cannot gain a member here, so the caches of all fonts are
// in one map of this thread, keyed by the font's address (fonts live as long as the thread, see
// gfxFontCache_Lookup) and the word's hash and length; the words of one bucket are compared in full.
// The expiration timer (gfxFontCache::AgeCachedWords, a word unused for kShapedWordCacheMaxAge = 3 ticks
// goes) is NOT PORTED: there is no event loop; a font's words are dropped when it reaches
// gfx.font_rendering.wordcache.maxentries (gfxFont::ProcessShapedWordInternal's limit check).
#[derive(Default)]
struct WordCache {
    mWords: std::collections::HashMap<(usize, u32, u32), Vec<Box<gfxShapedWord>>>,
    mCountPerFont: std::collections::HashMap<usize, usize>,
}

thread_local! {
    static WORD_CACHE: std::cell::RefCell<WordCache> = std::cell::RefCell::new(WordCache::default());
}

// Gecko: modules/libpref/init/StaticPrefList.yaml gfx.font_rendering.wordcache.maxentries
const kWordCacheMaxEntries: usize = 10000;

// Gecko: gfx/thebes/gfxPlatform.cpp gfxPlatform::WordCacheCharLimit (the pref
// gfx.font_rendering.wordcache.charlimit, 32). ADAPTER: the environment variable MRT_WORDCACHE_CHARLIMIT
// stands for the pref; 0 turns the cache off (every word is shaped into the text run).
fn WordCacheCharLimit() -> u32 {
    thread_local! {
        static LIMIT: u32 = std::env::var("MRT_WORDCACHE_CHARLIMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(32);
    }
    LIMIT.with(|l| *l)
}

// ADAPTER: the words and bytes in all word caches of this thread (memory reports; Gecko reports them as
// explicit/gfx/font-shaped-words, outside the windows).
pub fn WordCacheSize() -> (usize, usize) {
    WORD_CACHE.with(|c| {
        let c = c.borrow();
        let words: usize = c.mWords.values().map(|v| v.len()).sum();
        let bytes: usize = c.mWords.values().flatten().map(|w| w.SizeOfIncludingThis()).sum::<usize>() + c.mWords.capacity() * 48;
        (words, bytes)
    })
}

// Gecko: gfx/thebes/gfxFont.cpp gfxFont::ProcessShapedWordInternal
// The cached gfxShapedWord for this word in this font, shaped and added on a miss; `aCallback` gets it.
// NOT PORTED: the locks (one thread), text-perf counters, the expiration timer (see WordCache).
fn ProcessShapedWordInternal(
    aFont: &gfxFont,
    aText: &[u16],
    aHash: u32,
    aRunScript: Script,
    aLanguage: Option<nsAtom>,
    aVertical: bool,
    aAppUnitsPerDevUnit: i32,
    aFlags: ShapedTextFlags,
    aRounding: gfxFontShaper_RoundingFlags,
    aCallback: impl FnOnce(&gfxShapedWord),
) -> bool {
    let fontKey = aFont as *const gfxFont as usize;
    let key = (fontKey, aHash, aText.len() as u32);
    let units = aAppUnitsPerDevUnit as u16;
    // If we have a word cache, attempt to look up the word in it.
    let found = WORD_CACHE.with(|c| {
        let c = c.borrow();
        let w = c.mWords.get(&key)?.iter().find(|w| w.Matches(aText, aRunScript, aLanguage, units, aFlags, aRounding))?;
        // (a pointer: the callback below may shape, and so use the cache, again; a cached word is boxed and
        // only dropped by a clear, which nothing in the callback does)
        Some(&**w as *const gfxShapedWord)
    });
    if let Some(w) = found {
        aCallback(unsafe { &*w });
        return true;
    }

    // We didn't find a cached word (or don't even have a cache yet), so create
    // a new gfxShapedWord and cache it.
    let mut newShapedWord = Box::new(gfxShapedWord::Create(aText, aRunScript, aLanguage, units, aFlags, aRounding));
    let ok = aFont.ShapeText__char16_t_ptr_uint32_t_uint32_t_Script_nsAtom_ptr_bool_RoundingFlags_gfxShapedText_ptr(
        aText, 0, aText.len() as u32, aRunScript, aLanguage, aVertical, aRounding, &mut newShapedWord.mShapedText,
    );
    NS_WARNING_ASSERTION!(ok, "failed to shape word - expect garbled text");
    aCallback(&newShapedWord);
    WORD_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let count = c.mCountPerFont.entry(fontKey).or_insert(0);
        if *count >= kWordCacheMaxEntries {
            // gfxFont::ClearCachedWords for this font
            c.mWords.retain(|k, _| k.0 != fontKey);
            c.mCountPerFont.insert(fontKey, 0);
        }
        *c.mCountPerFont.get_mut(&fontKey).unwrap() += 1;
        c.mWords.entry(key).or_default().push(newShapedWord);
    });
    true
}

// Gecko: gfx/thebes/gfxFont.cpp gfxFont::ClearCachedWords (every font's; memory pressure)
pub fn ClearAllCachedWords() {
    WORD_CACHE.with(|c| *c.borrow_mut() = WordCache::default());
}

// The members of gfxFont (gfxFont.cpp) that need gfxTextRun; see the comment at the top of the file.
// Overloaded names follow CONVENTIONS.md.
// Gecko: gfx/thebes/gfxFont.h gfxFont
pub trait gfxFont_Ext {
    fn Measure(
        &self,
        aTextRun: &gfxTextRun,
        aStart: u32,
        aEnd: u32,
        aBoundingBoxType: gfxFont_BoundingBoxType,
        aRefDrawTarget: Option<&DrawTarget>,
        aSpacing: Option<&[gfxFont_Spacing]>,
        aLetterSpacing: nscoord,
        aOrientation: ShapedTextFlags,
    ) -> gfxFont_RunMetrics;

    fn MeasureGlyphs(
        &self,
        aTextRun: &gfxTextRun,
        aStart: u32,
        aEnd: u32,
        aBoundingBoxType: gfxFont_BoundingBoxType,
        aRefDrawTarget: Option<&DrawTarget>,
        aSpacing: Option<&[gfxFont_Spacing]>,
        aLetterSpacing: nscoord,
        aIsRTL: bool,
        aMetrics: &mut gfxFont_RunMetrics,
    ) -> bool;

    fn ShapeText__char16_t_ptr_uint32_t_uint32_t_Script_nsAtom_ptr_bool_RoundingFlags_gfxShapedText_ptr(
        &self,
        aText: &[u16],
        aOffset: u32,
        aLength: u32,
        aScript: Script,
        aLanguage: Option<nsAtom>,
        aVertical: bool,
        aRounding: gfxFontShaper_RoundingFlags,
        aShapedText: &mut gfxShapedText,
    ) -> bool;

    fn ShapeFragmentWithoutWordCache<T: TextFrameChar>(
        &self,
        aText: &[T],
        aOffset: u32,
        aLength: u32,
        aScript: Script,
        aLanguage: Option<nsAtom>,
        aVertical: bool,
        aRounding: gfxFontShaper_RoundingFlags,
        aTextRun: &mut gfxTextRun,
    ) -> bool;

    fn ShapeTextWithoutWordCache<T: TextFrameChar>(
        &self,
        aText: &[T],
        aOffset: u32,
        aLength: u32,
        aScript: Script,
        aLanguage: Option<nsAtom>,
        aVertical: bool,
        aRounding: gfxFontShaper_RoundingFlags,
        aTextRun: &mut gfxTextRun,
    ) -> bool;

    fn SplitAndInitTextRun<T: TextFrameChar>(
        self: &Rc<Self>,
        aDrawTarget: Option<&DrawTarget>,
        aTextRun: &mut gfxTextRun,
        aString: &[T],
        aRunStart: u32,
        aRunLength: u32,
        aRunScript: Script,
        aLanguage: Option<nsAtom>,
        aOrientation: ShapedTextFlags,
    ) -> bool;

    fn SetGlyphsFromRun(
        &self,
        aShapedText: &mut gfxShapedText,
        aOffset: u32,
        aLength: u32,
        aText: &[u16],
        aRun: &ShapedRun,
    );

    fn GetRoundOffsetsToPixels(&self, aDrawTarget: Option<&DrawTarget>) -> gfxFontShaper_RoundingFlags;
}

impl gfxFont_Ext for gfxFont {
    // Gecko: gfx/thebes/gfxFont.cpp gfxFont::Measure
    // NOT PORTED: the non-AA copy for TIGHT_HINTED_OUTLINE_EXTENTS, vertical metrics and the center
    // baseline offset, glyph extents (so the `else` branch of Gecko's `if (extents)` only), synthetic
    // oblique skew.
    fn Measure(
        &self,
        aTextRun: &gfxTextRun,
        aStart: u32,
        aEnd: u32,
        aBoundingBoxType: gfxFont_BoundingBoxType,
        aRefDrawTarget: Option<&DrawTarget>,
        aSpacing: Option<&[gfxFont_Spacing]>,
        aLetterSpacing: nscoord,
        aOrientation: ShapedTextFlags,
    ) -> gfxFont_RunMetrics {
        let appUnitsPerDevUnit: i32 = aTextRun.GetAppUnitsPerDevUnit();
        // Current position in appunits
        let fontMetrics: &gfxFont_Metrics = self.GetMetrics();

        let mut metrics = gfxFont_RunMetrics::new();
        metrics.mAscent = fontMetrics.maxAscent * appUnitsPerDevUnit as f64;
        metrics.mDescent = fontMetrics.maxDescent * appUnitsPerDevUnit as f64;

        if aStart == aEnd {
            // exit now before we look at aSpacing[0], which is undefined
            metrics.mBoundingBox = gfxRect::new(0.0, -metrics.mAscent, 0.0, metrics.mAscent + metrics.mDescent);
            return metrics;
        }

        let mut advanceMin: f64 = 0.0;
        let mut advanceMax: f64 = 0.0;
        let isRTL: bool = aTextRun.IsRightToLeft();

        let allGlyphsInvisible = self.MeasureGlyphs(
            aTextRun, aStart, aEnd, aBoundingBoxType, aRefDrawTarget, aSpacing, aLetterSpacing, isRTL, &mut metrics,
        );

        if allGlyphsInvisible {
            metrics.mBoundingBox.SetEmpty();
        } else if aBoundingBoxType == gfxFont_BoundingBoxType::LOOSE_INK_EXTENTS {
            // UnionRange(metrics.mAdvanceWidth, &advanceMin, &advanceMax);
            if metrics.mAdvanceWidth < advanceMin {
                advanceMin = metrics.mAdvanceWidth;
            } else if metrics.mAdvanceWidth > advanceMax {
                advanceMax = metrics.mAdvanceWidth;
            }
            let fontBox = gfxRect::new(advanceMin, -metrics.mAscent, advanceMax - advanceMin, metrics.mAscent + metrics.mDescent);
            metrics.mBoundingBox = metrics.mBoundingBox.Union(&fontBox);
        }

        if isRTL {
            // Reverse the effect of having swapped each glyph's sidebearings, to get
            // the correct sidebearings of the merged bounding box.
            metrics.mBoundingBox.MoveToX(metrics.mAdvanceWidth - metrics.mBoundingBox.XMost());
        }

        metrics
    }

    // Gecko: gfx/thebes/gfxFont.cpp gfxFont::MeasureGlyphs (the overload without glyph extents)
    // NOT PORTED: IsSpaceGlyphInvisible (needs a draw target); a space glyph counts as visible.
    fn MeasureGlyphs(
        &self,
        aTextRun: &gfxTextRun,
        aStart: u32,
        aEnd: u32,
        aBoundingBoxType: gfxFont_BoundingBoxType,
        aRefDrawTarget: Option<&DrawTarget>,
        aSpacing: Option<&[gfxFont_Spacing]>,
        aLetterSpacing: nscoord,
        aIsRTL: bool,
        aMetrics: &mut gfxFont_RunMetrics,
    ) -> bool {
        let charGlyphs = aTextRun.GetCharacterGlyphs();
        let mut x: f64 = 0.0;
        if let Some(spacing) = aSpacing {
            x += spacing[0].mBefore as f64;
        }
        let mut allGlyphsInvisible = true;

        for i in aStart..aEnd {
            let glyphData = &charGlyphs[i as usize];
            if glyphData.IsSimpleGlyph() {
                let advance: f64 = glyphData.GetSimpleAdvance() as f64;
                // NOT PORTED: the space glyph of an invisible-space font stays "invisible" here.
                allGlyphsInvisible = false;
                x += advance;
            } else {
                allGlyphsInvisible = false;
                let glyphCount: u32 = glyphData.GetGlyphCount();
                if glyphCount > 0 {
                    let details = aTextRun.GetDetailedGlyphs(i);
                    let mut j: u32 = 0;
                    while j < glyphCount {
                        let d = &details[j as usize];
                        let advance: f64 = d.mAdvance as f64;
                        let mut glyphRect = gfxRect::new(0.0, -aMetrics.mAscent, advance, aMetrics.mAscent + aMetrics.mDescent);
                        if aIsRTL {
                            // Swap left/right sidebearings of the glyph, because we're doing
                            // mirrored measurement.
                            glyphRect.MoveToX(advance - glyphRect.XMost());
                            // Move to current x position, mirroring any x-offset amount.
                            glyphRect.MoveByX(x - d.mOffset.x as f64);
                        } else {
                            glyphRect.MoveByX(x + d.mOffset.x as f64);
                        }
                        glyphRect.MoveByY(d.mOffset.y as f64);
                        aMetrics.mBoundingBox = aMetrics.mBoundingBox.Union(&glyphRect);
                        x += advance;
                        if glyphData.ApplyLetterSpacingBetweenDetailedGlyphs() && j < glyphCount - 1 {
                            x += aLetterSpacing as f64;
                        }
                        j += 1;
                    }
                }
            }
            if let Some(spacing) = aSpacing {
                let mut space: f64 = spacing[(i - aStart) as usize].mAfter as f64;
                if i + 1 < aEnd {
                    space += spacing[(i + 1 - aStart) as usize].mBefore as f64;
                }
                x += space;
            }
        }

        aMetrics.mAdvanceWidth = x;
        allGlyphsInvisible
    }

    // Gecko: gfx/thebes/gfxFont.cpp gfxFont::ShapeText(const char16_t*, ...)
    // NOT PORTED: Graphite, tracking, PostShapingFixup (synthetic bold); the shaper is mrt-text's
    // gfxFont::ShapeText (HarfBuzz) and SetGlyphsFromRun below stores its result.
    fn ShapeText__char16_t_ptr_uint32_t_uint32_t_Script_nsAtom_ptr_bool_RoundingFlags_gfxShapedText_ptr(
        &self,
        aText: &[u16],
        aOffset: u32,
        aLength: u32,
        aScript: Script,
        aLanguage: Option<nsAtom>,
        aVertical: bool,
        aRounding: gfxFontShaper_RoundingFlags,
        aShapedText: &mut gfxShapedText,
    ) -> bool {
        let text = &aText[..aLength as usize];
        let run = self.ShapeText(text, aShapedText.IsRightToLeft(), aLanguage);
        self.SetGlyphsFromRun(aShapedText, aOffset, aLength, text, &run);
        true
    }

    // Gecko: gfx/thebes/gfxFont.cpp gfxFont::ShapeFragmentWithoutWordCache
    fn ShapeFragmentWithoutWordCache<T: TextFrameChar>(
        &self,
        aText: &[T],
        aOffset: u32,
        aLength: u32,
        aScript: Script,
        aLanguage: Option<nsAtom>,
        aVertical: bool,
        aRounding: gfxFontShaper_RoundingFlags,
        aTextRun: &mut gfxTextRun,
    ) -> bool {
        let mut aText = aText;
        let mut aOffset = aOffset;
        let mut aLength = aLength;
        aTextRun.SetupClusterBoundaries(aOffset, aText, aLength);

        let mut ok = true;

        while ok && aLength > 0 {
            let mut fragLen: u32 = aLength;

            // limit the length of text we pass to shapers in a single call
            if fragLen > MAX_SHAPING_LENGTH {
                fragLen = MAX_SHAPING_LENGTH;

                // in the 8-bit case, there are no multi-char clusters,
                // so we don't need to do this check
                if T::IS_WIDE {
                    let mut i: u32 = 0;
                    while i < BACKTRACK_LIMIT {
                        if aTextRun.IsClusterStart(aOffset + fragLen - i) {
                            fragLen -= i;
                            break;
                        }
                        i += 1;
                    }
                    if i == BACKTRACK_LIMIT {
                        // if we didn't find any cluster start while backtracking,
                        // just check that we're not in the middle of a surrogate
                        // pair; back up by one code unit if we are.
                        if IsSurrogatePair(aText[fragLen as usize - 1].to_u32(), aText[fragLen as usize].to_u32()) {
                            fragLen -= 1;
                        }
                    }
                }
            }

            // ShapeText(const uint8_t*, ...) widens the text and calls the char16_t one.
            let text16: Vec<u16> = aText[..fragLen as usize].iter().map(|c| c.to_u32() as u16).collect();
            ok = self.ShapeText__char16_t_ptr_uint32_t_uint32_t_Script_nsAtom_ptr_bool_RoundingFlags_gfxShapedText_ptr(
                &text16, aOffset, fragLen, aScript, aLanguage, aVertical, aRounding, &mut **aTextRun,
            );

            aText = &aText[fragLen as usize..];
            aOffset += fragLen;
            aLength -= fragLen;
        }

        ok
    }

    // Gecko: gfx/thebes/gfxFont.cpp gfxFont::ShapeTextWithoutWordCache
    fn ShapeTextWithoutWordCache<T: TextFrameChar>(
        &self,
        aText: &[T],
        aOffset: u32,
        aLength: u32,
        aScript: Script,
        aLanguage: Option<nsAtom>,
        aVertical: bool,
        aRounding: gfxFontShaper_RoundingFlags,
        aTextRun: &mut gfxTextRun,
    ) -> bool {
        let mut fragStart: u32 = 0;
        let mut ok = true;

        let mut i: u32 = 0;
        while i <= aLength && ok {
            let ch: T = if i < aLength { aText[i as usize] } else { T::from_ascii(b'\n') };
            let invalid: bool = gfxFontGroup_IsInvalidChar(ch);
            let length: u32 = i - fragStart;

            // break into separate fragments when we hit an invalid char
            if !invalid {
                i += 1;
                continue;
            }

            if length > 0 {
                ok = self.ShapeFragmentWithoutWordCache(
                    &aText[fragStart as usize..], aOffset + fragStart, length, aScript, aLanguage, aVertical, aRounding, aTextRun,
                );
            }

            if i == aLength {
                break;
            }

            // fragment was terminated by an invalid char: skip it,
            // unless it's a control char that we want to show as a hexbox,
            // but record where TAB or NEWLINE occur
            let c = ch.to_u32();
            if c == '\t' as u32 {
                aTextRun.SetIsTab(aOffset + i);
            } else if c == '\n' as u32 {
                aTextRun.SetIsNewline(aOffset + i);
            } else if UnicodeProperties::CharType(c) == GeneralCategory::Format {
                aTextRun.SetIsFormattingControl(aOffset + i);
            } else if IsInvalidControlChar(c) && !(aTextRun.GetFlags() & ShapedTextFlags::TEXT_HIDE_CONTROL_CHARACTERS).to_bool() {
                // NOT PORTED: user fonts (`GetFontEntry()->IsUserFont() && HasCharacter(ch)`)
                aTextRun.SetMissingGlyph(aOffset + i, c, self);
            }
            fragStart = i + 1;
            i += 1;
        }

        NS_WARNING_ASSERTION!(ok, "failed to shape text - expect garbled text");
        ok
    }

    // Gecko: gfx/thebes/gfxFont.cpp gfxFont::SplitAndInitTextRun
    // NOT PORTED: SpaceMayParticipateInShaping (fonts with space lookups; always false here, so the word
    // path is always taken); text-perf counters.
    fn SplitAndInitTextRun<T: TextFrameChar>(
        self: &Rc<Self>,
        aDrawTarget: Option<&DrawTarget>,
        aTextRun: &mut gfxTextRun,
        aString: &[T],
        aRunStart: u32,
        aRunLength: u32,
        aRunScript: Script,
        aLanguage: Option<nsAtom>,
        aOrientation: ShapedTextFlags,
    ) -> bool {
        if aRunLength == 0 {
            return true;
        }

        let rounding = self.GetRoundOffsetsToPixels(aDrawTarget);

        let wordCacheCharLimit: u32 = WordCacheCharLimit();

        let vertical: bool = aOrientation == ShapedTextFlags::TEXT_ORIENT_VERTICAL_UPRIGHT;

        // the only flags we care about for ShapedWord construction/caching
        let mut flags: ShapedTextFlags = aTextRun.GetFlags();
        flags &= ShapedTextFlags::TEXT_IS_RTL | ShapedTextFlags::TEXT_DISABLE_OPTIONAL_LIGATURES | ShapedTextFlags::TEXT_USE_MATH_SCRIPT | ShapedTextFlags::TEXT_ORIENT_MASK;
        if !T::IS_WIDE {
            flags |= ShapedTextFlags::TEXT_IS_8BIT;
        }

        let mut wordStart: u32 = 0;
        let mut hash: u32 = 0;
        let mut wordIs8Bit = true;
        let appUnitsPerDevUnit: i32 = aTextRun.GetAppUnitsPerDevUnit();

        let mut nextCh: T = aString[0];
        let mut i: u32 = 0;
        while i <= aRunLength {
            let ch: T = nextCh;
            nextCh = if i + 1 < aRunLength { aString[i as usize + 1] } else { T::from_ascii(b'\n') };
            let boundary: u32 = IsBoundarySpace(ch, nextCh);
            let invalid: bool = boundary == 0 && gfxFontGroup_IsInvalidChar(ch);
            let length: u32 = i - wordStart;

            // break into separate ShapedWords when we hit an invalid char,
            // or a boundary space (always handled individually),
            // or the first non-space after a space
            if boundary == 0 && !invalid {
                if ch.to_u32() > 0xff {
                    wordIs8Bit = false;
                }
                // include this character in the hash, and move on to next
                hash = gfxShapedWord::HashMix(hash, ch.to_u32());
                i += 1;
                continue;
            }

            // We've decided to break here (i.e. we're at the end of a "word");
            // shape the word and add it to the textrun.
            // For words longer than the limit, we don't use the
            // font's word cache but just shape directly into the textrun.
            if length > wordCacheCharLimit {
                let ok = self.ShapeFragmentWithoutWordCache(
                    &aString[wordStart as usize..], aRunStart + wordStart, length, aRunScript, aLanguage, vertical, rounding, aTextRun,
                );
                if !ok {
                    return false;
                }
            } else if length > 0 {
                let mut wordFlags = flags;
                // in the 8-bit version of this method, TEXT_IS_8BIT was
                // already set as part of |flags|, so no need for a per-word
                // adjustment here
                if T::IS_WIDE && wordIs8Bit {
                    wordFlags |= ShapedTextFlags::TEXT_IS_8BIT;
                }
                let word16: Vec<u16> = aString[wordStart as usize..(wordStart + length) as usize].iter().map(|c| c.to_u32() as u16).collect();
                let offset = aRunStart + wordStart;
                let processed = ProcessShapedWordInternal(
                    self, &word16, hash, aRunScript, aLanguage, vertical, appUnitsPerDevUnit, wordFlags, rounding,
                    |aShapedWord| aTextRun.CopyGlyphDataFrom__gfxShapedWord_ptr_uint32_t(aShapedWord, offset),
                );
                if !processed {
                    return false; // failed, presumably out of memory?
                }
            }

            if boundary != 0 {
                // word was terminated by a space: add that to the textrun
                MOZ_ASSERT!(
                    aOrientation != ShapedTextFlags::TEXT_ORIENT_VERTICAL_MIXED,
                    "text-orientation:mixed should be resolved earlier"
                );
                if boundary != ' ' as u32 || !aTextRun.SetSpaceGlyphIfSimple(self, aRunStart + i, ch.to_u32() as u16, aOrientation) {
                    // Currently, the only "boundary" characters we recognize are
                    // space and no-break space, which are both 8-bit, so we force
                    // that flag (below). If we ever change IsBoundarySpace, we
                    // may need to revise this.
                    let boundaryChar: [u16; 1] = [boundary as u16];
                    let offset = aRunStart + i;
                    let processed = ProcessShapedWordInternal(
                        self, &boundaryChar, boundary, aRunScript, aLanguage, vertical, appUnitsPerDevUnit,
                        flags | ShapedTextFlags::TEXT_IS_8BIT, rounding,
                        |aShapedWord| aTextRun.CopyGlyphDataFrom__gfxShapedWord_ptr_uint32_t(aShapedWord, offset),
                    );
                    if !processed {
                        return false;
                    }
                    if boundary == ' ' as u32 {
                        aTextRun.GetCharacterGlyphs_mut()[(aRunStart + i) as usize].SetIsSpace();
                    }
                }
                hash = 0;
                wordStart = i + 1;
                wordIs8Bit = true;
                i += 1;
                continue;
            }

            if i == aRunLength {
                break;
            }

            NS_ASSERTION!(invalid, "how did we get here except via an invalid char?");

            // word was terminated by an invalid char: skip it,
            // unless it's a control char that we want to show as a hexbox,
            // but record where TAB or NEWLINE occur
            let c = ch.to_u32();
            if c == '\t' as u32 {
                aTextRun.SetIsTab(aRunStart + i);
            } else if c == '\n' as u32 {
                aTextRun.SetIsNewline(aRunStart + i);
            } else if UnicodeProperties::CharType(c) == GeneralCategory::Format {
                aTextRun.SetIsFormattingControl(aRunStart + i);
            } else if IsInvalidControlChar(c) && !(aTextRun.GetFlags() & ShapedTextFlags::TEXT_HIDE_CONTROL_CHARACTERS).to_bool() {
                // NOT PORTED: user fonts (`GetFontEntry()->IsUserFont() && HasCharacter(ch)`)
                aTextRun.SetMissingGlyph(aRunStart + i, c, self);
            }

            hash = 0;
            wordStart = i + 1;
            wordIs8Bit = true;
            i += 1;
        }

        true
    }

    // Gecko: gfx/thebes/gfxHarfBuzzShaper.cpp gfxHarfBuzzShaper::SetGlyphsFromRun
    // ADAPTER: HarfBuzz is run by mrt-text (gfxFont::ShapeText), which hands over the glyphs and the
    // clumps (the character-to-glyph groups) that this function walks in Gecko; the glyph records are
    // then built from them as Gecko builds them. Unrounded advances (roundI is false), horizontal only.
    fn SetGlyphsFromRun(
        &self,
        aShapedText: &mut gfxShapedText,
        aOffset: u32,
        aLength: u32,
        aText: &[u16],
        aRun: &ShapedRun,
    ) {
        let wordLength: u32 = aLength;
        let appUnitsPerDevUnit: i32 = aShapedText.GetAppUnitsPerDevUnit();
        // factor to convert 16.16 fixed-point pixels to app units
        let hb2appUnits: f64 = appUnitsPerDevUnit as f64 / 65536.0;
        let mut detailedGlyphs: Vec<gfxShapedText_DetailedGlyph> = Vec::new();

        for clump in aRun.mClumps.iter() {
            let mut glyphStart: u32 = clump.mGlyphStart;
            let glyphEnd: u32 = clump.mGlyphEnd;
            let mut baseCharIndex: i32 = clump.mCharStart as i32;
            let endCharIndex: i32 = clump.mCharEnd as i32;
            let mut bPos: nscoord = 0;

            // Now we're ready to set the glyph info in the textRun
            let glyphsInClump: i32 = (glyphEnd - glyphStart) as i32;

            // Check for default-ignorable char that didn't get filtered, combined,
            // etc by the shaping process, and remove from the run.
            // (This may be done within harfbuzz eventually.)
            if glyphsInClump == 1
                && baseCharIndex + 1 == endCharIndex
                && aShapedText.FilterIfIgnorable(aOffset + baseCharIndex as u32, aText[baseCharIndex as usize] as u32)
            {
                continue;
            }

            let glyph = &aRun.mGlyphs[glyphStart as usize];
            let i_offset = glyph.x_offset;
            let i_advance = glyph.x_advance;
            let mut b_offset = glyph.y_offset;
            let mut b_advance = glyph.y_advance;
            let mut iOffset: nscoord = (hb2appUnits * i_offset as f64 + 0.5).floor() as nscoord;
            let mut advance: nscoord = (hb2appUnits * i_advance as f64 + 0.5).floor() as nscoord;
            // Check if it's a simple one-to-one mapping
            if glyphsInClump == 1
                && gfxShapedText_CompressedGlyph::IsSimpleGlyphID(glyph.glyph)
                && gfxShapedText_CompressedGlyph::IsSimpleAdvance(advance as u32)
                && advance >= 0
                && aShapedText.mCharacterGlyphs[(aOffset as i32 + baseCharIndex) as usize].IsClusterStart()
                && iOffset == 0
                && b_offset == 0
                && b_advance == 0
                && bPos == 0
            {
                aShapedText.mCharacterGlyphs[(aOffset as i32 + baseCharIndex) as usize].SetSimpleGlyph(advance as u32, glyph.glyph);
            } else {
                // Collect all glyphs in a list to be assigned to the first char;
                // there must be at least one in the clump, and we already measured
                // its advance, hence the placement of the loop-exit test and the
                // measurement of the next glyph.
                loop {
                    let g = &aRun.mGlyphs[glyphStart as usize];
                    let mut details = gfxShapedText_DetailedGlyph::default();
                    details.mGlyphID = g.glyph;

                    details.mAdvance = advance;

                    details.mOffset.x = iOffset as f32;
                    details.mOffset.y = (bPos - (hb2appUnits * b_offset as f64 + 0.5).floor() as nscoord) as f32;
                    detailedGlyphs.push(details);

                    if b_advance != 0 {
                        bPos -= (hb2appUnits * b_advance as f64 + 0.5).floor() as nscoord;
                    }
                    glyphStart += 1;
                    if glyphStart >= glyphEnd {
                        break;
                    }

                    let g = &aRun.mGlyphs[glyphStart as usize];
                    let i_offset = g.x_offset;
                    let i_advance = g.x_advance;
                    b_offset = g.y_offset;
                    b_advance = g.y_advance;

                    iOffset = (hb2appUnits * i_offset as f64 + 0.5).floor() as nscoord;
                    advance = (hb2appUnits * i_advance as f64 + 0.5).floor() as nscoord;
                }

                aShapedText.SetDetailedGlyphs(aOffset + baseCharIndex as u32, detailedGlyphs.len() as u32, &detailedGlyphs);

                detailedGlyphs.clear();
            }

            // the rest of the chars in the group are ligature continuations,
            // no associated glyphs
            loop {
                baseCharIndex += 1;
                if !(baseCharIndex != endCharIndex && baseCharIndex < wordLength as i32) {
                    break;
                }
                let g = &mut aShapedText.mCharacterGlyphs[(aOffset as i32 + baseCharIndex) as usize];
                NS_ASSERTION!(!g.IsSimpleGlyph(), "overwriting a simple glyph");
                let cs = g.IsClusterStart();
                g.SetComplex(cs, false);
            }
        }
    }

    // Gecko: gfx/thebes/gfxFont.cpp gfxFont::GetRoundOffsetsToPixels
    // NOT PORTED: rounding (Firefox on Linux positions glyphs at subpixels, which is what mrt-text does).
    fn GetRoundOffsetsToPixels(&self, aDrawTarget: Option<&DrawTarget>) -> gfxFontShaper_RoundingFlags {
        gfxFontShaper_RoundingFlags::default()
    }
}

#[cfg(test)]
mod tests {
    // Not a gtest translation: gfx/tests/gtest has no test for CompressedGlyph. Written from the
    // documented behaviour of the glyph records.
    use super::*;

    #[test]
    fn compressed_glyph_simple_roundtrip() {
        let mut g = gfxShapedText_CompressedGlyph::MakeSimpleGlyph(480, 77);
        assert!(g.IsSimpleGlyph() && g.IsClusterStart() && g.IsLigatureGroupStart());
        assert_eq!((g.GetSimpleAdvance(), g.GetSimpleGlyph()), (480, 77));
        assert_eq!(g.CanBreakBefore(), gfxShapedText_CompressedGlyph::FLAG_BREAK_TYPE_NONE);
        assert_ne!(g.SetCanBreakBefore(gfxShapedText_CompressedGlyph::FLAG_BREAK_TYPE_NORMAL), 0);
        assert_eq!(g.SetCanBreakBefore(gfxShapedText_CompressedGlyph::FLAG_BREAK_TYPE_NORMAL), 0);
        assert!(g.IsSimpleGlyphNoBreakBefore() == false);
        g.SetIsSpace();
        assert!(g.CharIsSpace());
        // a break flag and the space flag survive SetSimpleGlyph
        g.SetSimpleGlyph(100, 3);
        assert!(g.CharIsSpace() && g.CanBreakBefore() == 1);
        assert_eq!(g.GetSimpleAdvance(), 100);
        assert!(!gfxShapedText_CompressedGlyph::IsSimpleAdvance(4096));
        assert!(gfxShapedText_CompressedGlyph::IsSimpleAdvance(4095));
    }

    #[test]
    fn compressed_glyph_complex_and_ligature_continuation() {
        let mut g = gfxShapedText_CompressedGlyph::default();
        assert!(g.IsMissing());
        g.SetComplex(true, false);
        assert!(g.IsLigatureContinuation() && g.IsClusterStart() && !g.IsLigatureGroupStart());
        g.SetGlyphCount(2);
        assert_eq!(g.GetGlyphCount(), 2);
        g.SetIsTab();
        assert!(g.CharIsTab());
        assert_eq!(g.CharTypeFlags(), gfxShapedText_CompressedGlyph::FLAG_CHAR_IS_TAB);
    }

    #[test]
    fn cluster_boundaries_mark_spaces_and_combining_marks() {
        let text: Vec<u16> = "a b\u{0301}c".encode_utf16().collect();
        let mut st = gfxShapedText::new(text.len() as u32, ShapedTextFlags::default(), 60);
        st.SetupClusterBoundaries__uint32_t_char16_t_ptr_uint32_t(0, &text, text.len() as u32);
        let g = st.GetCharacterGlyphs();
        assert!(g[1].CharIsSpace() && !g[0].CharIsSpace());
        // the combining acute (index 3) continues the cluster of 'b'
        assert!(g[3].IsMissing() == false && !g[3].IsClusterStart());
        assert!(g[4].CanBreakBefore() == 0);
    }

    #[test]
    fn emergency_wrap_after_hyphen() {
        let text: Vec<u16> = "ab-cd".encode_utf16().collect();
        let mut st = gfxShapedText::new(text.len() as u32, ShapedTextFlags::default(), 60);
        st.SetupClusterBoundaries__uint32_t_char16_t_ptr_uint32_t(0, &text, text.len() as u32);
        assert_eq!(st.GetCharacterGlyphs()[3].CanBreakBefore(), gfxShapedText_CompressedGlyph::FLAG_BREAK_TYPE_EMERGENCY_WRAP);
    }

    #[test]
    fn detailed_glyph_store_random_access() {
        let mut st = gfxShapedText::new(10, ShapedTextFlags::default(), 60);
        for (i, n) in [(2u32, 1u32), (5, 2), (7, 1)] {
            let d = st.AllocateDetailedGlyphs(i, n);
            for (k, g) in d.iter_mut().enumerate() {
                g.mGlyphID = i * 10 + k as u32;
            }
        }
        for (i, n) in [(2usize, 1u32), (5, 2), (7, 1)] {
            st.mCharacterGlyphs[i].SetComplex(true, true);
            st.mCharacterGlyphs[i].SetGlyphCount(n);
        }
        let d = st.GetDetailedGlyphs(5);
        assert_eq!((d[0].mGlyphID, d[1].mGlyphID), (50, 51));
        assert_eq!(st.GetDetailedGlyphs(2)[0].mGlyphID, 20);
        assert_eq!(st.GetDetailedGlyphs(7)[0].mGlyphID, 70);
    }
}
