// ADAPTER (not a translation of layout/base/nsPresContext.h): a minimal pres context. Gecko's
// nsPresContext is the per-document object that layout code reaches for the compatibility mode
// (quirks), the pagination state, the visible area and the device scale. This runtime has one
// document per thread, no printing and no zoom, so the stand-in holds the few values the
// translated code reads: 60 app units per CSS pixel, no zoom.
use crate::*;
use std::cell::RefCell;

// Gecko's `enum nsCompatibility` of layout/base/nsCompatibility.h
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum nsCompatibility {
    eCompatibility_FullStandards = 1,
    eCompatibility_AlmostStandards = 2,
    eCompatibility_NavQuirks = 3,
}
pub use nsCompatibility::*;

#[derive(Clone, Copy, Debug)]
pub struct nsPresContext {
    mCompatibilityMode: nsCompatibility,
    mPaginated: bool,
    mVisibleArea: nsRect,
}

thread_local! {
    // Gecko: Document::mBidiEnabled (nsPresContext::BidiEnabled reads it from the document)
    static BIDI_ENABLED: std::cell::Cell<bool> = std::cell::Cell::new(false);
    static PRES_CONTEXT: RefCell<nsPresContext> = RefCell::new(nsPresContext::default());
}

impl Default for nsPresContext {
    // A 1489 x 844 CSS pixel viewport (the size mrt-style resolves media queries against), in
    // standards mode.
    fn default() -> nsPresContext {
        nsPresContext {
            mCompatibilityMode: eCompatibility_FullStandards,
            mPaginated: false,
            mVisibleArea: nsRect::new(0, 0, 1489 * 60, 844 * 60),
        }
    }
}

impl nsPresContext {
    // ADAPTER: the pres context of this thread's document (Gecko: nsIFrame::PresContext()).
    pub fn Current() -> nsPresContext {
        PRES_CONTEXT.with(|p| *p.borrow())
    }

    // ADAPTER: replaces the thread's pres context (tests set the compatibility mode and the
    // viewport through this).
    pub fn SetCurrent(aPresContext: nsPresContext) {
        PRES_CONTEXT.with(|p| *p.borrow_mut() = aPresContext);
    }

    // ADAPTER: setters for the stand-in's values (they are set by the document in Gecko).
    pub fn SetCompatibilityMode(&mut self, aMode: nsCompatibility) {
        self.mCompatibilityMode = aMode;
    }
    pub fn SetVisibleArea(&mut self, aArea: nsRect) {
        self.mVisibleArea = aArea;
    }

    // Gecko: layout/base/nsPresContext.h nsPresContext::CompatibilityMode
    pub fn CompatibilityMode(&self) -> nsCompatibility {
        self.mCompatibilityMode
    }

    // Gecko: layout/base/nsPresContext.cpp nsPresContext::BidiEnabled (the flag is the document's)
    pub fn BidiEnabled(&self) -> bool {
        BIDI_ENABLED.with(|b| b.get())
    }

    // Gecko: layout/base/nsPresContext.cpp nsPresContext::SetBidiEnabled
    pub fn SetBidiEnabled(&self) {
        BIDI_ENABLED.with(|b| b.set(true));
    }

    // ADAPTER: forgets that the previous document had bidi text (a new document starts without).
    pub fn ResetBidiEnabled() {
        BIDI_ENABLED.with(|b| b.set(false));
    }

    // Gecko: layout/base/nsPresContext.h nsPresContext::IsVisualMode
    // NOT PORTED: visual mode (a bidi option of the document, never on here).
    pub fn IsVisualMode(&self) -> bool {
        false
    }

    // Gecko: layout/base/nsPresContext.h nsPresContext::IsPaginated
    // NOT PORTED: pagination; always false.
    pub fn IsPaginated(&self) -> bool {
        self.mPaginated
    }

    // Gecko: layout/base/nsPresContext.h nsPresContext::GetVisibleArea
    pub fn GetVisibleArea(&self) -> nsRect {
        self.mVisibleArea
    }

    // Gecko: layout/base/nsPresContext.h nsPresContext::GetPageSize
    // NOT PORTED: pagination (only read when IsPaginated()).
    pub fn GetPageSize(&self) -> nsSize {
        unimplemented!("NOT PORTED: pagination (nsPresContext::GetPageSize)")
    }

    // Gecko: layout/base/nsPresContext.h nsPresContext::CSSPixelsToAppUnits(int32_t)
    pub fn CSSPixelsToAppUnits__int32_t(aPixels: i32) -> nscoord {
        aPixels * AppUnitsPerCSSPixel()
    }

    // Gecko: layout/base/nsPresContext.h nsPresContext::CSSPixelsToAppUnits(float)
    pub fn CSSPixelsToAppUnits__float(aPixels: f32) -> nscoord {
        NSToCoordRoundWithClamp__float(aPixels * AppUnitsPerCSSPixel() as f32)
    }
}
