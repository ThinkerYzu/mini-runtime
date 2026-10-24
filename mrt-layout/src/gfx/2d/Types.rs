// Gecko: gfx/2d/Types.h (only the part inside `namespace mozilla` near the end of the file: Side, SideBits,
// Corner, HalfCorner and their helpers). The rest of Types.h (SurfaceFormat, pixel formats, ...) is NOT PORTED:
// not needed for layout.
use crate::*;

// Side constants for use in various places.
//
// C++ `enum Side : uint8_t` is an unscoped enum, so `eSideTop` is used without a qualifier. Rust enum variants
// are scoped, so the variants are re-exported (`pub use Side::*;`) to keep the bare C++ spelling.
// Gecko: gfx/2d/Types.h mozilla::Side
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Side {
    eSideTop = 0,
    eSideRight = 1,
    eSideBottom = 2,
    eSideLeft = 3,
}
pub use Side::*;

impl Side {
    // Rust has no int -> enum cast; stands in for C++ `Side(i)`.
    pub const fn from_int(aValue: i32) -> Side {
        match aValue {
            0 => eSideTop,
            1 => eSideRight,
            2 => eSideBottom,
            3 => eSideLeft,
            _ => panic!("Side::from_int: out of range"),
        }
    }
}

// NOT PORTED: std::ostream& operator<<(std::ostream&, const mozilla::Side&);  (Side derives Debug instead)

// constexpr auto AllPhysicalSides() {
//   return mozilla::MakeInclusiveEnumeratedRange(eSideTop, eSideLeft);
// }
// MakeInclusiveEnumeratedRange (mfbt/EnumeratedRange.h) is not ported; an array iterator gives the same
// sequence for `for (const auto side : AllPhysicalSides())`.
// Gecko: gfx/2d/Types.h mozilla::AllPhysicalSides
pub fn AllPhysicalSides() -> impl Iterator<Item = Side> + Clone {
    [eSideTop, eSideRight, eSideBottom, eSideLeft].into_iter()
}

// enum class SideBits {...} plus MOZ_MAKE_ENUM_CLASS_BITWISE_OPERATORS(SideBits).
// C++ enum class default underlying type is int.
// Gecko: gfx/2d/Types.h mozilla::SideBits
MOZ_ENUM_CLASS_BITS! {
    pub struct SideBits: i32 {
        eNone = 0,
        eTop = 1 << (eSideTop as i32),
        eRight = 1 << (eSideRight as i32),
        eBottom = 1 << (eSideBottom as i32),
        eLeft = 1 << (eSideLeft as i32),
        // eTop | eBottom (a const initializer cannot call the BitOr trait)
        eTopBottom = (1 << (eSideTop as i32)) | (1 << (eSideBottom as i32)),
        // eLeft | eRight
        eLeftRight = (1 << (eSideLeft as i32)) | (1 << (eSideRight as i32)),
        // eTopBottom | eLeftRight
        eAll = (1 << (eSideTop as i32)) | (1 << (eSideBottom as i32)) | (1 << (eSideLeft as i32)) | (1 << (eSideRight as i32)),
    }
}

// Gecko: gfx/2d/Types.h mozilla::SideToSideBit
pub const fn SideToSideBit(aSide: Side) -> SideBits {
    SideBits(1 << (aSide as i32))
}

// Gecko: gfx/2d/Types.h mozilla::Corner
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Corner {
    // This order is important!
    eCornerTopLeft = 0,
    eCornerTopRight = 1,
    eCornerBottomRight = 2,
    eCornerBottomLeft = 3,
}
pub use Corner::*;

impl Corner {
    // Stands in for C++ `Corner(i)`.
    pub const fn from_int(aValue: i32) -> Corner {
        match aValue {
            0 => eCornerTopLeft,
            1 => eCornerTopRight,
            2 => eCornerBottomRight,
            3 => eCornerBottomLeft,
            _ => panic!("Corner::from_int: out of range"),
        }
    }
}

// RectCornerRadii::radii depends on this value. It is not being added to
// Corner because we want to lift the responsibility to handle it in the
// switch-case.
// Gecko: gfx/2d/Types.h mozilla::eCornerCount
pub const eCornerCount: usize = 4;

// constexpr auto AllPhysicalCorners() {
//   return mozilla::MakeInclusiveEnumeratedRange(eCornerTopLeft,
//                                                eCornerBottomLeft);
// }
// Gecko: gfx/2d/Types.h mozilla::AllPhysicalCorners
pub fn AllPhysicalCorners() -> impl Iterator<Item = Corner> + Clone {
    [eCornerTopLeft, eCornerTopRight, eCornerBottomRight, eCornerBottomLeft].into_iter()
}

// Indices into "half corner" arrays (nsStyleCorners e.g.)
// Gecko: gfx/2d/Types.h mozilla::HalfCorner
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum HalfCorner {
    // This order is important!
    eCornerTopLeftX = 0,
    eCornerTopLeftY = 1,
    eCornerTopRightX = 2,
    eCornerTopRightY = 3,
    eCornerBottomRightX = 4,
    eCornerBottomRightY = 5,
    eCornerBottomLeftX = 6,
    eCornerBottomLeftY = 7,
}
pub use HalfCorner::*;

impl HalfCorner {
    // Stands in for C++ `HalfCorner(i)`.
    pub const fn from_int(aValue: i32) -> HalfCorner {
        match aValue {
            0 => eCornerTopLeftX,
            1 => eCornerTopLeftY,
            2 => eCornerTopRightX,
            3 => eCornerTopRightY,
            4 => eCornerBottomRightX,
            5 => eCornerBottomRightY,
            6 => eCornerBottomLeftX,
            7 => eCornerBottomLeftY,
            _ => panic!("HalfCorner::from_int: out of range"),
        }
    }
}

// constexpr auto AllPhysicalHalfCorners() {
//   return mozilla::MakeInclusiveEnumeratedRange(eCornerTopLeftX,
//                                                eCornerBottomLeftY);
// }
// Gecko: gfx/2d/Types.h mozilla::AllPhysicalHalfCorners
pub fn AllPhysicalHalfCorners() -> impl Iterator<Item = HalfCorner> + Clone {
    [
        eCornerTopLeftX,
        eCornerTopLeftY,
        eCornerTopRightX,
        eCornerTopRightY,
        eCornerBottomRightX,
        eCornerBottomRightY,
        eCornerBottomLeftX,
        eCornerBottomLeftY,
    ]
    .into_iter()
}

// The result of these conversion functions are exhaustively checked in
// nsFrame.cpp, which also serves as usage examples.

// Gecko: gfx/2d/Types.h mozilla::HalfCornerIsX
pub const fn HalfCornerIsX(aHalfCorner: HalfCorner) -> bool {
    !((aHalfCorner as i32) % 2 != 0)
}

// Gecko: gfx/2d/Types.h mozilla::HalfToFullCorner
pub const fn HalfToFullCorner(aHalfCorner: HalfCorner) -> Corner {
    Corner::from_int((aHalfCorner as i32) / 2)
}

// Gecko: gfx/2d/Types.h mozilla::FullToHalfCorner
pub const fn FullToHalfCorner(aCorner: Corner, aIsVertical: bool) -> HalfCorner {
    HalfCorner::from_int((aCorner as i32) * 2 + (aIsVertical as i32))
}

// Gecko: gfx/2d/Types.h mozilla::SideIsVertical
pub const fn SideIsVertical(aSide: Side) -> bool {
    (aSide as i32) % 2 != 0
}

// @param aIsSecond when true, return the clockwise second of the two
// corners associated with aSide. For example, with aSide = eSideBottom the
// result is eCornerBottomRight when aIsSecond is false, and
// eCornerBottomLeft when aIsSecond is true.
// Gecko: gfx/2d/Types.h mozilla::SideToFullCorner
pub const fn SideToFullCorner(aSide: Side, aIsSecond: bool) -> Corner {
    Corner::from_int(((aSide as i32) + (aIsSecond as i32)) % 4)
}

// @param aIsSecond see SideToFullCorner.
// @param aIsParallel return the half-corner that is parallel with aSide
// when aIsParallel is true. For example with aSide=eSideTop, aIsSecond=true
// the result is eCornerTopRightX when aIsParallel is true, and
// eCornerTopRightY when aIsParallel is false (because "X" is parallel with
// eSideTop/eSideBottom, similarly "Y" is parallel with
// eSideLeft/eSideRight)
// Gecko: gfx/2d/Types.h mozilla::SideToHalfCorner
pub const fn SideToHalfCorner(aSide: Side, aIsSecond: bool, aIsParallel: bool) -> HalfCorner {
    let s = aSide as i32;
    HalfCorner::from_int(((s + (aIsSecond as i32)) * 2 + (s + (!aIsParallel) as i32) % 2) % 8)
}

#[cfg(test)]
mod tests {
    // Not a gtest translation: Types.h has none. Written from the documented behaviour of the helpers.
    use super::*;

    #[test]
    fn gfx_corner_helpers() {
        assert_eq!(SideToFullCorner(eSideBottom, false), eCornerBottomRight);
        assert_eq!(SideToFullCorner(eSideBottom, true), eCornerBottomLeft);
        assert_eq!(SideToFullCorner(eSideLeft, true), eCornerTopLeft);
        assert_eq!(SideToHalfCorner(eSideTop, true, true), eCornerTopRightX);
        assert_eq!(SideToHalfCorner(eSideTop, true, false), eCornerTopRightY);
        assert_eq!(SideToHalfCorner(eSideLeft, true, true), eCornerTopLeftY);
        assert!(HalfCornerIsX(eCornerBottomLeftX));
        assert!(!HalfCornerIsX(eCornerBottomLeftY));
        assert_eq!(HalfToFullCorner(eCornerBottomRightY), eCornerBottomRight);
        assert_eq!(FullToHalfCorner(eCornerBottomRight, true), eCornerBottomRightY);
        assert!(SideIsVertical(eSideLeft) && !SideIsVertical(eSideTop));
        assert_eq!(AllPhysicalSides().count(), 4);
        assert_eq!(AllPhysicalHalfCorners().count(), 8);
    }

    #[test]
    fn gfx_side_bits() {
        assert_eq!(SideToSideBit(eSideBottom), SideBits::eBottom);
        assert_eq!(SideBits::eTop | SideBits::eBottom, SideBits::eTopBottom);
        assert_eq!(SideBits::eTopBottom | SideBits::eLeftRight, SideBits::eAll);
        assert_eq!(SideBits::eAll.0, 15);
    }
}
