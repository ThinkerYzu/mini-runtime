// Gecko: gfx/thebes/gfxTextRun.h, gfx/thebes/gfxTextRun.cpp (REDUCED). gfxFontGroup is declared and
// defined in the same two files in Gecko, so it lives here, after gfxTextRun.
//
// Text phase 2a: the part of gfxTextRun and gfxFontGroup that wrapping a paragraph needs --
// MakeTextRun (UTF-16 or 8-bit text, shaped by mrt-text), the per-character glyph records, break
// flags (SetPotentialLineBreaks), advances (GetAdvanceWidth, MeasureText) and BreakAndMeasureText.
//
// Text runs are shared by frames and the line breaker's sink, and break flags are set after creation,
// so Gecko's `RefPtr<gfxTextRun>` is `RefPtr_gfxTextRun` = `Rc<RefCell<gfxTextRun>>` here: readers
// `borrow()`, SetPotentialLineBreaks and the setters `borrow_mut()`.
//
// NOT PORTED: refcount logging; drawing (Draw, DrawPartialLigature, DrawEmphasisMarks, DrawGlyphs,
// gfxTextRunDrawCallbacks, DrawParams); glyph extents (FetchGlyphExtents, NeedsGlyphExtents); the
// text-run subclasses that override the virtuals (nsTransformedTextRun, nsCaseTransformTextRunFactory,
// MathMLTextRunFactory); user fonts and @font-face; font fallback (a text run uses the first font of
// its group only, so a character that font lacks shows that font's .notdef glyph, with its width);
// script itemizing (the whole string is one script run, Script::OTHER); numeral transformation
// (bidi.numeral is "nominal"); small caps and sub/superscript variant fallback (no font features);
// vertical text (the orientation values stay, only horizontal metrics exist); hyphenation of text
// (the provider can still return hyphen types; auto-hyphenation is off in nsLineBreaker); memory
// reporting (SizeOf*); Dump; the text-perf counters; justification.
use crate::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

// Container for either a single element of type T, or an nsTArray<T>.
// Using this instead of a simple nsTArray avoids an extra allocation in the
// common case where no more than one element is ever added to the list.
// Gecko: gfx/thebes/gfxUtils.h mozilla::ElementOrArray
// ADAPTER: a Rust enum instead of the tagged union; the array is a Vec. An empty list is an empty
// Array, as in Gecko. Gecko's nsTArray grows by powers of two; here AppendElement grows the Vec
// by exactly one element (glyph runs are few) so that capacity == length.
pub enum ElementOrArray<T> {
    Element(T),
    Array(Vec<T>),
}

impl<T> ElementOrArray<T> {
    // Construct as an empty array.
    pub fn new() -> Self {
        ElementOrArray::Array(Vec::new())
    }

    // gfxTextRun::SortGlyphRuns and SanitizeGlyphRuns directly access the array.
    pub fn Array(&mut self) -> &mut Vec<T> {
        match self {
            ElementOrArray::Array(a) => a,
            ElementOrArray::Element(_) => panic!("ElementOrArray::Array on an Element"),
        }
    }

    pub fn Length(&self) -> usize {
        match self {
            ElementOrArray::Element(_) => 1,
            ElementOrArray::Array(a) => a.len(),
        }
    }

    pub fn AppendElement(&mut self, aElement: T) {
        match self {
            ElementOrArray::Element(_) => {
                // Move the existing element into an array, then append the new one.
                let ElementOrArray::Element(temp) = std::mem::replace(self, ElementOrArray::Array(Vec::with_capacity(2))) else { unreachable!() };
                let ElementOrArray::Array(a) = self else { unreachable!() };
                a.push(temp);
                a.push(aElement);
            }
            ElementOrArray::Array(a) => {
                // If currently empty, just store the element directly.
                if a.is_empty() {
                    *self = ElementOrArray::Element(aElement);
                    return;
                }
                // Otherwise, append it to the array.
                a.reserve_exact(1);
                a.push(aElement);
            }
        }
    }

    pub fn LastElement(&self) -> &T {
        match self {
            ElementOrArray::Element(e) => e,
            ElementOrArray::Array(a) => a.last().unwrap(),
        }
    }

    pub fn LastElement_mut(&mut self) -> &mut T {
        match self {
            ElementOrArray::Element(e) => e,
            ElementOrArray::Array(a) => a.last_mut().unwrap(),
        }
    }

    pub fn IsEmpty(&self) -> bool {
        match self {
            ElementOrArray::Element(_) => false,
            ElementOrArray::Array(a) => a.is_empty(),
        }
    }

    pub fn TruncateLength(&mut self, aLength: usize) {
        MOZ_ASSERT!(aLength <= self.Length());
        match self {
            ElementOrArray::Element(_) => {
                if aLength == 0 {
                    // Destroy the single element, and convert to an empty array.
                    *self = ElementOrArray::Array(Vec::new());
                }
            }
            ElementOrArray::Array(a) => a.truncate(aLength),
        }
    }

    pub fn Clear(&mut self) {
        *self = ElementOrArray::Array(Vec::new());
    }

    // Convert from Array to Element storage. Only to be used when the current
    // state is a single-element array!
    pub fn ConvertToElement(&mut self) {
        let ElementOrArray::Array(a) = self else { panic!("ConvertToElement on an Element") };
        MOZ_ASSERT!(a.len() == 1);
        let temp = a.pop().unwrap();
        *self = ElementOrArray::Element(temp);
    }

    // Gecko: begin() and end(), the range for range-for loops.
    pub fn as_slice(&self) -> &[T] {
        match self {
            ElementOrArray::Element(e) => std::slice::from_ref(e),
            ElementOrArray::Array(a) => a,
        }
    }

    // ADAPTER: Rust has no ShallowSizeOfExcludingThis; the heap bytes of the array (instrumentation).
    pub fn ArrayCapacity(&self) -> usize {
        match self {
            ElementOrArray::Element(_) => 0,
            ElementOrArray::Array(a) => a.capacity(),
        }
    }
}

impl<T> std::ops::Index<usize> for ElementOrArray<T> {
    type Output = T;
    fn index(&self, aIndex: usize) -> &T {
        &self.as_slice()[aIndex]
    }
}

impl<T> std::ops::IndexMut<usize> for ElementOrArray<T> {
    fn index_mut(&mut self, aIndex: usize) -> &mut T {
        match self {
            ElementOrArray::Element(e) => {
                MOZ_ASSERT!(aIndex == 0);
                e
            }
            ElementOrArray::Array(a) => &mut a[aIndex],
        }
    }
}

// Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::GetAdvanceForGlyph etc. use nscoord; the shared
// pointer to a text run:
pub type RefPtr_gfxTextRun = Rc<RefCell<gfxTextRun>>;

// ADAPTER (generated by cbindgen from Stylo in Gecko, no header): mozilla::StyleFontFamilyList
pub type StyleFontFamilyList = mrt_style::style::values::computed::font::FontFamilyList;

// ADAPTER (generated by cbindgen from Stylo in Gecko, no header): mozilla::StyleHyphens
// (the `hyphens` property is not in the servo configuration of Stylo).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StyleHyphens {
    None,
    Manual,
    Auto,
}

// Gecko: gfx/thebes/gfxTextRun.h gfxBreakPriority
// (moved here from nsLineLayoutAdapter.rs, where nsLineLayout.rs got it before gfxTextRun existed)
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum gfxBreakPriority {
    eNoBreak = 0,
    eWordWrapBreak,
    eNormalBreak,
}

// Struct used by BreakAndMeasureText to return the amount of trimmable
// trailing whitespace included in the run.
// Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::TrimmableWS
// (moved here from nsLineLayoutAdapter.rs; mAdvance is f64 where Gecko has gfx::Float)
#[derive(Clone, Copy, Debug, Default)]
pub struct gfxTextRun_TrimmableWS {
    pub mAdvance: f64,
    pub mCount: u32,
}

// All offsets are in terms of the string passed into MakeTextRun.

// Describe range [start, end) of a text run. The range is
// restricted to grapheme cluster boundaries.
// Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::Range
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct gfxTextRun_Range {
    pub start: u32,
    pub end: u32,
}

impl gfxTextRun_Range {
    pub fn Length(&self) -> u32 {
        self.end - self.start
    }

    // Gecko: Range(uint32_t aStart, uint32_t aEnd)
    pub fn new(aStart: u32, aEnd: u32) -> gfxTextRun_Range {
        gfxTextRun_Range { start: aStart, end: aEnd }
    }
    // Gecko: explicit Range(const gfxTextRun* aTextRun)
    pub fn new__gfxTextRun_ptr(aTextRun: &gfxTextRun) -> gfxTextRun_Range {
        gfxTextRun_Range::new(0, aTextRun.GetLength())
    }

    pub fn Intersects(&self, aOther: &gfxTextRun_Range) -> bool {
        self.start < aOther.end && self.end > aOther.start
    }
}

// Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::HyphenType
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum gfxTextRun_HyphenType {
    // Code in BreakAndMeasureText depends on the ordering of these values!
    #[default]
    None,
    Explicit,
    Soft,
    AutoWithManualInSameWord,
    AutoWithoutManualInSameWord,
}

// Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::IsOptionalHyphenBreak
pub fn gfxTextRun_IsOptionalHyphenBreak(aType: gfxTextRun_HyphenType) -> bool {
    aType >= gfxTextRun_HyphenType::Soft
}

// Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::HyphenationState
#[derive(Clone, Copy, Debug, Default)]
pub struct gfxTextRun_HyphenationState {
    pub mostRecentBoundary: u32,
    pub hasManualHyphen: bool,
    pub hasExplicitHyphen: bool,
    pub hasAutoHyphen: bool,
}

/**
 * Layout provides PropertyProvider objects. These allow detection of
 * potential line break points and computation of spacing. We pass the data
 * this way to allow lazy data acquisition; for example BreakAndMeasureText
 * will want to only ask for properties of text it's actually looking at.
 *
 * NOTE that requested spacing may not actually be applied, if the textrun
 * is unable to apply it in some context. Exception: spacing around a
 * whitespace character MUST always be applied.
 */
// Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::PropertyProvider
// (the abstract class is a trait; `gfxFont::Spacing` is gfxFont_Spacing)
pub trait gfxTextRun_PropertyProvider {
    // Detect hyphenation break opportunities in the given range; breaks
    // not at cluster boundaries will be ignored.
    fn GetHyphenationBreaks(&self, aRange: gfxTextRun_Range, aBreakBefore: &mut [gfxTextRun_HyphenType]);

    // Returns the provider's hyphenation setting, so callers can decide
    // whether it is necessary to call GetHyphenationBreaks.
    // Result is an StyleHyphens value.
    fn GetHyphensOption(&self) -> StyleHyphens;

    // Returns the extra width that will be consumed by a hyphen. This should
    // be constant for a given textrun.
    fn GetHyphenWidth(&self) -> gfxFloat;

    // Return orientation flags to be used when creating a hyphen textrun.
    fn GetShapedTextFlags(&self) -> ShapedTextFlags;

    /**
     * Get the spacing around the indicated characters. Spacing must be zero
     * inside clusters. In other words, if character i is not
     * CLUSTER_START, then character i-1 must have zero after-spacing and
     * character i must have zero before-spacing.
     * Returns true if there is (or may be) any custom spacing; false if we
     * are sure that aSpacing contains only zero values.
     */
    fn GetSpacing(&self, aRange: gfxTextRun_Range, aSpacing: &mut [gfxFont_Spacing]) -> bool;

    // Returns a gfxContext that can be used to measure the hyphen glyph.
    // Only called if the hyphen width is requested.
    fn GetDrawTarget(&self) -> Option<DrawTarget>;

    // Return the appUnitsPerDevUnit value to be used when measuring.
    // Only called if the hyphen width is requested.
    fn GetAppUnitsPerDevUnit(&self) -> u32;

    fn LetterSpacing(&self) -> nscoord;
}

// Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::SuppressBreak
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum gfxTextRun_SuppressBreak {
    eNoSuppressBreak,
    // Measure the range of text as if there is no break before it.
    eSuppressInitialBreak,
    // Measure the range of text as if it contains no break
    eSuppressAllBreaks,
}

// The text is divided into GlyphRuns as necessary. (In the vast majority
// of cases, a gfxTextRun contains just a single GlyphRun.)
// Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::GlyphRun
#[derive(Clone)]
pub struct gfxTextRun_GlyphRun {
    pub mFont: Rc<gfxFont>,    // never null in a valid GlyphRun
    pub mCharacterOffset: u32, // into original UTF16 string
    pub mOrientation: ShapedTextFlags, // gfxTextRunFactory::TEXT_ORIENT_* value
    pub mMatchType: FontMatchType,
    pub mIsCJK: bool, // Whether the text was a CJK script run (used to decide if
                      // text-decoration-skip-ink should not be applied)
}

impl gfxTextRun_GlyphRun {
    // Set up the properties (but NOT offset) of the GlyphRun.
    pub fn SetProperties(&mut self, aFont: &Rc<gfxFont>, aOrientation: ShapedTextFlags, aIsCJK: bool, aMatchType: FontMatchType) {
        self.mFont = aFont.clone();
        self.mOrientation = aOrientation;
        self.mIsCJK = aIsCJK;
        self.mMatchType = aMatchType;
    }

    // Return whether the GlyphRun matches the given properties;
    // the given FontMatchType will be added to the run if not present.
    pub fn Matches(&mut self, aFont: &Rc<gfxFont>, aOrientation: ShapedTextFlags, aIsCJK: bool, aMatchType: FontMatchType) -> bool {
        if Rc::ptr_eq(&self.mFont, aFont) && self.mOrientation == aOrientation && self.mIsCJK == aIsCJK {
            self.mMatchType.kind |= aMatchType.kind;
            if self.mMatchType.generic == StyleGenericFontFamily::None {
                self.mMatchType.generic = aMatchType.generic;
            }
            return true;
        }
        false
    }

    pub fn IsSidewaysLeft(&self) -> bool {
        (self.mOrientation & ShapedTextFlags::TEXT_ORIENT_MASK) == ShapedTextFlags::TEXT_ORIENT_VERTICAL_SIDEWAYS_LEFT
    }

    pub fn IsSidewaysRight(&self) -> bool {
        (self.mOrientation & ShapedTextFlags::TEXT_ORIENT_MASK) == ShapedTextFlags::TEXT_ORIENT_VERTICAL_SIDEWAYS_RIGHT
    }
}

// Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::GlyphRunIterator
// (the C++ pointer into mGlyphRuns is an index; None is nullptr)
pub struct gfxTextRun_GlyphRunIterator<'a> {
    mTextRun: &'a gfxTextRun,
    mGlyphRun: Option<usize>,
    mStringStart: u32,
    mStringEnd: u32,
    mStartOffset: u32,
    mEndOffset: u32,
    mReverse: bool,
}

impl<'a> gfxTextRun_GlyphRunIterator<'a> {
    // Gecko: GlyphRunIterator(const gfxTextRun* aTextRun, Range aRange, bool aReverse = false)
    pub fn new(aTextRun: &'a gfxTextRun, aRange: gfxTextRun_Range, aReverse: bool) -> Self {
        let mut it = gfxTextRun_GlyphRunIterator {
            mTextRun: aTextRun,
            mGlyphRun: None,
            mStringStart: 0,
            mStringEnd: 0,
            mStartOffset: aRange.start,
            mEndOffset: aRange.end,
            mReverse: aReverse,
        };
        it.mGlyphRun = aTextRun.FindFirstGlyphRunContaining(if aReverse { aRange.end.wrapping_sub(1) } else { aRange.start });
        let Some(run) = it.mGlyphRun else {
            it.mStringStart = it.mStartOffset;
            it.mStringEnd = it.mStartOffset;
            return it;
        };
        let glyphRunEndOffset: u32 = if run == it.mTextRun.mGlyphRuns.Length() - 1 {
            it.mTextRun.GetLength()
        } else {
            it.mTextRun.mGlyphRuns[run + 1].mCharacterOffset
        };
        it.mStringEnd = it.mEndOffset.min(glyphRunEndOffset);
        it.mStringStart = it.mStartOffset.max(it.mTextRun.mGlyphRuns[run].mCharacterOffset);
        it
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::GlyphRunIterator::NextRun
    pub fn NextRun(&mut self) {
        let mut run = self.mGlyphRun.unwrap();
        if self.mReverse {
            if run == 0 {
                self.mGlyphRun = None;
                return;
            }
            run -= 1;
        } else {
            MOZ_ASSERT!(run != self.mTextRun.mGlyphRuns.Length());
            run += 1;
            if run == self.mTextRun.mGlyphRuns.Length() {
                self.mGlyphRun = None;
                return;
            }
        }
        self.mGlyphRun = Some(run);
        if self.mTextRun.mGlyphRuns[run].mCharacterOffset >= self.mEndOffset {
            self.mGlyphRun = None;
            return;
        }
        let glyphRunEndOffset: u32 = if run == self.mTextRun.mGlyphRuns.Length() - 1 {
            self.mTextRun.GetLength()
        } else {
            self.mTextRun.mGlyphRuns[run + 1].mCharacterOffset
        };
        if glyphRunEndOffset < self.mStartOffset {
            self.mGlyphRun = None;
            return;
        }
        self.mStringEnd = self.mEndOffset.min(glyphRunEndOffset);
        self.mStringStart = self.mStartOffset.max(self.mTextRun.mGlyphRuns[run].mCharacterOffset);
    }
    pub fn AtEnd(&self) -> bool {
        self.mGlyphRun.is_none()
    }
    pub fn GlyphRun(&self) -> &'a gfxTextRun_GlyphRun {
        &self.mTextRun.mGlyphRuns[self.mGlyphRun.unwrap()]
    }
    pub fn StringStart(&self) -> u32 {
        self.mStringStart
    }
    pub fn StringEnd(&self) -> u32 {
        self.mStringEnd
    }
}

// Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::LigatureData
pub struct gfxTextRun_LigatureData {
    // textrun range of the containing ligature
    pub mRange: gfxTextRun_Range,
    // appunits advance to the start of the ligature part within the ligature;
    // never includes any spacing
    pub mPartAdvance: gfxFloat,
    // appunits width of the ligature part; includes before-spacing
    // when the part is at the start of the ligature, and after-spacing
    // when the part is as the end of the ligature
    pub mPartWidth: gfxFloat,

    pub mClipBeforePart: bool,
    pub mClipAfterPart: bool,
}

// shaping state - for some font features, fallback is required that
// affects the entire run. for example, fallback for one script/font
// portion of a textrun requires fallback to be applied to the entire run
// Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::ShapingState
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum gfxTextRun_ShapingState {
    eShapingState_Normal,              // default state
    eShapingState_ShapingWithFeature,  // have shaped with feature
    eShapingState_ShapingWithFallback, // have shaped with fallback
    eShapingState_Aborted,             // abort initial iteration
    eShapingState_ForceFallbackFeature, // redo with fallback forced on
}

/**
 * gfxTextRun is an abstraction for drawing and measuring substrings of a run
 * of text. It stores runs of positioned glyph data, each run having a single
 * gfxFont. The glyphs are associated with a string of source text, and the
 * gfxTextRun APIs take parameters that are offsets into that source text.
 *
 * gfxTextRuns are mostly immutable. The only things that can change are
 * inter-cluster spacing and line break placement. Spacing is always obtained
 * lazily by methods that need it, it is not cached. Line breaks are stored
 * persistently (insofar as they affect the shaping of glyphs; gfxTextRun does
 * not actually do anything to explicitly account for line breaks). Initially
 * there are no line breaks. The textrun can record line breaks before or after
 * any given cluster. (Line breaks specified inside clusters are ignored.)
 *
 * It is important that zero-length substrings are handled correctly. This will
 * be on the test!
 */
// The user data of a text run (Gecko: `void* mUserData`, set by nsTextFrame.cpp CreateUserData):
// None is nullptr; SimpleFlow is the nsTextFrame* of a run that maps to only one flow (the
// IsSimpleFlow flag); Flows is the TextRunUserData of a run that maps to several flows. The enum
// is the size of its largest variant, so the table that held the user data outside the run is gone.
// Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::mUserData
pub enum gfxTextRun_UserData {
    None,
    SimpleFlow(nsTextFrame),
    Flows(TextRunUserData),
}

// Gecko: gfx/thebes/gfxTextRun.h gfxTextRun
// (a C++ subclass of gfxShapedText: the base is the field `base` and the struct Derefs to it)
pub struct gfxTextRun {
    base: gfxShapedText,

    // The textrun holds either a single GlyphRun -or- an array.
    mGlyphRuns: ElementOrArray<gfxTextRun_GlyphRun>,

    // ADAPTER: Gecko's `void* mUserData`, a pointer that nsTextFrame casts to the nsTextFrame of a
    // single-flow run or to a heap-allocated TextRunUserData; here the same two cases are an enum
    // stored in the run (see gfxTextRun_UserData below).
    mUserData: gfxTextRun_UserData,

    // mFontGroup is usually a strong reference, but refcounting is managed
    // manually because it may be explicitly released by ReleaseFontGroup()
    // in the case where the font group actually owns the textrun.
    mFontGroup: Rc<gfxFontGroup>,

    mSkipChars: gfxSkipChars,

    mFlags2: nsTextFrameUtils_Flags, // additional flags (see also gfxShapedText::mFlags)

    mDontSkipDrawing: bool, // true if the text run must not skip drawing, even if
                            // waiting for a user font download, e.g. because we
                            // are using it to draw canvas text

    // shaping state for handling variant fallback features
    // such as subscript/superscript variant glyphs
    mShapingState: gfxTextRun_ShapingState,

    // ADAPTER: nsTransformedTextRun::mCapitalize (layout/generic/nsTextRunTransformations.h), the
    // characters text-transform: capitalize makes title case, as the line breaker marked them;
    // kept on every run since the port makes no transformed runs (boxed: one pointer a run). Read by
    // the rendered text only (nsTextFrame's TransformChars); layout itself does not transform.
    mCapitalize: Option<Box<Vec<bool>>>,
}

impl Deref for gfxTextRun {
    type Target = gfxShapedText;
    fn deref(&self) -> &gfxShapedText {
        &self.base
    }
}
impl DerefMut for gfxTextRun {
    fn deref_mut(&mut self) -> &mut gfxShapedText {
        &mut self.base
    }
}

impl gfxTextRun {
    // Instrumentation (bench/textrun_profile.py): the sizes of the parts of this run.
    // (shaped-text stats, glyph runs len, glyph runs capacity, size_of GlyphRun, skip chars stats,
    // size_of gfxTextRun, address of the first glyph run's font, length in chars, Rc strong count of the font group)
    pub fn MemStats(&self) -> ([usize; 11], usize, usize, usize, (usize, usize, usize), usize, usize, u32) {
        (
            self.base.MemStats(), self.mGlyphRuns.Length(), self.mGlyphRuns.ArrayCapacity(), std::mem::size_of::<gfxTextRun_GlyphRun>(),
            self.mSkipChars.MemStats(), std::mem::size_of::<gfxTextRun>(),
            self.mGlyphRuns.as_slice().first().map_or(0, |g| Rc::as_ptr(&g.mFont) as usize), self.GetLength(),
        )
    }

    // Public textrun API for general use

    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::IsClusterStart
    pub fn IsClusterStart(&self, aPos: u32) -> bool {
        MOZ_ASSERT!(aPos < self.GetLength());
        self.mCharacterGlyphs[aPos as usize].IsClusterStart()
    }
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::IsLigatureGroupStart
    pub fn IsLigatureGroupStart(&self, aPos: u32) -> bool {
        MOZ_ASSERT!(aPos < self.GetLength());
        self.mCharacterGlyphs[aPos as usize].IsLigatureGroupStart()
    }
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::CanBreakLineBefore
    pub fn CanBreakLineBefore(&self, aPos: u32) -> bool {
        self.CanBreakBefore(aPos) == gfxShapedText_CompressedGlyph::FLAG_BREAK_TYPE_NORMAL
    }
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::CanHyphenateBefore
    pub fn CanHyphenateBefore(&self, aPos: u32) -> bool {
        self.CanBreakBefore(aPos) == gfxShapedText_CompressedGlyph::FLAG_BREAK_TYPE_HYPHEN
    }

    // Returns a gfxShapedText::CompressedGlyph::FLAG_BREAK_TYPE_* value
    // as defined in gfxFont.h (may be NONE, NORMAL, HYPHEN or EMERGENCY_WRAP).
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::CanBreakBefore
    pub fn CanBreakBefore(&self, aPos: u32) -> u8 {
        MOZ_ASSERT!(aPos < self.GetLength());
        self.mCharacterGlyphs[aPos as usize].CanBreakBefore()
    }

    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::CharIsSpace
    pub fn CharIsSpace(&self, aPos: u32) -> bool {
        MOZ_ASSERT!(aPos < self.GetLength());
        self.mCharacterGlyphs[aPos as usize].CharIsSpace()
    }
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::CharIsTab
    pub fn CharIsTab(&self, aPos: u32) -> bool {
        MOZ_ASSERT!(aPos < self.GetLength());
        self.mCharacterGlyphs[aPos as usize].CharIsTab()
    }
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::CharIsNewline
    pub fn CharIsNewline(&self, aPos: u32) -> bool {
        MOZ_ASSERT!(aPos < self.GetLength());
        self.mCharacterGlyphs[aPos as usize].CharIsNewline()
    }
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::CharMayHaveEmphasisMark
    pub fn CharMayHaveEmphasisMark(&self, aPos: u32) -> bool {
        MOZ_ASSERT!(aPos < self.GetLength());
        self.mCharacterGlyphs[aPos as usize].CharMayHaveEmphasisMark()
    }
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::CharIsFormattingControl
    pub fn CharIsFormattingControl(&self, aPos: u32) -> bool {
        MOZ_ASSERT!(aPos < self.GetLength());
        self.mCharacterGlyphs[aPos as usize].CharIsFormattingControl()
    }

    // All coordinates are in layout/app units

    /**
     * Set the potential linebreaks for a substring of the textrun. These are
     * the "allow break before" points. Initially, there are no potential
     * linebreaks.
     *
     * This can change glyphs and/or geometry! Some textruns' shapes
     * depend on potential line breaks (e.g., title-case-converting textruns).
     * This function is virtual so that those textruns can reshape themselves.
     *
     * @return true if this changed the linebreaks, false if the new line
     * breaks are the same as the old
     */
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::SetPotentialLineBreaks
    // (virtual; the overriding nsTransformedTextRun is NOT PORTED)
    pub fn SetPotentialLineBreaks(&mut self, aRange: gfxTextRun_Range, aBreakBefore: &[u8]) -> bool {
        NS_ASSERTION!(aRange.end <= self.GetLength(), "Overflow");

        let mut changed: u32 = 0;
        let mut cg: usize = aRange.start as usize;
        let end: usize = cg + aRange.Length() as usize;
        let mut breakIndex = 0;
        while cg < end {
            let mut canBreak: u8 = aBreakBefore[breakIndex];
            breakIndex += 1;
            if canBreak != 0 && !self.mCharacterGlyphs[cg].IsClusterStart() {
                // XXX If we replace the line-breaker with one based more closely
                // on UAX#14 (e.g. using ICU), this may not be needed any more.
                // Avoid possible breaks inside a cluster, EXCEPT when the previous
                // character was a space (compare UAX#14 rules LB9, LB10).
                if cg == 0 || !self.mCharacterGlyphs[cg - 1].CharIsSpace() {
                    canBreak = gfxShapedText_CompressedGlyph::FLAG_BREAK_TYPE_NONE;
                }
            }
            // If a break is allowed here, set the break flag, but don't clear a
            // possible pre-existing emergency-break flag already in the run.
            if canBreak != 0 {
                changed |= self.mCharacterGlyphs[cg].SetCanBreakBefore(canBreak);
            }
            cg += 1;
        }
        changed != 0
    }

    /**
     * Draws a substring. NOT PORTED: drawing.
     */
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::Draw
    pub fn Draw(&self, aRange: gfxTextRun_Range, aPt: Point) {
        unimplemented!("NOT PORTED: drawing")
    }

    /**
     * Draws the emphasis marks for this text run. NOT PORTED: drawing.
     */
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::DrawEmphasisMarks
    pub fn DrawEmphasisMarks(&self) {
        unimplemented!("NOT PORTED: drawing")
    }

    /**
     * Computes the ReflowMetrics for a substring.
     * Uses GetSpacing from aBreakProvider.
     * @param aBoundingBoxType which kind of bounding box (loose/tight)
     */
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::MeasureText(Range, ...)
    pub fn MeasureText__Range_BoundingBoxType_DrawTarget_ptr_PropertyProvider_ptr(
        &self,
        aRange: gfxTextRun_Range,
        aBoundingBoxType: gfxFont_BoundingBoxType,
        aRefDrawTarget: Option<&DrawTarget>,
        aProvider: Option<&dyn gfxTextRun_PropertyProvider>,
    ) -> gfxFont_RunMetrics {
        NS_ASSERTION!(aRange.end <= self.GetLength(), "Substring out of range");

        let mut accumulatedMetrics = gfxFont_RunMetrics::new();
        let mut iter = gfxTextRun_GlyphRunIterator::new(self, aRange, false);
        while !iter.AtEnd() {
            let font: &Rc<gfxFont> = &iter.GlyphRun().mFont;
            let start: u32 = iter.StringStart();
            let end: u32 = iter.StringEnd();
            let mut ligatureRange = gfxTextRun_Range::new(start, end);
            let adjusted: bool = self.ShrinkToLigatureBoundaries(&mut ligatureRange);

            if adjusted {
                self.AccumulatePartialLigatureMetrics(
                    font,
                    gfxTextRun_Range::new(start, ligatureRange.start),
                    aBoundingBoxType,
                    aRefDrawTarget,
                    aProvider,
                    iter.GlyphRun().mOrientation,
                    &mut accumulatedMetrics,
                );
            }

            // XXX This sucks. We have to get glyph extents just so we can detect
            // glyphs outside the font box, even when aBoundingBoxType is LOOSE,
            // even though in almost all cases we could get correct results just
            // by getting some ascent/descent from the font and using our stored
            // advance widths.
            self.AccumulateMetricsForRun(
                font,
                ligatureRange,
                aBoundingBoxType,
                aRefDrawTarget,
                aProvider,
                ligatureRange,
                iter.GlyphRun().mOrientation,
                &mut accumulatedMetrics,
            );

            if adjusted {
                self.AccumulatePartialLigatureMetrics(
                    font,
                    gfxTextRun_Range::new(ligatureRange.end, end),
                    aBoundingBoxType,
                    aRefDrawTarget,
                    aProvider,
                    iter.GlyphRun().mOrientation,
                    &mut accumulatedMetrics,
                );
            }
            iter.NextRun();
        }

        accumulatedMetrics
    }

    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::MeasureText(BoundingBoxType, ...)
    pub fn MeasureText__BoundingBoxType_DrawTarget_ptr_PropertyProvider_ptr(
        &self,
        aBoundingBoxType: gfxFont_BoundingBoxType,
        aDrawTargetForTightBoundingBox: Option<&DrawTarget>,
        aProvider: Option<&dyn gfxTextRun_PropertyProvider>,
    ) -> gfxFont_RunMetrics {
        self.MeasureText__Range_BoundingBoxType_DrawTarget_ptr_PropertyProvider_ptr(
            gfxTextRun_Range::new__gfxTextRun_ptr(self),
            aBoundingBoxType,
            aDrawTargetForTightBoundingBox,
            aProvider,
        )
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::GetLineHeightMetrics(Range, ...)
    pub fn GetLineHeightMetrics__Range_gfxFloat_ref_gfxFloat_ref(&self, aRange: gfxTextRun_Range, aAscent: &mut gfxFloat, aDescent: &mut gfxFloat) {
        let mut accumulatedMetrics = gfxFont_RunMetrics::new();
        let mut iter = gfxTextRun_GlyphRunIterator::new(self, aRange, false);
        while !iter.AtEnd() {
            let font: &Rc<gfxFont> = &iter.GlyphRun().mFont;
            let metrics = font.Measure(
                self, 0, 0, gfxFont_BoundingBoxType::LOOSE_INK_EXTENTS, None, None, 0, iter.GlyphRun().mOrientation,
            );
            accumulatedMetrics.CombineWith(&metrics, false);
            iter.NextRun();
        }
        *aAscent = accumulatedMetrics.mAscent;
        *aDescent = accumulatedMetrics.mDescent;
    }

    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::GetLineHeightMetrics(gfxFloat&, gfxFloat&)
    pub fn GetLineHeightMetrics__gfxFloat_ref_gfxFloat_ref(&self, aAscent: &mut gfxFloat, aDescent: &mut gfxFloat) {
        self.GetLineHeightMetrics__Range_gfxFloat_ref_gfxFloat_ref(gfxTextRun_Range::new__gfxTextRun_ptr(self), aAscent, aDescent)
    }

    /**
     * Computes just the advance width for a substring.
     * Uses GetSpacing from aBreakProvider.
     * If aSpacing is not null, the spacing attached before and after
     * the substring would be returned in it. NOTE: the spacing is
     * included in the advance width.
     */
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::GetAdvanceWidth(Range, ...)
    pub fn GetAdvanceWidth__Range_PropertyProvider_ptr_Spacing_ptr(
        &self,
        aRange: gfxTextRun_Range,
        aProvider: Option<&dyn gfxTextRun_PropertyProvider>,
        aSpacing: Option<&mut gfxFont_Spacing>,
    ) -> gfxFloat {
        NS_ASSERTION!(aRange.end <= self.GetLength(), "Substring out of range");

        let mut ligatureRange = aRange;
        let adjusted: bool = self.ShrinkToLigatureBoundaries(&mut ligatureRange);

        let mut result: gfxFloat = if adjusted {
            self.ComputePartialLigatureWidth(gfxTextRun_Range::new(aRange.start, ligatureRange.start), aProvider)
                + self.ComputePartialLigatureWidth(gfxTextRun_Range::new(ligatureRange.end, aRange.end), aProvider)
        } else {
            0.0
        };

        let mut aSpacing = aSpacing;
        if let Some(s) = aSpacing.as_deref_mut() {
            s.mBefore = 0;
            s.mAfter = 0;
        }

        // Account for all remaining spacing here. This is more efficient than
        // processing it along with the glyphs.
        if let Some(provider) = aProvider {
            if (self.mFlags & ShapedTextFlags::TEXT_ENABLE_SPACING).to_bool() {
                let mut spacingBuffer: Vec<gfxFont_Spacing> = vec![gfxFont_Spacing::default(); aRange.Length() as usize];
                if GetAdjustedSpacing(self, ligatureRange, provider, &mut spacingBuffer) {
                    for i in 0..ligatureRange.Length() as usize {
                        let space = &spacingBuffer[i];
                        result += (space.mBefore + space.mAfter) as f64;
                    }
                    if let Some(s) = aSpacing.as_deref_mut() {
                        s.mBefore = spacingBuffer[0].mBefore;
                        s.mAfter = spacingBuffer[ligatureRange.Length() as usize - 1].mAfter;
                    }
                }
            }
        }

        result + self.GetAdvanceForGlyphs(ligatureRange, aProvider.map_or(0, |p| p.LetterSpacing())) as f64
    }

    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::GetAdvanceWidth()
    pub fn GetAdvanceWidth__(&self) -> gfxFloat {
        self.GetAdvanceWidth__Range_PropertyProvider_ptr_Spacing_ptr(gfxTextRun_Range::new__gfxTextRun_ptr(self), None, None)
    }

    /**
     * Computes the minimum advance width for a substring assuming line
     * breaking is allowed everywhere.
     */
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::GetMinAdvanceWidth
    pub fn GetMinAdvanceWidth(&self, aRange: gfxTextRun_Range, aLetterSpacing: nscoord) -> gfxFloat {
        MOZ_ASSERT!(aRange.end <= self.GetLength(), "Substring out of range");

        let mut ligatureRange = aRange;
        let adjusted: bool = self.ShrinkToLigatureBoundaries(&mut ligatureRange);

        let mut result: gfxFloat = if adjusted {
            self.ComputePartialLigatureWidth(gfxTextRun_Range::new(aRange.start, ligatureRange.start), None)
                .max(self.ComputePartialLigatureWidth(gfxTextRun_Range::new(ligatureRange.end, aRange.end), None))
        } else {
            0.0
        };

        // Compute min advance width by assuming each grapheme cluster takes its own
        // line.
        let mut clusterAdvance: gfxFloat = 0.0;
        for i in ligatureRange.start..ligatureRange.end {
            if self.mCharacterGlyphs[i as usize].CharIsSpace() {
                // Skip space char to prevent its advance width contributing to the
                // result. That is, don't consider a space can be in its own line.
                continue;
            }
            clusterAdvance += self.GetAdvanceForGlyph(i, aLetterSpacing) as f64;
            if i + 1 == ligatureRange.end || self.IsClusterStart(i + 1) {
                result = result.max(clusterAdvance);
                clusterAdvance = 0.0;
            }
        }

        result
    }

    /**
     * Clear all stored line breaks for the given range (both before and after),
     * and then set the line-break state before aRange.start to aBreakBefore and
     * after the last cluster to aBreakAfter.
     *
     * We require that before and after line breaks be consistent. For clusters
     * i and i+1, we require that if there is a break after cluster i, a break
     * will be specified before cluster i+1. This may be temporarily violated
     * (e.g. after reflowing line L and before reflowing line L+1); to handle
     * these temporary violations, we say that there is a break betwen i and i+1
     * if a break is specified after i OR a break is specified before i+1.
     *
     * This can change textrun geometry! The existence of a linebreak can affect
     * the advance width of the cluster before the break (when kerning) or the
     * geometry of one cluster before the break or any number of clusters
     * after the break. (The one-cluster-before-the-break limit is somewhat
     * arbitrary; if some scripts require breaking it, then we need to
     * alter nsTextFrame::TrimTrailingWhitespace, perhaps drastically becase
     * it could affect the layout of frames before it...)
     *
     * We return true if glyphs or geometry changed, false otherwise. This
     * function is virtual so that gfxTextRun subclasses can reshape
     * properly.
     *
     * @param aAdvanceWidthDelta if non-null, returns the change in advance
     * width of the given range.
     */
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::SetLineBreaks
    pub fn SetLineBreaks(&mut self, aRange: gfxTextRun_Range, aLineBreakBefore: bool, aLineBreakAfter: bool, aAdvanceWidthDelta: Option<&mut gfxFloat>) -> bool {
        // Do nothing because our shaping does not currently take linebreaks into
        // account. There is no change in advance width.
        if let Some(delta) = aAdvanceWidthDelta {
            *delta = 0.0;
        }
        false
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::ClassifyAutoHyphenations
    pub fn ClassifyAutoHyphenations(
        &self,
        aStart: u32,
        aRange: gfxTextRun_Range,
        aHyphenBuffer: &mut Vec<gfxTextRun_HyphenType>,
        aWordState: &mut gfxTextRun_HyphenationState,
    ) {
        MOZ_ASSERT!(
            aRange.end - aStart <= aHyphenBuffer.len() as u32 && aRange.start >= aStart,
            "Range out of bounds"
        );
        MOZ_ASSERT!(aWordState.mostRecentBoundary >= aStart, "Unexpected aMostRecentWordBoundary!!");

        let start: u32 = aRange.start.min(aWordState.mostRecentBoundary);

        for i in start..aRange.end {
            if aHyphenBuffer[(i - aStart) as usize] == gfxTextRun_HyphenType::Explicit && !aWordState.hasExplicitHyphen {
                aWordState.hasExplicitHyphen = true;
            }
            if !aWordState.hasManualHyphen
                && (aHyphenBuffer[(i - aStart) as usize] == gfxTextRun_HyphenType::Soft
                    || aHyphenBuffer[(i - aStart) as usize] == gfxTextRun_HyphenType::Explicit)
            {
                aWordState.hasManualHyphen = true;
                // This is the first manual hyphen in the current word. We can only
                // know if the current word has a manual hyphen until now. So, we need
                // to run a sub loop to update the auto hyphens between the start of
                // the current word and this manual hyphen.
                if aWordState.hasAutoHyphen {
                    for j in aWordState.mostRecentBoundary..i {
                        if aHyphenBuffer[(j - aStart) as usize] == gfxTextRun_HyphenType::AutoWithoutManualInSameWord {
                            aHyphenBuffer[(j - aStart) as usize] = gfxTextRun_HyphenType::AutoWithManualInSameWord;
                        }
                    }
                }
            }
            if aHyphenBuffer[(i - aStart) as usize] == gfxTextRun_HyphenType::AutoWithoutManualInSameWord {
                if !aWordState.hasAutoHyphen {
                    aWordState.hasAutoHyphen = true;
                }
                if aWordState.hasManualHyphen {
                    aHyphenBuffer[(i - aStart) as usize] = gfxTextRun_HyphenType::AutoWithManualInSameWord;
                }
            }

            // If we're at the word boundary, clear/reset couple states.
            if self.mCharacterGlyphs[i as usize].CharIsSpace()
                || self.mCharacterGlyphs[i as usize].CharIsTab()
                || self.mCharacterGlyphs[i as usize].CharIsNewline()
                // Since we will not have a boundary in the end of the string, let's
                // call the end of the string a special case for word boundary.
                || i == self.GetLength() - 1
            {
                // We can only get to know whether we should raise/clear an explicit
                // manual hyphen until we get to the end of a word, because this depends
                // on whether there exists at least one auto hyphen in the same word.
                if !aWordState.hasAutoHyphen && aWordState.hasExplicitHyphen {
                    for j in aWordState.mostRecentBoundary..=i {
                        if aHyphenBuffer[(j - aStart) as usize] == gfxTextRun_HyphenType::Explicit {
                            aHyphenBuffer[(j - aStart) as usize] = gfxTextRun_HyphenType::None;
                        }
                    }
                }
                aWordState.mostRecentBoundary = i;
                aWordState.hasManualHyphen = false;
                aWordState.hasAutoHyphen = false;
                aWordState.hasExplicitHyphen = false;
            }
        }
    }

    /**
     * Finds the longest substring that will fit into the given width.
     * Uses GetHyphenationBreaks and GetSpacing from aProvider.
     * Guarantees the following:
     * -- 0 <= result <= aMaxLength
     * -- result is the maximal value of N such that either
     *       N < aMaxLength && line break at N &&
     *       GetAdvanceWidth(Range(aStart, N), aProvider) <= aWidth
     *   OR  N < aMaxLength && hyphen break at N &&
     *       GetAdvanceWidth(Range(aStart, N), aProvider) +
     *       GetHyphenWidth() <= aWidth
     *   OR  N == aMaxLength &&
     *       GetAdvanceWidth(Range(aStart, N), aProvider) <= aWidth
     * where GetAdvanceWidth assumes the effect of
     * SetLineBreaks(Range(aStart, N),
     *               aLineBreakBefore, N < aMaxLength, aProvider)
     * -- if no such N exists, then result is the smallest N such that
     *       N < aMaxLength && line break at N
     *   OR  N < aMaxLength && hyphen break at N
     *   OR  N == aMaxLength
     *
     * The call has the effect of
     * SetLineBreaks(Range(aStart, result), aLineBreakBefore,
     *               result < aMaxLength, aProvider)
     * and the returned metrics and the invariants above reflect this.
     *
     * @param aMaxLength this can be UINT32_MAX, in which case the length used
     * is up to the end of the string
     * @param aLineBreakBefore set to true if and only if there is an actual
     * line break at the start of this string.
     * @param aSuppressBreak what break should be suppressed.
     * @param aOutTrimmableWhitespace if non-null, returns the advance of any
     * run of trailing spaces that might be trimmed if the run ends up at
     * end-of-line.
     * Trimmable spaces are still counted in the "characters fit" result, and
     * contribute to the returned Metrics values.
     * @param aOutMetrics we fill this in for the returned substring.
     * If a hyphenation break was used, the hyphen is NOT included in the returned
     * metrics.
     * @param aBoundingBoxType whether to make the bounding box in aMetrics tight
     * @param aRefDrawTarget a reference DrawTarget to get the tight bounding box,
     * if requested
     * @param aOutUsedHyphenation records if we selected a hyphenation break
     * @param aOutLastBreak if result is aMaxLength, we set this to
     * the maximal N such that
     *       N < aMaxLength && line break at N &&
     *       GetAdvanceWidth(Range(aStart, N), aProvider) <= aWidth
     *   OR  N < aMaxLength && hyphen break at N &&
     *       GetAdvanceWidth(Range(aStart, N), aProvider) +
     *         GetHyphenWidth() <= aWidth
     * or UINT32_MAX if no such N exists, where GetAdvanceWidth assumes
     * the effect of
     * SetLineBreaks(Range(aStart, N), aLineBreakBefore,
     *               N < aMaxLength, aProvider)
     *
     * @param aCanWordWrap true if we can break between any two grapheme
     * clusters. This is set by overflow-wrap|word-wrap: break-word
     *
     * @param aBreakPriority in/out the priority of the break opportunity
     * saved in the line. If we are prioritizing break opportunities, we will
     * not set a break with a lower priority. @see gfxBreakPriority.
     *
     * Note that negative advance widths are possible especially if negative
     * spacing is provided.
     */
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::BreakAndMeasureText
    pub fn BreakAndMeasureText(
        &self,
        aStart: u32,
        aMaxLength: u32,
        aLineBreakBefore: bool,
        aWidth: gfxFloat,
        aProvider: &dyn gfxTextRun_PropertyProvider,
        aSuppressBreak: gfxTextRun_SuppressBreak,
        aBoundingBoxType: gfxFont_BoundingBoxType,
        aRefDrawTarget: Option<&DrawTarget>,
        aCanWordWrap: bool,
        aCanWhitespaceWrap: bool,
        aIsBreakSpaces: bool,
        // Output parameters:
        aOutTrimmableWhitespace: Option<&mut gfxTextRun_TrimmableWS>, // may be null
        aOutMetrics: &mut gfxFont_RunMetrics,
        aOutUsedHyphenation: &mut bool,
        aOutLastBreak: &mut u32,
        // In/out:
        aBreakPriority: &mut gfxBreakPriority,
    ) -> u32 {
        use gfxTextRun_SuppressBreak::*;
        let aMaxLength = aMaxLength.min(self.GetLength() - aStart);

        NS_ASSERTION!(aStart + aMaxLength <= self.GetLength(), "Substring out of range");

        const kMeasurementBufferSize: u32 = 100;
        let mut bufferRange = gfxTextRun_Range::new(aStart, aStart + aMaxLength.min(kMeasurementBufferSize));
        let mut spacingBuffer: [gfxFont_Spacing; kMeasurementBufferSize as usize] = [gfxFont_Spacing::default(); kMeasurementBufferSize as usize];
        let haveSpacing: bool = (self.mFlags & ShapedTextFlags::TEXT_ENABLE_SPACING).to_bool();
        if haveSpacing {
            GetAdjustedSpacing(self, bufferRange, aProvider, &mut spacingBuffer);
        }
        let mut hyphenBuffer: Vec<gfxTextRun_HyphenType> = Vec::new();
        let mut wordState = gfxTextRun_HyphenationState::default();
        wordState.mostRecentBoundary = aStart;
        let mut haveHyphenation: bool = aProvider.GetHyphensOption() == StyleHyphens::Auto
            || (aProvider.GetHyphensOption() == StyleHyphens::Manual && (self.mFlags & ShapedTextFlags::TEXT_ENABLE_HYPHEN_BREAKS).to_bool());
        if haveHyphenation {
            hyphenBuffer.resize(bufferRange.Length() as usize, gfxTextRun_HyphenType::None);
            aProvider.GetHyphenationBreaks(bufferRange, &mut hyphenBuffer[..]);
            if aProvider.GetHyphensOption() == StyleHyphens::Auto {
                self.ClassifyAutoHyphenations(aStart, bufferRange, &mut hyphenBuffer, &mut wordState);
            }
        }

        let mut width: gfxFloat = 0.0;
        let mut advance: gfxFloat = 0.0;
        // The number of space characters that can be trimmed or hang at a soft-wrap
        let mut trimmableChars: u32 = 0;
        // The amount of space removed by ignoring trimmableChars
        let mut trimmableAdvance: gfxFloat = 0.0;
        let mut lastBreak: i32 = -1;
        let mut lastBreakTrimmableChars: i32 = -1;
        let mut lastBreakTrimmableAdvance: gfxFloat = -1.0;
        // Cache the last candidate break
        let mut lastCandidateBreak: i32 = -1;
        let mut lastCandidateBreakTrimmableChars: i32 = -1;
        let mut lastCandidateBreakTrimmableAdvance: gfxFloat = -1.0;
        let mut lastCandidateBreakUsedHyphenation = false;
        let mut lastCandidateBreakPriority = gfxBreakPriority::eNoBreak;
        let mut aborted = false;
        let end: u32 = aStart + aMaxLength;
        let mut lastBreakUsedHyphenation = false;
        let mut ligatureRange = gfxTextRun_Range::new(aStart, end);
        self.ShrinkToLigatureBoundaries(&mut ligatureRange);

        // We may need to move `i` backwards in the following loop, and re-scan
        // part of the textrun; we'll use `rescanLimit` so we can tell when that
        // is happening: if `i < rescanLimit` then we're rescanning.
        let mut rescanLimit: u32 = aStart;
        let mut i: u32 = aStart;
        // (the C++ for loop's `++i` is the wrapping_add at the end of the body and before each
        // `continue`: `i` can be set to prevMostRecentWordBoundary - 1, which wraps when it is 0)
        while i < end {
            if i >= bufferRange.end {
                // Fetch more spacing and hyphenation data
                let oldHyphenBufferLength: u32 = hyphenBuffer.len() as u32;
                bufferRange.start = i;
                bufferRange.end = (aStart + aMaxLength).min(i + kMeasurementBufferSize);
                // For spacing, we always overwrite the old data with the newly
                // fetched one. However, for hyphenation, hyphenation data sometimes
                // depends on the context in every word (if "hyphens: auto" is set).
                // To ensure we get enough information between neighboring buffers,
                // we grow the hyphenBuffer instead of overwrite it.
                // NOTE that this means bufferRange does not correspond to the
                // entire hyphenBuffer, but only to the most recently added portion.
                // Therefore, we need to add the old length to hyphenBuffer.Elements()
                // when getting more data.
                if haveSpacing {
                    GetAdjustedSpacing(self, bufferRange, aProvider, &mut spacingBuffer);
                }
                if haveHyphenation {
                    hyphenBuffer.resize(oldHyphenBufferLength as usize + bufferRange.Length() as usize, gfxTextRun_HyphenType::None);
                    aProvider.GetHyphenationBreaks(bufferRange, &mut hyphenBuffer[oldHyphenBufferLength as usize..]);
                    if aProvider.GetHyphensOption() == StyleHyphens::Auto {
                        let prevMostRecentWordBoundary: u32 = wordState.mostRecentBoundary;
                        self.ClassifyAutoHyphenations(aStart, bufferRange, &mut hyphenBuffer, &mut wordState);
                        // If the buffer boundary is in the middle of a word,
                        // we need to go back to the start of the current word.
                        // So, we can correct the wrong candidates that we set
                        // in the previous runs of the loop.
                        if prevMostRecentWordBoundary < oldHyphenBufferLength {
                            rescanLimit = i;
                            i = prevMostRecentWordBoundary.wrapping_sub(1);
                            i = i.wrapping_add(1);
                            continue;
                        }
                    }
                }
            }

            // There can't be a word-wrap break opportunity at the beginning of the
            // line: if the width is too small for even one character to fit, it
            // could be the first and last break opportunity on the line, and that
            // would trigger an infinite loop.
            if aSuppressBreak != eSuppressAllBreaks && (aSuppressBreak != eSuppressInitialBreak || i > aStart) {
                let atNaturalBreak: bool = self.mCharacterGlyphs[i as usize].CanBreakBefore() == gfxShapedText_CompressedGlyph::FLAG_BREAK_TYPE_NORMAL;
                // atHyphenationBreak indicates we're at a "soft" hyphen, where an extra
                // hyphen glyph will need to be painted. It is NOT set for breaks at an
                // explicit hyphen present in the text.
                //
                // NOTE(emilio): If you change this condition you also need to change
                // nsTextFrame::AddInlineMinISizeForFlow to match.
                let atHyphenationBreak: bool = !atNaturalBreak
                    && haveHyphenation
                    && (!aLineBreakBefore || i > aStart)
                    && gfxTextRun_IsOptionalHyphenBreak(hyphenBuffer[(i - aStart) as usize]);
                let atAutoHyphenWithManualHyphenInSameWord: bool =
                    atHyphenationBreak && hyphenBuffer[(i - aStart) as usize] == gfxTextRun_HyphenType::AutoWithManualInSameWord;
                let atBreak: bool = atNaturalBreak || atHyphenationBreak;
                let wordWrapping: bool = (aCanWordWrap
                    || (aCanWhitespaceWrap
                        && self.mCharacterGlyphs[i as usize].CanBreakBefore() == gfxShapedText_CompressedGlyph::FLAG_BREAK_TYPE_EMERGENCY_WRAP))
                    && self.mCharacterGlyphs[i as usize].IsClusterStart()
                    && *aBreakPriority <= gfxBreakPriority::eWordWrapBreak;

                let mut whitespaceWrapping = false;
                if i > aStart {
                    // The spec says the breaking opportunity is *after* whitespace.
                    let g = &self.mCharacterGlyphs[i as usize - 1];
                    whitespaceWrapping = aIsBreakSpaces && (g.CharIsSpace() || g.CharIsTab() || g.CharIsNewline());
                }

                if atBreak || wordWrapping || whitespaceWrapping {
                    let mut hyphenatedAdvance: gfxFloat = advance;
                    if atHyphenationBreak {
                        hyphenatedAdvance += aProvider.GetHyphenWidth();
                    }

                    if lastBreak < 0 || width + hyphenatedAdvance - trimmableAdvance <= aWidth {
                        // We can break here.
                        lastBreak = i as i32;
                        lastBreakTrimmableChars = trimmableChars as i32;
                        lastBreakTrimmableAdvance = trimmableAdvance;
                        lastBreakUsedHyphenation = atHyphenationBreak;
                        *aBreakPriority = if atBreak || whitespaceWrapping {
                            gfxBreakPriority::eNormalBreak
                        } else {
                            gfxBreakPriority::eWordWrapBreak
                        };
                    }

                    width += advance;
                    advance = 0.0;
                    if width - trimmableAdvance > aWidth {
                        // No more text fits. Abort
                        aborted = true;
                        break;
                    }
                    // There are various kinds of break opportunities:
                    // 1. word wrap break,
                    // 2. natural break,
                    // 3. manual hyphenation break,
                    // 4. auto hyphenation break without any manual hyphenation
                    //    in the same word,
                    // 5. auto hyphenation break with another manual hyphenation
                    //    in the same word.
                    // Allow all of them except the last one to be a candidate.
                    // So, we can ensure that we don't use an automatic
                    // hyphenation opportunity within a word that contains another
                    // manual hyphenation, unless it is the only choice.
                    if wordWrapping || !atAutoHyphenWithManualHyphenInSameWord {
                        lastCandidateBreak = lastBreak;
                        lastCandidateBreakTrimmableChars = lastBreakTrimmableChars;
                        lastCandidateBreakTrimmableAdvance = lastBreakTrimmableAdvance;
                        lastCandidateBreakUsedHyphenation = lastBreakUsedHyphenation;
                        lastCandidateBreakPriority = *aBreakPriority;
                    }
                }
            }

            // If we're re-scanning part of a word (to re-process potential
            // hyphenation types) then we don't want to accumulate widths again
            // for the characters that were already added to `advance`.
            if i < rescanLimit {
                i = i.wrapping_add(1);
                continue;
            }

            let mut charAdvance: gfxFloat;
            if i >= ligatureRange.start && i < ligatureRange.end {
                charAdvance = self.GetAdvanceForGlyphs(gfxTextRun_Range::new(i, i + 1), aProvider.LetterSpacing()) as f64;
                if haveSpacing {
                    let space = &spacingBuffer[(i - bufferRange.start) as usize];
                    charAdvance += (space.mBefore + space.mAfter) as f64;
                }
            } else {
                charAdvance = self.ComputePartialLigatureWidth(gfxTextRun_Range::new(i, i + 1), Some(aProvider));
            }

            advance += charAdvance;
            if aOutTrimmableWhitespace.is_some() {
                if self.mCharacterGlyphs[i as usize].CharIsSpace() {
                    trimmableChars += 1;
                    trimmableAdvance += charAdvance;
                } else {
                    trimmableAdvance = 0.0;
                    trimmableChars = 0;
                }
            }
            i = i.wrapping_add(1);
        }

        if !aborted {
            width += advance;
        }

        // There are three possibilities:
        // 1) all the text fit (width <= aWidth)
        // 2) some of the text fit up to a break opportunity (width > aWidth &&
        //    lastBreak >= 0)
        // 3) none of the text fits before a break opportunity (width > aWidth &&
        //    lastBreak < 0)
        let charsFit: u32;
        *aOutUsedHyphenation = false;
        if width - trimmableAdvance <= aWidth {
            charsFit = aMaxLength;
        } else if lastBreak >= 0 {
            if lastCandidateBreak >= 0 && lastCandidateBreak != lastBreak {
                lastBreak = lastCandidateBreak;
                lastBreakTrimmableChars = lastCandidateBreakTrimmableChars;
                lastBreakTrimmableAdvance = lastCandidateBreakTrimmableAdvance;
                lastBreakUsedHyphenation = lastCandidateBreakUsedHyphenation;
                *aBreakPriority = lastCandidateBreakPriority;
            }
            charsFit = lastBreak as u32 - aStart;
            trimmableChars = lastBreakTrimmableChars as u32;
            trimmableAdvance = lastBreakTrimmableAdvance;
            *aOutUsedHyphenation = lastBreakUsedHyphenation;
        } else {
            charsFit = aMaxLength;
        }

        // Get the overall metrics of the range that fit (including any potentially
        // trimmable or hanging whitespace).
        *aOutMetrics = self.MeasureText__Range_BoundingBoxType_DrawTarget_ptr_PropertyProvider_ptr(
            gfxTextRun_Range::new(aStart, aStart + charsFit),
            aBoundingBoxType,
            aRefDrawTarget,
            Some(aProvider),
        );

        if let Some(ws) = aOutTrimmableWhitespace {
            ws.mAdvance = trimmableAdvance;
            ws.mCount = trimmableChars;
        }

        if charsFit == aMaxLength {
            if lastBreak < 0 {
                *aOutLastBreak = u32::MAX;
            } else {
                *aOutLastBreak = lastBreak as u32 - aStart;
            }
        }

        charsFit
    }

    // Utility getters

    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::GetUserData
    pub fn GetUserData(&self) -> &gfxTextRun_UserData {
        &self.mUserData
    }
    pub fn GetUserData_mut(&mut self) -> &mut gfxTextRun_UserData {
        &mut self.mUserData
    }
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::SetUserData
    pub fn SetUserData(&mut self, aUserData: gfxTextRun_UserData) {
        self.mUserData = aUserData;
    }

    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::SetFlagBits
    pub fn SetFlagBits(&mut self, aFlags: nsTextFrameUtils_Flags) {
        self.mFlags2 |= aFlags;
    }
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::ClearFlagBits
    pub fn ClearFlagBits(&mut self, aFlags: nsTextFrameUtils_Flags) {
        self.mFlags2 &= !aFlags;
    }
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::GetSkipChars
    pub fn GetSkipChars(&self) -> &gfxSkipChars {
        &self.mSkipChars
    }
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::GetFontGroup
    pub fn GetFontGroup(&self) -> &Rc<gfxFontGroup> {
        &self.mFontGroup
    }

    // Call this, don't call "new gfxTextRun" directly. This does custom
    // allocation and initialization
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::Create
    // (the glyph storage is allocated by gfxShapedText::new and zeroed)
    pub fn Create(
        aParams: &mut gfxTextRunFactory_Parameters,
        aLength: u32,
        aFontGroup: &Rc<gfxFontGroup>,
        aFlags: ShapedTextFlags,
        aFlags2: nsTextFrameUtils_Flags,
    ) -> Option<RefPtr_gfxTextRun> {
        Some(Rc::new(RefCell::new(gfxTextRun::new(aParams, aLength, aFontGroup, aFlags, aFlags2))))
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::gfxTextRun
    fn new(
        aParams: &mut gfxTextRunFactory_Parameters,
        aLength: u32,
        aFontGroup: &Rc<gfxFontGroup>,
        aFlags: ShapedTextFlags,
        aFlags2: nsTextFrameUtils_Flags,
    ) -> gfxTextRun {
        let mut run = gfxTextRun {
            base: gfxShapedText::new(aLength, aFlags, aParams.mAppUnitsPerDevUnit as u16),
            mGlyphRuns: ElementOrArray::new(),
            mUserData: gfxTextRun_UserData::None,
            mFontGroup: aFontGroup.clone(),
            mSkipChars: gfxSkipChars::new(),
            mFlags2: aFlags2,
            mDontSkipDrawing: false,
            mShapingState: gfxTextRun_ShapingState::eShapingState_Normal,
            mCapitalize: None,
        };
        NS_ASSERTION!(run.GetAppUnitsPerDevUnit() > 0, "Invalid app unit scale");

        // NOT PORTED: refcount logging, text-perf counters.

        if let Some(skipChars) = aParams.mSkipChars.as_deref_mut() {
            run.mSkipChars.TakeFrom(skipChars);
        }

        run.mDontSkipDrawing = (aFlags2 & nsTextFrameUtils_Flags::DontSkipDrawingForPendingUserFonts).to_bool();
        run
    }

    // Script run codes that we will mark as CJK to suppress skip-ink behavior.
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::IsCJKScript
    // NOT PORTED: the Script enum here knows only the scripts the line breaker needs; only Hangul of
    // the CJK ones is among them.
    pub fn IsCJKScript(aScript: Script) -> bool {
        matches!(aScript, Script::HANGUL)
    }

    // API for setting up the textrun glyphs. Should only be called by
    // things that construct textruns.
    /**
     * We've found a run of text that should use a particular font. Call this
     * only during initialization when font substitution has been computed.
     * Call it before setting up the glyphs for the characters in this run;
     * SetMissingGlyph requires that the correct glyphrun be installed.
     *
     * If aForceNewRun, a new glyph run will be added, even if the
     * previously added run uses the same font.  If glyph runs are
     * added out of strictly increasing aStartCharIndex order (via
     * force), then SortGlyphRuns must be called after all glyph runs
     * are added before any further operations are performed with this
     * TextRun.
     */
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::AddGlyphRun
    pub fn AddGlyphRun(
        &mut self,
        aFont: &Rc<gfxFont>,
        aMatchType: FontMatchType,
        aUTF16Offset: u32,
        aForceNewRun: bool,
        aOrientation: ShapedTextFlags,
        aIsCJK: bool,
    ) {
        MOZ_ASSERT!(
            aOrientation != ShapedTextFlags::TEXT_ORIENT_VERTICAL_MIXED,
            "mixed orientation should have been resolved"
        );

        if self.mGlyphRuns.IsEmpty() {
            self.mGlyphRuns.AppendElement(gfxTextRun_GlyphRun {
                mFont: aFont.clone(),
                mCharacterOffset: aUTF16Offset,
                mOrientation: aOrientation,
                mMatchType: aMatchType,
                mIsCJK: aIsCJK,
            });
            return;
        }

        let numGlyphRuns: usize = self.mGlyphRuns.Length();
        if !aForceNewRun {
            let lastGlyphRun = self.mGlyphRuns.LastElement_mut();

            MOZ_ASSERT!(lastGlyphRun.mCharacterOffset <= aUTF16Offset, "Glyph runs out of order (and run not forced)");

            // Don't append a run if the font is already the one we want
            if lastGlyphRun.Matches(aFont, aOrientation, aIsCJK, aMatchType) {
                return;
            }

            // If the offset has not changed, avoid leaving a zero-length run
            // by overwriting the last entry instead of appending...
            if lastGlyphRun.mCharacterOffset == aUTF16Offset {
                // ...except that if the run before the last entry had the same
                // font as the new one wants, merge with it instead of creating
                // adjacent runs with the same font
                if numGlyphRuns > 1 && self.mGlyphRuns[numGlyphRuns - 2].Matches(aFont, aOrientation, aIsCJK, aMatchType) {
                    self.mGlyphRuns.TruncateLength(numGlyphRuns - 1);
                    return;
                }

                self.mGlyphRuns[numGlyphRuns - 1].SetProperties(aFont, aOrientation, aIsCJK, aMatchType);
                return;
            }
        }

        MOZ_ASSERT!(
            aForceNewRun || numGlyphRuns > 0 || aUTF16Offset == 0,
            "First run doesn't cover the first character (and run not forced)?"
        );

        self.mGlyphRuns.AppendElement(gfxTextRun_GlyphRun {
            mFont: aFont.clone(),
            mCharacterOffset: aUTF16Offset,
            mOrientation: aOrientation,
            mMatchType: aMatchType,
            mIsCJK: aIsCJK,
        });
    }

    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::ResetGlyphRuns
    pub fn ResetGlyphRuns(&mut self) {
        self.mGlyphRuns.Clear();
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::SanitizeGlyphRuns
    pub fn SanitizeGlyphRuns(&mut self) {
        if self.mGlyphRuns.Length() < 2 {
            return;
        }

        // ADAPTER: the runs are taken out of mGlyphRuns while they are rewritten (Gecko edits the
        // array in place through mGlyphRuns.Array()), and put back at the end.
        let mut runs: Vec<gfxTextRun_GlyphRun> = std::mem::take(self.mGlyphRuns.Array());

        // The runs are almost certain to be already sorted, so it's worth avoiding
        // the Sort() call if possible.
        let mut isSorted = true;
        let mut prevOffset: u32 = 0;
        for r in runs.iter() {
            if r.mCharacterOffset < prevOffset {
                isSorted = false;
                break;
            }
            prevOffset = r.mCharacterOffset;
        }
        if !isSorted {
            // GlyphRunOffsetComparator
            runs.sort_by_key(|r| r.mCharacterOffset);
        }

        // Coalesce adjacent glyph runs that have the same properties, and eliminate
        // any empty runs.
        // (RemoveElementsBy with a pointer to the previous kept run: the kept runs are collected in
        // `kept`, whose last element is prevRun)
        let length = self.GetLength();
        let all = runs;
        let mut kept: Vec<gfxTextRun_GlyphRun> = Vec::with_capacity(all.len());
        for mut aRun in all {
            // First run is always retained.
            let Some(prevRun) = kept.last_mut() else {
                kept.push(aRun);
                continue;
            };

            // Merge any run whose properties match its predecessor.
            if prevRun.Matches(&aRun.mFont, aRun.mOrientation, aRun.mIsCJK, aRun.mMatchType) {
                continue;
            }

            if prevRun.mCharacterOffset >= aRun.mCharacterOffset {
                // Preceding run is empty (or has become so due to the adjusting for
                // ligature boundaries), so we will overwrite it with this one, which
                // will then be discarded.
                *prevRun = aRun;
                continue;
            }

            // If any glyph run starts with ligature-continuation characters, we need to
            // advance it to the first "real" character to avoid drawing partial
            // ligature glyphs from wrong font (seen with U+FEFF in reftest 474417-1, as
            // Core Text eliminates the glyph, which makes it appear as if a ligature
            // has been formed)
            while aRun.mCharacterOffset < length && self.base.mCharacterGlyphs[aRun.mCharacterOffset as usize].IsLigatureContinuation() {
                aRun.mCharacterOffset += 1;
            }

            // We're keeping another run
            kept.push(aRun);
        }
        let mut runs = kept;

        // Drop any trailing empty run.
        if runs.len() > 1 && runs.last().unwrap().mCharacterOffset == length {
            runs.pop();
        }

        MOZ_ASSERT!(!runs.is_empty());
        if runs.len() == 1 {
            *self.mGlyphRuns.Array() = runs;
            self.mGlyphRuns.ConvertToElement();
        } else {
            // ADAPTER: capacity == length (the kept array was allocated for all the runs).
            runs.shrink_to_fit();
            *self.mGlyphRuns.Array() = runs;
        }
    }

    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::GetCharacterGlyphs
    // (the accessor is inherited from gfxShapedText here: self.GetCharacterGlyphs())

    // clean out results from shaping in progress, used for fallback scenarios
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::ClearGlyphsAndCharacters
    pub fn ClearGlyphsAndCharacters(&mut self) {
        self.ResetGlyphRuns();
        for g in self.base.mCharacterGlyphs.iter_mut() {
            *g = gfxShapedText_CompressedGlyph::default();
        }
        self.base.mDetailedGlyphs = None;
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::SetSpaceGlyph
    // NOT PORTED: ProcessSingleSpaceShapedWord (the word cache); the space is shaped directly.
    pub fn SetSpaceGlyph(&mut self, aFont: &Rc<gfxFont>, aDrawTarget: Option<&DrawTarget>, aCharIndex: u32, aOrientation: ShapedTextFlags) {
        if self.SetSpaceGlyphIfSimple(aFont, aCharIndex, ' ' as u16, aOrientation) {
            return;
        }

        let flags: ShapedTextFlags = ShapedTextFlags::TEXT_IS_8BIT | aOrientation;
        let vertical: bool = (self.GetFlags() & ShapedTextFlags::TEXT_ORIENT_VERTICAL_UPRIGHT).to_bool();
        let roundingFlags = aFont.GetRoundOffsetsToPixels(aDrawTarget);
        let prevRun = self.TrailingGlyphRun();
        let isCJK: bool = match prevRun {
            Some(p) if Rc::ptr_eq(&p.mFont, aFont) && p.mOrientation == aOrientation => p.mIsCJK,
            _ => false,
        };
        self.AddGlyphRun(aFont, FontMatchType::new(FontMatchType_Kind::kUnspecified), aCharIndex, false, aOrientation, isCJK);
        let space: [u16; 1] = [' ' as u16];
        self.base.SetupClusterBoundaries__uint32_t_char16_t_ptr_uint32_t(aCharIndex, &space, 1);
        aFont.ShapeText__char16_t_ptr_uint32_t_uint32_t_Script_nsAtom_ptr_bool_RoundingFlags_gfxShapedText_ptr(
            &space, aCharIndex, 1, Script::OTHER, None, vertical, roundingFlags, &mut self.base,
        );
        let _ = flags;
        self.base.mCharacterGlyphs[aCharIndex as usize].SetIsSpace();
    }

    // Set the glyph data for the given character index to the font's
    // space glyph, IF this can be done as a "simple" glyph record
    // (not requiring a DetailedGlyph entry). This avoids the need to call
    // the font shaper and go through the shaped-word cache for most spaces.
    //
    // The parameter aSpaceChar is the original character code for which
    // this space glyph is being used; if this is U+0020, we need to record
    // that it could be trimmed at a run edge, whereas other kinds of space
    // (currently just U+00A0) would not be trimmable/breakable.
    //
    // Returns true if it was able to set simple glyph data for the space;
    // if it returns false, the caller needs to fall back to some other
    // means to create the necessary (detailed) glyph data.
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::SetSpaceGlyphIfSimple
    pub fn SetSpaceGlyphIfSimple(&mut self, aFont: &Rc<gfxFont>, aCharIndex: u32, aSpaceChar: u16, aOrientation: ShapedTextFlags) -> bool {
        let spaceGlyph: u32 = aFont.GetSpaceGlyph();
        if spaceGlyph == 0 || !gfxShapedText_CompressedGlyph::IsSimpleGlyphID(spaceGlyph) {
            return false;
        }

        // (vertical metrics do not exist here: eHorizontal)
        let spaceWidthAppUnits: u32 = NS_lroundf((aFont.GetMetrics().spaceWidth * self.GetAppUnitsPerDevUnit() as f64) as f32) as u32;
        if !gfxShapedText_CompressedGlyph::IsSimpleAdvance(spaceWidthAppUnits) {
            return false;
        }

        let prevRun = self.TrailingGlyphRun();
        let isCJK: bool = match prevRun {
            Some(p) if Rc::ptr_eq(&p.mFont, aFont) && p.mOrientation == aOrientation => p.mIsCJK,
            _ => false,
        };
        self.AddGlyphRun(aFont, FontMatchType::new(FontMatchType_Kind::kUnspecified), aCharIndex, false, aOrientation, isCJK);
        let mut g = gfxShapedText_CompressedGlyph::MakeSimpleGlyph(spaceWidthAppUnits, spaceGlyph);
        if aSpaceChar == ' ' as u16 {
            g.SetIsSpace();
        }
        self.base.mCharacterGlyphs[aCharIndex as usize] = g;
        true
    }

    // Record the positions of specific characters that layout may need to
    // detect in the textrun, even though it doesn't have an explicit copy
    // of the original text. These are recorded using flag bits in the
    // CompressedGlyph record; if necessary, we convert "simple" glyph records
    // to "complex" ones as the Tab and Newline flags are not present in
    // simple CompressedGlyph records.
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::SetIsTab
    pub fn SetIsTab(&mut self, aIndex: u32) {
        self.EnsureComplexGlyph(aIndex).SetIsTab();
    }
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::SetIsNewline
    pub fn SetIsNewline(&mut self, aIndex: u32) {
        self.EnsureComplexGlyph(aIndex).SetIsNewline();
    }
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::SetNoEmphasisMark
    pub fn SetNoEmphasisMark(&mut self, aIndex: u32) {
        self.EnsureComplexGlyph(aIndex).SetNoEmphasisMark();
    }
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::SetIsFormattingControl
    pub fn SetIsFormattingControl(&mut self, aIndex: u32) {
        self.EnsureComplexGlyph(aIndex).SetIsFormattingControl();
    }

    /**
     * Prefetch all the glyph extents needed to ensure that Measure calls
     * on this textrun not requesting tight boundingBoxes will succeed.
     */
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::FetchGlyphExtents
    // NOT PORTED: glyph extents (nothing reads them: bounding boxes are the loose font box).
    pub fn FetchGlyphExtents(&self, aRefDrawTarget: Option<&DrawTarget>) {}

    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::GetGlyphRuns
    pub fn GetGlyphRuns(&self, aNumGlyphRuns: &mut u32) -> &[gfxTextRun_GlyphRun] {
        *aNumGlyphRuns = self.mGlyphRuns.Length() as u32;
        self.mGlyphRuns.as_slice()
    }

    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::GlyphRunCount
    pub fn GlyphRunCount(&self) -> u32 {
        self.mGlyphRuns.Length() as u32
    }

    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::TrailingGlyphRun
    pub fn TrailingGlyphRun(&self) -> Option<&gfxTextRun_GlyphRun> {
        if self.mGlyphRuns.IsEmpty() { None } else { Some(self.mGlyphRuns.LastElement()) }
    }

    // Returns the GlyphRun containing the given offset.
    // (Returns mGlyphRuns.end()-1 when aOffset is mCharacterCount; returns
    // nullptr if textrun is empty and no glyph runs are present.)
    // ADAPTER: the result is an index into mGlyphRuns.
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::FindFirstGlyphRunContaining
    pub fn FindFirstGlyphRunContaining(&self, aOffset: u32) -> Option<usize> {
        MOZ_ASSERT!(aOffset <= self.GetLength(), "Bad offset looking for glyphrun");
        MOZ_ASSERT!(self.GetLength() == 0 || !self.mGlyphRuns.IsEmpty(), "non-empty text but no glyph runs present!");
        if self.mGlyphRuns.Length() <= 1 {
            return if self.mGlyphRuns.IsEmpty() { None } else { Some(0) };
        }
        if aOffset == self.GetLength() {
            return Some(self.mGlyphRuns.Length() - 1);
        }
        let mut start: usize = 0;
        let mut limit: usize = self.mGlyphRuns.Length();
        while limit - start > 1 {
            let mid = start + (limit - start) / 2;
            if self.mGlyphRuns[mid].mCharacterOffset <= aOffset {
                start = mid;
            } else {
                limit = mid;
            }
        }
        MOZ_ASSERT!(
            self.mGlyphRuns[start].mCharacterOffset <= aOffset,
            "Hmm, something went wrong, aOffset should have been found"
        );
        Some(start)
    }

    // Copy glyph data from a ShapedWord into this textrun.
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::CopyGlyphDataFrom(gfxShapedWord*, uint32_t)
    pub fn CopyGlyphDataFrom__gfxShapedWord_ptr_uint32_t(&mut self, aShapedWord: &gfxShapedWord, aOffset: u32) {
        let mut aOffset = aOffset;
        let wordLen: u32 = aShapedWord.GetLength();
        MOZ_ASSERT!(aOffset + wordLen <= self.GetLength(), "word overruns end of textrun");

        let wordGlyphs = aShapedWord.mShapedText.GetCharacterGlyphs();
        if aShapedWord.mShapedText.HasDetailedGlyphs() {
            for i in 0..wordLen {
                let g = wordGlyphs[i as usize];
                if !g.IsSimpleGlyph() {
                    let details: &[gfxShapedText_DetailedGlyph] = if g.GetGlyphCount() > 0 { aShapedWord.mShapedText.GetDetailedGlyphs(i) } else { &[] };
                    self.SetDetailedGlyphs(aOffset, g.GetGlyphCount(), details);
                }
                self.GetCharacterGlyphs_mut()[aOffset as usize] = g;
                aOffset += 1;
            }
        } else {
            self.GetCharacterGlyphs_mut()[aOffset as usize..(aOffset + wordLen) as usize].copy_from_slice(&wordGlyphs[..wordLen as usize]);
        }
    }

    // Copy glyph data for a range of characters from aSource to this
    // textrun.
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::CopyGlyphDataFrom(gfxTextRun*, Range, uint32_t)
    // NOT PORTED: only nsTransformedTextRun and the ellipsis/hyphen helpers use it.
    pub fn CopyGlyphDataFrom__gfxTextRun_ptr_Range_uint32_t(&mut self) {
        unimplemented!("NOT PORTED: copying glyph data between text runs")
    }

    // Tell the textrun to release its reference to its creating gfxFontGroup
    // immediately, rather than on destruction.
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::ReleaseFontGroup
    // NOT PORTED: font group caches of text runs do not exist here; there is no reference cycle.
    pub fn ReleaseFontGroup(&mut self) {}

    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::GetFlags2
    pub fn GetFlags2(&self) -> nsTextFrameUtils_Flags {
        self.mFlags2
    }

    // Gecko: layout/generic/nsTextRunTransformations.cpp nsTransformedTextRun::SetCapitalization
    // (mNeedsRebuild is not kept: nothing is rebuilt, see mCapitalize)
    pub fn SetCapitalization(&mut self, aStart: u32, aLength: u32, aCapitalization: &[bool]) {
        let length = self.GetLength() as usize;
        let capitalize = self.mCapitalize.get_or_insert_with(|| Box::new(vec![false; length]));
        capitalize[aStart as usize..(aStart + aLength) as usize].copy_from_slice(&aCapitalization[..aLength as usize]);
    }

    // ADAPTER: nsTransformedTextRun::mCapitalize[aOffset], false past its end or when it is empty
    pub fn Capitalize(&self, aOffset: u32) -> bool {
        self.mCapitalize.as_ref().is_some_and(|c| c.get(aOffset as usize).copied().unwrap_or(false))
    }

    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::GetShapingState
    pub fn GetShapingState(&self) -> gfxTextRun_ShapingState {
        self.mShapingState
    }
    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::SetShapingState
    pub fn SetShapingState(&mut self, aShapingState: gfxTextRun_ShapingState) {
        self.mShapingState = aShapingState;
    }

    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::GetAdvanceForGlyph
    pub fn GetAdvanceForGlyph(&self, aIndex: u32, aLetterSpacing: nscoord) -> nscoord {
        let glyphData = &self.mCharacterGlyphs[aIndex as usize];
        if glyphData.IsSimpleGlyph() {
            return glyphData.GetSimpleAdvance() as nscoord;
        }
        let mut glyphCount: u32 = glyphData.GetGlyphCount();
        if glyphCount == 0 {
            return 0;
        }
        let details = self.GetDetailedGlyphs(aIndex);
        let mut advance: nscoord = 0;
        if glyphData.ApplyLetterSpacingBetweenDetailedGlyphs() {
            advance += (glyphCount as i32 - 1) * aLetterSpacing;
        }
        let mut k = 0;
        while glyphCount > 0 {
            glyphCount -= 1;
            advance += details[k].mAdvance;
            k += 1;
        }
        advance
    }

    // Gecko: gfx/thebes/gfxTextRun.h gfxTextRun::EnsureComplexGlyph
    fn EnsureComplexGlyph(&mut self, aIndex: u32) -> &mut gfxShapedText_CompressedGlyph {
        self.base.EnsureComplexGlyph(aIndex)
    }

    // **** general helpers ****

    // Get the total advance for a range of glyphs.
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::GetAdvanceForGlyphs
    fn GetAdvanceForGlyphs(&self, aRange: gfxTextRun_Range, aLetterSpacing: nscoord) -> i32 {
        let mut advance: i32 = 0;
        for i in aRange.start..aRange.end {
            advance += self.GetAdvanceForGlyph(i, aLetterSpacing);
        }
        advance
    }

    // Spacing for characters outside the range aSpacingStart/aSpacingEnd
    // is assumed to be zero; such characters are not passed to aProvider.
    // This is useful to protect aProvider from being passed character indices
    // it is not currently able to handle.
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::GetAdjustedSpacingArray
    fn GetAdjustedSpacingArray(
        &self,
        aRange: gfxTextRun_Range,
        aProvider: Option<&dyn gfxTextRun_PropertyProvider>,
        aSpacingRange: gfxTextRun_Range,
        aSpacing: &mut Vec<gfxFont_Spacing>,
    ) -> bool {
        let Some(aProvider) = aProvider else { return false };
        if !(self.mFlags & ShapedTextFlags::TEXT_ENABLE_SPACING).to_bool() {
            return false;
        }
        aSpacing.resize(aRange.Length() as usize, gfxFont_Spacing::default());
        let spacingOffset = (aSpacingRange.start - aRange.start) as usize;
        for s in aSpacing[..spacingOffset].iter_mut() {
            *s = gfxFont_Spacing::default();
        }
        if !GetAdjustedSpacing(self, aSpacingRange, aProvider, &mut aSpacing[spacingOffset..]) {
            aSpacing.clear();
            return false;
        }
        for s in aSpacing[spacingOffset + aSpacingRange.Length() as usize..].iter_mut() {
            *s = gfxFont_Spacing::default();
        }
        true
    }

    //  **** ligature helpers ****
    // (Platforms do the actual ligaturization, but we need to do a bunch of stuff
    // to handle requests that begin or end inside a ligature)

    // if aProvider is null then mBeforeSpacing and mAfterSpacing are set to zero
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::ComputeLigatureData
    fn ComputeLigatureData(&self, aPartRange: gfxTextRun_Range, aProvider: Option<&dyn gfxTextRun_PropertyProvider>) -> gfxTextRun_LigatureData {
        NS_ASSERTION!(aPartRange.start < aPartRange.end, "Computing ligature data for empty range");
        NS_ASSERTION!(aPartRange.end <= self.GetLength(), "Character length overflow");

        let charGlyphs = &self.mCharacterGlyphs;

        let mut mRange = gfxTextRun_Range::default();
        let mut i: u32 = aPartRange.start;
        while !charGlyphs[i as usize].IsLigatureGroupStart() {
            NS_ASSERTION!(i > 0, "Ligature at the start of the run??");
            i -= 1;
        }
        mRange.start = i;
        i = aPartRange.start + 1;
        while i < self.GetLength() && !charGlyphs[i as usize].IsLigatureGroupStart() {
            i += 1;
        }
        mRange.end = i;

        let ligatureWidth: i32 = self.GetAdvanceForGlyphs(mRange, aProvider.map_or(0, |p| p.LetterSpacing()));
        // Count the number of started clusters we have seen
        let mut totalClusterCount: u32 = 0;
        let mut partClusterIndex: u32 = 0;
        let mut partClusterCount: u32 = 0;
        for i in mRange.start..mRange.end {
            // Treat the first character of the ligature as the start of a
            // cluster for our purposes of allocating ligature width to its
            // characters.
            if i == mRange.start || charGlyphs[i as usize].IsClusterStart() {
                totalClusterCount += 1;
                if i < aPartRange.start {
                    partClusterIndex += 1;
                } else if i < aPartRange.end {
                    partClusterCount += 1;
                }
            }
        }
        NS_ASSERTION!(totalClusterCount > 0, "Ligature involving no clusters??");
        // (C++: `ligatureWidth / totalClusterCount` is int32 / uint32, i.e. unsigned division converted
        // to double; the ligature width is not negative here)
        let mut mPartAdvance: gfxFloat = (partClusterIndex * (ligatureWidth as u32 / totalClusterCount)) as f64;
        let mut mPartWidth: gfxFloat = (partClusterCount * (ligatureWidth as u32 / totalClusterCount)) as f64;

        // Any rounding errors are apportioned to the final part of the ligature,
        // so that measuring all parts of a ligature and summing them is equal to
        // the ligature width.
        if aPartRange.end == mRange.end {
            let allParts: gfxFloat = (totalClusterCount * (ligatureWidth as u32 / totalClusterCount)) as f64;
            mPartWidth += ligatureWidth as f64 - allParts;
        }

        let (mClipBeforePart, mClipAfterPart);
        if partClusterCount == 0 {
            // nothing to draw
            mClipBeforePart = true;
            mClipAfterPart = true;
        } else {
            // Determine whether we should clip before or after this part when
            // drawing its slice of the ligature.
            // We need to clip before the part if any cluster is drawn before
            // this part.
            mClipBeforePart = partClusterIndex > 0;
            // We need to clip after the part if any cluster is drawn after
            // this part.
            mClipAfterPart = partClusterIndex + partClusterCount < totalClusterCount;
        }

        if let Some(aProvider) = aProvider {
            if (self.mFlags & ShapedTextFlags::TEXT_ENABLE_SPACING).to_bool() {
                let mut spacing = [gfxFont_Spacing::default()];
                if aPartRange.start == mRange.start {
                    if aProvider.GetSpacing(gfxTextRun_Range::new(aPartRange.start, aPartRange.start + 1), &mut spacing) {
                        mPartWidth += spacing[0].mBefore as f64;
                    }
                }
                if aPartRange.end == mRange.end {
                    if aProvider.GetSpacing(gfxTextRun_Range::new(aPartRange.end - 1, aPartRange.end), &mut spacing) {
                        mPartWidth += spacing[0].mAfter as f64;
                    }
                }
            }
        }

        gfxTextRun_LigatureData { mRange, mPartAdvance, mPartWidth, mClipBeforePart, mClipAfterPart }
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::ComputePartialLigatureWidth
    fn ComputePartialLigatureWidth(&self, aPartRange: gfxTextRun_Range, aProvider: Option<&dyn gfxTextRun_PropertyProvider>) -> gfxFloat {
        if aPartRange.start >= aPartRange.end {
            return 0.0;
        }
        let data = self.ComputeLigatureData(aPartRange, aProvider);
        data.mPartWidth
    }

    // Advance aRange.start to the start of the nearest ligature, back
    // up aRange.end to the nearest ligature end; may result in
    // aRange->start == aRange->end.
    // Returns whether any adjustment was made.
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::ShrinkToLigatureBoundaries
    fn ShrinkToLigatureBoundaries(&self, aRange: &mut gfxTextRun_Range) -> bool {
        if aRange.start >= aRange.end {
            return false;
        }

        let charGlyphs = &self.mCharacterGlyphs;
        let mut adjusted = false;
        while aRange.start < aRange.end && !charGlyphs[aRange.start as usize].IsLigatureGroupStart() {
            aRange.start += 1;
            adjusted = true;
        }
        if aRange.end < self.GetLength() {
            while aRange.end > aRange.start && !charGlyphs[aRange.end as usize].IsLigatureGroupStart() {
                aRange.end -= 1;
                adjusted = true;
            }
        }
        adjusted
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::AccumulateMetricsForRun
    fn AccumulateMetricsForRun(
        &self,
        aFont: &Rc<gfxFont>,
        aRange: gfxTextRun_Range,
        aBoundingBoxType: gfxFont_BoundingBoxType,
        aRefDrawTarget: Option<&DrawTarget>,
        aProvider: Option<&dyn gfxTextRun_PropertyProvider>,
        aSpacingRange: gfxTextRun_Range,
        aOrientation: ShapedTextFlags,
        aMetrics: &mut gfxFont_RunMetrics,
    ) {
        let mut spacingBuffer: Vec<gfxFont_Spacing> = Vec::new();
        let haveSpacing: bool = self.GetAdjustedSpacingArray(aRange, aProvider, aSpacingRange, &mut spacingBuffer);
        let metrics = aFont.Measure(
            self,
            aRange.start,
            aRange.end,
            aBoundingBoxType,
            aRefDrawTarget,
            if haveSpacing { Some(&spacingBuffer[..]) } else { None },
            aProvider.map_or(0, |p| p.LetterSpacing()),
            aOrientation,
        );
        aMetrics.CombineWith(&metrics, self.IsRightToLeft());
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::AccumulatePartialLigatureMetrics
    fn AccumulatePartialLigatureMetrics(
        &self,
        aFont: &Rc<gfxFont>,
        aRange: gfxTextRun_Range,
        aBoundingBoxType: gfxFont_BoundingBoxType,
        aRefDrawTarget: Option<&DrawTarget>,
        aProvider: Option<&dyn gfxTextRun_PropertyProvider>,
        aOrientation: ShapedTextFlags,
        aMetrics: &mut gfxFont_RunMetrics,
    ) {
        if aRange.start >= aRange.end {
            return;
        }

        // Measure partial ligature. We hack this by clipping the metrics in the
        // same way we clip the drawing.
        let mut data = self.ComputeLigatureData(aRange, aProvider);

        // First measure the complete ligature
        let mut metrics = gfxFont_RunMetrics::new();
        self.AccumulateMetricsForRun(aFont, data.mRange, aBoundingBoxType, aRefDrawTarget, aProvider, aRange, aOrientation, &mut metrics);

        // Clip the bounding box to the ligature part
        let mut bboxLeft: gfxFloat = metrics.mBoundingBox.X();
        let mut bboxRight: gfxFloat = metrics.mBoundingBox.XMost();
        // Where we are going to start "drawing" relative to our left baseline origin
        let origin: gfxFloat = if self.IsRightToLeft() { metrics.mAdvanceWidth - data.mPartAdvance } else { 0.0 };
        ClipPartialLigature(self, &mut bboxLeft, &mut bboxRight, origin, &mut data);
        metrics.mBoundingBox.SetBoxX(bboxLeft, bboxRight);

        // mBoundingBox is now relative to the left baseline origin for the entire
        // ligature. Shift it left.
        metrics.mBoundingBox.MoveByX(-(if self.IsRightToLeft() {
            metrics.mAdvanceWidth - (data.mPartAdvance + data.mPartWidth)
        } else {
            data.mPartAdvance
        }));
        metrics.mAdvanceWidth = data.mPartWidth;

        aMetrics.CombineWith(&metrics, self.IsRightToLeft());
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxTextRun::Dump
    // NOT PORTED: debug dump.
    pub fn Dump(&self) {}
}

// Returns false if there is definitely no spacing to apply.
// Gecko: gfx/thebes/gfxTextRun.cpp GetAdjustedSpacing
fn GetAdjustedSpacing(
    aTextRun: &gfxTextRun,
    aRange: gfxTextRun_Range,
    aProvider: &dyn gfxTextRun_PropertyProvider,
    aSpacing: &mut [gfxFont_Spacing],
) -> bool {
    if aRange.start >= aRange.end {
        return false;
    }

    let result: bool = aProvider.GetSpacing(aRange, aSpacing);

    #[cfg(debug_assertions)]
    {
        // Check to see if we have spacing inside ligatures

        let charGlyphs = aTextRun.GetCharacterGlyphs();

        for i in aRange.start..aRange.end {
            if !charGlyphs[i as usize].IsLigatureGroupStart() {
                NS_ASSERTION!(
                    i == aRange.start || aSpacing[(i - aRange.start) as usize].mBefore == 0,
                    "Before-spacing inside a ligature!"
                );
                NS_ASSERTION!(
                    i - 1 <= aRange.start || aSpacing[(i - 1 - aRange.start) as usize].mAfter == 0,
                    "After-spacing inside a ligature!"
                );
            }
        }
    }

    result
}

// Gecko: gfx/thebes/gfxTextRun.cpp ClipPartialLigature
fn ClipPartialLigature(aTextRun: &gfxTextRun, aStart: &mut gfxFloat, aEnd: &mut gfxFloat, aOrigin: gfxFloat, aLigature: &mut gfxTextRun_LigatureData) {
    if aLigature.mClipBeforePart {
        if aTextRun.IsRightToLeft() {
            *aEnd = aEnd.min(aOrigin);
        } else {
            *aStart = aStart.max(aOrigin);
        }
    }
    if aLigature.mClipAfterPart {
        let endEdge: gfxFloat = aOrigin + aTextRun.GetDirection() * aLigature.mPartWidth;
        if aTextRun.IsRightToLeft() {
            *aStart = aStart.max(endEdge);
        } else {
            *aEnd = aEnd.min(endEdge);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// gfxFontGroup
// ---------------------------------------------------------------------------------------------

// Gecko: gfx/thebes/gfxTextRun.h gfxFontGroup::TextRange
pub struct gfxFontGroup_TextRange {
    pub start: u32,
    pub end: u32,
    pub font: Rc<gfxFont>,
    pub matchType: FontMatchType,
    pub orientation: ShapedTextFlags,
}

impl gfxFontGroup_TextRange {
    // Gecko: gfxFontGroup::TextRange::TextRange
    pub fn new(aStart: u32, aEnd: u32, aFont: &Rc<gfxFont>, aMatchType: FontMatchType, aOrientation: ShapedTextFlags) -> Self {
        gfxFontGroup_TextRange { start: aStart, end: aEnd, font: aFont.clone(), matchType: aMatchType, orientation: aOrientation }
    }
    pub fn Length(&self) -> u32 {
        self.end - self.start
    }
}

// Gecko: gfx/thebes/gfxFont.cpp gfxFontCache::Lookup (the idea)
// ADAPTER: one gfxFont per (family list, style), kept for the thread. Making a gfxFont reads the font
// file and builds the HarfBuzz font; Gecko's gfxFontCache shares them across font groups the same way.
fn gfxFontCache_Lookup(aFamilies: &[String], aStyle: &gfxFontStyle, aLanguage: Option<nsAtom>) -> Option<Rc<gfxFont>> {
    thread_local! {
        static sFontCache: RefCell<HashMap<(Vec<String>, u64, u32, bool, Option<&'static str>), Option<Rc<gfxFont>>>> = RefCell::new(HashMap::new());
    }
    let key = (aFamilies.to_vec(), aStyle.size.to_bits(), aStyle.weight.to_bits(), aStyle.italic, aLanguage);
    sFontCache.with(|cache| {
        cache
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                let names: Vec<&str> = aFamilies.iter().map(|s| s.as_str()).collect();
                gfxFont::LookupForLanguage(&names, *aStyle, aLanguage).map(Rc::new)
            })
            .clone()
    })
}

// Gecko: gfx/thebes/gfxPlatformGtk.cpp gfxPlatformGtk::GetCommonFallbackFonts
// (the Twemoji font for color emoji, NOT PORTED)
const kCommonFallbackFonts: [&str; 7] = ["DejaVu Serif", "FreeSerif", "DejaVu Sans", "FreeSans", "Symbola", "Noto Sans Symbols", "Noto Sans Symbols2"];
// the fonts it adds for the CJK ranges
const kCommonFallbackCJKFonts: [&str; 4] = ["TakaoPGothic", "Droid Sans Fallback", "WenQuanYi Micro Hei", "NanumGothic"];

// ADAPTER: the font of a font file in the style (see gfxFontCache_LookupFallback)
fn gfxFontCache_LookupFile(aFile: mrt_text::FontFile, aStyle: &gfxFontStyle) -> Option<Rc<gfxFont>> {
    thread_local! {
        static sFileFonts: RefCell<HashMap<(String, u32, u64, u32, bool), Option<Rc<gfxFont>>>> = RefCell::new(HashMap::new());
    }
    let key = (aFile.mPath.clone(), aFile.mIndex, aStyle.size.to_bits(), aStyle.weight.to_bits(), aStyle.italic);
    sFileFonts.with(|cache| cache.borrow_mut().entry(key).or_insert_with(|| gfxFontEntry::new(aFile).map(|entry| Rc::new(gfxFont::new(entry, *aStyle)))).clone())
}

// Gecko: gfx/thebes/gfxPlatformFontList.cpp gfxPlatformFontList::GlobalFontFallback (the idea)
// ADAPTER: the font for a character that the font group does not have, in the style of the group: one gfxFont
// per (font file, face, style), kept for the thread. The common fallback families come first
// (CommonFontFallback), then fontconfig's sorted list for the character (mrt-text's FindFallbackFontFiles,
// one fc-match run, about 40 ms). The fonts found for a character are tried first for the next characters of
// the same block of 4096 (a script's characters are mostly in one), which saves most of the runs: the block's
// own sorted order, not that of each character, decides among the fonts that have it (the best six are kept). None if no installed
// font has the character.
fn gfxFontCache_LookupFallback(aCh: u32, aStyle: &gfxFontStyle) -> Option<Rc<gfxFont>> {
    thread_local! {
        static sFallbackChains: RefCell<HashMap<(u32, u64, u32, bool), Vec<mrt_text::FontFile>>> = RefCell::new(HashMap::new());
    }
    let chainKey = (aCh >> 12, aStyle.size.to_bits(), aStyle.weight.to_bits(), aStyle.italic);
    // CommonFontFallback: gfxPlatformGtk::GetCommonFallbackFonts, the families that are tried first (the CJK
    // ones for CJK characters)
    let isCJK = aCh >= 0x3000 && (aCh < 0xe000 || (0xf900..0xfff0).contains(&aCh) || (aCh >> 16) == 2);
    for family in kCommonFallbackFonts.iter().chain(if isCJK { kCommonFallbackCJKFonts.iter() } else { [].iter() }) {
        if let Some(file) = mrt_text::FindInstalledFamilyFontFile(family, aStyle.weight as f32, aStyle.italic) {
            if let Some(font) = gfxFontCache_LookupFile(file, aStyle) {
                if font.HasCharacter(aCh) {
                    return Some(font);
                }
            }
        }
    }
    // GlobalFontFallback: the fonts that served this block of characters, then fontconfig's list
    let chain: Vec<mrt_text::FontFile> = sFallbackChains.with(|c| c.borrow().get(&chainKey).cloned().unwrap_or_default());
    for file in chain {
        if let Some(font) = gfxFontCache_LookupFile(file, aStyle) {
            if font.HasCharacter(aCh) {
                return Some(font);
            }
        }
    }
    let files = mrt_text::FindFallbackFontFiles(aCh, aStyle.weight as f32, aStyle.italic);
    let found = files.iter().find_map(|file| gfxFontCache_LookupFile(file.clone(), aStyle).filter(|f| f.HasCharacter(aCh)));
    if found.is_some() {
        sFallbackChains.with(|c| {
            let mut c = c.borrow_mut();
            let chain = c.entry(chainKey).or_default();
            for file in files.into_iter().take(6) {
                if !chain.contains(&file) {
                    chain.push(file);
                }
            }
        });
    }
    found
}

// Gecko: gfx/thebes/gfxPlatformFontList.cpp gfxPlatformFontList::GetFontPrefLangFor
// The font pref language of a character, as the fontconfig language that its pref font is looked up with, and
// None for the characters of the Western, Math and Others groups (their pref font is the group's own).
enum PrefLang {
    One(&'static str),
    CJKSet,
}

fn PrefLangFor(aCh: u32) -> Option<PrefLang> {
    use PrefLang::*;
    // (the Unicode blocks of the switch of GetFontPrefLangFor)
    let lang = match aCh {
        0x0370..=0x03FF | 0x1F00..=0x1FFF => One("el"),
        0x0400..=0x052F | 0x1C80..=0x1C8F | 0x2DE0..=0x2DFF | 0xA640..=0xA69F => One("ru"),
        0x0530..=0x058F => One("hy"),
        0x0590..=0x05FF => One("he"),
        0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF | 0x1EE00..=0x1EEFF => One("ar"),
        0x0900..=0x097F | 0xA8E0..=0xA8FF => One("hi"),
        0x0980..=0x09FF => One("bn"),
        0x0A00..=0x0A7F => One("pa"),
        0x0A80..=0x0AFF => One("gu"),
        0x0B00..=0x0B7F => One("or"),
        0x0B80..=0x0BFF => One("ta"),
        0x0C00..=0x0C7F => One("te"),
        0x0C80..=0x0CFF => One("kn"),
        0x0D00..=0x0D7F => One("ml"),
        0x0D80..=0x0DFF | 0x111E0..=0x111FF => One("si"),
        0x0E00..=0x0E7F => One("th"),
        0x0F00..=0x0FFF => One("bo"),
        0x10A0..=0x10FF | 0x1C90..=0x1CBF | 0x2D00..=0x2D2F => One("ka"),
        0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF => One("ko"),
        0x1200..=0x139F | 0x2D80..=0x2DDF | 0xAB00..=0xAB2F => One("am"),
        0x1400..=0x167F | 0x18B0..=0x18FF => One("iu"),
        0x1780..=0x17FF | 0x19E0..=0x19FF => One("km"),
        0x2E80..=0x33FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFE10..=0xFE1F | 0xFE30..=0xFE6F | 0xFF00..=0xFFEF | 0x16FE0..=0x1B2FF | 0x20000..=0x2FFFF => CJKSet,
        _ => return None,
    };
    Some(lang)
}

// Gecko: gfx/thebes/gfxTextRun.h gfxFontGroup
// REDUCED: the group has the family names and the style, and makes one font from them (the first
// font of the list that fontconfig can match); a character that font does not have gets the font that
// fontconfig names for it (FindFontForChar), as the system font fallback does.
pub struct gfxFontGroup {
    mFontFamilyList: StyleFontFamilyList,
    mStyle: gfxFontStyle,
    mLanguage: Option<nsAtom>,
    mExplicitLanguage: bool,
    mDevToCssSize: gfxFloat,
    // the fonts of the group (mFonts); at most one here
    mFonts: RefCell<Vec<Rc<gfxFont>>>,
    // gfxFontGroup::mFallbackGeneric: the generic family the pref fonts for another script are looked up in (the
    // first generic of the family list, `serif` when it has none), as a fontconfig name
    mFallbackGeneric: RefCell<&'static str>,
    mFontListInitialized: Cell<bool>,
    mHyphenWidth: Cell<gfxFloat>,
    mUnderlineOffset: Cell<gfxFloat>,
    mSkipDrawing: Cell<bool>,
}

impl gfxFontGroup {
    pub const kCSSFirstAvailableFont: u32 = u32::MAX;
    pub const UNDERLINE_OFFSET_NOT_SET: gfxFloat = i16::MAX as gfxFloat;

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxFontGroup::gfxFontGroup
    // NOT PORTED: aFontVisibilityProvider, aTextPerf, aUserFontSet, aVariantEmoji (unit placeholders).
    pub fn new(
        aFontVisibilityProvider: (),
        aFontFamilyList: &StyleFontFamilyList,
        aStyle: &gfxFontStyle,
        aLanguage: Option<nsAtom>,
        aExplicitLanguage: bool,
        aTextPerf: (),
        aUserFontSet: (),
        aDevToCssSize: gfxFloat,
        aVariantEmoji: (),
    ) -> gfxFontGroup {
        gfxFontGroup {
            mFontFamilyList: aFontFamilyList.clone(),
            mStyle: *aStyle,
            mLanguage: aLanguage,
            mExplicitLanguage: aExplicitLanguage,
            mDevToCssSize: aDevToCssSize,
            mFonts: RefCell::new(Vec::new()),
            mFallbackGeneric: RefCell::new("serif"),
            mFontListInitialized: Cell::new(false),
            mHyphenWidth: Cell::new(-1.0),
            mUnderlineOffset: Cell::new(gfxFontGroup::UNDERLINE_OFFSET_NOT_SET),
            mSkipDrawing: Cell::new(false),
        }
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxFontGroup::EnsureFontList
    // Makes the font once, from the family names; Gecko fills mFonts with one entry per family.
    fn EnsureFontList(&self) {
        use mrt_style::style::values::computed::font::{GenericFontFamily, SingleFontFamily};
        if self.mFontListInitialized.get() {
            return;
        }
        self.mFontListInitialized.set(true);
        let mut names: Vec<String> = Vec::new();
        let mut fallbackGeneric: Option<&'static str> = None;
        for family in self.mFontFamilyList.iter() {
            match family {
                SingleFontFamily::FamilyName(n) => names.push(n.name.to_string()),
                SingleFontFamily::Generic(g) => {
                    let name = match g {
                        GenericFontFamily::Serif => Some("serif"),
                        GenericFontFamily::SansSerif => Some("sans-serif"),
                        GenericFontFamily::Monospace => Some("monospace"),
                        GenericFontFamily::Cursive => Some("cursive"),
                        GenericFontFamily::Fantasy => Some("fantasy"),
                        GenericFontFamily::SystemUi => Some("sans-serif"),
                        GenericFontFamily::None => None,
                    };
                    if let Some(name) = name {
                        names.push(name.to_string());
                        fallbackGeneric.get_or_insert(name);
                    }
                }
            }
        }
        *self.mFallbackGeneric.borrow_mut() = fallbackGeneric.unwrap_or("serif");
        if let Some(font) = gfxFontCache_Lookup(&names, &self.mStyle, self.mLanguage) {
            self.mFonts.borrow_mut().push(font);
        }
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxFontGroup::HasFont
    // NOT PORTED: compares the font entries of the group; one font here.
    pub fn HasFont(&self, aFontEntry: &Rc<gfxFontEntry>) -> bool {
        self.EnsureFontList();
        self.mFonts.borrow().iter().any(|f| Rc::ptr_eq(f.GetFontEntry(), aFontEntry))
    }

    // ADAPTER: whether fontconfig gave the group a font at all (Gecko's GetDefaultFont would crash).
    pub fn HasValidFont(&self) -> bool {
        self.EnsureFontList();
        !self.mFonts.borrow().is_empty()
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxFontGroup::GetDefaultFont
    // NOT PORTED: the pref-based default font; last resort is the system sans-serif, then a crash.
    pub fn GetDefaultFont(&self) -> Rc<gfxFont> {
        match gfxFontCache_Lookup(&["sans-serif".to_string()], &self.mStyle, None) {
            Some(f) => f,
            None => MOZ_CRASH!("Failed to create a default font! (is fc-match installed?)"),
        }
    }

    // Returns first valid font in the fontlist or default font.
    // Initiates userfont loads if userfont not loaded.
    // aCh: character to look for, or kCSSFirstAvailableFont for default "first
    //      available font" as defined by CSS Fonts (i.e. the first font whose
    //      unicode-range includes <space>, but does not require space to
    //      actually be present)
    // aGeneric: if non-null, returns the CSS generic type that was mapped to
    //           this font
    // aIsFirst: if non-null, returns whether the font was first in the list
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxFontGroup::GetFirstValidFont
    pub fn GetFirstValidFont(&self, aCh: u32, aGeneric: Option<&mut StyleGenericFontFamily>, aIsFirst: Option<&mut bool>) -> Rc<gfxFont> {
        self.EnsureFontList();

        let font = self.mFonts.borrow().first().cloned();
        if let Some(font) = font {
            // Check whether the font supports the given character, unless aCh is the
            // kCSSFirstAvailableFont constant, in which case (as per CSS Fonts spec)
            // we want the first font whose unicode-range does not exclude <space>,
            // regardless of whether it in fact supports the <space> character.
            if aCh == gfxFontGroup::kCSSFirstAvailableFont || font.HasCharacter(aCh) {
                if let Some(g) = aGeneric {
                    *g = StyleGenericFontFamily::None;
                }
                if let Some(f) = aIsFirst {
                    *f = true;
                }
                return font;
            }
        }
        if let Some(g) = aGeneric {
            *g = StyleGenericFontFamily::None;
        }
        if let Some(f) = aIsFirst {
            *f = false;
        }
        self.GetDefaultFont()
    }

    // Gecko: gfx/thebes/gfxTextRun.h gfxFontGroup::GetStyle
    pub fn GetStyle(&self) -> &gfxFontStyle {
        &self.mStyle
    }

    /**
     * The listed characters should be treated as invisible and zero-width
     * when creating textruns.
     */
    // Gecko: gfx/thebes/gfxTextRun.h gfxFontGroup::IsInvalidChar(uint8_t) / IsInvalidChar(char16_t)
    // (one generic function over the character type)
    pub fn IsInvalidChar<T: TextFrameChar>(ch: T) -> bool {
        gfxFontGroup_IsInvalidChar(ch)
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxFontGroup::MakeEmptyTextRun
    fn MakeEmptyTextRun(self: &Rc<Self>, aParams: &mut gfxTextRunFactory_Parameters, aFlags: ShapedTextFlags, aFlags2: nsTextFrameUtils_Flags) -> Option<RefPtr_gfxTextRun> {
        let aFlags = aFlags | ShapedTextFlags::TEXT_IS_8BIT;
        gfxTextRun::Create(aParams, 0, self, aFlags, aFlags2)
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxFontGroup::MakeSpaceTextRun
    fn MakeSpaceTextRun(self: &Rc<Self>, aParams: &mut gfxTextRunFactory_Parameters, aFlags: ShapedTextFlags, aFlags2: nsTextFrameUtils_Flags) -> Option<RefPtr_gfxTextRun> {
        let aFlags = aFlags | ShapedTextFlags::TEXT_IS_8BIT;

        let textRun = gfxTextRun::Create(aParams, 1, self, aFlags, aFlags2)?;

        let mut orientation: ShapedTextFlags = aFlags & ShapedTextFlags::TEXT_ORIENT_MASK;
        if orientation == ShapedTextFlags::TEXT_ORIENT_VERTICAL_MIXED {
            orientation = ShapedTextFlags::TEXT_ORIENT_VERTICAL_SIDEWAYS_RIGHT;
        }

        let font = self.GetFirstValidFont(gfxFontGroup::kCSSFirstAvailableFont, None, None);
        if self.GetStyle().size == 0.0 {
            // Short-circuit for size-0 fonts, as Windows and ATSUI can't handle
            // them, and always create at least size 1 fonts, i.e. they still
            // render something for size 0 fonts.
            textRun.borrow_mut().AddGlyphRun(&font, FontMatchType::new(FontMatchType_Kind::kUnspecified), 0, false, orientation, false);
        } else {
            if font.GetSpaceGlyph() != 0 {
                // Normally, the font has a cached space glyph, so we can avoid
                // the cost of calling FindFontForChar.
                textRun.borrow_mut().SetSpaceGlyph(&font, aParams.mDrawTarget, 0, orientation);
            } else {
                // In case the primary font doesn't have <space> (bug 970891),
                // find one that does.
                let mut matchType = FontMatchType::new(FontMatchType_Kind::kUnspecified);
                let spaceFont = self.FindFontForChar(' ' as u32, 0, 0, Script::OTHER, None, &mut matchType);
                if let Some(spaceFont) = spaceFont {
                    textRun.borrow_mut().SetSpaceGlyph(&spaceFont, aParams.mDrawTarget, 0, orientation);
                }
            }
        }

        // Note that the gfxGlyphExtents glyph bounds storage for the font will
        // always contain an entry for the font's space glyph, so we don't have
        // to call FetchGlyphExtents here.
        Some(textRun)
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxFontGroup::MakeBlankTextRun
    fn MakeBlankTextRun<T: TextFrameChar>(
        self: &Rc<Self>,
        aString: &[T],
        aLength: u32,
        aParams: &mut gfxTextRunFactory_Parameters,
        aFlags: ShapedTextFlags,
        aFlags2: nsTextFrameUtils_Flags,
    ) -> Option<RefPtr_gfxTextRun> {
        let textRun = gfxTextRun::Create(aParams, aLength, self, aFlags, aFlags2)?;

        let mut orientation: ShapedTextFlags = aFlags & ShapedTextFlags::TEXT_ORIENT_MASK;
        if orientation == ShapedTextFlags::TEXT_ORIENT_VERTICAL_MIXED {
            orientation = ShapedTextFlags::TEXT_ORIENT_VERTICAL_UPRIGHT;
        }
        let font = self.GetFirstValidFont(gfxFontGroup::kCSSFirstAvailableFont, None, None);
        {
            let mut run = textRun.borrow_mut();
            run.AddGlyphRun(&font, FontMatchType::new(FontMatchType_Kind::kUnspecified), 0, false, orientation, false);

            run.SetupClusterBoundaries(0, aString, aLength);

            for i in 0..aLength {
                if aString[i as usize].to_u32() == '\n' as u32 {
                    run.SetIsNewline(i);
                } else if aString[i as usize].to_u32() == '\t' as u32 {
                    run.SetIsTab(i);
                }
            }
        }

        Some(textRun)
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxFontGroup::MakeHyphenTextRun
    /**
     * Make a text run representing a single hyphen character.
     * This will use U+2010 HYPHEN if available in the first font,
     * otherwise fall back to U+002D HYPHEN-MINUS.
     * The caller is responsible for deleting the returned text run
     * when no longer required.
     */
    pub fn MakeHyphenTextRun(self: &Rc<Self>, aDrawTarget: Option<&DrawTarget>, aFlags: ShapedTextFlags, aAppUnitsPerDevUnit: u32) -> Option<RefPtr_gfxTextRun> {
        // only use U+2010 if it is supported by the first font in the group;
        // it's better to use ASCII '-' from the primary font than to fall back to
        // U+2010 from some other, possibly poorly-matching face
        const hyphen: u16 = 0x2010;
        let font = self.GetFirstValidFont(hyphen as u32, None, None);
        if font.HasCharacter(hyphen as u32) {
            return self.MakeTextRun__T_ptr_uint32_t_DrawTarget_ptr_int32_t_ShapedTextFlags_Flags_gfxMissingFontRecorder_ptr(
                &[hyphen], 1, aDrawTarget, aAppUnitsPerDevUnit as i32, aFlags, nsTextFrameUtils_Flags::default(), None,
            );
        }

        const dash: u8 = b'-';
        self.MakeTextRun__T_ptr_uint32_t_DrawTarget_ptr_int32_t_ShapedTextFlags_Flags_gfxMissingFontRecorder_ptr(
            &[dash], 1, aDrawTarget, aAppUnitsPerDevUnit as i32, aFlags, nsTextFrameUtils_Flags::default(), None,
        )
    }

    // Get the (possibly-cached) width of the hyphen character.
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxFontGroup::GetHyphenWidth
    pub fn GetHyphenWidth(self: &Rc<Self>, aProvider: &dyn gfxTextRun_PropertyProvider) -> gfxFloat {
        if self.mHyphenWidth.get() < 0.0 {
            let dt = aProvider.GetDrawTarget();
            if dt.is_some() {
                let hyphRun = self.MakeHyphenTextRun(dt.as_ref(), aProvider.GetShapedTextFlags(), aProvider.GetAppUnitsPerDevUnit());
                self.mHyphenWidth.set(match hyphRun {
                    Some(r) => r.borrow().GetAdvanceWidth__(),
                    None => 0.0,
                });
            }
        }
        self.mHyphenWidth.get()
    }

    /**
     * Make a textrun for a given string.
     * If aText is not persistent (aFlags & TEXT_IS_PERSISTENT), the
     * textrun will copy it.
     * This calls FetchGlyphExtents on the textrun.
     */
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxFontGroup::MakeTextRun(const T*, uint32_t, const Parameters*, ...)
    // (the template over char16_t / uint8_t is a generic over TextFrameChar)
    pub fn MakeTextRun__T_ptr_uint32_t_Parameters_ptr_ShapedTextFlags_Flags_gfxMissingFontRecorder_ptr<T: TextFrameChar>(
        self: &Rc<Self>,
        aString: &[T],
        aLength: u32,
        aParams: &mut gfxTextRunFactory_Parameters,
        aFlags: ShapedTextFlags,
        aFlags2: nsTextFrameUtils_Flags,
        aMFR: Option<&mut gfxMissingFontRecorder>,
    ) -> Option<RefPtr_gfxTextRun> {
        if aLength == 0 {
            return self.MakeEmptyTextRun(aParams, aFlags, aFlags2);
        }
        if aLength == 1 && aString[0].to_u32() == ' ' as u32 {
            return self.MakeSpaceTextRun(aParams, aFlags, aFlags2);
        }

        let mut aFlags = aFlags;
        if !T::IS_WIDE {
            aFlags |= ShapedTextFlags::TEXT_IS_8BIT;
        }

        if self.GetStyle().size == 0.0 {
            // Short-circuit for size-0 fonts, as Windows and ATSUI can't handle
            // them, and always create at least size 1 fonts, i.e. they still
            // render something for size 0 fonts.
            return self.MakeBlankTextRun(aString, aLength, aParams, aFlags, aFlags2);
        }

        let textRun = gfxTextRun::Create(aParams, aLength, self, aFlags, aFlags2)?;

        self.InitTextRun(aParams.mDrawTarget, &mut textRun.borrow_mut(), aString, aLength, aMFR);

        textRun.borrow().FetchGlyphExtents(aParams.mDrawTarget);

        Some(textRun)
    }

    /**
     * Textrun creation helper for clients that don't want to pass
     * a full Parameters record.
     */
    // Gecko: gfx/thebes/gfxTextRun.h gfxFontGroup::MakeTextRun(const T*, uint32_t, DrawTarget*, int32_t, ...)
    pub fn MakeTextRun__T_ptr_uint32_t_DrawTarget_ptr_int32_t_ShapedTextFlags_Flags_gfxMissingFontRecorder_ptr<T: TextFrameChar>(
        self: &Rc<Self>,
        aString: &[T],
        aLength: u32,
        aRefDrawTarget: Option<&DrawTarget>,
        aAppUnitsPerDevUnit: i32,
        aFlags: ShapedTextFlags,
        aFlags2: nsTextFrameUtils_Flags,
        aMFR: Option<&mut gfxMissingFontRecorder>,
    ) -> Option<RefPtr_gfxTextRun> {
        let mut params = gfxTextRunFactory_Parameters {
            mDrawTarget: aRefDrawTarget,
            mSkipChars: None,
            mInitialBreaks: None,
            mInitialBreakCount: 0,
            mAppUnitsPerDevUnit: aAppUnitsPerDevUnit,
        };
        self.MakeTextRun__T_ptr_uint32_t_Parameters_ptr_ShapedTextFlags_Flags_gfxMissingFontRecorder_ptr(aString, aLength, &mut params, aFlags, aFlags2, aMFR)
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxFontGroup::InitTextRun
    // NOT PORTED: numeral transformation (bidi.numeral is nominal), the script itemizer (one script
    // run, Script::OTHER), the fast ASCII script detection, the variant-fallback redo loop, text logs.
    fn InitTextRun<T: TextFrameChar>(
        &self,
        aDrawTarget: Option<&DrawTarget>,
        aTextRun: &mut gfxTextRun,
        aString: &[T],
        aLength: u32,
        aMFR: Option<&mut gfxMissingFontRecorder>,
    ) {
        MOZ_ASSERT!(aLength > 0, "don't call InitTextRun for a zero-length run");

        let script = Script::OTHER;
        self.InitScriptRun(aDrawTarget, aTextRun, aString, 0, aLength, script, aMFR);

        // Guard against text that starts with a combining mark (which cannot occur
        // in 8-bit text, even after numeral processing).
        if T::IS_WIDE {
            let glyph = &mut aTextRun.GetCharacterGlyphs_mut()[0];
            if !glyph.IsSimpleGlyph() {
                glyph.SetClusterStart(true);
            }
        }

        // It's possible for CoreText to omit glyph runs if it decides they contain
        // only invisibles (e.g., U+FEFF, see reftest 474417-1). In this case, we
        // need to eliminate them from the glyph run array to avoid drawing "partial
        // ligatures" with the wrong font.
        aTextRun.SanitizeGlyphRuns();
        aTextRun.ShrinkDetailedGlyphs();
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxFontGroup::InitScriptRun
    // NOT PORTED: user font updates, Mongolian/Phags-pa orientation, the font ranges (ComputeRanges
    // yields one range with the first font), the small-caps and sub/superscript fallbacks, missing
    // characters (the first font's .notdef glyph is used) and the missing font recorder.
    fn InitScriptRun<T: TextFrameChar>(
        &self,
        aDrawTarget: Option<&DrawTarget>,
        aTextRun: &mut gfxTextRun,
        aString: &[T],  // text for this script run, not the entire textrun
        aOffset: u32,   // position of the script run within the textrun
        aLength: u32,   // length of the script run
        aRunScript: Script,
        aMFR: Option<&mut gfxMissingFontRecorder>,
    ) {
        NS_ASSERTION!(aLength > 0, "don't call InitScriptRun for a 0-length run");
        NS_ASSERTION!(
            aTextRun.GetShapingState() != gfxTextRun_ShapingState::eShapingState_Aborted,
            "don't call InitScriptRun with aborted shaping state"
        );

        let mainFont = self.GetFirstValidFont(gfxFontGroup::kCSSFirstAvailableFont, None, None);

        let mut orientation: ShapedTextFlags = aTextRun.GetFlags() & ShapedTextFlags::TEXT_ORIENT_MASK;

        let mut fontRanges: Vec<gfxFontGroup_TextRange> = Vec::new();
        self.ComputeRanges(&mut fontRanges, aString, aLength, aRunScript, orientation);
        let numRanges: usize = fontRanges.len();
        let isCJK: bool = gfxTextRun::IsCJKScript(aRunScript);

        for r in 0..numRanges {
            let range = &fontRanges[r];
            let runStart: u32 = range.start;
            let matchedLength: u32 = range.Length();
            let matchedFont: &Rc<gfxFont> = &range.font;
            // create the glyph run for this range
            // common case - just do glyph layout and record the
            // resulting positioned glyphs
            aTextRun.AddGlyphRun(matchedFont, range.matchType, aOffset + runStart, matchedLength > 0, range.orientation, isCJK);
            if !matchedFont.SplitAndInitTextRun(
                aDrawTarget,
                aTextRun,
                &aString[runStart as usize..],
                aOffset + runStart,
                matchedLength,
                aRunScript,
                self.mLanguage,
                range.orientation,
            ) {
                // glyph layout failed! treat as missing glyphs
                // NOT PORTED: the missing-glyph path of the C++ (hexboxes); the characters stay missing.
                aTextRun.SetupClusterBoundaries(aOffset + runStart, &aString[runStart as usize..], matchedLength);
            }
        }
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxFontGroup::ComputeRanges
    // REDUCED: the ranges are the runs of characters that FindFontForChar gives the same font. NOT PORTED:
    // the font list of the group (one font), the Unicode-range and font-variant ranges, and the merging of
    // ranges for the vertical orientation (sideways text, upright CJK): every range has the orientation of
    // the text.
    fn ComputeRanges<T: TextFrameChar>(
        &self,
        aRanges: &mut Vec<gfxFontGroup_TextRange>,
        aString: &[T],
        aLength: u32,
        aRunScript: Script,
        aOrientation: ShapedTextFlags,
    ) {
        NS_ASSERTION!(aRanges.is_empty(), "aRanges must be initially empty");
        NS_ASSERTION!(aLength > 0, "don't call ComputeRanges for zero-length text");

        let orientation = if aOrientation == ShapedTextFlags::TEXT_ORIENT_VERTICAL_MIXED {
            ShapedTextFlags::TEXT_ORIENT_VERTICAL_SIDEWAYS_RIGHT
        } else {
            aOrientation
        };

        let mut prevCh: u32 = 0;
        let mut prevFont: Option<Rc<gfxFont>> = None;
        let mut i: u32 = 0;
        while i < aLength {
            let mut ch: u32 = aString[i as usize].to_u32();
            let mut chLength: u32 = 1;
            // a surrogate pair is one character
            if T::IS_WIDE && (0xD800..0xDC00).contains(&ch) && i + 1 < aLength {
                let low = aString[(i + 1) as usize].to_u32();
                if (0xDC00..0xE000).contains(&low) {
                    ch = 0x10000 + ((ch - 0xD800) << 10) + (low - 0xDC00);
                    chLength = 2;
                }
            }
            let nextCh: u32 = if i + chLength < aLength { aString[(i + chLength) as usize].to_u32() } else { 0 };

            let mut matchType = FontMatchType::new(FontMatchType_Kind::kFontGroup);
            let font = self
                .FindFontForChar(ch, prevCh, nextCh, aRunScript, prevFont.as_ref(), &mut matchType)
                .unwrap_or_else(|| self.GetFirstValidFont(gfxFontGroup::kCSSFirstAvailableFont, None, None));
            match aRanges.last_mut() {
                Some(last) if Rc::ptr_eq(&last.font, &font) => last.end = i + chLength,
                _ => aRanges.push(gfxFontGroup_TextRange::new(i, i + chLength, &font, matchType, orientation)),
            }
            prevFont = Some(font);
            prevCh = ch;
            i += chLength;
        }
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxFontGroup::FindFontForChar
    // REDUCED: the font group has one font; the special cases of Gecko that keep a cluster in one font
    // (cluster extenders, joiners, variation selectors, default-ignorable characters, control characters)
    // are here, with the first font tried first and then the system fallback font that fontconfig names
    // for the character (gfxFontCache_LookupFallback). None if no font has the character (the caller
    // uses the first font and its missing glyph).
    // NOT PORTED: pref fonts and the font-variant-emoji and text/emoji presentation handling, the NNBSP and
    // the fallback characters for hyphens and NBSP (U+2010, U+2011 to '-', U+00A0 to ' '), fallback faces
    // within the family, unicode-range, and the Private Use Area rule.
    pub fn FindFontForChar(
        &self,
        aCh: u32,
        aPrevCh: u32,
        aNextCh: u32,
        aRunScript: Script,
        aPrevMatchedFont: Option<&Rc<gfxFont>>,
        aMatchType: &mut FontMatchType,
    ) -> Option<Rc<gfxFont>> {
        // If the char is a cluster extender, we want to use the same font as the
        // preceding character if possible. This is preferable to using the font
        // group because it avoids breaks in shaping within a cluster.
        if let Some(prev) = aPrevMatchedFont {
            if IsClusterExtender(aCh) && (prev.HasCharacter(aCh) || IsDefaultIgnorable(aCh)) {
                return Some(prev.clone());
            }
        }

        let isJoinControl = aCh == 0x200C || aCh == 0x200D;
        let wasJoinCauser = aPrevCh == 0x200D;
        let isVarSelector = (0xFE00..=0xFE0F).contains(&aCh) || (0xE0100..=0xE01EF).contains(&aCh) || (0x180B..=0x180D).contains(&aCh) || aCh == 0x180F;
        let nextIsVarSelector = (0xFE00..=0xFE0F).contains(&aNextCh) || (0xE0100..=0xE01EF).contains(&aNextCh);

        // To optimize common cases, try the first font in the font-group
        // before going into the more detailed checks below
        if !isJoinControl && !wasJoinCauser && !isVarSelector && !nextIsVarSelector {
            let firstFont = self.GetFirstValidFont(gfxFontGroup::kCSSFirstAvailableFont, None, None);
            if firstFont.HasCharacter(aCh) {
                *aMatchType = FontMatchType::new(FontMatchType_Kind::kFontGroup);
                return Some(firstFont);
            }
        }

        if let Some(prev) = aPrevMatchedFont {
            // Don't switch fonts for control characters, regardless of
            // whether they are present in the current font, as they won't
            // actually be rendered (see bug 716229)
            if isJoinControl || UnicodeProperties::CharType(aCh) == GeneralCategory::Control {
                return Some(prev.clone());
            }

            // if previous character was a join-causer (ZWJ),
            // use the same font as the previous range if we can
            if wasJoinCauser && prev.HasCharacter(aCh) {
                return Some(prev.clone());
            }
        }

        // If this character is a variation selector or default-ignorable, use the
        // previous font regardless of whether it supports the codepoint or not.
        // (We don't want to unnecessarily split glyph runs, and the character will
        // not be visibly rendered.)
        if isVarSelector || IsDefaultIgnorable(aCh) {
            return aPrevMatchedFont.cloned();
        }

        // The font group's other fonts (none here), then the pref fonts for the script of the character
        if let Some(font) = self.WhichPrefFontSupportsChar(aCh) {
            *aMatchType = FontMatchType::new(FontMatchType_Kind::kPrefsFallback);
            return Some(font);
        }

        // -- before searching for something else check the font used for the previous character
        if let Some(prev) = aPrevMatchedFont {
            if prev.HasCharacter(aCh) {
                *aMatchType = FontMatchType::new(FontMatchType_Kind::kSystemFallback);
                return Some(prev.clone());
            }
        }

        // otherwise the system fallback: WhichSystemFontSupportsChar
        if let Some(font) = gfxFontCache_LookupFallback(aCh, &self.mStyle) {
            *aMatchType = FontMatchType::new(FontMatchType_Kind::kSystemFallback);
            return Some(font);
        }

        None
    }

    // Gecko: gfx/thebes/gfxTextRun.cpp gfxFontGroup::WhichPrefFontSupportsChar
    // REDUCED: the pref fonts of Firefox on Linux for a script are the generic family of the group (the
    // `serif`, `sans-serif` ... the group falls back to) in the language of the script
    // (`font.name-list.<generic>.<lang group>` is the generic itself in all.js, the language group a fontconfig
    // language: `serif:lang=ta`), so this asks fontconfig for the generic with that language. The pref
    // languages of a character are its script's, and for a CJK character the language of the text if that is
    // CJK, then Japanese, Chinese (Taiwan, China, Hong Kong) and Korean in that order (AppendCJKPrefLangs for a
    // locale that is none of them); then "x-unicode", the generic with no language, which the group's
    // own font is. NOT PORTED: the font.name.* user prefs, font.default.* and the emoji prefs.
    fn WhichPrefFontSupportsChar(&self, aCh: u32) -> Option<Rc<gfxFont>> {
        let generic: &'static str = *self.mFallbackGeneric.borrow();
        let langs: &[&str] = match PrefLangFor(aCh)? {
            PrefLang::One(lang) => &[lang][..],
            PrefLang::CJKSet => &["ja", "zh-tw", "zh-cn", "zh-hk", "ko"][..],
        };
        let mut candidates: Vec<&str> = Vec::new();
        // the language of the text, if CJK, comes first
        if let (PrefLang::CJKSet, Some(page)) = (PrefLangFor(aCh)?, self.mLanguage) {
            if matches!(page.split('-').next(), Some("ja" | "zh" | "ko")) {
                candidates.push(page);
            }
        }
        candidates.extend(langs.iter().copied());
        for lang in candidates {
            if let Some(font) = gfxFontCache_Lookup(&[generic.to_string()], &self.mStyle, Some(lang)) {
                if font.HasCharacter(aCh) {
                    return Some(font);
                }
            }
        }
        None
    }

    // This returns the preferred underline for this font group.
    // Some CJK fonts have wrong underline offset in its metrics.
    // If this group has such "bad" font, each platform's gfxFontGroup
    // initialized mUnderlineOffset. The value should be lower value of
    // first font's metrics and the bad font's metrics. Otherwise, this
    // returns from first font's metrics.
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxFontGroup::GetUnderlineOffset
    // NOT PORTED: the "bad underline family" check (a list of CJK families).
    pub fn GetUnderlineOffset(&self) -> gfxFloat {
        if self.mUnderlineOffset.get() == gfxFontGroup::UNDERLINE_OFFSET_NOT_SET {
            // no bad underline fonts, use the first valid font's metric
            let firstValidFont = self.GetFirstValidFont(gfxFontGroup::kCSSFirstAvailableFont, None, None);
            self.mUnderlineOffset.set(firstValidFont.GetMetrics().underlineOffset);
        }

        self.mUnderlineOffset.get()
    }

    // Gecko: gfx/thebes/gfxTextRun.h gfxFontGroup::ClearCachedData
    pub fn ClearCachedData(&self) {
        self.mUnderlineOffset.set(gfxFontGroup::UNDERLINE_OFFSET_NOT_SET);
        self.mSkipDrawing.set(false);
        self.mHyphenWidth.set(-1.0);
    }

    // Gecko: gfx/thebes/gfxTextRun.h gfxFontGroup::ShouldSkipDrawing
    pub fn ShouldSkipDrawing(&self) -> bool {
        self.mSkipDrawing.get()
    }

    // Gecko: gfx/thebes/gfxTextRun.h gfxFontGroup::Language
    pub fn Language(&self) -> Option<nsAtom> {
        self.mLanguage
    }

    // Get font metrics to be used as the basis for CSS font-relative units.
    // Gecko: gfx/thebes/gfxTextRun.cpp gfxFontGroup::GetMetricsForCSSUnits
    // NOT PORTED: 'ch' / 'ic' composite metrics (StyleQueryFontMetricsFlags).
    pub fn GetMetricsForCSSUnits(&self) -> gfxFont_Metrics {
        unimplemented!("NOT PORTED: composite metrics for CSS units")
    }
}

// Gecko: gfx/thebes/gfxTextRun.h gfxFontGroup::Shutdown
// NOT PORTED: the language atom service.

#[cfg(test)]
mod tests {
    // Not a gtest translation: gfx/tests/gtest has no text run tests that run without a platform font
    // list. Written from the documented behaviour of gfxTextRun (the comments in gfxTextRun.h) with
    // the system sans-serif font that fc-match finds. Widths are compared with each other and with
    // mrt-text's shaper, not with fixed numbers, so any installed font works.
    use super::*;
    use mrt_style::style::values::computed::font::{FontFamilyList, GenericFontFamily, SingleFontFamily};

    fn group(aPx: f64) -> Rc<gfxFontGroup> {
        let list = FontFamilyList {
            list: mrt_style::style::ArcSlice::from_iter(vec![SingleFontFamily::Generic(GenericFontFamily::SansSerif)].into_iter()),
        };
        let style = gfxFontStyle { size: aPx, ..Default::default() };
        Rc::new(gfxFontGroup::new((), &list, &style, None, false, (), (), 1.0, ()))
    }

    fn u16s(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn make(aGroup: &Rc<gfxFontGroup>, s: &str, aFlags: ShapedTextFlags) -> RefPtr_gfxTextRun {
        let t = u16s(s);
        aGroup
            .MakeTextRun__T_ptr_uint32_t_DrawTarget_ptr_int32_t_ShapedTextFlags_Flags_gfxMissingFontRecorder_ptr(
                &t, t.len() as u32, None, 60, aFlags, nsTextFrameUtils_Flags::default(), None,
            )
            .expect("MakeTextRun")
    }

    // The line breaker (already translated) tells the run where it may break: the same
    // chain nsTextFrame uses (nsLineBreaker -> sink -> SetPotentialLineBreaks).
    struct RunSink {
        mRun: RefPtr_gfxTextRun,
    }
    impl nsILineBreakSink for RunSink {
        fn SetBreaks(&mut self, aStart: u32, aLength: u32, aBreakBefore: &mut [u8]) {
            self.mRun.borrow_mut().SetPotentialLineBreaks(gfxTextRun_Range::new(aStart, aStart + aLength), aBreakBefore);
        }
        fn SetCapitalization(&mut self, aStart: u32, aLength: u32, aCapitalize: &mut [bool]) {}
    }

    fn make_with_breaks(aGroup: &Rc<gfxFontGroup>, s: &str, aFlags: ShapedTextFlags) -> RefPtr_gfxTextRun {
        let run = make(aGroup, s, aFlags);
        let text = u16s(s);
        let mut sink = RunSink { mRun: run.clone() };
        let mut lb = nsLineBreaker::default();
        lb.AppendText__nsAtom_ptr_char16_t_ptr_uint32_t_uint32_t_nsILineBreakSink_ptr(
            None, &text, text.len() as u32, 0, Some(&mut sink as *mut dyn nsILineBreakSink),
        );
        let mut trailing = false;
        lb.Reset(&mut trailing);
        run
    }

    // A provider with no spacing and no hyphenation, plus the knobs the tests turn.
    #[derive(Default)]
    struct TestProvider {
        mLetterSpacing: nscoord,
        mHyphens: Option<StyleHyphens>,
        mHyphenWidth: gfxFloat,
        mSoftHyphenBefore: Vec<u32>,
    }
    impl gfxTextRun_PropertyProvider for TestProvider {
        fn GetHyphenationBreaks(&self, aRange: gfxTextRun_Range, aBreakBefore: &mut [gfxTextRun_HyphenType]) {
            for i in aRange.start..aRange.end {
                aBreakBefore[(i - aRange.start) as usize] = if self.mSoftHyphenBefore.contains(&i) {
                    gfxTextRun_HyphenType::Soft
                } else {
                    gfxTextRun_HyphenType::None
                };
            }
        }
        fn GetHyphensOption(&self) -> StyleHyphens {
            self.mHyphens.unwrap_or(StyleHyphens::None)
        }
        fn GetHyphenWidth(&self) -> gfxFloat {
            self.mHyphenWidth
        }
        fn GetShapedTextFlags(&self) -> ShapedTextFlags {
            ShapedTextFlags::default()
        }
        fn GetSpacing(&self, aRange: gfxTextRun_Range, aSpacing: &mut [gfxFont_Spacing]) -> bool {
            for s in aSpacing[..aRange.Length() as usize].iter_mut() {
                *s = gfxFont_Spacing { mBefore: 0, mAfter: self.mLetterSpacing };
            }
            self.mLetterSpacing != 0
        }
        fn GetDrawTarget(&self) -> Option<DrawTarget> {
            Some(DrawTarget)
        }
        fn GetAppUnitsPerDevUnit(&self) -> u32 {
            60
        }
        fn LetterSpacing(&self) -> nscoord {
            self.mLetterSpacing
        }
    }

    fn width_of(aRun: &RefPtr_gfxTextRun, aStart: u32, aEnd: u32) -> gfxFloat {
        aRun.borrow().GetAdvanceWidth__Range_PropertyProvider_ptr_Spacing_ptr(gfxTextRun_Range::new(aStart, aEnd), None, None)
    }

    fn shaped_width(aPx: f64, s: &str) -> nscoord {
        let font = mrt_text::gfxFont::Lookup(&["sans-serif"], gfxFontStyle { size: aPx, ..Default::default() }).expect("fc-match");
        font.ShapeText(&u16s(s), false, None).mWidth
    }

    #[allow(clippy::too_many_arguments)]
    fn break_and_measure(
        aRun: &RefPtr_gfxTextRun,
        aStart: u32,
        aMaxLength: u32,
        aWidth: gfxFloat,
        aProvider: &TestProvider,
        aSuppress: gfxTextRun_SuppressBreak,
        aCanWordWrap: bool,
        aPriority: &mut gfxBreakPriority,
    ) -> (u32, gfxTextRun_TrimmableWS, gfxFont_RunMetrics, bool, u32) {
        let mut trim = gfxTextRun_TrimmableWS::default();
        let mut metrics = gfxFont_RunMetrics::new();
        let mut usedHyphen = false;
        let mut lastBreak = 12345u32;
        let fit = aRun.borrow().BreakAndMeasureText(
            aStart, aMaxLength, aStart == 0, aWidth, aProvider, aSuppress, gfxFont_BoundingBoxType::LOOSE_INK_EXTENTS, None,
            aCanWordWrap, false, false, Some(&mut trim), &mut metrics, &mut usedHyphen, &mut lastBreak, aPriority,
        );
        (fit, trim, metrics, usedHyphen, lastBreak)
    }

    #[test]
    fn a_word_is_as_wide_as_the_shaper_says() {
        let g = group(16.0);
        for w in ["Hello", "world", "AVATAR", "iiiiii", "WWWWWW"] {
            let run = make(&g, w, ShapedTextFlags::default());
            assert_eq!(run.borrow().GetLength(), w.len() as u32);
            assert_eq!(run.borrow().GetAdvanceWidth__() as nscoord, shaped_width(16.0, w), "{w}");
        }
        // wider letters make wider words
        let narrow = make(&g, "iiiiii", ShapedTextFlags::default()).borrow().GetAdvanceWidth__();
        let wide = make(&g, "WWWWWW", ShapedTextFlags::default()).borrow().GetAdvanceWidth__();
        assert!(narrow > 0.0 && wide > narrow, "{narrow} {wide}");
    }

    #[test]
    fn a_space_is_the_font_space_width_and_words_are_shaped_separately() {
        let g = group(16.0);
        let both = make(&g, "Hello world", ShapedTextFlags::default());
        let hello = shaped_width(16.0, "Hello");
        let world = shaped_width(16.0, "world");
        let space = g.GetFirstValidFont(gfxFontGroup::kCSSFirstAvailableFont, None, None).GetMetrics().spaceWidth;
        let space = NS_lroundf((space * 60.0) as f32);
        assert_eq!(both.borrow().GetAdvanceWidth__() as nscoord, hello + space + world);
        // the space record is a "simple" glyph marked as a space
        let r = both.borrow();
        assert!(r.CharIsSpace(5) && !r.CharIsSpace(4));
        assert!(r.GetCharacterGlyphs()[5].IsSimpleGlyph());
        assert_eq!(r.GetCharacterGlyphs()[5].GetSimpleAdvance() as nscoord, space);
        // one glyph run covers the text
        assert_eq!(r.GlyphRunCount(), 1);
        assert_eq!(r.FindFirstGlyphRunContaining(0), Some(0));
        assert_eq!(r.FindFirstGlyphRunContaining(11), Some(0));
    }

    #[test]
    fn a_single_space_and_the_empty_string_make_text_runs() {
        let g = group(16.0);
        let space = make(&g, " ", ShapedTextFlags::default());
        assert_eq!(space.borrow().GetLength(), 1);
        assert!(space.borrow().CharIsSpace(0));
        assert!(space.borrow().GetAdvanceWidth__() > 0.0);
        let empty = g
            .MakeTextRun__T_ptr_uint32_t_DrawTarget_ptr_int32_t_ShapedTextFlags_Flags_gfxMissingFontRecorder_ptr(
                &[] as &[u16], 0, None, 60, ShapedTextFlags::default(), nsTextFrameUtils_Flags::default(), None,
            )
            .unwrap();
        assert_eq!(empty.borrow().GetLength(), 0);
        assert_eq!(empty.borrow().GetAdvanceWidth__(), 0.0);
    }

    #[test]
    fn eight_bit_text_has_the_widths_of_sixteen_bit_text() {
        let g = group(16.0);
        let s = "Hello, world";
        let wide = make(&g, s, ShapedTextFlags::default());
        let bytes: Vec<u8> = s.bytes().collect();
        let narrow = g
            .MakeTextRun__T_ptr_uint32_t_DrawTarget_ptr_int32_t_ShapedTextFlags_Flags_gfxMissingFontRecorder_ptr(
                &bytes, bytes.len() as u32, None, 60, ShapedTextFlags::default(), nsTextFrameUtils_Flags::default(), None,
            )
            .unwrap();
        assert!(narrow.borrow().TextIs8Bit());
        assert_eq!(narrow.borrow().GetAdvanceWidth__(), wide.borrow().GetAdvanceWidth__());
    }

    #[test]
    fn tabs_and_newlines_are_recorded() {
        let g = group(16.0);
        let run = make(&g, "a\tb\nc", ShapedTextFlags::default());
        let r = run.borrow();
        assert!(r.CharIsTab(1) && !r.CharIsTab(0));
        assert!(r.CharIsNewline(3) && !r.CharIsNewline(2));
        // a tab or newline takes no advance of its own
        assert_eq!(r.GetAdvanceForGlyph(1, 0), 0);
        assert_eq!(r.GetAdvanceForGlyph(3, 0), 0);
    }

    #[test]
    fn a_combining_mark_continues_the_cluster_of_its_base() {
        let g = group(16.0);
        let run = make(&g, "e\u{0301}x", ShapedTextFlags::default());
        let r = run.borrow();
        assert!(r.IsClusterStart(0) && !r.IsClusterStart(1) && r.IsClusterStart(2));
        drop(r);
        let all = width_of(&run, 0, 3);
        assert_eq!(all, width_of(&run, 0, 2) + width_of(&run, 2, 3));
        assert!(width_of(&run, 2, 3) > 0.0);
    }

    #[test]
    fn the_line_breaker_sets_the_break_flags_and_a_second_call_changes_nothing() {
        let g = group(16.0);
        let run = make_with_breaks(&g, "aaa bbb ccc", ShapedTextFlags::default());
        {
            let r = run.borrow();
            let breaks: Vec<u32> = (0..11).filter(|&i| r.CanBreakLineBefore(i)).collect();
            assert_eq!(breaks, vec![4, 8]);
            assert!(!r.CanHyphenateBefore(4));
        }
        // setting the same breaks again changes nothing (the return value says so)
        let mut flags = vec![0u8; 11];
        flags[4] = 1;
        flags[8] = 1;
        assert!(!run.borrow_mut().SetPotentialLineBreaks(gfxTextRun_Range::new(0, 11), &flags));
        flags[2] = 1;
        assert!(run.borrow_mut().SetPotentialLineBreaks(gfxTextRun_Range::new(0, 11), &flags));
        assert!(run.borrow().CanBreakLineBefore(2));
    }

    #[test]
    fn a_break_inside_a_cluster_is_ignored() {
        let g = group(16.0);
        let run = make(&g, "e\u{0301}x", ShapedTextFlags::default());
        let mut flags = vec![0u8; 3];
        flags[1] = 1; // before the combining mark
        assert!(!run.borrow_mut().SetPotentialLineBreaks(gfxTextRun_Range::new(0, 3), &flags));
        assert!(!run.borrow().CanBreakLineBefore(1));
    }

    #[test]
    fn break_and_measure_stops_at_the_last_break_that_fits() {
        let g = group(16.0);
        let run = make_with_breaks(&g, "aaa bbb ccc ddd", ShapedTextFlags::default());
        let p = TestProvider::default();
        let w_aaa_bbb = width_of(&run, 0, 7);
        let w_aaa_bbb_sp = width_of(&run, 0, 8);
        let mut prio = gfxBreakPriority::eNoBreak;
        let (fit, trim, metrics, hyph, _) =
            break_and_measure(&run, 0, u32::MAX, w_aaa_bbb + 0.5, &p, gfxTextRun_SuppressBreak::eNoSuppressBreak, false, &mut prio);
        // "aaa bbb " fits; the trailing space is counted in the characters that fit
        assert_eq!(fit, 8);
        assert!(!hyph);
        assert_eq!(prio, gfxBreakPriority::eNormalBreak);
        // the metrics include the trailing space; the trailing space is reported as trimmable
        assert_eq!(metrics.mAdvanceWidth, w_aaa_bbb_sp);
        assert_eq!(trim.mCount, 1);
        assert_eq!(trim.mAdvance, w_aaa_bbb_sp - w_aaa_bbb);
        // an ascent and a descent come with the metrics
        assert!(metrics.mAscent > 0.0 && metrics.mDescent > 0.0);
    }

    #[test]
    fn break_and_measure_takes_everything_if_it_fits_and_reports_the_last_break() {
        let g = group(16.0);
        let run = make_with_breaks(&g, "aaa bbb ccc ddd", ShapedTextFlags::default());
        let p = TestProvider::default();
        let mut prio = gfxBreakPriority::eNoBreak;
        let (fit, _trim, metrics, _hyph, lastBreak) =
            break_and_measure(&run, 0, u32::MAX, 100000.0, &p, gfxTextRun_SuppressBreak::eNoSuppressBreak, false, &mut prio);
        assert_eq!(fit, 15);
        assert_eq!(metrics.mAdvanceWidth, width_of(&run, 0, 15));
        // the last break that fits: before "ddd"
        assert_eq!(lastBreak, 12);
    }

    #[test]
    fn break_and_measure_with_a_width_too_small_takes_up_to_the_first_break() {
        let g = group(16.0);
        let run = make_with_breaks(&g, "aaaa bbbb", ShapedTextFlags::default());
        let p = TestProvider::default();
        let mut prio = gfxBreakPriority::eNoBreak;
        let (fit, trim, _m, _h, _l) = break_and_measure(&run, 0, u32::MAX, 1.0, &p, gfxTextRun_SuppressBreak::eNoSuppressBreak, false, &mut prio);
        // "none fits": the smallest N with a break, "aaaa " (5), the first line must make progress
        assert_eq!(fit, 5);
        assert_eq!(trim.mCount, 1);
    }

    #[test]
    fn break_and_measure_continues_from_a_start_offset() {
        let g = group(16.0);
        let run = make_with_breaks(&g, "aaa bbb ccc ddd", ShapedTextFlags::default());
        let p = TestProvider::default();
        let mut prio = gfxBreakPriority::eNoBreak;
        let w = width_of(&run, 8, 15) - 0.5;
        let (fit, trim, metrics, _h, _l) = break_and_measure(&run, 8, u32::MAX, w, &p, gfxTextRun_SuppressBreak::eNoSuppressBreak, false, &mut prio);
        // "ccc ddd" is a little too wide: "ccc " goes on this line
        assert_eq!(fit, 4);
        assert_eq!(trim.mCount, 1);
        assert_eq!(metrics.mAdvanceWidth, width_of(&run, 8, 12));
        // the limit aMaxLength cuts the run short
        let (fit, _t, _m, _h, _l) = break_and_measure(&run, 8, 3, 100000.0, &p, gfxTextRun_SuppressBreak::eNoSuppressBreak, false, &mut prio);
        assert_eq!(fit, 3);
    }

    #[test]
    fn suppress_all_breaks_measures_the_whole_text() {
        let g = group(16.0);
        let run = make_with_breaks(&g, "aaa bbb ccc", ShapedTextFlags::default());
        let p = TestProvider::default();
        let mut prio = gfxBreakPriority::eNoBreak;
        let (fit, _t, _m, _h, _l) = break_and_measure(&run, 0, u32::MAX, 1.0, &p, gfxTextRun_SuppressBreak::eSuppressAllBreaks, false, &mut prio);
        assert_eq!(fit, 11);
    }

    #[test]
    fn suppress_initial_break_ignores_a_break_at_the_start() {
        let g = group(16.0);
        // the break before "bbb" (index 4) is the first break opportunity of the range starting there
        let run = make_with_breaks(&g, "aaa bbb ccc", ShapedTextFlags::default());
        let p = TestProvider::default();
        let mut prio = gfxBreakPriority::eNoBreak;
        let (fit, _t, _m, _h, _l) = break_and_measure(&run, 4, u32::MAX, 1.0, &p, gfxTextRun_SuppressBreak::eSuppressInitialBreak, false, &mut prio);
        // no break at 4 is used; the next one, before "ccc", ends the range
        assert_eq!(fit, 4);
        let (fit2, _t, _m, _h, _l) = break_and_measure(&run, 4, u32::MAX, 1.0, &p, gfxTextRun_SuppressBreak::eNoSuppressBreak, false, &mut prio);
        assert_eq!(fit2, 0, "a break at the start with no room gives zero characters");
    }

    #[test]
    fn overflow_wrap_breaks_a_long_word_between_clusters() {
        let g = group(16.0);
        let run = make_with_breaks(&g, "abcdefghij", ShapedTextFlags::default());
        let p = TestProvider::default();
        let mut prio = gfxBreakPriority::eNoBreak;
        let w5 = width_of(&run, 0, 5);
        let (fit, _t, _m, _h, _l) =
            break_and_measure(&run, 0, u32::MAX, w5 + 0.5, &p, gfxTextRun_SuppressBreak::eSuppressInitialBreak, true, &mut prio);
        assert_eq!(fit, 5);
        assert_eq!(prio, gfxBreakPriority::eWordWrapBreak);
        // without overflow-wrap the word does not break
        let mut prio = gfxBreakPriority::eNoBreak;
        let (fit, _t, _m, _h, _l) =
            break_and_measure(&run, 0, u32::MAX, w5 + 0.5, &p, gfxTextRun_SuppressBreak::eSuppressInitialBreak, false, &mut prio);
        assert_eq!(fit, 10);
    }

    #[test]
    fn a_soft_hyphen_break_counts_the_hyphen_width() {
        let g = group(16.0);
        let flags = ShapedTextFlags::TEXT_ENABLE_HYPHEN_BREAKS;
        let run = make_with_breaks(&g, "abcdefgh", flags);
        let p = TestProvider { mHyphens: Some(StyleHyphens::Manual), mHyphenWidth: 300.0, mSoftHyphenBefore: vec![4], ..Default::default() };
        let w4 = width_of(&run, 0, 4);
        let mut prio = gfxBreakPriority::eNoBreak;
        // room for "abcd" and the hyphen: break there
        let (fit, _t, _m, hyph, _l) =
            break_and_measure(&run, 0, u32::MAX, w4 + 300.0 + 0.5, &p, gfxTextRun_SuppressBreak::eNoSuppressBreak, false, &mut prio);
        assert_eq!((fit, hyph), (4, true));
        // not enough room for the hyphen after "abcd": the break is still the only choice, but it is
        // not preferred over the whole text fitting; with the whole text fitting no hyphen is used
        let wall = width_of(&run, 0, 8);
        let (fit, _t, _m, hyph, _l) =
            break_and_measure(&run, 0, u32::MAX, wall + 0.5, &p, gfxTextRun_SuppressBreak::eNoSuppressBreak, false, &mut prio);
        assert_eq!((fit, hyph), (8, false));
    }

    #[test]
    fn letter_spacing_is_added_to_every_character() {
        let g = group(16.0);
        let run = make(&g, "abcd", ShapedTextFlags::TEXT_ENABLE_SPACING);
        let plain = width_of(&run, 0, 4);
        let p = TestProvider { mLetterSpacing: 30, ..Default::default() };
        let spaced = run.borrow().GetAdvanceWidth__Range_PropertyProvider_ptr_Spacing_ptr(gfxTextRun_Range::new(0, 4), Some(&p), None);
        assert_eq!(spaced, plain + 4.0 * 30.0);
        // the spacing before the first and after the last character comes back with the width
        let mut spacing = gfxFont_Spacing::default();
        run.borrow().GetAdvanceWidth__Range_PropertyProvider_ptr_Spacing_ptr(gfxTextRun_Range::new(1, 3), Some(&p), Some(&mut spacing));
        assert_eq!((spacing.mBefore, spacing.mAfter), (0, 30));
        // BreakAndMeasureText sees the spacing too
        let mut prio = gfxBreakPriority::eNoBreak;
        let (_fit, _t, metrics, _h, _l) = break_and_measure(&run, 0, u32::MAX, 100000.0, &p, gfxTextRun_SuppressBreak::eNoSuppressBreak, false, &mut prio);
        assert_eq!(metrics.mAdvanceWidth, plain + 120.0);
    }

    #[test]
    fn measure_text_gives_the_ascent_and_descent_of_the_font() {
        let g = group(16.0);
        let run = make(&g, "Hello", ShapedTextFlags::default());
        let m = run.borrow().MeasureText__BoundingBoxType_DrawTarget_ptr_PropertyProvider_ptr(gfxFont_BoundingBoxType::LOOSE_INK_EXTENTS, None, None);
        let fm = *g.GetFirstValidFont(gfxFontGroup::kCSSFirstAvailableFont, None, None).GetMetrics();
        assert_eq!(m.mAscent, fm.maxAscent * 60.0);
        assert_eq!(m.mDescent, fm.maxDescent * 60.0);
        assert!(m.mAscent > 0.0 && m.mDescent > 0.0);
        assert_eq!(m.mAdvanceWidth, run.borrow().GetAdvanceWidth__());
        // the loose bounding box is the font box over the advance
        assert_eq!(m.mBoundingBox.Y(), -m.mAscent);
        assert_eq!(m.mBoundingBox.Height(), m.mAscent + m.mDescent);
        assert_eq!(m.mBoundingBox.X(), 0.0);
        assert_eq!(m.mBoundingBox.XMost(), m.mAdvanceWidth);
        let (mut a, mut d) = (0.0, 0.0);
        run.borrow().GetLineHeightMetrics__gfxFloat_ref_gfxFloat_ref(&mut a, &mut d);
        assert_eq!((a, d), (m.mAscent, m.mDescent));
    }

    #[test]
    fn measure_text_of_parts_adds_up() {
        let g = group(16.0);
        let run = make(&g, "Hello world", ShapedTextFlags::default());
        let r = run.borrow();
        let b = gfxFont_BoundingBoxType::LOOSE_INK_EXTENTS;
        let left = r.MeasureText__Range_BoundingBoxType_DrawTarget_ptr_PropertyProvider_ptr(gfxTextRun_Range::new(0, 5), b, None, None);
        let right = r.MeasureText__Range_BoundingBoxType_DrawTarget_ptr_PropertyProvider_ptr(gfxTextRun_Range::new(5, 11), b, None, None);
        let all = r.MeasureText__BoundingBoxType_DrawTarget_ptr_PropertyProvider_ptr(b, None, None);
        assert_eq!(left.mAdvanceWidth + right.mAdvanceWidth, all.mAdvanceWidth);
    }

    #[test]
    fn the_minimum_width_is_the_widest_character() {
        let g = group(16.0);
        let run = make(&g, "ii WW ii", ShapedTextFlags::default());
        let r = run.borrow();
        let min = r.GetMinAdvanceWidth(gfxTextRun_Range::new(0, 8), 0);
        let widest = (0..8).map(|i| r.GetAdvanceForGlyph(i, 0)).max().unwrap() as f64;
        assert_eq!(min, widest);
        assert!(min > 0.0);
    }

    #[test]
    fn wrapping_a_paragraph_fills_lines_without_overflow() {
        // The loop nsTextFrame runs: BreakAndMeasureText from where the last line stopped, with the
        // available width, taking off the trailing space. Properties of a correct wrap, not fixed numbers.
        let g = group(16.0);
        let text = "The quick brown fox jumps over the lazy dog and keeps running through the forest";
        let run = make_with_breaks(&g, text, ShapedTextFlags::default());
        let len = text.len() as u32;
        let p = TestProvider::default();
        let avail = width_of(&run, 0, 22); // roughly 22 characters per line
        let mut start = 0u32;
        let mut lines: Vec<(u32, u32)> = Vec::new();
        while start < len {
            let mut prio = gfxBreakPriority::eNoBreak;
            let (fit, trim, metrics, _h, _l) =
                break_and_measure(&run, start, u32::MAX, avail, &p, gfxTextRun_SuppressBreak::eNoSuppressBreak, false, &mut prio);
            assert!(fit > 0, "a line makes progress");
            // the line's width without the trailing space fits (every word here is narrower than the line)
            assert!(metrics.mAdvanceWidth - trim.mAdvance <= avail, "line {start}+{fit}");
            lines.push((start, fit));
            start += fit;
        }
        assert!(lines.len() >= 3, "{lines:?}");
        assert_eq!(lines.iter().map(|l| l.1).sum::<u32>(), len);
        // greedy: the first word of the next line did not fit on the line before
        for pair in lines.windows(2) {
            let (s, n) = pair[0];
            let next_word_end = (pair[1].0..len).find(|&i| text.as_bytes()[i as usize] == b' ').map_or(len, |i| i + 1);
            let too_wide = width_of(&run, s, next_word_end) - trailing_space(&run, next_word_end);
            assert!(too_wide > avail, "line {s}+{n} could have taken the next word");
        }
    }

    fn trailing_space(aRun: &RefPtr_gfxTextRun, aEnd: u32) -> gfxFloat {
        let r = aRun.borrow();
        if aEnd > 0 && r.CharIsSpace(aEnd - 1) { r.GetAdvanceForGlyph(aEnd - 1, 0) as f64 } else { 0.0 }
    }

    #[test]
    fn a_font_group_gives_the_same_font_for_the_same_style() {
        let a = group(16.0);
        let b = group(16.0);
        let c = group(17.0);
        let fa = a.GetFirstValidFont(gfxFontGroup::kCSSFirstAvailableFont, None, None);
        let fb = b.GetFirstValidFont(gfxFontGroup::kCSSFirstAvailableFont, None, None);
        let fc = c.GetFirstValidFont(gfxFontGroup::kCSSFirstAvailableFont, None, None);
        assert!(Rc::ptr_eq(&fa, &fb));
        assert!(!Rc::ptr_eq(&fa, &fc));
        assert!(fa.HasCharacter('a' as u32));
        assert!(a.GetUnderlineOffset() < 0.0, "the underline is below the baseline");
    }

    // The runs of a text run: (character offset, font) of each glyph run
    fn font_runs(aRun: &RefPtr_gfxTextRun) -> Vec<(u32, Rc<gfxFont>)> {
        let r = aRun.borrow();
        let mut n = 0;
        r.GetGlyphRuns(&mut n).iter().map(|g| (g.mCharacterOffset, g.mFont.clone())).collect()
    }

    // Written from the documented behaviour of gfxFontGroup::FindFontForChar and ComputeRanges (a character that
    // the first font lacks is shaped by the font the system fallback names for it, and the runs of characters in
    // the same font are one glyph run). They run only where an installed font has the character (the CJK one).
    #[test]
    fn a_character_the_first_font_lacks_gets_a_fallback_font_in_its_own_glyph_run() {
        let g = group(16.0);
        let first = g.GetFirstValidFont(gfxFontGroup::kCSSFirstAvailableFont, None, None);
        let han = 0x4E00u32;
        if first.HasCharacter(han) || gfxFontCache_LookupFallback(han, g.GetStyle()).is_none() {
            eprintln!("skipped: the first font has U+4E00, or no installed font does");
            return;
        }
        let run = make(&g, "ab\u{4E00}\u{4E8C}cd", ShapedTextFlags::default());
        let runs = font_runs(&run);
        // Latin, the two Han characters, Latin: three glyph runs, the first and last in the first font
        assert_eq!(runs.len(), 3, "{:?}", runs.iter().map(|r| r.0).collect::<Vec<_>>());
        assert_eq!(runs.iter().map(|r| r.0).collect::<Vec<_>>(), vec![0, 2, 4]);
        assert!(Rc::ptr_eq(&runs[0].1, &first) && Rc::ptr_eq(&runs[2].1, &first));
        assert!(!Rc::ptr_eq(&runs[1].1, &first) && runs[1].1.HasCharacter(han));
        // the text has a width: every character got a glyph
        assert!(run.borrow().GetAdvanceWidth__Range_PropertyProvider_ptr_Spacing_ptr(gfxTextRun_Range::new(0, 6), None, None) > 0.0);
    }

    #[test]
    fn text_in_the_first_font_stays_one_glyph_run() {
        let g = group(16.0);
        let run = make(&g, "plain latin text", ShapedTextFlags::default());
        assert_eq!(font_runs(&run).len(), 1);
    }

    #[test]
    fn a_combining_mark_stays_in_the_font_of_its_base_character() {
        // FindFontForChar: a cluster extender takes the previous character's font if that has it (the base is
        // Han in a fallback font here; the mark U+0301 is in the first font too, but must not split the cluster)
        let g = group(16.0);
        let first = g.GetFirstValidFont(gfxFontGroup::kCSSFirstAvailableFont, None, None);
        let han = 0x4E00u32;
        if first.HasCharacter(han) || !first.HasCharacter(0x301) {
            return;
        }
        let Some(fallback) = gfxFontCache_LookupFallback(han, g.GetStyle()) else { return };
        if !fallback.HasCharacter(0x301) {
            return; // the rule only applies when the base's font has the mark
        }
        let run = make(&g, "\u{4E00}\u{0301}", ShapedTextFlags::default());
        assert_eq!(font_runs(&run).len(), 1);
    }

    // CSS: a control character takes the font before it, whether the font has it or not (it is not drawn)
    #[test]
    fn a_zero_width_joiner_does_not_start_a_new_glyph_run() {
        let g = group(16.0);
        let first = g.GetFirstValidFont(gfxFontGroup::kCSSFirstAvailableFont, None, None);
        let han = 0x4E00u32;
        if first.HasCharacter(han) || gfxFontCache_LookupFallback(han, g.GetStyle()).is_none() {
            return;
        }
        let run = make(&g, "\u{4E00}\u{200D}\u{4E8C}", ShapedTextFlags::default());
        assert_eq!(font_runs(&run).len(), 1);
    }
}
