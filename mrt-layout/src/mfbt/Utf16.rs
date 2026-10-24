// Gecko: mfbt/Utf16.h (the surrogate helpers that the intl and text code use)
use crate::*;

// Gecko: mfbt/Utf16.h mozilla::IsInBMP
pub fn IsInBMP(aCodePoint: u32) -> bool {
    aCodePoint < 0x10000
}

// Gecko: mfbt/Utf16.h mozilla::IsHighSurrogate
pub fn IsHighSurrogate(aChar: u32) -> bool {
    0xD800 <= aChar && aChar <= 0xDBFF
}

// Gecko: mfbt/Utf16.h mozilla::IsLowSurrogate
pub fn IsLowSurrogate(aChar: u32) -> bool {
    0xDC00 <= aChar && aChar <= 0xDFFF
}

// Gecko: mfbt/Utf16.h mozilla::IsSurrogatePair
pub fn IsSurrogatePair(aHigh: u32, aLow: u32) -> bool {
    IsHighSurrogate(aHigh) && IsLowSurrogate(aLow)
}

// Gecko: mfbt/Utf16.h mozilla::SurrogateToUCS4
pub fn SurrogateToUCS4(aHigh: u16, aLow: u16) -> u32 {
    MOZ_ASSERT!(IsHighSurrogate(aHigh as u32));
    MOZ_ASSERT!(IsLowSurrogate(aLow as u32));
    (((aHigh as u32) - 0xD800) << 10) + ((aLow as u32) - 0xDC00) + 0x10000
}

// Gecko: mfbt/Utf16.h mozilla::HighSurrogate
pub fn HighSurrogate(aCodePoint: u32) -> u16 {
    MOZ_ASSERT!(!IsInBMP(aCodePoint));
    (0xD7C0 + (aCodePoint >> 10)) as u16
}

// Gecko: mfbt/Utf16.h mozilla::LowSurrogate
pub fn LowSurrogate(aCodePoint: u32) -> u16 {
    MOZ_ASSERT!(!IsInBMP(aCodePoint));
    (0xDC00 + (aCodePoint & 0x3FF)) as u16
}
