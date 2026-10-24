// Gecko: gfx/2d/BaseMargin.h
use crate::*;

/**
 * Sides represents a set of physical sides.
 */
// Gecko: gfx/2d/BaseMargin.h mozilla::Sides
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sides {
    // private:
    mBits: SideBits,
}

impl Sides {
    // Sides() : mBits(SideBits::eNone) {}   -> Default::default()
    // explicit Sides(SideBits aSideBits)
    // Gecko: gfx/2d/BaseMargin.h Sides::Sides
    pub fn new(aSideBits: SideBits) -> Sides {
        MOZ_ASSERT!((aSideBits & !SideBits::eAll) == SideBits::eNone, "illegal side bits");
        Sides { mBits: aSideBits }
    }
    // Gecko: gfx/2d/BaseMargin.h Sides::IsEmpty
    pub fn IsEmpty(&self) -> bool {
        self.mBits == SideBits::eNone
    }
    // Gecko: gfx/2d/BaseMargin.h Sides::Intersects
    pub fn Intersects(&self, aSideBits: SideBits) -> bool {
        (self.mBits & aSideBits).to_bool()
    }
    // Gecko: gfx/2d/BaseMargin.h Sides::Top
    pub fn Top(&self) -> bool {
        self.Intersects(SideBits::eTop)
    }
    // Gecko: gfx/2d/BaseMargin.h Sides::Right
    pub fn Right(&self) -> bool {
        self.Intersects(SideBits::eRight)
    }
    // Gecko: gfx/2d/BaseMargin.h Sides::Bottom
    pub fn Bottom(&self) -> bool {
        self.Intersects(SideBits::eBottom)
    }
    // Gecko: gfx/2d/BaseMargin.h Sides::Left
    pub fn Left(&self) -> bool {
        self.Intersects(SideBits::eLeft)
    }
    // Gecko: gfx/2d/BaseMargin.h Sides::Contains
    pub fn Contains(&self, aSideBits: SideBits) -> bool {
        MOZ_ASSERT!(!((aSideBits & !SideBits::eAll).to_bool()), "illegal side bits");
        (self.mBits & aSideBits) == aSideBits
    }
}

// Gecko: gfx/2d/BaseMargin.h Sides::operator|  (Sides | Sides)
impl std::ops::BitOr<Sides> for Sides {
    type Output = Sides;
    fn bitor(self, aOther: Sides) -> Sides {
        Sides::new(self.mBits | aOther.mBits)
    }
}
// Gecko: gfx/2d/BaseMargin.h Sides::operator|  (Sides | SideBits)
impl std::ops::BitOr<SideBits> for Sides {
    type Output = Sides;
    fn bitor(self, aSideBits: SideBits) -> Sides {
        self | Sides::new(aSideBits)
    }
}
// Gecko: gfx/2d/BaseMargin.h Sides::operator|=  (Sides |= Sides)
impl std::ops::BitOrAssign<Sides> for Sides {
    fn bitor_assign(&mut self, aOther: Sides) {
        self.mBits |= aOther.mBits;
    }
}
// Gecko: gfx/2d/BaseMargin.h Sides::operator|=  (Sides |= SideBits)
impl std::ops::BitOrAssign<SideBits> for Sides {
    fn bitor_assign(&mut self, aSideBits: SideBits) {
        *self |= Sides::new(aSideBits);
    }
}
// operator== and operator!= are the derived PartialEq / Eq.

/**
 * Do not use this class directly. Subclass it, pass that subclass as the
 * Sub parameter, and only use that subclass.
 */
// BaseMargin<T, Sub, Coord = T> as a macro: `BaseMargin!(Sub, T)` defines the struct `Sub` (with the base
// class's members top, right, bottom, left) and the base class's methods on it. `Coord` is always `T` here.
// Gecko: gfx/2d/BaseMargin.h BaseMargin
#[macro_export]
macro_rules! BaseMargin {
    ($Sub:ident, $T:ty) => {
        // Do not change the layout of these members; the Side() methods below
        // depend on this order.
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $Sub {
            pub top: $T,
            pub right: $T,
            pub bottom: $T,
            pub left: $T,
        }

        impl $Sub {
            // typedef mozilla::Side SideT;  // because we have a method named Side
            // (Rust: the type `Side` and the method `Side` do not clash, so `Side` is used directly)

            // Constructors
            // constexpr BaseMargin() : top(0), right(0), bottom(0), left(0) {}   -> Default::default()
            // constexpr BaseMargin(Coord aTop, Coord aRight, Coord aBottom, Coord aLeft)
            // Gecko: gfx/2d/BaseMargin.h BaseMargin::BaseMargin
            pub const fn new(aTop: $T, aRight: $T, aBottom: $T, aLeft: $T) -> $Sub {
                $Sub { top: aTop, right: aRight, bottom: aBottom, left: aLeft }
            }

            // Gecko: gfx/2d/BaseMargin.h BaseMargin::SizeTo
            pub fn SizeTo(&mut self, aTop: $T, aRight: $T, aBottom: $T, aLeft: $T) {
                self.top = aTop;
                self.right = aRight;
                self.bottom = aBottom;
                self.left = aLeft;
            }

            // Gecko: gfx/2d/BaseMargin.h BaseMargin::LeftRight
            pub fn LeftRight(&self) -> $T {
                self.left + self.right
            }
            // Gecko: gfx/2d/BaseMargin.h BaseMargin::TopBottom
            pub fn TopBottom(&self) -> $T {
                self.top + self.bottom
            }

            // The C++ has two overloads that differ in constness: `Coord& Side(SideT)` and
            // `Coord Side(SideT) const`. Rust: `Side_mut` is the mutable one, `Side` the const one.
            // The "*(&top + int(aSide))" pointer arithmetic is a match on the side.
            // Gecko: gfx/2d/BaseMargin.h BaseMargin::Side  (non-const)
            pub fn Side_mut(&mut self, aSide: Side) -> &mut $T {
                // This is ugly!
                match aSide {
                    eSideTop => &mut self.top,
                    eSideRight => &mut self.right,
                    eSideBottom => &mut self.bottom,
                    eSideLeft => &mut self.left,
                }
            }
            // Gecko: gfx/2d/BaseMargin.h BaseMargin::Side  (const)
            pub fn Side(&self, aSide: Side) -> $T {
                // This is ugly!
                match aSide {
                    eSideTop => self.top,
                    eSideRight => self.right,
                    eSideBottom => self.bottom,
                    eSideLeft => self.left,
                }
            }

            // Gecko: gfx/2d/BaseMargin.h BaseMargin::ApplySkipSides
            pub fn ApplySkipSides(&mut self, aSkipSides: Sides) -> &mut $Sub {
                if aSkipSides.Top() {
                    self.top = 0;
                }
                if aSkipSides.Right() {
                    self.right = 0;
                }
                if aSkipSides.Bottom() {
                    self.bottom = 0;
                }
                if aSkipSides.Left() {
                    self.left = 0;
                }
                self
            }

            // Ensures that all our sides are at least as big as the argument.
            // Gecko: gfx/2d/BaseMargin.h BaseMargin::EnsureAtLeast
            pub fn EnsureAtLeast(&mut self, aMargin: $Sub) {
                self.top = std::cmp::max(self.top, aMargin.top);
                self.right = std::cmp::max(self.right, aMargin.right);
                self.bottom = std::cmp::max(self.bottom, aMargin.bottom);
                self.left = std::cmp::max(self.left, aMargin.left);
            }

            // Ensures that all our sides are at most as big as the argument.
            // Gecko: gfx/2d/BaseMargin.h BaseMargin::EnsureAtMost
            pub fn EnsureAtMost(&mut self, aMargin: $Sub) {
                self.top = std::cmp::min(self.top, aMargin.top);
                self.right = std::cmp::min(self.right, aMargin.right);
                self.bottom = std::cmp::min(self.bottom, aMargin.bottom);
                self.left = std::cmp::min(self.left, aMargin.left);
            }

            // Gecko: gfx/2d/BaseMargin.h BaseMargin::IsAll
            pub fn IsAll(&self, aCoord: $T) -> bool {
                self.left == aCoord && self.top == aCoord && self.right == aCoord && self.bottom == aCoord
            }

            // Gecko: gfx/2d/BaseMargin.h BaseMargin::IsAllZero
            pub fn IsAllZero(&self) -> bool {
                self.IsAll(0)
            }
            // Gecko: gfx/2d/BaseMargin.h BaseMargin::IsAllEqual
            pub fn IsAllEqual(&self) -> bool {
                self.IsAll(self.top)
            }

            // NOT PORTED: friend std::ostream& operator<<  (the struct derives Debug instead)
        }

        // Overloaded operators. Note that '=' isn't defined so we'll get the
        // compiler generated default assignment operator
        // Gecko: gfx/2d/BaseMargin.h BaseMargin::operator==, operator!=
        impl PartialEq for $Sub {
            fn eq(&self, aMargin: &$Sub) -> bool {
                self.top == aMargin.top
                    && self.right == aMargin.right
                    && self.bottom == aMargin.bottom
                    && self.left == aMargin.left
            }
        }
        // Gecko: gfx/2d/BaseMargin.h BaseMargin::operator+
        impl std::ops::Add for $Sub {
            type Output = $Sub;
            fn add(self, aMargin: $Sub) -> $Sub {
                $Sub::new(
                    self.top + aMargin.top,
                    self.right + aMargin.right,
                    self.bottom + aMargin.bottom,
                    self.left + aMargin.left,
                )
            }
        }
        // Gecko: gfx/2d/BaseMargin.h BaseMargin::operator-
        impl std::ops::Sub for $Sub {
            type Output = $Sub;
            fn sub(self, aMargin: $Sub) -> $Sub {
                $Sub::new(
                    self.top - aMargin.top,
                    self.right - aMargin.right,
                    self.bottom - aMargin.bottom,
                    self.left - aMargin.left,
                )
            }
        }
        // Gecko: gfx/2d/BaseMargin.h BaseMargin::operator- (unary)
        impl std::ops::Neg for $Sub {
            type Output = $Sub;
            fn neg(self) -> $Sub {
                $Sub::new(-self.top, -self.right, -self.bottom, -self.left)
            }
        }
        // Gecko: gfx/2d/BaseMargin.h BaseMargin::operator+=
        impl std::ops::AddAssign for $Sub {
            fn add_assign(&mut self, aMargin: $Sub) {
                self.top += aMargin.top;
                self.right += aMargin.right;
                self.bottom += aMargin.bottom;
                self.left += aMargin.left;
            }
        }
        // Gecko: gfx/2d/BaseMargin.h BaseMargin::operator-=
        impl std::ops::SubAssign for $Sub {
            fn sub_assign(&mut self, aMargin: $Sub) {
                self.top -= aMargin.top;
                self.right -= aMargin.right;
                self.bottom -= aMargin.bottom;
                self.left -= aMargin.left;
            }
        }
    };
}
