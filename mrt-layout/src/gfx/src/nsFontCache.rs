// Gecko: gfx/src/nsFontCache.h, gfx/src/nsFontCache.cpp (REDUCED), and nsPresContext::GetMetricsFor
// (layout/base/nsPresContext.cpp).
//
// The cache of font metrics, keyed by font and parameters, so that every GetFontMetricsForFrame of
// the same font reuses the font group (and with it the HarfBuzz font and the shaped-text state).
//
// NOT PORTED: the observer (memory pressure, "font-info-updated"), the fingerprinting miss counter,
// the user font set updates, Compact, Flush by count (Flush clears everything), one cache per pres
// context (this cache is per thread: one document per thread, see nsPresContext.rs).
use crate::*;
use std::cell::RefCell;

// Gecko: gfx/src/nsFontCache.h nsFontCache
pub struct nsFontCache;

thread_local! {
    // Gecko: nsFontCache::mFontMetrics. The "head" (the most recently used) is at the end, because
    // append is cheaper than insert.
    static sFontMetrics: RefCell<Vec<nsFontMetrics>> = RefCell::new(Vec::new());
}

impl nsFontCache {
    // Gecko: gfx/src/nsFontCache.h nsFontCache::kMaxCacheEntries
    pub const kMaxCacheEntries: usize = 128;

    // Gecko: gfx/src/nsFontCache.cpp nsFontCache::GetMetricsFor
    // NOT PORTED: mLocaleLanguage (an empty language stays empty), user font sets, fingerprinting
    // detection, the flush posted to the main thread during a Servo traversal (styles are resolved
    // before layout here, so the flush is always done at once).
    pub fn GetMetricsFor(aContext: &nsPresContext, aFont: &nsFont, aParams: &nsFontMetrics_Params) -> nsFontMetrics {
        let language = aParams.language;

        // First check our cache
        // start from the end, which is where we put the most-recent-used element
        let hit = sFontMetrics.with(|m| {
            let mut m = m.borrow_mut();
            let n = m.len() as i32 - 1;
            let mut i = n;
            while i >= 0 {
                let fm = &m[i as usize];
                if fm.Font().Equals(aFont) && fm.Language() == language && fm.Orientation() == aParams.orientation && fm.ExplicitLanguage() == aParams.explicitLanguage {
                    if i != n {
                        // promote it to the end of the cache
                        let fm = m.remove(i as usize);
                        m.push(fm);
                    }
                    return Some(m.last().unwrap().clone());
                }
                i -= 1;
            }
            None
        });
        if let Some(fm) = hit {
            return fm;
        }

        // It's not in the cache. Get font metrics and then cache them.
        // If the cache has reached its size limit, drop the older half of the
        // entries.
        let n = sFontMetrics.with(|m| m.borrow().len()) as i32 - 1;
        if n >= nsFontCache::kMaxCacheEntries as i32 - 1 {
            let count = sFontMetrics.with(|m| m.borrow().len());
            nsFontCache::Flush(count as i32 - nsFontCache::kMaxCacheEntries as i32 / 2);
        }

        let fm = nsFontMetrics::new(aFont, aParams, aContext);
        // the mFontMetrics list has the "head" at the end, because append
        // is cheaper than insert
        sFontMetrics.with(|m| m.borrow_mut().push(fm.clone()));
        fm
    }

    // Gecko: gfx/src/nsFontCache.cpp nsFontCache::Flush
    // Drops the oldest `aFlushCount` entries (all of them when it is negative).
    pub fn Flush(aFlushCount: i32) {
        sFontMetrics.with(|m| {
            let mut m = m.borrow_mut();
            let n = if aFlushCount < 0 { m.len() } else { (aFlushCount as usize).min(m.len()) };
            m.drain(..n);
        });
    }
}

// Gecko: layout/base/nsPresContext.cpp nsPresContext::GetMetricsFor
// ADAPTER: a trait on the adapter nsPresContext (see nsPresContext.rs).
pub trait nsPresContext_FontCache {
    fn GetMetricsFor(&self, aFont: &nsFont, aParams: &nsFontMetrics_Params) -> nsFontMetrics;
    fn FlushFontCache(&self);
}

impl nsPresContext_FontCache for nsPresContext {
    fn GetMetricsFor(&self, aFont: &nsFont, aParams: &nsFontMetrics_Params) -> nsFontMetrics {
        nsFontCache::GetMetricsFor(self, aFont, aParams)
    }
    fn FlushFontCache(&self) {
        nsFontCache::Flush(-1);
    }
}
