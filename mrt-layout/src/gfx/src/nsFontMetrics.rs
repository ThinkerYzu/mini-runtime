// Gecko: gfx/src/nsFontMetrics.h, gfx/src/nsFontMetrics.cpp
//
// Font metrics in app units for a font (nsFont) and language. The numbers come from the first font of
// the font group (gfxFont::Metrics, mrt-text) with Gecko's rounding for each value. mrt-text has the
// same rounding for a bare gfxFont (mrt_text::nsFontMetrics); this is the version that has a font
// group and the nsFont, as layout's code expects.
//
// NOT PORTED: refcount logging; the pres context link (Destroy, FontMetricsDeleted); drawing
// (DrawString, GetBoundingMetrics, GetInkBoundsForInkOverflow); vertical orientation (the member is
// kept, the metrics are the horizontal ones); the baseline tables of the font (the baselines are the
// ones HarfBuzz would synthesize from the ascent and descent, see GetBaseline below); GetMaxStringLength.
//
// ADAPTER: `mStub` -- tests (SetFontMetricsStub) and a missing font (no fc-match) give fixed
// numbers (nsFontMetricsStub, nsLineLayoutAdapter.rs) that every method returns instead.
use crate::nsLineLayoutAdapter::nsFontMetricsStub;
use crate::*;
use std::rc::Rc;

// Gecko: gfx/thebes/gfxFontConstants.h NS_FONT_VARIANT_WIDTH_*
pub const NS_FONT_VARIANT_WIDTH_NORMAL: u8 = 0;
pub const NS_FONT_VARIANT_WIDTH_HALF: u8 = 1;
pub const NS_FONT_VARIANT_WIDTH_THIRD: u8 = 2;
pub const NS_FONT_VARIANT_WIDTH_QUARTER: u8 = 3;

// Gecko: gfx/thebes/gfxFontConstants.h NS_FONT_SUBSCRIPT_OFFSET_RATIO
pub const NS_FONT_SUBSCRIPT_OFFSET_RATIO: f64 = 0.20;
// Gecko: gfx/thebes/gfxFontConstants.h NS_FONT_SUPERSCRIPT_OFFSET_RATIO
pub const NS_FONT_SUPERSCRIPT_OFFSET_RATIO: f64 = 0.34;

// Gecko: gfx/src/nsFontMetrics.h nsFontMetrics::FontOrientation
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum nsFontMetrics_FontOrientation {
    eHorizontal,
    eVertical,
}

// Gecko: gfx/src/nsFontMetrics.h nsFontMetrics::Params
// NOT PORTED: userFontSet, textPerf, featureValueLookup (unit placeholders).
#[derive(Clone, Copy, Debug)]
pub struct nsFontMetrics_Params {
    pub language: Option<nsAtom>,
    pub explicitLanguage: bool,
    pub orientation: nsFontMetrics_FontOrientation,
    pub userFontSet: (),
    pub textPerf: (),
    pub featureValueLookup: (),
}

impl Default for nsFontMetrics_Params {
    fn default() -> Self {
        nsFontMetrics_Params {
            language: None,
            explicitLanguage: false,
            orientation: nsFontMetrics_FontOrientation::eHorizontal,
            userFontSet: (),
            textPerf: (),
            featureValueLookup: (),
        }
    }
}

/**
 * Font metrics
 *
 * This class may be somewhat misnamed. A better name might be
 * nsFontList. The style system uses the nsFont struct for various
 * font properties, one of which is font-family, which can contain a
 * *list* of font names. The nsFont struct is "realized" by asking the
 * pres context to cough up an nsFontMetrics object, which contains
 * a list of real font handles, one for each font mentioned in
 * font-family (and for each fallback when we fall off the end of that
 * list).
 *
 * The style system needs to have access to certain metrics, such as
 * the em height (for the CSS "em" unit), and we use the first Western
 * font's metrics for that purpose. The platform-specific
 * implementations are expected to select non-Western fonts that "fit"
 * reasonably well with the Western font that is loaded at Init time.
 */
// Gecko: gfx/src/nsFontMetrics.h nsFontMetrics
// (shared as a value: the font group inside is an Rc, so Clone is Gecko's RefPtr copy)
#[derive(Clone)]
pub struct nsFontMetrics {
    mFont: nsFont,
    mFontGroup: Rc<gfxFontGroup>,
    mLanguage: Option<nsAtom>,
    mP2A: i32,
    mOrientation: nsFontMetrics_FontOrientation,
    mExplicitLanguage: bool,
    mTextRunRTL: bool,
    mVertical: bool,
    mStub: Option<nsFontMetricsStub>,
}

// XXXTODO get rid of this macro
// Gecko: gfx/src/nsFontMetrics.cpp ROUND_TO_TWIPS
fn ROUND_TO_TWIPS(x: f64, mP2A: i32) -> nscoord {
    ((x * mP2A as f64) + 0.5).floor() as nscoord
}
// Gecko: gfx/src/nsFontMetrics.cpp CEIL_TO_TWIPS
fn CEIL_TO_TWIPS(x: f64, mP2A: i32) -> nscoord {
    (x * mP2A as f64).ceil() as nscoord
}

// Gecko: gfx/src/nsFontMetrics.cpp ComputeMaxDescent
fn ComputeMaxDescent(aMetrics: &gfxFont_Metrics, aFontGroup: &gfxFontGroup) -> gfxFloat {
    let offset: gfxFloat = (-aFontGroup.GetUnderlineOffset() + 0.5).floor();
    let size: gfxFloat = aMetrics.underlineSize.round();
    let minDescent: gfxFloat = offset + size;
    (minDescent.max(aMetrics.maxDescent) + 0.5).floor()
}

// Gecko: gfx/src/nsFontMetrics.cpp ComputeMaxAscent
fn ComputeMaxAscent(aMetrics: &gfxFont_Metrics) -> gfxFloat {
    (aMetrics.maxAscent + 0.5).floor()
}

// Gecko: gfx/src/nsFontMetrics.cpp GetBaseline (the baselines, as gfxFont::GetBaseline answers)
// NOT PORTED: the font's BASE table, read through hb_ot_layout_get_baseline_with_fallback. These are
// the fallbacks HarfBuzz uses when a font has no BASE table (CSS Inline Layout Level 3), in CSS pixels
// above the alphabetic baseline: the em box is [-maxDescent, maxAscent].
#[derive(Clone, Copy)]
enum Baseline {
    kAlphabetic,
    kHanging,
    kIdeographicUnder,
    kIdeographicOver,
    kIdeographicInkUnder,
    kIdeographicInkOver,
    kCentral,
    kMath,
}

impl nsFontMetrics {
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::nsFontMetrics
    pub fn new(aFont: &nsFont, aParams: &nsFontMetrics_Params, aContext: &nsPresContext) -> nsFontMetrics {
        let mP2A: i32 = AppUnitsPerCSSPixel(); // aContext->DeviceContext()->AppUnitsPerDevPixel()
        let size: gfxFloat = StyleCSSPixelLength_ToAppUnits(aFont.size) as f64 / mP2A as f64;
        // gfxFontStyle style(aFont.style, aFont.weight, aFont.stretch, size, ...)
        // NOT PORTED: stretch, sizeAdjust, synthesis, languageOverride, font features and variations.
        let style = gfxFontStyle {
            size,
            weight: aFont.weight.value(),
            italic: aFont.style != mrt_style::style::values::computed::font::FontStyle::NORMAL,
        };

        let devToCssSize: gfxFloat = mP2A as f64 / AppUnitsPerCSSPixel() as f64;
        let mFontGroup = Rc::new(gfxFontGroup::new(
            (),
            &aFont.family.families,
            &style,
            aParams.language,
            aParams.explicitLanguage,
            aParams.textPerf,
            aParams.userFontSet,
            devToCssSize,
            (),
        ));
        let mut fm = nsFontMetrics {
            mFont: aFont.clone(),
            mFontGroup,
            mLanguage: aParams.language,
            mP2A,
            mOrientation: aParams.orientation,
            mExplicitLanguage: aParams.explicitLanguage,
            mTextRunRTL: false,
            mVertical: false,
            mStub: None,
        };
        // ADAPTER: without any font (no fc-match) the metrics are derived from the font size.
        if !fm.mFontGroup.HasValidFont() {
            fm.mStub = Some(nsFontMetricsStub::FromFontSize(StyleCSSPixelLength_ToAppUnits(aFont.size)));
        }
        fm
    }

    // ADAPTER: metrics with fixed numbers (SetFontMetricsStub, tests), on a font group of the size
    // given by the font.
    pub fn FromStub(aFont: &nsFont, aStub: nsFontMetricsStub) -> nsFontMetrics {
        let style = gfxFontStyle { size: 16.0, weight: 400.0, italic: false };
        nsFontMetrics {
            mFont: aFont.clone(),
            mFontGroup: Rc::new(gfxFontGroup::new((), &aFont.family.families, &style, None, false, (), (), 1.0, ())),
            mLanguage: None,
            mP2A: AppUnitsPerCSSPixel(),
            mOrientation: nsFontMetrics_FontOrientation::eHorizontal,
            mExplicitLanguage: false,
            mTextRunRTL: false,
            mVertical: false,
            mStub: Some(aStub),
        }
    }

    /**
     * Destroy this font metrics. This breaks the association between
     * the font metrics and the pres context.
     */
    // ADAPTER: whether the two are the same metrics (the same font group, language and so on), so that a
    // frame can share a RefPtr to them (nsTextFrame::InternFontMetrics) instead of holding a copy.
    pub fn IsSameAs(&self, aOther: &nsFontMetrics) -> bool {
        Rc::ptr_eq(&self.mFontGroup, &aOther.mFontGroup)
            && self.mLanguage == aOther.mLanguage
            && self.mP2A == aOther.mP2A
            && self.mOrientation == aOther.mOrientation
            && self.mExplicitLanguage == aOther.mExplicitLanguage
            && self.mTextRunRTL == aOther.mTextRunRTL
            && self.mVertical == aOther.mVertical
            && self.mFont.size == aOther.mFont.size
            && self.mFont.style == aOther.mFont.style
            && self.mFont.weight == aOther.mFont.weight
            && match (&self.mStub, &aOther.mStub) {
                (None, None) => true,
                (Some(a), Some(b)) => (a.mMaxAscent, a.mMaxDescent, a.mXHeight, a.mSubscriptOffset, a.mSuperscriptOffset) == (b.mMaxAscent, b.mMaxDescent, b.mXHeight, b.mSubscriptOffset, b.mSuperscriptOffset),
                _ => false,
            }
    }

    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::Destroy
    pub fn Destroy(&mut self) {}

    // The metrics of the first font of the group (Gecko: the static GetMetrics helpers).
    // NOT PORTED: vertical metrics; both orientations answer with the horizontal metrics.
    fn GetMetrics(&self) -> gfxFont_Metrics {
        let font = self.mFontGroup.GetFirstValidFont(gfxFontGroup::kCSSFirstAvailableFont, None, None);
        *font.GetMetrics()
    }

    // Gecko: gfx/src/nsFontMetrics.cpp GetBaseline(const nsFontMetrics*, gfxFont::Baseline)
    fn GetBaseline(&self, aBaseline: Baseline) -> gfxFloat {
        let m = self.GetMetrics();
        match aBaseline {
            Baseline::kAlphabetic => 0.0,
            // 80% of the ascent, as HarfBuzz synthesizes the hanging baseline
            Baseline::kHanging => m.maxAscent * 0.8,
            Baseline::kIdeographicUnder | Baseline::kIdeographicInkUnder => -m.maxDescent,
            Baseline::kIdeographicOver | Baseline::kIdeographicInkOver => m.maxAscent,
            Baseline::kCentral => (m.maxAscent - m.maxDescent) / 2.0,
            Baseline::kMath => m.xHeight,
        }
    }

    /**
     * Return the font's alphabetic baseline.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::AlphabeticBaseline
    pub fn AlphabeticBaseline(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return 0;
        }
        ROUND_TO_TWIPS(self.GetBaseline(Baseline::kAlphabetic), self.mP2A)
    }

    /**
     * Return the font's central baseline.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::CentralBaseline
    pub fn CentralBaseline(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return (s.mMaxAscent - s.mMaxDescent) / 2;
        }
        ROUND_TO_TWIPS(self.GetBaseline(Baseline::kCentral), self.mP2A)
    }

    /**
     * Return the font's x-middle baseline.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::XMiddleBaseline
    pub fn XMiddleBaseline(&self) -> nscoord {
        (self.AlphabeticBaseline() + self.XHeight()) / 2
    }

    /**
     * Return the font's ideographic-under baseline.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::IdeographicUnderBaseline
    pub fn IdeographicUnderBaseline(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return -s.mMaxDescent;
        }
        ROUND_TO_TWIPS(self.GetBaseline(Baseline::kIdeographicUnder), self.mP2A)
    }

    /**
     * Return the font's ideographic-over baseline.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::IdeographicOverBaseline
    pub fn IdeographicOverBaseline(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return s.mMaxAscent;
        }
        ROUND_TO_TWIPS(self.GetBaseline(Baseline::kIdeographicOver), self.mP2A)
    }

    /**
     * Return the font's ideographic-ink-under baseline.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::IdeographicInkUnderBaseline
    pub fn IdeographicInkUnderBaseline(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return -s.mMaxDescent;
        }
        ROUND_TO_TWIPS(self.GetBaseline(Baseline::kIdeographicInkUnder), self.mP2A)
    }

    /**
     * Return the font's ideographic-ink-over baseline.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::IdeographicInkOverBaseline
    pub fn IdeographicInkOverBaseline(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return s.mMaxAscent;
        }
        ROUND_TO_TWIPS(self.GetBaseline(Baseline::kIdeographicInkOver), self.mP2A)
    }

    /**
     * Return the font's hanging baseline.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::HangingBaseline
    pub fn HangingBaseline(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return s.mMaxAscent * 4 / 5;
        }
        ROUND_TO_TWIPS(self.GetBaseline(Baseline::kHanging), self.mP2A)
    }

    /**
     * Return the font's math baseline.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::MathBaseline
    pub fn MathBaseline(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return s.mXHeight;
        }
        ROUND_TO_TWIPS(self.GetBaseline(Baseline::kMath), self.mP2A)
    }

    /**
     * Return the font's x-height.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::XHeight
    pub fn XHeight(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return s.mXHeight;
        }
        ROUND_TO_TWIPS(self.GetMetrics().xHeight, self.mP2A)
    }

    /**
     * Return the font's cap-height.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::CapHeight
    pub fn CapHeight(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return s.mMaxAscent * 7 / 10;
        }
        ROUND_TO_TWIPS(self.GetMetrics().capHeight, self.mP2A)
    }

    /**
     * Return the font's superscript offset (the distance from the
     * baseline to where a superscript's baseline should be placed).
     * The value returned will be positive.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::SuperscriptOffset
    pub fn SuperscriptOffset(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return s.mSuperscriptOffset;
        }
        ROUND_TO_TWIPS(self.GetMetrics().emHeight * NS_FONT_SUPERSCRIPT_OFFSET_RATIO, self.mP2A)
    }

    /**
     * Return the font's subscript offset (the distance from the
     * baseline to where a subscript's baseline should be placed).
     * The value returned will be positive.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::SubscriptOffset
    pub fn SubscriptOffset(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return s.mSubscriptOffset;
        }
        ROUND_TO_TWIPS(self.GetMetrics().emHeight * NS_FONT_SUBSCRIPT_OFFSET_RATIO, self.mP2A)
    }

    /**
     * Return the font's strikeout offset (the distance from the
     * baseline to where a strikeout should be placed) and size.
     * Positive values are above the baseline, negative below.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::GetStrikeout
    pub fn GetStrikeout(&self, aOffset: &mut nscoord, aSize: &mut nscoord) {
        let m = self.GetMetrics();
        *aOffset = ROUND_TO_TWIPS(m.strikeoutOffset, self.mP2A);
        *aSize = ROUND_TO_TWIPS(m.strikeoutSize, self.mP2A);
    }

    /**
     * Return the font's underline offset (the distance from the
     * baseline to where a underline should be placed) and size.
     * Positive values are above the baseline, negative below.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::GetUnderline
    pub fn GetUnderline(&self, aOffset: &mut nscoord, aSize: &mut nscoord) {
        *aOffset = ROUND_TO_TWIPS(self.mFontGroup.GetUnderlineOffset(), self.mP2A);
        *aSize = ROUND_TO_TWIPS(self.GetMetrics().underlineSize, self.mP2A);
    }

    /**
     * Returns the amount of internal leading for the font.
     * This is normally the difference between the max ascent
     * and the em ascent.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::InternalLeading
    pub fn InternalLeading(&self) -> nscoord {
        if self.mStub.is_some() {
            return 0;
        }
        ROUND_TO_TWIPS(self.GetMetrics().internalLeading, self.mP2A)
    }

    /**
     * Returns the amount of external leading for the font.
     * em ascent(?) plus external leading is the font designer's
     * recommended line-height for this font.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::ExternalLeading
    pub fn ExternalLeading(&self) -> nscoord {
        if self.mStub.is_some() {
            return 0;
        }
        ROUND_TO_TWIPS(self.GetMetrics().externalLeading, self.mP2A)
    }

    /**
     * Returns the height of the em square.
     * This is em ascent plus em descent.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::EmHeight
    pub fn EmHeight(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return s.mMaxAscent + s.mMaxDescent;
        }
        ROUND_TO_TWIPS(self.GetMetrics().emHeight, self.mP2A)
    }

    /**
     * Returns the ascent with half the internal leading trimmed.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::TrimmedAscent
    pub fn TrimmedAscent(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return s.mMaxAscent;
        }
        let m = self.GetMetrics();
        ROUND_TO_TWIPS((m.maxAscent - m.internalLeading / 2.0).max(0.0), self.mP2A)
    }

    /**
     * Returns the descent with half the internal leading trimmed.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::TrimmedDescent
    pub fn TrimmedDescent(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return s.mMaxDescent;
        }
        let m = self.GetMetrics();
        ROUND_TO_TWIPS((m.maxDescent - m.internalLeading / 2.0).max(0.0), self.mP2A)
    }

    /**
     * Returns the height of the bounding box.
     * This is max ascent plus max descent.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::MaxHeight
    pub fn MaxHeight(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return s.mMaxAscent + s.mMaxDescent;
        }
        let m = self.GetMetrics();
        CEIL_TO_TWIPS(ComputeMaxAscent(&m), self.mP2A) + CEIL_TO_TWIPS(ComputeMaxDescent(&m, &self.mFontGroup), self.mP2A)
    }

    /**
     * Returns the maximum distance characters in this font extend
     * above the base line.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::MaxAscent
    pub fn MaxAscent(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return s.mMaxAscent;
        }
        CEIL_TO_TWIPS(ComputeMaxAscent(&self.GetMetrics()), self.mP2A)
    }

    /**
     * Returns the maximum distance characters in this font extend
     * below the base line.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::MaxDescent
    pub fn MaxDescent(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return s.mMaxDescent;
        }
        CEIL_TO_TWIPS(ComputeMaxDescent(&self.GetMetrics(), &self.mFontGroup), self.mP2A)
    }

    /**
     * Returns the maximum character advance for the font.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::MaxAdvance
    pub fn MaxAdvance(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return s.mMaxAscent + s.mMaxDescent;
        }
        CEIL_TO_TWIPS(self.GetMetrics().maxAdvance, self.mP2A)
    }

    /**
     * Returns the average character width
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::AveCharWidth
    pub fn AveCharWidth(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return s.mXHeight;
        }
        // Use CEIL instead of ROUND for consistency with GetMaxAdvance
        CEIL_TO_TWIPS(self.GetMetrics().aveCharWidth, self.mP2A)
    }

    /**
     * Returns width of the zero character, or AveCharWidth if no zero present.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::ZeroOrAveCharWidth
    pub fn ZeroOrAveCharWidth(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return s.mXHeight;
        }
        CEIL_TO_TWIPS(self.GetMetrics().ZeroOrAveCharWidth(), self.mP2A)
    }

    /**
     * Returns the often needed width of the space character
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::SpaceWidth
    pub fn SpaceWidth(&self) -> nscoord {
        if let Some(s) = self.mStub {
            return s.mXHeight / 2;
        }
        // (vertical text with mixed or sideways orientation: horizontal space, which is all there is)
        CEIL_TO_TWIPS(self.GetMetrics().spaceWidth, self.mP2A)
    }

    /**
     * Returns the inter-script spacing width for this font, in app units.
     */
    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::InterScriptSpacingWidth
    pub fn InterScriptSpacingWidth(&self) -> nscoord {
        let m = self.GetMetrics();
        // If there is no advance measure of the CJK water ideograph, use 1em instead.
        // https://drafts.csswg.org/css-values-4/#ic
        let ic: f64 = if m.ideographicWidth >= 0.0 { m.ideographicWidth } else { m.emHeight };

        // The inter-script spacing is defined as 1/8 of the CJK advance measure, i.e.
        // 0.125ic: https://drafts.csswg.org/css-text-4/#inter-script-spacing
        const kFraction: f64 = 0.125;
        NSToCoordRound__double(ic * kFraction * self.mP2A as f64)
    }

    // Gecko: gfx/src/nsFontMetrics.h nsFontMetrics::GetThebesFontGroup
    pub fn GetThebesFontGroup(&self) -> &Rc<gfxFontGroup> {
        &self.mFontGroup
    }

    /**
     * Returns the font associated with these metrics. The return value
     * is only defined after Init() has been called.
     */
    // Gecko: gfx/src/nsFontMetrics.h nsFontMetrics::Font
    pub fn Font(&self) -> &nsFont {
        &self.mFont
    }

    /**
     * Returns the language associated with these metrics
     */
    // Gecko: gfx/src/nsFontMetrics.h nsFontMetrics::Language
    pub fn Language(&self) -> Option<nsAtom> {
        self.mLanguage
    }

    /**
     * Returns the orientation (horizontal/vertical) of these metrics.
     */
    // Gecko: gfx/src/nsFontMetrics.h nsFontMetrics::Orientation
    pub fn Orientation(&self) -> nsFontMetrics_FontOrientation {
        self.mOrientation
    }

    // Gecko: gfx/src/nsFontMetrics.h nsFontMetrics::ExplicitLanguage
    pub fn ExplicitLanguage(&self) -> bool {
        self.mExplicitLanguage
    }

    // Gecko: gfx/src/nsFontMetrics.h nsFontMetrics::GetTextRunRTL
    pub fn GetTextRunRTL(&self) -> bool {
        self.mTextRunRTL
    }

    // Gecko: gfx/src/nsFontMetrics.h nsFontMetrics::SetTextRunRTL
    pub fn SetTextRunRTL(&mut self, aIsRTL: bool) {
        self.mTextRunRTL = aIsRTL;
    }

    // Gecko: gfx/src/nsFontMetrics.h nsFontMetrics::GetVertical
    pub fn GetVertical(&self) -> bool {
        self.mVertical
    }

    // Gecko: gfx/src/nsFontMetrics.h nsFontMetrics::AppUnitsPerDevPixel
    pub fn AppUnitsPerDevPixel(&self) -> i32 {
        self.mP2A
    }

    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::GetMaxStringLength
    // NOT PORTED: only the DrawString callers use it.
    pub fn GetMaxStringLength(&self) -> i32 {
        unimplemented!("NOT PORTED: GetMaxStringLength")
    }

    // Gecko: gfx/src/nsFontMetrics.cpp AutoTextRun::ComputeFlags
    fn AutoTextRun_ComputeFlags(&self) -> ShapedTextFlags {
        let mut flags = ShapedTextFlags::default();
        if self.GetTextRunRTL() {
            flags |= ShapedTextFlags::TEXT_IS_RTL;
        }
        // NOT PORTED: vertical text (GetVertical / GetTextOrientation)
        flags
    }

    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::GetWidth(const char*, uint32_t, DrawTarget*)
    pub fn GetWidth__char_ptr_uint32_t_DrawTarget_ptr(&self, aString: &[u8], aLength: u32, aDrawTarget: Option<&DrawTarget>) -> nscoord {
        if aLength == 0 {
            return 0;
        }
        if aLength == 1 && aString[0] == b' ' {
            return self.SpaceWidth();
        }
        let provider = StubPropertyProvider;
        // AutoTextRun
        let textRun = self.mFontGroup.MakeTextRun__T_ptr_uint32_t_DrawTarget_ptr_int32_t_ShapedTextFlags_Flags_gfxMissingFontRecorder_ptr(
            aString, aLength, aDrawTarget, self.AppUnitsPerDevPixel(), self.AutoTextRun_ComputeFlags(), nsTextFrameUtils_Flags::default(), None,
        );
        if let Some(textRun) = textRun {
            return NSToCoordRound__double(
                textRun.borrow().GetAdvanceWidth__Range_PropertyProvider_ptr_Spacing_ptr(gfxTextRun_Range::new(0, aLength), Some(&provider), None),
            );
        }
        0
    }

    // Gecko: gfx/src/nsFontMetrics.cpp nsFontMetrics::GetWidth(const char16_t*, uint32_t, DrawTarget*)
    pub fn GetWidth__char16_t_ptr_uint32_t_DrawTarget_ptr(&self, aString: &[u16], aLength: u32, aDrawTarget: Option<&DrawTarget>) -> nscoord {
        if aLength == 0 {
            return 0;
        }
        if aLength == 1 && aString[0] == ' ' as u16 {
            return self.SpaceWidth();
        }
        let provider = StubPropertyProvider;
        // AutoTextRun
        let textRun = self.mFontGroup.MakeTextRun__T_ptr_uint32_t_DrawTarget_ptr_int32_t_ShapedTextFlags_Flags_gfxMissingFontRecorder_ptr(
            aString, aLength, aDrawTarget, self.AppUnitsPerDevPixel(), self.AutoTextRun_ComputeFlags(), nsTextFrameUtils_Flags::default(), None,
        );
        if let Some(textRun) = textRun {
            return NSToCoordRound__double(
                textRun.borrow().GetAdvanceWidth__Range_PropertyProvider_ptr_Spacing_ptr(gfxTextRun_Range::new(0, aLength), Some(&provider), None),
            );
        }
        0
    }
}

// Gecko: gfx/src/nsFontMetrics.cpp (anonymous namespace) StubPropertyProvider
struct StubPropertyProvider;

impl gfxTextRun_PropertyProvider for StubPropertyProvider {
    fn GetHyphenationBreaks(&self, aRange: gfxTextRun_Range, aBreakBefore: &mut [gfxTextRun_HyphenType]) {
        NS_ERROR!("This shouldn't be called because we never call BreakAndMeasureText");
    }
    fn GetHyphensOption(&self) -> StyleHyphens {
        NS_ERROR!("This shouldn't be called because we never call BreakAndMeasureText");
        StyleHyphens::None
    }
    fn GetHyphenWidth(&self) -> gfxFloat {
        NS_ERROR!("This shouldn't be called because we never enable hyphens");
        0.0
    }
    fn GetDrawTarget(&self) -> Option<DrawTarget> {
        NS_ERROR!("This shouldn't be called because we never enable hyphens");
        None
    }
    fn GetAppUnitsPerDevUnit(&self) -> u32 {
        NS_ERROR!("This shouldn't be called because we never enable hyphens");
        60
    }
    fn GetSpacing(&self, aRange: gfxTextRun_Range, aSpacing: &mut [gfxFont_Spacing]) -> bool {
        NS_ERROR!("This shouldn't be called because we never enable spacing");
        false
    }
    fn GetShapedTextFlags(&self) -> ShapedTextFlags {
        NS_ERROR!("This shouldn't be called because we never enable hyphens");
        ShapedTextFlags::default()
    }
    fn LetterSpacing(&self) -> nscoord {
        0
    }
}

#[cfg(test)]
mod tests {
    // Not a gtest translation (gfx/tests/gtest has no nsFontMetrics test). Written from the documented
    // behaviour and the rounding rules; they use the system sans-serif found with fc-match.
    use super::*;
    use mrt_style::style::values::computed::font::{FontFamily, FontStyle, FontWeight};

    pub(crate) fn sans_font(aPx: f32) -> nsFont {
        use mrt_style::style::values::computed::font::{FontFamilyList, GenericFontFamily, SingleFontFamily};
        nsFont {
            family: FontFamily {
                families: FontFamilyList { list: mrt_style::style::ArcSlice::from_iter(vec![SingleFontFamily::Generic(GenericFontFamily::SansSerif)].into_iter()) },
                is_system_font: false,
                is_initial: false,
            },
            size: ComputedLength::new(aPx),
            style: FontStyle::NORMAL,
            weight: FontWeight::NORMAL,
        }
    }

    #[test]
    fn metrics_are_nonzero_and_consistent() {
        let fm = nsFontMetrics::new(&sans_font(16.0), &nsFontMetrics_Params::default(), &nsPresContext::Current());
        assert!(fm.mStub.is_none(), "no sans-serif font found by fc-match");
        assert!(fm.MaxAscent() > 0 && fm.MaxDescent() > 0, "{} {}", fm.MaxAscent(), fm.MaxDescent());
        assert_eq!(fm.MaxHeight(), fm.MaxAscent() + fm.MaxDescent());
        // the em square of a 16px font is 16 * 60 app units
        assert_eq!(fm.EmHeight(), 16 * 60);
        assert!(fm.XHeight() > 0 && fm.XHeight() < fm.EmHeight());
        assert!(fm.SpaceWidth() > 0 && fm.AveCharWidth() > 0);
        assert_eq!(fm.AlphabeticBaseline(), 0);
        // the font is taller than the em square or equal, never smaller (ascent + descent cover the em box)
        assert!(fm.MaxHeight() >= fm.EmHeight() - 60);
    }

    #[test]
    fn metrics_scale_with_font_size() {
        let a = nsFontMetrics::new(&sans_font(10.0), &nsFontMetrics_Params::default(), &nsPresContext::Current());
        let b = nsFontMetrics::new(&sans_font(20.0), &nsFontMetrics_Params::default(), &nsPresContext::Current());
        assert!(b.MaxHeight() > a.MaxHeight());
        assert!((b.EmHeight() - 2 * a.EmHeight()).abs() <= 1);
        assert!(b.GetWidth__char16_t_ptr_uint32_t_DrawTarget_ptr(&"Hello".encode_utf16().collect::<Vec<_>>(), 5, None)
            > a.GetWidth__char16_t_ptr_uint32_t_DrawTarget_ptr(&"Hello".encode_utf16().collect::<Vec<_>>(), 5, None));
    }

    #[test]
    fn get_width_matches_mrt_text_for_a_word_and_space_rule() {
        let fm = nsFontMetrics::new(&sans_font(16.0), &nsFontMetrics_Params::default(), &nsPresContext::Current());
        let word: Vec<u16> = "Hello".encode_utf16().collect();
        let font = mrt_text::gfxFont::Lookup(&["sans-serif"], gfxFontStyle { size: 16.0, ..Default::default() }).unwrap();
        // one word: the glyph advances are the shaper's, summed and rounded to app units
        let expected = mrt_text::nsFontMetrics::new(&font).GetWidth(&word);
        let got = fm.GetWidth__char16_t_ptr_uint32_t_DrawTarget_ptr(&word, 5, None);
        assert!((got - expected).abs() <= 1, "{got} vs {expected}");
        // a lone space is the font's space width (nsFontMetrics::GetWidth special case)
        assert_eq!(fm.GetWidth__char16_t_ptr_uint32_t_DrawTarget_ptr(&[0x20], 1, None), fm.SpaceWidth());
        assert_eq!(fm.GetWidth__char16_t_ptr_uint32_t_DrawTarget_ptr(&[], 0, None), 0);
    }

    #[test]
    fn stub_overrides_every_number() {
        let stub = nsFontMetricsStub { mMaxAscent: 12 * 60, mMaxDescent: 3 * 60, mXHeight: 6 * 60, mSubscriptOffset: 2 * 60, mSuperscriptOffset: 4 * 60 };
        let fm = nsFontMetrics::FromStub(&sans_font(16.0), stub);
        assert_eq!((fm.MaxAscent(), fm.MaxDescent(), fm.MaxHeight()), (720, 180, 900));
        assert_eq!((fm.XHeight(), fm.SubscriptOffset(), fm.SuperscriptOffset()), (360, 120, 240));
        assert_eq!(fm.CentralBaseline(), (720 - 180) / 2);
    }
}
