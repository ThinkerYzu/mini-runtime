// Translated from layout/generic/IntrinsicISizesCache.h (GECKO_REV).
//
// Some frame classes keep a cache of intrinsic inline sizes. This class
// encapsulates the logic for caching them depending on the IntrinsicSizeInput.
//
// NOT PORTED: the space optimization (the union of InlineCache with a tagged pointer, "max(sizeof(nscoord)
// * 2, sizeof(void*))"): the out-of-line cache is an Option<Box<OutOfLineCache>> next to the inline one.
use crate::*;

// Gecko: layout/generic/IntrinsicISizesCache.h mozilla::IntrinsicISizesCache::kNotFound
// We use nscoord_MAX rather than NS_INTRINSIC_ISIZE_UNKNOWN as our sentinel
// value so that our high bit is always free.
const kNotFound: nscoord = nscoord_MAX;

// Gecko: layout/generic/IntrinsicISizesCache.h mozilla::IntrinsicISizesCache::InlineCache
#[derive(Clone, Copy)]
struct IntrinsicISizesCache_InlineCache {
    mCachedMinISize: nscoord,
    mCachedPrefISize: nscoord,
}

impl Default for IntrinsicISizesCache_InlineCache {
    fn default() -> Self {
        IntrinsicISizesCache_InlineCache { mCachedMinISize: kNotFound, mCachedPrefISize: kNotFound }
    }
}

impl IntrinsicISizesCache_InlineCache {
    fn Get(&self, aType: IntrinsicISizeType) -> nscoord {
        if aType == IntrinsicISizeType::MinISize { self.mCachedMinISize } else { self.mCachedPrefISize }
    }
    fn Set(&mut self, aType: IntrinsicISizeType, aValue: nscoord) {
        MOZ_ASSERT!(aValue >= 0);
        if aType == IntrinsicISizeType::MinISize {
            self.mCachedMinISize = aValue;
        } else {
            self.mCachedPrefISize = aValue;
        }
    }

    fn Clear(&mut self) {
        *self = Self::default();
    }
}

// Gecko: layout/generic/IntrinsicISizesCache.h mozilla::IntrinsicISizesCache::OutOfLineCache
#[derive(Default)]
struct IntrinsicISizesCache_OutOfLineCache {
    mCacheWithoutPercentageBasis: IntrinsicISizesCache_InlineCache,
    mCacheWithPercentageBasis: IntrinsicISizesCache_InlineCache,
    mLastPercentageBasis: Option<LogicalSize>,
}

// Gecko: layout/generic/IntrinsicISizesCache.h mozilla::IntrinsicISizesCache
#[derive(Default)]
pub struct IntrinsicISizesCache {
    mInline: IntrinsicISizesCache_InlineCache,
    mOutOfLine: Option<Box<IntrinsicISizesCache_OutOfLineCache>>,
}

impl IntrinsicISizesCache {
    // Gecko: layout/generic/IntrinsicISizesCache.h IntrinsicISizesCache::GetOrSet
    // (the template argument `Compute` is a closure.)
    pub fn GetOrSet<Compute: FnOnce() -> nscoord>(&mut self, aFrame: nsIFrame, aType: IntrinsicISizeType, aInput: &IntrinsicSizeInput, aCompute: Compute) -> nscoord {
        let mut dependentOnPercentBSize = aFrame.HasAnyStateBits(NS_FRAME_DESCENDANT_INTRINSIC_ISIZE_DEPENDS_ON_BSIZE);
        let mut value = self.Get(dependentOnPercentBSize, aType, aInput);
        if value != kNotFound {
            return value;
        }
        value = aCompute();
        // Inside of aCompute(), we might have newly discovered that we do have a
        // descendant whose intrinsic isize depends on our bsize; so we check that
        // state bit again before updating the cache.
        dependentOnPercentBSize = aFrame.HasAnyStateBits(NS_FRAME_DESCENDANT_INTRINSIC_ISIZE_DEPENDS_ON_BSIZE);
        self.Set(dependentOnPercentBSize, aType, aInput, value);
        value
    }

    // Gecko: layout/generic/IntrinsicISizesCache.h IntrinsicISizesCache::Clear
    pub fn Clear(&mut self) {
        if let Some(ool) = self.mOutOfLine.as_mut() {
            ool.mCacheWithPercentageBasis.Clear();
            ool.mCacheWithoutPercentageBasis.Clear();
            ool.mLastPercentageBasis = None;
        } else {
            self.mInline.Clear();
        }
    }

    // Gecko: layout/generic/IntrinsicISizesCache.h IntrinsicISizesCache::Get
    fn Get(&self, aDependentOnPercentBSize: bool, aType: IntrinsicISizeType, aInput: &IntrinsicSizeInput) -> nscoord {
        let usePercentageAwareCache = aDependentOnPercentBSize && aInput.HasSomePercentageBasisForChildren();
        if !usePercentageAwareCache {
            if let Some(ool) = self.mOutOfLine.as_ref() {
                return ool.mCacheWithoutPercentageBasis.Get(aType);
            }
            return self.mInline.Get(aType);
        }
        if let Some(ool) = self.mOutOfLine.as_ref() {
            if ool.mLastPercentageBasis == aInput.mPercentageBasisForChildren {
                return ool.mCacheWithPercentageBasis.Get(aType);
            }
        }
        kNotFound
    }

    // Gecko: layout/generic/IntrinsicISizesCache.h IntrinsicISizesCache::Set
    fn Set(&mut self, aDependentOnPercentBSize: bool, aType: IntrinsicISizeType, aInput: &IntrinsicSizeInput, mut aValue: nscoord) {
        // Intrinsic sizes should be nonnegative, so this std::max clamping should
        // rarely be necessary except in cases of integer overflow.  We have to be
        // strict about it, though, because of how we (ab)use the high bit
        // (see kHighBit)
        aValue = std::cmp::max(aValue, 0);
        let usePercentageAwareCache = aDependentOnPercentBSize && aInput.HasSomePercentageBasisForChildren();
        if usePercentageAwareCache {
            let ool = self.EnsureOutOfLine();
            if ool.mLastPercentageBasis != aInput.mPercentageBasisForChildren {
                ool.mLastPercentageBasis = aInput.mPercentageBasisForChildren;
                ool.mCacheWithPercentageBasis.Clear();
            }
            ool.mCacheWithPercentageBasis.Set(aType, aValue);
        } else if let Some(ool) = self.mOutOfLine.as_mut() {
            ool.mCacheWithoutPercentageBasis.Set(aType, aValue);
        } else {
            self.mInline.Set(aType, aValue);
        }
    }

    // Gecko: layout/generic/IntrinsicISizesCache.h IntrinsicISizesCache::EnsureOutOfLine
    fn EnsureOutOfLine(&mut self) -> &mut IntrinsicISizesCache_OutOfLineCache {
        if self.mOutOfLine.is_none() {
            let inlineCache = self.mInline;
            let mut ool = Box::new(IntrinsicISizesCache_OutOfLineCache::default());
            ool.mCacheWithoutPercentageBasis = inlineCache;
            self.mOutOfLine = Some(ool);
        }
        self.mOutOfLine.as_mut().unwrap()
    }
}
