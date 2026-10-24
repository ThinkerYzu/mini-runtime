//! gfxFontEntry (one font file and its face), gfxFont (a face at a size: metrics and shaping).
//!
//! Metrics follow what Firefox does on Linux, gfxFT2FontBase::InitMetrics plus gfxFont::SanitizeMetrics,
//! but without FreeType: the few FreeType steps that change a number (the 26.6 size, the scale, the
//! ascender and descender rounding) are redone here in integers. Shaping is gfxHarfBuzzShaper's: the
//! cluster walk in SetGlyphsFromRun, unrounded advances (subpixel positioning, Firefox's default on Linux).
use crate::harfbuzz::*;
use crate::{FindFontFile, FindFontFileForLanguage, FontFile, nscoord};
use std::ffi::CString;
use std::rc::Rc;

// ---- sfnt table access ------------------------------------------------------------------------

fn be16(d: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*d.get(o)?, *d.get(o + 1)?]))
}
fn be16s(d: &[u8], o: usize) -> Option<i16> {
    be16(d, o).map(|v| v as i16)
}

/// A table of the font, copied out of HarfBuzz's blob. Gecko: gfxFontEntry::AutoTable.
pub struct AutoTable(pub Vec<u8>);

impl std::ops::Deref for AutoTable {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

// ---- gfxFontEntry -----------------------------------------------------------------------------

// Gecko: gfx/thebes/gfxFontEntry.h gfxFontEntry
/// One face of one font file: the HarfBuzz face over the mmapped file, plus numbers read once from 'head'.
pub struct gfxFontEntry {
    pub mFile: FontFile,
    mFace: *mut hb_face_t,
    mBlob: *mut hb_blob_t,
    pub mUnitsPerEm: u16,
    mIsCFF: bool,
}

impl Drop for gfxFontEntry {
    fn drop(&mut self) {
        unsafe {
            hb_face_destroy(self.mFace);
            hb_blob_destroy(self.mBlob);
        }
    }
}

impl gfxFontEntry {
    pub const kInvalidUPEM: u16 = 0;

    // Gecko: gfx/thebes/gfxFontEntry.cpp gfxFontEntry::GetHBFace (the file is mapped, not read, by HarfBuzz)
    pub fn new(aFile: FontFile) -> Option<Rc<gfxFontEntry>> {
        let path = CString::new(aFile.mPath.clone()).ok()?;
        unsafe {
            let blob = hb_blob_create_from_file_or_fail(path.as_ptr());
            if blob.is_null() {
                return None;
            }
            let face = hb_face_create(blob, aFile.mIndex);
            let upem = hb_face_get_upem(face) as u16;
            let mut entry = gfxFontEntry {
                mFile: aFile, mFace: face, mBlob: blob, mUnitsPerEm: upem, mIsCFF: false,
            };
            entry.mIsCFF = entry.HasFontTable(*b"CFF ") || entry.HasFontTable(*b"CFF2");
            if upem == Self::kInvalidUPEM {
                return None;
            }
            Some(Rc::new(entry))
        }
    }

    /// Open the font for a CSS family list, weight and style (through fc-match).
    pub fn Lookup(aFamilies: &[&str], aWeight: f32, aItalic: bool) -> Option<Rc<gfxFontEntry>> {
        Self::new(FindFontFile(aFamilies, aWeight, aItalic)?)
    }

    /// Lookup for text of a language.
    pub fn LookupForLanguage(aFamilies: &[&str], aWeight: f32, aItalic: bool, aLanguage: Option<&str>) -> Option<Rc<gfxFontEntry>> {
        Self::new(FindFontFileForLanguage(aFamilies, aWeight, aItalic, aLanguage)?)
    }

    pub fn GetHBFace(&self) -> *mut hb_face_t {
        self.mFace
    }

    pub fn UnitsPerEm(&self) -> u16 {
        self.mUnitsPerEm
    }

    // Gecko: gfx/thebes/gfxFontEntry.cpp gfxFontEntry::GetFontTable
    pub fn GetFontTable(&self, aTag: [u8; 4]) -> Option<AutoTable> {
        unsafe {
            let blob = hb_face_reference_table(self.mFace, HB_TAG(aTag[0], aTag[1], aTag[2], aTag[3]));
            let mut len = 0u32;
            let data = hb_blob_get_data(blob, &mut len);
            let table = if len == 0 { None } else { Some(AutoTable(std::slice::from_raw_parts(data as *const u8, len as usize).to_vec())) };
            hb_blob_destroy(blob);
            table
        }
    }

    pub fn HasFontTable(&self, aTag: [u8; 4]) -> bool {
        self.GetFontTable(aTag).is_some()
    }
}

// ---- gfxFont::Metrics -------------------------------------------------------------------------

// Gecko: gfx/thebes/gfxFont.h gfxFont::Metrics
/// Font metrics in CSS pixels (not app units), as gfxFont keeps them.
#[derive(Clone, Copy, Debug, Default)]
pub struct gfxFont_Metrics {
    pub capHeight: f64,
    pub xHeight: f64,
    pub maxAscent: f64,
    pub maxDescent: f64,
    pub maxHeight: f64,
    pub internalLeading: f64,
    pub externalLeading: f64,
    pub emHeight: f64,
    pub emAscent: f64,
    pub emDescent: f64,
    pub maxAdvance: f64,
    pub aveCharWidth: f64,
    pub spaceWidth: f64,
    pub zeroWidth: f64,
    pub ideographicWidth: f64,
    pub underlineOffset: f64,
    pub underlineSize: f64,
    pub strikeoutOffset: f64,
    pub strikeoutSize: f64,
}

impl gfxFont_Metrics {
    // Gecko: gfx/thebes/gfxFont.h gfxFont::Metrics::ZeroOrAveCharWidth
    pub fn ZeroOrAveCharWidth(&self) -> f64 {
        if self.zeroWidth >= 0.0 { self.zeroWidth } else { self.aveCharWidth }
    }
}

// ---- FreeType's integer arithmetic, as far as the metrics need it ---------------------------------

// FT_MulFix: a * b / 65536, rounded to nearest, half away from zero.
fn FT_MulFix(a: i64, b: i64) -> i64 {
    let p = a * b;
    if p >= 0 { (p + 0x8000) >> 16 } else { -((-p + 0x8000) >> 16) }
}
// FT_DivFix: a * 65536 / b, rounded to nearest.
fn FT_DivFix(a: i64, b: i64) -> i64 {
    let s = if (a < 0) != (b < 0) { -1 } else { 1 };
    let (a, b) = (a.abs(), b.abs());
    s * (((a << 16) + (b >> 1)) / b)
}
fn FT_PIX_ROUND(x: i64) -> i64 { (x + 32) & !63 }
fn FT_PIX_CEIL(x: i64) -> i64 { (x + 63) & !63 }
fn FT_PIX_FLOOR(x: i64) -> i64 { x & !63 }
fn FLOAT_FROM_26_6(x: i64) -> f64 { x as f64 / 64.0 }

// Gecko: gfx/thebes/gfxFont.cpp RoundToNearestMultiple is not used on this path; NS_round:
fn NS_round(x: f64) -> f64 { x.round() }

// ---- gfxFont ----------------------------------------------------------------------------------

// Gecko: gfx/thebes/gfxFont.h gfxFontStyle
#[derive(Clone, Copy, Debug)]
pub struct gfxFontStyle {
    /// Font size in CSS px.
    pub size: f64,
    pub weight: f32,
    pub italic: bool,
}

impl Default for gfxFontStyle {
    fn default() -> Self {
        gfxFontStyle { size: 16.0, weight: 400.0, italic: false }
    }
}

/// Result of shaping a run: what gfxTextRun keeps per character, without the compression.
#[derive(Clone, Debug, Default)]
pub struct ShapedRun {
    /// One entry per UTF-16 unit: advance in app units, on the first unit of a cluster; 0 on the
    /// other units of the cluster (like the ligature continuations in SetGlyphsFromRun).
    pub mCharAdvance: Vec<nscoord>,
    /// Advance of every glyph in app units, in the shaper's order.
    pub mGlyphAdvance: Vec<nscoord>,
    pub mGlyphs: Vec<mrt_glyph>,
    /// Sum of mCharAdvance.
    pub mWidth: nscoord,
    /// The "clumps" SetGlyphsFromRun walks: a run of characters and the glyphs HarfBuzz made for it
    /// (a ligature, a base with its marks, or one character with one glyph). Added for mrt-layout's
    /// gfxTextRun, which needs the character-to-glyph association to fill its glyph records.
    pub mClumps: Vec<ShapedClump>,
}

/// One character-to-glyph clump of a ShapedRun: characters [mCharStart, mCharEnd) are drawn by
/// glyphs [mGlyphStart, mGlyphEnd) of ShapedRun::mGlyphs.
#[derive(Clone, Copy, Debug, Default)]
pub struct ShapedClump {
    pub mCharStart: u32,
    pub mCharEnd: u32,
    pub mGlyphStart: u32,
    pub mGlyphEnd: u32,
}

// Gecko: gfx/thebes/gfxFont.h gfxFont
pub struct gfxFont {
    mFontEntry: Rc<gfxFontEntry>,
    mStyle: gfxFontStyle,
    mHBFont: *mut hb_font_t,
    mMetrics: gfxFont_Metrics,
    mIsValid: bool,
    mAppUnitsPerDevUnit: i32,
}

impl Drop for gfxFont {
    fn drop(&mut self) {
        unsafe { hb_font_destroy(self.mHBFont) }
    }
}

impl gfxFont {
    pub fn new(aFontEntry: Rc<gfxFontEntry>, aStyle: gfxFontStyle) -> gfxFont {
        // Firefox's glyph widths come from FreeType (gfxFT2FontBase::GetGlyphWidth), whose size is the
        // font size rounded to 1/64 px (FT_Set_Char_Size); the HarfBuzz font is given the same size so
        // the advances agree. At 13.4px this is 13.40625px, about 0.05% wider.
        let hb = unsafe { mrt_create_hb_font(aFontEntry.GetHBFace(), (aStyle.size.max(1.0) * 64.0 + 0.5).floor() / 64.0) };
        let mut font = gfxFont {
            mFontEntry: aFontEntry, mStyle: aStyle, mHBFont: hb, mMetrics: Default::default(),
            mIsValid: false, mAppUnitsPerDevUnit: crate::AppUnitsPerCSSPixel,
        };
        font.InitMetrics();
        font
    }

    /// Look up a font by CSS family list and style and make the gfxFont for it.
    pub fn Lookup(aFamilies: &[&str], aStyle: gfxFontStyle) -> Option<gfxFont> {
        Some(gfxFont::new(gfxFontEntry::Lookup(aFamilies, aStyle.weight, aStyle.italic)?, aStyle))
    }

    /// Lookup for text of a language.
    pub fn LookupForLanguage(aFamilies: &[&str], aStyle: gfxFontStyle, aLanguage: Option<&str>) -> Option<gfxFont> {
        Some(gfxFont::new(gfxFontEntry::LookupForLanguage(aFamilies, aStyle.weight, aStyle.italic, aLanguage)?, aStyle))
    }

    pub fn GetFontEntry(&self) -> &Rc<gfxFontEntry> { &self.mFontEntry }
    pub fn GetStyle(&self) -> &gfxFontStyle { &self.mStyle }
    pub fn GetMetrics(&self) -> &gfxFont_Metrics { &self.mMetrics }
    pub fn IsValid(&self) -> bool { self.mIsValid }

    /// The glyph for U+0020, 0 if the font has none. Gecko: gfxFont::GetSpaceGlyph.
    pub fn GetSpaceGlyph(&self) -> u32 { self.GetNominalGlyph(0x20) }

    /// Whether the font has a glyph for the character. Gecko: gfxFont::HasCharacter.
    pub fn HasCharacter(&self, aCh: u32) -> bool { self.GetNominalGlyph(aCh) != 0 }

    pub fn GetNominalGlyph(&self, aCh: u32) -> u32 {
        let mut gid = 0u32;
        unsafe { hb_font_get_nominal_glyph(self.mHBFont, aCh, &mut gid) };
        gid
    }

    /// Advance of a character in CSS px, from the font's own tables (no shaping).
    /// Gecko: gfx/thebes/gfxFT2FontBase.cpp gfxFT2FontBase::GetCharExtents, the width part.
    fn GetCharAdvance(&self, aCh: u32) -> Option<f64> {
        let gid = self.GetNominalGlyph(aCh);
        if gid == 0 {
            return None;
        }
        Some(unsafe { hb_font_get_glyph_h_advance(self.mHBFont, gid) } as f64 / 65536.0)
    }

    /// Top of the glyph box above the baseline in CSS px, from glyph extents (only used when the font
    /// has no x-height or cap-height). Gecko: gfxFT2FontBase::GetCharExtents, the bounds part (-bounds.y).
    fn GetCharTop(&self, aCh: u32) -> Option<f64> {
        let gid = self.GetNominalGlyph(aCh);
        if gid == 0 {
            return None;
        }
        let mut e = hb_glyph_extents_t::default();
        if unsafe { hb_font_get_glyph_extents(self.mHBFont, gid, &mut e) } == 0 {
            return None;
        }
        let top = e.y_bearing as f64 / 65536.0;
        if top > 0.0 { Some(top) } else { None }
    }

    // Gecko: gfx/thebes/gfxFT2FontBase.cpp gfxFT2FontBase::InitMetrics
    // NOT PORTED: font-size-adjust, bitmap strikes (FindClosestSize), the "no FT_Face" fallback block.
    // Unlike Gecko the sizes come from the sfnt tables directly; the FreeType size step is FT_ComputeSize.
    fn InitMetrics(&mut self) {
        let entry = &self.mFontEntry;
        let upem = entry.mUnitsPerEm as i64;
        let hhea = entry.GetFontTable(*b"hhea");
        let os2 = entry.GetFontTable(*b"OS/2");
        let post = entry.GetFontTable(*b"post");
        let adjustedSize = self.mStyle.size;
        let mut m = gfxFont_Metrics::default();

        let (Some(hhea), true) = (hhea, adjustedSize > 0.0) else {
            // No 'hhea' (not an sfnt) or a zero-size font: Gecko's AdjustedSizeMustBeZero path returns
            // zeroed metrics; for a font with no tables we do the same.
            self.mMetrics = m;
            self.mIsValid = false;
            return;
        };

        // FT_Set_Char_Size(face, size * 64 rounded, ...) then tt_size_reset / FT_Request_Metrics.
        let charSize = ((adjustedSize.max(1.0)) * 64.0 + 0.5) as i64; // 26.6 (FindClosestSize clamps to 1)
        // (The 'head' flag-8 rule, "scale from a whole-pixel ppem", is not applied: Firefox 157 on
        // DejaVu Sans at 13.4px has ex = 0.5 * 13.4, so its scale comes from the fractional size.)
        let scale = FT_DivFix(charSize, upem);

        // FT_Face ascender, descender, height (sfobjs.c): from 'hhea'; if both are zero, from OS/2.
        let mut ascender = be16s(&hhea, 4).unwrap_or(0) as i64;
        let mut descender = be16s(&hhea, 6).unwrap_or(0) as i64;
        let lineGap = be16s(&hhea, 8).unwrap_or(0) as i64;
        let advanceWidthMax = be16(&hhea, 10).unwrap_or(0) as i64;
        let mut height = ascender - descender + lineGap;
        let os2ver = os2.as_ref().and_then(|t| be16(t, 0));
        if ascender == 0 && descender == 0 {
            if let (Some(t), Some(v)) = (&os2, os2ver) {
                if v != 0xFFFF {
                    let ta = be16s(t, 68).unwrap_or(0) as i64;
                    let td = be16s(t, 70).unwrap_or(0) as i64;
                    let tg = be16s(t, 72).unwrap_or(0) as i64;
                    if ta != 0 || td != 0 {
                        ascender = ta;
                        descender = td;
                        height = ta - td + tg;
                    } else {
                        ascender = be16(t, 74).unwrap_or(0) as i64;
                        descender = -(be16(t, 76).unwrap_or(0) as i64);
                        height = ascender - descender;
                    }
                }
            }
        }

        // FT_Size_Metrics (26.6): scaled and rounded as FreeType does.
        // Checked against Firefox 157 on DejaVu Sans (head flag 8 set) and Noto Sans (not set): both
        // come out as ceil/floor, so the TrueType rounding branch FreeType's tt_size_reset has for flag 8
        // is not what reaches Firefox's numbers.
        let ftAscender = FT_PIX_CEIL(FT_MulFix(ascender, scale));
        let ftDescender = FT_PIX_FLOOR(FT_MulFix(descender, scale));
        let ftHeight = FT_PIX_ROUND(FT_MulFix(height, scale));
        let ftMaxAdvance = FT_PIX_ROUND(FT_MulFix(advanceWidthMax, scale));

        m.maxAscent = FLOAT_FROM_26_6(ftAscender);
        m.maxDescent = -FLOAT_FROM_26_6(ftDescender);
        m.maxAdvance = FLOAT_FROM_26_6(ftMaxAdvance);
        let mut lineHeight = FLOAT_FROM_26_6(ftHeight);

        // Negative maxDescent here almost certainly indicates a font with a sign error in the descent
        // field of the 'hhea' table; invert it.
        if m.maxDescent < 0.0 {
            m.maxDescent = -m.maxDescent;
        }

        // Pixels per design unit. (FLOAT_FROM_26_6(FLOAT_FROM_16_16(y_scale)).)
        let yScale = scale as f64 / 65536.0 / 64.0;
        let emHeight = upem as f64 * yScale;
        let fUnitsConvFactor = yScale;

        let os2i16 = |off: usize| os2.as_ref().and_then(|t| be16s(t, off)).unwrap_or(0) as i64;
        let os2u16 = |off: usize| os2.as_ref().and_then(|t| be16(t, off)).unwrap_or(0) as i64;
        // FreeType zeroes the fields added in OS/2 version 2 (sxHeight, sCapHeight) for older tables.
        let v2 = os2ver.map_or(false, |v| v >= 2);
        let sTypoAscender = os2i16(68);
        let sTypoDescender = os2i16(70);
        let sTypoLineGap = os2i16(72);

        if os2.is_some() && sTypoAscender != 0 && yScale > 0.0 {
            m.emAscent = sTypoAscender as f64 * yScale;
            m.emDescent = -(sTypoDescender as f64) * yScale;
            let typoHeight = sTypoAscender - sTypoDescender + sTypoLineGap;
            lineHeight = typoHeight as f64 * yScale;

            // If the OS/2 fsSelection USE_TYPO_METRICS bit is set, set maxAscent/Descent from the sTypo*
            // fields instead of hhea.
            let kUseTypoMetricsMask = 1 << 7;
            if (os2u16(62) & kUseTypoMetricsMask) != 0 || (m.maxAscent == 0.0 && m.maxDescent == 0.0) {
                // We use NS_round here to parallel the pixel-rounded values that freetype gives us for
                // ftMetrics.ascender/descender.
                m.maxAscent = NS_round(m.emAscent);
                m.maxDescent = NS_round(m.emDescent);
            }
        } else {
            m.emAscent = m.maxAscent;
            m.emDescent = m.maxDescent;
        }

        // Underline: 'post' underlinePosition is the top of the line (not the center, as FreeType's
        // underline_position says), so read it from the table. FreeType's underline_position and
        // underline_thickness are those same two fields.
        let postPos = post.as_ref().and_then(|t| be16s(t, 8)).unwrap_or(0) as i64;
        let postThick = post.as_ref().and_then(|t| be16s(t, 10)).unwrap_or(0) as i64;
        if postPos != 0 && postThick != 0 && yScale > 0.0 {
            m.underlineSize = postThick as f64 * yScale;
            m.underlineOffset = postPos as f64 * yScale;
        } else {
            // Imitate Pango.
            m.underlineSize = emHeight / 14.0;
            m.underlineOffset = -m.underlineSize;
        }

        let yStrikeoutSize = os2i16(26);
        let yStrikeoutPosition = os2i16(28);
        if os2.is_some() && yStrikeoutSize != 0 && yStrikeoutPosition != 0 && yScale > 0.0 {
            m.strikeoutSize = yStrikeoutSize as f64 * yScale;
            m.strikeoutOffset = yStrikeoutPosition as f64 * yScale;
        } else {
            m.strikeoutSize = m.underlineSize;
            // Use OpenType spec's suggested position for Roman font.
            m.strikeoutOffset = emHeight * 409.0 / 2048.0 + 0.5 * m.strikeoutSize;
        }
        SnapLineToPixels(&mut m.strikeoutOffset, &mut m.strikeoutSize);

        let sxHeight = if v2 { os2i16(86) } else { 0 };
        let sCapHeight = if v2 { os2i16(88) } else { 0 };
        if sxHeight != 0 && yScale > 0.0 {
            m.xHeight = sxHeight as f64 * yScale;
        } else {
            // CSS 2.1, section 4.3.2 Lengths: "In the cases where it is impossible or impractical to
            // determine the x-height, a value of 0.5em should be used."
            m.xHeight = 0.5 * emHeight;
        }

        // aveCharWidth is used for the width of text input elements so be liberal rather than
        // conservative in the estimate. Round to pixels (ScaleRoundDesignUnits).
        let xAvgCharWidth = os2i16(2);
        if os2.is_some() && xAvgCharWidth != 0 {
            let fixed26dot6 = FT_MulFix(xAvgCharWidth, scale);
            m.aveCharWidth = ((fixed26dot6 + 32) >> 6) as f64;
        } else {
            m.aveCharWidth = 0.0; // updated below
        }

        if sCapHeight != 0 && yScale > 0.0 {
            m.capHeight = sCapHeight as f64 * yScale;
        } else {
            m.capHeight = m.maxAscent;
        }

        match self.GetCharAdvance(' ' as u32) {
            Some(w) => m.spaceWidth = w,
            None => m.spaceWidth = m.maxAdvance, // guess
        }
        m.zeroWidth = self.GetCharAdvance('0' as u32).unwrap_or(-1.0); // -1 indicates not found
        m.ideographicWidth = self.GetCharAdvance(0x6C34).unwrap_or(-1.0); // kWaterIdeograph

        // If we didn't get a usable x-height or cap-height above, try measuring specific glyphs.
        // (Here xHeight is never 0.0 because the 0.5em default above is applied first; Gecko has the
        // same order. The branches are kept for capHeight == 0 only.)
        if m.xHeight == 0.0 {
            if let Some(top) = self.GetCharTop('x' as u32) {
                m.xHeight = top;
                m.aveCharWidth = m.aveCharWidth.max(self.GetCharAdvance('x' as u32).unwrap_or(0.0));
            }
        }
        if m.capHeight == 0.0 {
            if let Some(top) = self.GetCharTop('H' as u32) {
                m.capHeight = top;
            }
        }

        m.aveCharWidth = m.aveCharWidth.max(m.zeroWidth);
        if m.aveCharWidth == 0.0 {
            m.aveCharWidth = m.spaceWidth;
        }
        // Apparently hinting can mean that max_advance is not always accurate.
        m.maxAdvance = m.maxAdvance.max(m.aveCharWidth);

        m.maxHeight = m.maxAscent + m.maxDescent;

        // Make the line height an integer number of pixels so that lines will be equally spaced
        // (rather than just being snapped to pixels, some up and some down). Layout calculates line
        // height from the emHeight + internalLeading + externalLeading, but first each of these is
        // rounded to layout units. To ensure that the result is an integer number of pixels, round each
        // of the components to pixels.
        m.emHeight = (emHeight + 0.5).floor();

        // maxHeight will normally be an integer, but round anyway in case FreeType is configured
        // differently.
        m.internalLeading = (m.maxHeight - m.emHeight + 0.5).floor();

        // Text input boxes currently don't work well with lineHeight significantly less than
        // maxHeight (with Verdana, for example).
        lineHeight = (lineHeight.max(m.maxHeight) + 0.5).floor();
        m.externalLeading = lineHeight - m.internalLeading - m.emHeight;

        // Ensure emAscent + emDescent == emHeight
        let sum = m.emAscent + m.emDescent;
        m.emAscent = if sum > 0.0 { m.emAscent * m.emHeight / sum } else { 0.0 };
        m.emDescent = m.emHeight - m.emAscent;

        let _ = fUnitsConvFactor;
        self.SanitizeMetrics(&mut m);
        self.mMetrics = m;
        self.mIsValid = true;
    }

    // Gecko: gfx/thebes/gfxFont.cpp gfxFont::SanitizeMetrics
    // NOT PORTED: the ascent/descent/line-gap overrides of @font-face, the bad-underline (CJK) font
    // branch (aIsBadUnderlineFont is always false for FreeType fonts here).
    fn SanitizeMetrics(&self, aMetrics: &mut gfxFont_Metrics) {
        aMetrics.underlineSize = aMetrics.underlineSize.max(1.0);
        aMetrics.strikeoutSize = aMetrics.strikeoutSize.max(1.0);

        aMetrics.underlineOffset = aMetrics.underlineOffset.min(-1.0);

        if aMetrics.maxAscent < 1.0 {
            // We cannot draw strikeout line and overline in the ascent...
            aMetrics.underlineSize = 0.0;
            aMetrics.underlineOffset = 0.0;
            aMetrics.strikeoutSize = 0.0;
            aMetrics.strikeoutOffset = 0.0;
            return;
        }

        // If underline positioned is too far from the text, descent position is preferred so that
        // underline will stay within the boundary.
        if aMetrics.underlineSize - aMetrics.underlineOffset > aMetrics.maxDescent {
            if aMetrics.underlineSize > aMetrics.maxDescent {
                aMetrics.underlineSize = aMetrics.maxDescent.max(1.0);
            }
            // The max underlineOffset is 1px (the min underlineSize is 1px, and min maxDescent is 0px.)
            aMetrics.underlineOffset = aMetrics.underlineSize - aMetrics.maxDescent;
        }

        // If strikeout line is overflowed from the ascent, the line should be resized and moved for that
        // being in the ascent space. Note that the strikeoutOffset is *middle* of the strikeout line position.
        let mut halfOfStrikeoutSize = (aMetrics.strikeoutSize / 2.0 + 0.5).floor();
        if halfOfStrikeoutSize + aMetrics.strikeoutOffset > aMetrics.maxAscent {
            if aMetrics.strikeoutSize > aMetrics.maxAscent {
                aMetrics.strikeoutSize = aMetrics.maxAscent.max(1.0);
                halfOfStrikeoutSize = (aMetrics.strikeoutSize / 2.0 + 0.5).floor();
            }
            let ascent = (aMetrics.maxAscent + 0.5).floor();
            aMetrics.strikeoutOffset = halfOfStrikeoutSize.max(ascent / 2.0);
        }

        // If overline is larger than the ascent, the line should be resized.
        if aMetrics.underlineSize > aMetrics.maxAscent {
            aMetrics.underlineSize = aMetrics.maxAscent;
        }
    }

    // Gecko: gfx/thebes/gfxHarfBuzzShaper.cpp gfxHarfBuzzShaper::ShapeText + SetGlyphsFromRun
    /// Shape one UTF-16 run (one font, one direction) with the default features and give the advances
    /// per character and per glyph in app units. `aLanguage` is a BCP 47 tag such as "en" or None.
    pub fn ShapeText(&self, aText: &[u16], aRightToLeft: bool, aLanguage: Option<&str>) -> ShapedRun {
        let wordLength = aText.len();
        let mut run = ShapedRun { mCharAdvance: vec![0; wordLength], ..Default::default() };
        if wordLength == 0 {
            return run;
        }
        let lang = aLanguage.and_then(|l| CString::new(l).ok());
        let mut glyphs = vec![mrt_glyph::default(); wordLength * 2 + 8];
        let mut n = unsafe {
            mrt_shape_utf16(
                self.mHBFont, aText.as_ptr(), wordLength as u32, aRightToLeft as i32,
                lang.as_ref().map_or(std::ptr::null(), |l| l.as_ptr()), glyphs.as_mut_ptr(), glyphs.len() as u32,
            )
        };
        if n > glyphs.len() as i32 {
            glyphs.resize(n as usize, mrt_glyph::default());
            n = unsafe {
                mrt_shape_utf16(
                    self.mHBFont, aText.as_ptr(), wordLength as u32, aRightToLeft as i32,
                    lang.as_ref().map_or(std::ptr::null(), |l| l.as_ptr()), glyphs.as_mut_ptr(), glyphs.len() as u32,
                )
            };
        }
        if n <= 0 {
            return run;
        }
        glyphs.truncate(n as usize);
        let numGlyphs = glyphs.len() as i32;

        const NO_GLYPH: i32 = -1;
        let mut charToGlyph = vec![NO_GLYPH; wordLength];
        for (i, g) in glyphs.iter().enumerate() {
            let loc = g.cluster as usize;
            if loc < wordLength {
                charToGlyph[loc] = i as i32;
            }
        }

        // factor to convert 16.16 fixed-point pixels to app units (only used if not rounding)
        let hb2appUnits = self.mAppUnitsPerDevUnit as f64 * (1.0 / 65536.0);
        let toAppUnits = |a: i32| (hb2appUnits * a as f64 + 0.5).floor() as nscoord;
        run.mGlyphAdvance = glyphs.iter().map(|g| toAppUnits(g.x_advance)).collect();

        let mut glyphStart = 0i32; // looking for a clump that starts at this glyph
        let mut charStart = 0i32; // and this char index within the range of the run
        let wl = wordLength as i32;
        while glyphStart < numGlyphs {
            let mut charEnd = glyphs[glyphStart as usize].cluster as i32;
            let mut glyphEnd = glyphStart;
            let charLimit = wl;
            while charEnd < charLimit {
                // This is normally executed once for each iteration of the outer loop, but in unusual
                // cases where the character/glyph association is complex, the initial character range
                // might correspond to a non-contiguous glyph range with "holes" in it. If so, we will
                // repeat this loop to extend the character range until we have a contiguous glyph sequence.
                charEnd += 1;
                while charEnd != charLimit && charToGlyph[charEnd as usize] == NO_GLYPH {
                    charEnd += 1;
                }

                // find the maximum glyph index covered by the clump so far
                for i in charStart..charEnd {
                    if charToGlyph[i as usize] != NO_GLYPH {
                        glyphEnd = glyphEnd.max(charToGlyph[i as usize] + 1);
                    }
                }

                if glyphEnd == glyphStart + 1 {
                    // for the common case of a single-glyph clump, we can skip the following checks
                    break;
                }
                if glyphEnd == glyphStart {
                    // no glyphs, try to extend the clump
                    continue;
                }

                // check whether all glyphs in the range are associated with the characters in our
                // clump; if not, we have a discontinuous range, and should extend it unless we've
                // reached the end of the text
                let mut allGlyphsAreWithinCluster = true;
                for i in glyphStart..glyphEnd {
                    let glyphCharIndex = glyphs[i as usize].cluster as i32;
                    if glyphCharIndex < charStart || glyphCharIndex >= charEnd {
                        allGlyphsAreWithinCluster = false;
                        break;
                    }
                }
                if allGlyphsAreWithinCluster {
                    break;
                }
            }

            // Now charStart..charEnd is a ligature clump, corresponding to glyphStart..glyphEnd.
            // Gecko attaches the first glyph as the simple glyph and the rest as detailed glyphs of the
            // base character; the advance of the clump is the sum.
            let baseCharIndex = charStart;
            if baseCharIndex >= wl {
                glyphStart = glyphEnd;
                charStart = charEnd;
                continue;
            }
            if glyphEnd <= glyphStart {
                // Defensive: HarfBuzz returned nothing usable for these characters.
                glyphStart += 1;
                charStart = charEnd.max(charStart + 1);
                continue;
            }
            let mut advance: nscoord = 0;
            for i in glyphStart..glyphEnd {
                advance += run.mGlyphAdvance[i as usize];
            }
            run.mCharAdvance[baseCharIndex as usize] = advance;
            run.mClumps.push(ShapedClump {
                mCharStart: baseCharIndex as u32,
                mCharEnd: (charEnd as usize).min(wordLength) as u32,
                mGlyphStart: glyphStart as u32,
                mGlyphEnd: glyphEnd as u32,
            });

            glyphStart = glyphEnd;
            charStart = charEnd;
        }
        run.mWidth = run.mCharAdvance.iter().sum();
        run.mGlyphs = glyphs;
        run
    }
}

// Gecko: gfx/thebes/gfxFT2FontBase.cpp SnapLineToPixels
// Snap a line to pixels while keeping the center and size of the line as close to the original position as possible.
fn SnapLineToPixels(aOffset: &mut f64, aSize: &mut f64) {
    let snappedSize = (*aSize + 0.5).floor().max(1.0);
    // Correct offset for change in size
    let offset = *aOffset - 0.5 * (*aSize - snappedSize);
    // Snap offset
    *aOffset = (offset + 0.5).floor();
    *aSize = snappedSize;
}
