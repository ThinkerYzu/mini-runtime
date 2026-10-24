// Gecko: gfx/2d/BaseSize.h
use crate::*;

/**
 * Do not use this class directly. Subclass it, pass that subclass as the
 * Sub parameter, and only use that subclass. This allows methods to safely
 * cast 'this' to 'Sub*'.
 */
// BaseSize<T, Sub, Coord = T> as a macro: `BaseSize!(Sub, T)` defines the struct `Sub` (with the base
// class's members width, height) and the base class's methods on it. `Coord` is always `T` here. The union
// with `components[2]` is not ported (Rust has no anonymous unions; nothing in layout uses it).
// Gecko: gfx/2d/BaseSize.h BaseSize
#[macro_export]
macro_rules! BaseSize {
    ($Sub:ident, $T:ty) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $Sub {
            pub width: $T,
            pub height: $T,
        }

        impl $Sub {
            // Constructors
            // constexpr BaseSize() : width(0), height(0) {}   -> Default::default()
            // constexpr BaseSize(Coord aWidth, Coord aHeight) : width(aWidth), height(aHeight) {}
            // Gecko: gfx/2d/BaseSize.h BaseSize::BaseSize
            pub const fn new(aWidth: $T, aHeight: $T) -> $Sub {
                $Sub { width: aWidth, height: aHeight }
            }

            // Gecko: gfx/2d/BaseSize.h BaseSize::SizeTo
            pub fn SizeTo(&mut self, aWidth: $T, aHeight: $T) {
                self.width = aWidth;
                self.height = aHeight;
            }

            // Gecko: gfx/2d/BaseSize.h BaseSize::IsEmpty
            pub fn IsEmpty(&self) -> bool {
                self.width <= 0 || self.height <= 0
            }

            // Gecko: gfx/2d/BaseSize.h BaseSize::IsSquare
            pub fn IsSquare(&self) -> bool {
                self.width == self.height
            }

            // Gecko: gfx/2d/BaseSize.h BaseSize::Width
            #[inline(always)]
            pub fn Width(&self) -> $T {
                self.width
            }
            // Gecko: gfx/2d/BaseSize.h BaseSize::Height
            #[inline(always)]
            pub fn Height(&self) -> $T {
                self.height
            }

            // Note that '=' isn't defined so we'll get the
            // compiler generated default assignment operator
            // (Rust: Copy / Clone)

            // Gecko: gfx/2d/BaseSize.h BaseSize::Scale
            pub fn Scale(&mut self, aXScale: $T, aYScale: $T) {
                self.width *= aXScale;
                self.height *= aYScale;
            }

            // friend Sub Min(const Sub& aA, const Sub& aB): a friend function found by ADL has no Rust
            // equivalent; it is an associated function here and the free function with the overload suffix
            // is defined next to the instantiation (e.g. Min__nsSize_nsSize in nsSize.rs).
            // Gecko: gfx/2d/BaseSize.h BaseSize::Min
            pub fn Min(aA: $Sub, aB: $Sub) -> $Sub {
                $Sub::new(std::cmp::min(aA.width, aB.width), std::cmp::min(aA.height, aB.height))
            }

            // Gecko: gfx/2d/BaseSize.h BaseSize::Max
            pub fn Max(aA: $Sub, aB: $Sub) -> $Sub {
                $Sub::new(std::cmp::max(aA.width, aB.width), std::cmp::max(aA.height, aB.height))
            }

            // NOT PORTED: friend std::ostream& operator<<  (the struct derives Debug instead)
        }

        // Gecko: gfx/2d/BaseSize.h BaseSize::operator==, operator!=
        impl PartialEq for $Sub {
            fn eq(&self, aSize: &$Sub) -> bool {
                self.width == aSize.width && self.height == aSize.height
            }
        }

        // operator<= is "both components <=" (a partial order); operator< is
        // `*this <= aSize && *this != aSize`. partial_cmp is built from those two so that `<=` and `<`
        // keep the C++ meaning (`>` and `>=` come for free in Rust and are not in the C++ class).
        // Gecko: gfx/2d/BaseSize.h BaseSize::operator<=, operator<
        impl PartialOrd for $Sub {
            fn partial_cmp(&self, aSize: &$Sub) -> Option<std::cmp::Ordering> {
                let le = self.width <= aSize.width && self.height <= aSize.height;
                let ge = aSize.width <= self.width && aSize.height <= self.height;
                if le && ge {
                    Some(std::cmp::Ordering::Equal)
                } else if le {
                    Some(std::cmp::Ordering::Less)
                } else if ge {
                    Some(std::cmp::Ordering::Greater)
                } else {
                    None
                }
            }
        }

        // Gecko: gfx/2d/BaseSize.h BaseSize::operator+
        impl std::ops::Add for $Sub {
            type Output = $Sub;
            fn add(self, aSize: $Sub) -> $Sub {
                $Sub::new(self.width + aSize.width, self.height + aSize.height)
            }
        }
        // Gecko: gfx/2d/BaseSize.h BaseSize::operator-
        impl std::ops::Sub for $Sub {
            type Output = $Sub;
            fn sub(self, aSize: $Sub) -> $Sub {
                $Sub::new(self.width - aSize.width, self.height - aSize.height)
            }
        }
        // Gecko: gfx/2d/BaseSize.h BaseSize::operator+=
        impl std::ops::AddAssign for $Sub {
            fn add_assign(&mut self, aSize: $Sub) {
                self.width += aSize.width;
                self.height += aSize.height;
            }
        }
        // Gecko: gfx/2d/BaseSize.h BaseSize::operator-=
        impl std::ops::SubAssign for $Sub {
            fn sub_assign(&mut self, aSize: $Sub) {
                self.width -= aSize.width;
                self.height -= aSize.height;
            }
        }

        // Gecko: gfx/2d/BaseSize.h BaseSize::operator*  (Sub * T)
        impl std::ops::Mul<$T> for $Sub {
            type Output = $Sub;
            fn mul(self, aScale: $T) -> $Sub {
                $Sub::new(self.width * aScale, self.height * aScale)
            }
        }
        // Gecko: gfx/2d/BaseSize.h BaseSize::operator/  (Sub / T)
        impl std::ops::Div<$T> for $Sub {
            type Output = $Sub;
            fn div(self, aScale: $T) -> $Sub {
                $Sub::new(self.width / aScale, self.height / aScale)
            }
        }
        // friend Sub operator*(T aScale, const Sub& aSize)
        // Gecko: gfx/2d/BaseSize.h BaseSize::operator*  (T * Sub)
        impl std::ops::Mul<$Sub> for $T {
            type Output = $Sub;
            fn mul(self, aSize: $Sub) -> $Sub {
                $Sub::new(self * aSize.width, self * aSize.height)
            }
        }

        // Gecko: gfx/2d/BaseSize.h BaseSize::operator*  (Sub * Sub)
        impl std::ops::Mul<$Sub> for $Sub {
            type Output = $Sub;
            fn mul(self, aSize: $Sub) -> $Sub {
                $Sub::new(self.width * aSize.width, self.height * aSize.height)
            }
        }
        // Gecko: gfx/2d/BaseSize.h BaseSize::operator/  (Sub / Sub)
        impl std::ops::Div<$Sub> for $Sub {
            type Output = $Sub;
            fn div(self, aSize: $Sub) -> $Sub {
                $Sub::new(self.width / aSize.width, self.height / aSize.height)
            }
        }
    };
}
