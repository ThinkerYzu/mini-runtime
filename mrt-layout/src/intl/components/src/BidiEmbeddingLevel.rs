// Gecko: intl/components/src/BidiEmbeddingLevel.h and BidiEmbeddingLevel.cpp (mozilla::intl)
use crate::*;

/**
 * This enum unambiguously classifies text runs as either being left to right or
 * right to left.
 */
// Gecko: intl/components/src/BidiEmbeddingLevel.h mozilla::intl::BidiDirection
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BidiDirection {
    /**
     * Left to right text.
     */
    LTR,
    /**
     * Right to left text.
     */
    RTL,
}

/**
 * A type-safe wrapper for the bidi embedding level: even levels are left to
 * right, odd levels right to left. The two default levels ask the bidi engine
 * to take the direction of the first strong character.
 */
// Gecko: intl/components/src/BidiEmbeddingLevel.h mozilla::intl::BidiEmbeddingLevel
// (it converts implicitly to uint8_t in C++, so the comparisons and `min` of the translated code
// work on the value: Ord is on mValue)
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct BidiEmbeddingLevel {
    mValue: u8,
}

impl BidiEmbeddingLevel {
    /**
     * Constants indicating that the base direction depends on the first strong
     * directional character in the text according to the Unicode Bidirectional
     * Algorithm.
     * If no strong directional character is present, then set the paragraph level
     * to 0 (LTR) or 1 (RTL) respectively.
     * (Compatible with ICU4C's UBIDI_DEFAULT_LTR / UBIDI_DEFAULT_RTL.)
     */
    const kDefaultLTR: u8 = 0xfe;
    const kDefaultRTL: u8 = 0xff;

    pub const fn new(aValue: u8) -> Self {
        BidiEmbeddingLevel { mValue: aValue }
    }

    /**
     * Get the underlying value as a uint8_t.
     */
    pub fn Value(&self) -> u8 {
        self.mValue
    }

    /**
     * Return the direction of this embedding level: it is determined by the least
     * significant bit. If it is 0, then it is LTR. If it is 1, then it is RTL.
     */
    pub fn Direction(&self) -> BidiDirection {
        if self.IsRTL() {
            BidiDirection::RTL
        } else {
            BidiDirection::LTR
        }
    }

    /**
     * Create a left-to-right embedding level.
     */
    pub fn LTR() -> BidiEmbeddingLevel {
        BidiEmbeddingLevel::new(0)
    }

    /**
     * Create an right-to-left embedding level.
     */
    pub fn RTL() -> BidiEmbeddingLevel {
        BidiEmbeddingLevel::new(1)
    }

    /**
     * When passed into `SetParagraph`, the direction is determined by first
     * strongly directional character, with the default set to left-to-right if
     * none is found.
     *
     * This is encoded with the highest bit set to 1.
     */
    pub fn DefaultLTR() -> BidiEmbeddingLevel {
        BidiEmbeddingLevel::new(Self::kDefaultLTR)
    }

    /**
     * When passed into `SetParagraph`, the direction is determined by first
     * strongly directional character, with the default set to right-to-left if
     * none is found.
     *
     * * This is encoded with the highest and lowest bits set to 1.
     */
    pub fn DefaultRTL() -> BidiEmbeddingLevel {
        BidiEmbeddingLevel::new(Self::kDefaultRTL)
    }

    pub fn IsDefaultLTR(&self) -> bool {
        self.mValue == Self::kDefaultLTR
    }

    pub fn IsDefaultRTL(&self) -> bool {
        self.mValue == Self::kDefaultRTL
    }

    pub fn IsLTR(&self) -> bool {
        // If the least significant bit is 0, then the embedding level
        // is left-to-right.
        self.mValue & 1 == 0
    }

    pub fn IsRTL(&self) -> bool {
        // If the least significant bit is 1, then the embedding level
        // is right-to-left.
        self.mValue & 1 != 0
    }

    pub fn IsSameDirection(&self, aOther: BidiEmbeddingLevel) -> bool {
        ((self.mValue ^ aOther.mValue) & 1) == 0
    }
}
