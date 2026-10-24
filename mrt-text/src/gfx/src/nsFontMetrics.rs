//! nsFontMetrics: the font metrics in app units, with Gecko's rounding for each value.
use crate::{gfxFont, gfxFont_Metrics, nscoord};

// Gecko: gfx/src/nsFontMetrics.h nsFontMetrics
/// Wraps a gfxFont. `mP2A` is app units per CSS px (60), as in Gecko.
pub struct nsFontMetrics<'a> {
    mFont: &'a gfxFont,
    mP2A: f64,
}

// Gecko: gfx/src/nsFontMetrics.cpp ROUND_TO_TWIPS / CEIL_TO_TWIPS
fn ROUND_TO_TWIPS(x: f64, mP2A: f64) -> nscoord { (x * mP2A + 0.5).floor() as nscoord }
fn CEIL_TO_TWIPS(x: f64, mP2A: f64) -> nscoord { (x * mP2A).ceil() as nscoord }

// Gecko: gfx/src/nsFontMetrics.cpp ComputeMaxDescent. aUnderlineOffset stands in for
// aFontGroup->GetUnderlineOffset(): the underline offset of the first font.
fn ComputeMaxDescent(aMetrics: &gfxFont_Metrics, aUnderlineOffset: f64) -> f64 {
    let offset = (-aUnderlineOffset + 0.5).floor();
    let size = aMetrics.underlineSize.round();
    let minDescent = offset + size;
    (minDescent.max(aMetrics.maxDescent) + 0.5).floor()
}

// Gecko: gfx/src/nsFontMetrics.cpp ComputeMaxAscent
fn ComputeMaxAscent(aMetrics: &gfxFont_Metrics) -> f64 { (aMetrics.maxAscent + 0.5).floor() }

impl<'a> nsFontMetrics<'a> {
    pub fn new(aFont: &'a gfxFont) -> Self {
        nsFontMetrics { mFont: aFont, mP2A: crate::AppUnitsPerCSSPixel as f64 }
    }
    fn m(&self) -> &gfxFont_Metrics { self.mFont.GetMetrics() }

    pub fn XHeight(&self) -> nscoord { ROUND_TO_TWIPS(self.m().xHeight, self.mP2A) }
    pub fn CapHeight(&self) -> nscoord { ROUND_TO_TWIPS(self.m().capHeight, self.mP2A) }
    pub fn InternalLeading(&self) -> nscoord { ROUND_TO_TWIPS(self.m().internalLeading, self.mP2A) }
    pub fn ExternalLeading(&self) -> nscoord { ROUND_TO_TWIPS(self.m().externalLeading, self.mP2A) }
    pub fn EmHeight(&self) -> nscoord { ROUND_TO_TWIPS(self.m().emHeight, self.mP2A) }
    pub fn EmAscent(&self) -> nscoord { ROUND_TO_TWIPS(self.m().emAscent, self.mP2A) }
    pub fn EmDescent(&self) -> nscoord { ROUND_TO_TWIPS(self.m().emDescent, self.mP2A) }
    pub fn MaxHeight(&self) -> nscoord { self.MaxAscent() + self.MaxDescent() }
    pub fn MaxAscent(&self) -> nscoord { CEIL_TO_TWIPS(ComputeMaxAscent(self.m()), self.mP2A) }
    pub fn MaxDescent(&self) -> nscoord {
        CEIL_TO_TWIPS(ComputeMaxDescent(self.m(), self.m().underlineOffset), self.mP2A)
    }
    pub fn MaxAdvance(&self) -> nscoord { CEIL_TO_TWIPS(self.m().maxAdvance, self.mP2A) }
    /// Use CEIL instead of ROUND for consistency with MaxAdvance.
    pub fn AveCharWidth(&self) -> nscoord { CEIL_TO_TWIPS(self.m().aveCharWidth, self.mP2A) }
    pub fn ZeroOrAveCharWidth(&self) -> nscoord { CEIL_TO_TWIPS(self.m().ZeroOrAveCharWidth(), self.mP2A) }
    pub fn SpaceWidth(&self) -> nscoord { CEIL_TO_TWIPS(self.m().spaceWidth, self.mP2A) }

    /// Width of a UTF-16 string, shaped left-to-right with the default features.
    /// Gecko: nsFontMetrics::GetWidth(const char16_t*, ...) (through a gfxTextRun).
    pub fn GetWidth(&self, aString: &[u16]) -> nscoord {
        self.mFont.ShapeText(aString, false, None).mWidth
    }

    pub fn GetWidthUtf8(&self, aString: &str) -> nscoord {
        let utf16: Vec<u16> = aString.encode_utf16().collect();
        self.GetWidth(&utf16)
    }
}
