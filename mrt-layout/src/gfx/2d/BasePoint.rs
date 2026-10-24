// Gecko: gfx/2d/BasePoint.h
use crate::*;

// NOT PORTED: FloatType / FloatType_t (a type-level helper for the Length() return type of the template;
// only the integral instantiation exists here, for which FloatType_t is `float`, written as f32 below):
// template <class T, typename EnableT = void>
// struct FloatType;
//
// template <typename T>
// struct FloatType<T, typename std::enable_if_t<std::is_integral_v<T>>> {
//   using type = float;
// };
//
// template <typename T>
// struct FloatType<T, typename std::enable_if_t<std::is_floating_point_v<T>>> {
//   using type = T;
// };
//
// template <typename Units, typename Rep>
// struct FloatType<IntCoordTyped<Units, Rep>> {
//   using type = CoordTyped<Units, float>;
// };
//
// template <typename Units, typename Rep>
// struct FloatType<CoordTyped<Units, Rep>> {
//   using type = CoordTyped<Units, Rep>;
// };
//
// template <typename T>
// using FloatType_t = typename FloatType<T>::type;

/**
 * Do not use this class directly. Subclass it, pass that subclass as the
 * Sub parameter, and only use that subclass. This allows methods to safely
 * cast 'this' to 'Sub*'.
 */
// BasePoint<T, Sub, Coord = T> as a macro: `BasePoint!(Sub, T)` defines the struct `Sub` (with the base
// class's members x, y) and the base class's methods on it. `Coord` is always `T` here (the integral
// instantiation), so the macro takes (Sub, T). The union with `components[2]` is not ported (Rust has no
// anonymous unions; nothing in layout uses `components`).
// Gecko: gfx/2d/BasePoint.h BasePoint
#[macro_export]
macro_rules! BasePoint {
    ($Sub:ident, $T:ty) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $Sub {
            pub x: $T,
            pub y: $T,
        }

        impl $Sub {
            // Constructors
            // constexpr BasePoint() : x(0), y(0) {}   -> Default::default()
            // constexpr BasePoint(Coord aX, Coord aY) : x(aX), y(aY) {}
            // Gecko: gfx/2d/BasePoint.h BasePoint::BasePoint
            pub const fn new(aX: $T, aY: $T) -> $Sub {
                $Sub { x: aX, y: aY }
            }

            // Gecko: gfx/2d/BasePoint.h BasePoint::X
            #[inline(always)]
            pub fn X(&self) -> $T {
                self.x
            }
            // Gecko: gfx/2d/BasePoint.h BasePoint::Y
            #[inline(always)]
            pub fn Y(&self) -> $T {
                self.y
            }

            // Gecko: gfx/2d/BasePoint.h BasePoint::MoveTo
            pub fn MoveTo(&mut self, aX: $T, aY: $T) {
                self.x = aX;
                self.y = aY;
            }
            // Gecko: gfx/2d/BasePoint.h BasePoint::MoveBy
            pub fn MoveBy(&mut self, aDx: $T, aDy: $T) {
                self.x += aDx;
                self.y += aDy;
            }

            // Note that '=' isn't defined so we'll get the
            // compiler generated default assignment operator
            // (Rust: Copy / Clone)

            // Gecko: gfx/2d/BasePoint.h BasePoint::DotProduct
            pub fn DotProduct(&self, aPoint: $Sub) -> $T {
                self.x * aPoint.x + self.y * aPoint.y
            }

            // FloatType_t<Coord> is `float` for an integral Coord.
            // Gecko: gfx/2d/BasePoint.h BasePoint::Length
            pub fn Length(&self) -> f32 {
                (self.x as f64).hypot(self.y as f64) as f32
            }

            // Gecko: gfx/2d/BasePoint.h BasePoint::LengthSquare
            pub fn LengthSquare(&self) -> $T {
                self.x * self.x + self.y * self.y
            }

            // Round() is *not* rounding to nearest integer if the values are negative.
            // They are always rounding as floor(n + 0.5).
            // See https://bugzilla.mozilla.org/show_bug.cgi?id=410748#c14
            // For an integral T, `T(0.5f)` is 0, so the C++ `Coord(std::floor(T(x) + T(0.5f)))` is the identity.
            // Gecko: gfx/2d/BasePoint.h BasePoint::Round
            pub fn Round(&mut self) -> &mut $Sub {
                // x = Coord(std::floor(T(x) + T(0.5f)));  -- unchanged for an integral T
                // y = Coord(std::floor(T(y) + T(0.5f)));
                self
            }

            // "Finite" means not inf and not NaN
            // (an integer is always finite)
            // Gecko: gfx/2d/BasePoint.h BasePoint::IsFinite
            pub fn IsFinite(&self) -> bool {
                true
            }

            // Gecko: gfx/2d/BasePoint.h BasePoint::Clamp
            pub fn Clamp(&mut self, aMaxAbsValue: $T) {
                self.x = std::cmp::max(-aMaxAbsValue, std::cmp::min(self.x, aMaxAbsValue));
                self.y = std::cmp::max(-aMaxAbsValue, std::cmp::min(self.y, aMaxAbsValue));
            }

            // NOT PORTED: friend std::ostream& operator<<  (the struct derives Debug instead)
        }

        // Gecko: gfx/2d/BasePoint.h BasePoint::operator==, operator!=
        impl PartialEq for $Sub {
            fn eq(&self, aPoint: &$Sub) -> bool {
                self.x == aPoint.x && self.y == aPoint.y
            }
        }

        // Gecko: gfx/2d/BasePoint.h BasePoint::operator+
        impl std::ops::Add for $Sub {
            type Output = $Sub;
            fn add(self, aPoint: $Sub) -> $Sub {
                $Sub::new(self.x + aPoint.x, self.y + aPoint.y)
            }
        }
        // Gecko: gfx/2d/BasePoint.h BasePoint::operator-
        impl std::ops::Sub for $Sub {
            type Output = $Sub;
            fn sub(self, aPoint: $Sub) -> $Sub {
                $Sub::new(self.x - aPoint.x, self.y - aPoint.y)
            }
        }
        // Gecko: gfx/2d/BasePoint.h BasePoint::operator+=
        impl std::ops::AddAssign for $Sub {
            fn add_assign(&mut self, aPoint: $Sub) {
                self.x += aPoint.x;
                self.y += aPoint.y;
            }
        }
        // Gecko: gfx/2d/BasePoint.h BasePoint::operator-=
        impl std::ops::SubAssign for $Sub {
            fn sub_assign(&mut self, aPoint: $Sub) {
                self.x -= aPoint.x;
                self.y -= aPoint.y;
            }
        }

        // Gecko: gfx/2d/BasePoint.h BasePoint::operator*
        impl std::ops::Mul<$T> for $Sub {
            type Output = $Sub;
            fn mul(self, aScale: $T) -> $Sub {
                $Sub::new(self.x * aScale, self.y * aScale)
            }
        }
        // Gecko: gfx/2d/BasePoint.h BasePoint::operator/
        impl std::ops::Div<$T> for $Sub {
            type Output = $Sub;
            fn div(self, aScale: $T) -> $Sub {
                $Sub::new(self.x / aScale, self.y / aScale)
            }
        }

        // Gecko: gfx/2d/BasePoint.h BasePoint::operator- (unary)
        impl std::ops::Neg for $Sub {
            type Output = $Sub;
            fn neg(self) -> $Sub {
                $Sub::new(-self.x, -self.y)
            }
        }
    };
}
