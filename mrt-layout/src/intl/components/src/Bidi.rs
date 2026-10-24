// Gecko: intl/components/src/Bidi.h and Bidi.cpp (mozilla::intl::Bidi), the USE_RUST_UNICODE_BIDI
// path: Firefox's `unicode-bidi` crate, called the way intl/bidi/rust/unicode-bidi-ffi/src/lib.rs
// calls it.
//
// ADAPTER (what differs from the C++):
//  - The FFI object keeps a ParagraphBidiInfo that borrows the caller's text, and resolves the
//    levels and runs lazily. Rust cannot keep that borrow in `Bidi`, so SetParagraph resolves at
//    once (the same `visual_runs(0..len)` call the FFI makes on first use) and keeps the results:
//    the levels, the number of runs, the direction and the paragraph level.
//  - The crate's built-in character data stands in for the icu_properties data source of the FFI
//    (`new_with_data_source`); they differ only if the two Unicode versions differ.
//  - Gecko's `ICUResult` is `nsresult` here (SetParagraph cannot fail, as in the Rust path).
use crate::*;
use unicode_bidi::level::Level;
use unicode_bidi::utf16::{BidiInfo, ParagraphBidiInfo};
use unicode_bidi::Direction;

// Gecko: intl/components/src/Bidi.h mozilla::intl::Bidi::ParagraphDirection
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bidi_ParagraphDirection {
    LTR,
    RTL,
    Mixed,
}

// Gecko: intl/components/src/Bidi.h mozilla::intl::Bidi::BaseDirection
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bidi_BaseDirection {
    LTR,
    RTL,
    Neutral,
}

// Gecko: intl/components/src/Bidi.h mozilla::intl::Bidi (the object behind `mBidi`)
pub struct Bidi {
    mLevels: Vec<u8>,
    mRunCount: i32,
    mDirection: Bidi_ParagraphDirection,
    mParagraphLevel: u8,
}

impl Bidi {
    // Gecko: intl/components/src/Bidi.cpp Bidi::Bidi
    pub fn new() -> Bidi {
        Bidi { mLevels: Vec::new(), mRunCount: 0, mDirection: Bidi_ParagraphDirection::LTR, mParagraphLevel: 0 }
    }

    /**
     * Set the current paragraph of text to analyze for its bidi properties. This
     * performs the Unicode bidi algorithm as specified by:
     * https://unicode.org/reports/tr9/
     */
    // Gecko: intl/components/src/Bidi.cpp Bidi::SetParagraph
    pub fn SetParagraph(&mut self, aParagraph: &[u16], aLevel: BidiEmbeddingLevel) -> nsresult {
        // (the FFI: Level::new fails for the two default levels, which then mean "detect")
        let level = Level::new(aLevel.Value()).ok();
        let info = ParagraphBidiInfo::new(aParagraph, level);
        self.mDirection = match info.direction() {
            Direction::Mixed => Bidi_ParagraphDirection::Mixed,
            Direction::Ltr => Bidi_ParagraphDirection::LTR,
            Direction::Rtl => Bidi_ParagraphDirection::RTL,
        };
        self.mParagraphLevel = info.paragraph_level.into();
        let (levels, runs) = info.visual_runs(0..aParagraph.len());
        self.mLevels = levels.iter().map(|l| u8::from(*l)).collect();
        self.mRunCount = if aParagraph.is_empty() { 0 } else { runs.len() as i32 };
        NS_OK
    }

    /**
     * Get the embedding level for the paragraph that was set by SetParagraph.
     */
    // Gecko: intl/components/src/Bidi.cpp Bidi::GetParagraphEmbeddingLevel
    pub fn GetParagraphEmbeddingLevel(&self) -> BidiEmbeddingLevel {
        BidiEmbeddingLevel::new(self.mParagraphLevel)
    }

    /**
     * Get the directionality of the paragraph text that was set by SetParagraph.
     */
    // Gecko: intl/components/src/Bidi.cpp Bidi::GetParagraphDirection
    pub fn GetParagraphDirection(&self) -> Bidi_ParagraphDirection {
        self.mDirection
    }

    /**
     * Get the number of runs.
     */
    // Gecko: intl/components/src/Bidi.cpp Bidi::CountRuns
    pub fn CountRuns(&self) -> Result<i32, nsresult> {
        Ok(self.mRunCount)
    }

    /**
     * Get the level and the end of the run of equal levels that starts at
     * aLogicalStart.
     */
    // Gecko: intl/components/src/Bidi.cpp Bidi::GetLogicalRun
    pub fn GetLogicalRun(&self, aLogicalStart: i32, aLogicalLimitOut: &mut i32, aLevelOut: &mut BidiEmbeddingLevel) {
        let length = self.mLevels.len() as i32;
        MOZ_ASSERT!(aLogicalStart < length);
        let level = self.mLevels[aLogicalStart as usize];
        let mut limit = aLogicalStart + 1;
        while limit < length {
            if self.mLevels[limit as usize] != level {
                break;
            }
            limit += 1;
        }
        *aLogicalLimitOut = limit;
        *aLevelOut = BidiEmbeddingLevel::new(level);
    }

    /**
     * Reorder a line of levels into visual order: aIndexMap[visual] is the logical index.
     */
    // Gecko: intl/components/src/Bidi.cpp Bidi::ReorderVisual
    pub fn ReorderVisual(aLevels: &[BidiEmbeddingLevel], aIndexMap: &mut [i32]) {
        let levels: Vec<Level> = aLevels.iter().map(|l| Level::new(l.Value()).unwrap_or(Level::ltr())).collect();
        let reordered = BidiInfo::reorder_visual(&levels);
        for i in 0..aLevels.len() {
            aIndexMap[i] = reordered[i] as i32;
        }
    }
}
