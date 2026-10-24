// Gecko: gfx/2d/Rect.h
use crate::*;

// NOT PORTED: everything in gfx/2d/Rect.h except BaseRectCornerRadii below (IntMarginTyped, MarginTyped,
// IntRectTyped, RectTyped and their typedefs IntMargin / Margin / IntRect / Rect, the ToUnknownRect-style
// conversion helpers, IntersectMaybeRects / UnionMaybeRects, RectCornerRadii, RoundedRect): the typed pixel
// rectangles are for painting, not for layout in app units, and nsRect / nsMargin are defined without them.

// BaseRectCornerRadii<Coord, Size, Margin> as a macro: `BaseRectCornerRadii!(Sub, Coord, Size, Margin)`
// defines the struct `Sub` (C++: `struct nsRectCornerRadii final : public BaseRectCornerRadii<...>`
// with `using BaseRectCornerRadii::BaseRectCornerRadii;`) with the base class's members and methods.
// It is `#[macro_export]` because lib.rs does not `#[macro_use]` this module; the macro is reachable from
// the crate root through `use crate::*;`.
//
// Overload suffixes spell the instantiated type names (nscoord, nsSize), as in BaseRect.rs.
// `Float` (C++ `typedef float Float`) is written as f32.
// Gecko: gfx/2d/Rect.h BaseRectCornerRadii
#[macro_export]
macro_rules! BaseRectCornerRadii {
    ($Sub:ident, $Coord:ty, $Size:ident, $Margin:ident) => {
        #[derive(Clone, Copy, Debug)]
        pub struct $Sub {
            pub radii: [$Size; eCornerCount],
            // float mShapeK[eCornerCount] = {1.0f, 1.0f, 1.0f, 1.0f};
            pub mShapeK: [f32; eCornerCount],
        }

        // BaseRectCornerRadii() = default;  (with the member initializer for mShapeK)
        // derive(Default) would give mShapeK = 0, so Default is written out.
        impl Default for $Sub {
            fn default() -> $Sub {
                $Sub { radii: [$Size::default(); eCornerCount], mShapeK: [1.0f32, 1.0f32, 1.0f32, 1.0f32] }
            }
        }

        impl $Sub {
            // explicit BaseRectCornerRadii(Coord radius)
            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::BaseRectCornerRadii
            pub fn new__nscoord(radius: $Coord) -> $Sub {
                let mut this = $Sub::default();
                for i in AllPhysicalCorners() {
                    this.radii[i as usize].SizeTo(radius, radius);
                }
                this
            }

            // BaseRectCornerRadii(Coord radiusX, Coord radiusY)
            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::BaseRectCornerRadii
            pub fn new__nscoord_nscoord(radiusX: $Coord, radiusY: $Coord) -> $Sub {
                let mut this = $Sub::default();
                for i in AllPhysicalCorners() {
                    this.radii[i as usize].SizeTo(radiusX, radiusY);
                }
                this
            }

            // BaseRectCornerRadii(Coord tl, Coord tr, Coord br, Coord bl)
            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::BaseRectCornerRadii
            pub fn new__nscoord_nscoord_nscoord_nscoord(tl: $Coord, tr: $Coord, br: $Coord, bl: $Coord) -> $Sub {
                let mut this = $Sub::default();
                this.radii[eCornerTopLeft as usize].SizeTo(tl, tl);
                this.radii[eCornerTopRight as usize].SizeTo(tr, tr);
                this.radii[eCornerBottomRight as usize].SizeTo(br, br);
                this.radii[eCornerBottomLeft as usize].SizeTo(bl, bl);
                this
            }

            // BaseRectCornerRadii(const Size& tl, const Size& tr, const Size& br, const Size& bl)
            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::BaseRectCornerRadii
            pub fn new__nsSize_nsSize_nsSize_nsSize(tl: $Size, tr: $Size, br: $Size, bl: $Size) -> $Sub {
                let mut this = $Sub::default();
                this.radii[eCornerTopLeft as usize] = tl;
                this.radii[eCornerTopRight as usize] = tr;
                this.radii[eCornerBottomRight as usize] = br;
                this.radii[eCornerBottomLeft as usize] = bl;
                this
            }

            // operator[](Corner) and operator[](HalfCorner) are the Index / IndexMut impls below.
            // operator!= is `!=` on the PartialEq below.

            // True if every corner uses the default round (K=1) shape.
            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::AreShapesAllRound
            pub fn AreShapesAllRound(&self) -> bool {
                self.mShapeK.iter().all(|k| *k == 1.0f32)
            }

            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::AreRadiiSame
            pub fn AreRadiiSame(&self) -> bool {
                *self.TopLeft() == *self.TopRight()
                    && *self.TopLeft() == *self.BottomRight()
                    && *self.TopLeft() == *self.BottomLeft()
            }

            // BaseSize::Scale takes T (nscoord) here, so the Float scales are converted as C++ does implicitly.
            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::Scale
            pub fn Scale(&mut self, aXScale: f32, aYScale: f32) {
                for corner in self.radii.iter_mut() {
                    corner.Scale(aXScale as $Coord, aYScale as $Coord);
                }
            }

            // The C++ has a const and a non-const overload of each of TopLeft/TopRight/BottomRight/BottomLeft
            // (`const Size& X() const` and `Size& X()`): `X` is the const one, `X_mut` the mutable one.
            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::TopLeft  (const)
            pub fn TopLeft(&self) -> &$Size {
                &self.radii[eCornerTopLeft as usize]
            }
            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::TopLeft  (non-const)
            pub fn TopLeft_mut(&mut self) -> &mut $Size {
                &mut self.radii[eCornerTopLeft as usize]
            }

            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::TopRight  (const)
            pub fn TopRight(&self) -> &$Size {
                &self.radii[eCornerTopRight as usize]
            }
            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::TopRight  (non-const)
            pub fn TopRight_mut(&mut self) -> &mut $Size {
                &mut self.radii[eCornerTopRight as usize]
            }

            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::BottomRight  (const)
            pub fn BottomRight(&self) -> &$Size {
                &self.radii[eCornerBottomRight as usize]
            }
            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::BottomRight  (non-const)
            pub fn BottomRight_mut(&mut self) -> &mut $Size {
                &mut self.radii[eCornerBottomRight as usize]
            }

            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::BottomLeft  (const)
            pub fn BottomLeft(&self) -> &$Size {
                &self.radii[eCornerBottomLeft as usize]
            }
            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::BottomLeft  (non-const)
            pub fn BottomLeft_mut(&mut self) -> &mut $Size {
                &mut self.radii[eCornerBottomLeft as usize]
            }

            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::IsEmpty
            pub fn IsEmpty(&self) -> bool {
                self.TopLeft().IsEmpty()
                    && self.TopRight().IsEmpty()
                    && self.BottomRight().IsEmpty()
                    && self.BottomLeft().IsEmpty()
            }

            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::AdjustOutwards
            pub fn AdjustOutwards(&mut self, aMargin: $Margin) {
                self.Adjust::<true>(aMargin)
            }
            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::AdjustInwards
            pub fn AdjustInwards(&mut self, aMargin: $Margin) {
                self.Adjust::<false>(-aMargin)
            }

            // private:
            // template <bool aOut>
            // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::Adjust
            fn Adjust<const aOut: bool>(&mut self, aMargin: $Margin) {
                let kZero: $Coord = 0;
                let C_TL = eCornerTopLeft as usize;
                let C_TR = eCornerTopRight as usize;
                let C_BL = eCornerBottomLeft as usize;
                let C_BR = eCornerBottomRight as usize;
                // round the edges that have radii > 0.0 to start with
                if !aOut || (self.radii[C_TL].width > kZero && self.radii[C_TL].height > kZero) {
                    self.radii[C_TL].width = std::cmp::max(kZero, self.radii[C_TL].width + aMargin.left);
                    self.radii[C_TL].height = std::cmp::max(kZero, self.radii[C_TL].height + aMargin.top);
                }

                if !aOut || (self.radii[C_TR].width > kZero && self.radii[C_TR].height > kZero) {
                    self.radii[C_TR].width = std::cmp::max(kZero, self.radii[C_TR].width + aMargin.right);
                    self.radii[C_TR].height = std::cmp::max(kZero, self.radii[C_TR].height + aMargin.top);
                }

                if !aOut || (self.radii[C_BR].width > kZero && self.radii[C_BR].height > kZero) {
                    self.radii[C_BR].width = std::cmp::max(kZero, self.radii[C_BR].width + aMargin.right);
                    self.radii[C_BR].height = std::cmp::max(kZero, self.radii[C_BR].height + aMargin.bottom);
                }

                if !aOut || (self.radii[C_BL].width > kZero && self.radii[C_BL].height > kZero) {
                    self.radii[C_BL].width = std::cmp::max(kZero, self.radii[C_BL].width + aMargin.left);
                    self.radii[C_BL].height = std::cmp::max(kZero, self.radii[C_BL].height + aMargin.bottom);
                }
            }
        }

        // const Size& operator[](Corner aCorner) const { return radii[aCorner]; }
        // Size& operator[](Corner aCorner) { return radii[aCorner]; }
        // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::operator[]  (Corner)
        impl std::ops::Index<Corner> for $Sub {
            type Output = $Size;
            fn index(&self, aCorner: Corner) -> &$Size {
                &self.radii[aCorner as usize]
            }
        }
        impl std::ops::IndexMut<Corner> for $Sub {
            fn index_mut(&mut self, aCorner: Corner) -> &mut $Size {
                &mut self.radii[aCorner as usize]
            }
        }

        // const Coord& operator[](HalfCorner aCorner) const {
        //   return reinterpret_cast<const Coord*>(&radii)[aCorner];
        // }
        // The reinterpret_cast views `radii` as a flat array of (width, height) pairs; here it is the
        // equivalent field selection.
        // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::operator[]  (HalfCorner)
        impl std::ops::Index<HalfCorner> for $Sub {
            type Output = $Coord;
            fn index(&self, aCorner: HalfCorner) -> &$Coord {
                let i = aCorner as usize;
                if i % 2 == 0 { &self.radii[i / 2].width } else { &self.radii[i / 2].height }
            }
        }
        impl std::ops::IndexMut<HalfCorner> for $Sub {
            fn index_mut(&mut self, aCorner: HalfCorner) -> &mut $Coord {
                let i = aCorner as usize;
                if i % 2 == 0 { &mut self.radii[i / 2].width } else { &mut self.radii[i / 2].height }
            }
        }

        // Gecko: gfx/2d/Rect.h BaseRectCornerRadii::operator==
        impl PartialEq for $Sub {
            fn eq(&self, aOther: &$Sub) -> bool {
                *self.TopLeft() == *aOther.TopLeft()
                    && *self.TopRight() == *aOther.TopRight()
                    && *self.BottomRight() == *aOther.BottomRight()
                    && *self.BottomLeft() == *aOther.BottomLeft()
                    && self.mShapeK == aOther.mShapeK
            }
        }
    };
}
