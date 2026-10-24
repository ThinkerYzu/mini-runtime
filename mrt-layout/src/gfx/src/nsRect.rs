// Gecko: gfx/src/nsRect.h and gfx/src/nsRect.cpp (header first, in its order; the .cpp bodies of Intersect,
// IntersectRect and Overflows are placed where the header declares them)
use crate::*;

// NOT PORTED: refcount logging (MOZ_COUNT_CTOR / MOZ_COUNTED_DTOR in the nsRect constructors and destructor).
// NOT PORTED: the SSE2 intrinsics block (floor_ps2epi32 / ceil_ps2epi32) used only by the pixel-rounding methods below.

// typedef mozilla::gfx::IntRect nsIntRect;  -- NOT PORTED: IntRect is a painting type, see Rect.rs

// static_assert from nsRect.cpp
const _: () = assert!(
    (eSideTop as i32 == 0) && (eSideRight as i32 == 1) && (eSideBottom as i32 == 2) && (eSideLeft as i32 == 3),
    "The mozilla::Side sequence must match the nsMargin nscoord sequence"
);

// struct nsRect : public mozilla::gfx::BaseRect<nscoord, nsRect, nsPoint, nsSize, nsMargin>
// Gecko: gfx/src/nsRect.h nsRect
BaseRect!(nsRect, nscoord, nsPoint, nsSize, nsMargin);

impl nsRect {
    // typedef mozilla::gfx::BaseRect<nscoord, nsRect, nsPoint, nsSize, nsMargin> Super;

    // Constructors
    // nsRect() { MOZ_COUNT_CTOR(nsRect); }                      -> Default::default()
    // nsRect(const nsRect& aRect) : Super(aRect) {...}          -> Copy
    // nsRect(const nsPoint& aOrigin, const nsSize& aSize)       -> new__nsPoint_nsSize, from BaseRect!
    // nsRect(nscoord aX, nscoord aY, nscoord aWidth, nscoord aHeight)   -> new, from BaseRect!
    // nsRect& operator=(const nsRect&) = default;               -> Copy
    //
    // MOZ_COUNTED_DTOR(nsRect)   -- NOT PORTED: refcount logging

    // We have saturating versions of all the Union methods. These avoid
    // overflowing nscoord values in the 'width' and 'height' fields by
    // clamping the width and height values to nscoord_MAX if necessary.

    // Returns the smallest rectangle that contains both the area of both
    // this and aRect. Thus, empty input rectangles are ignored.
    // Note: if both rectangles are empty, returns aRect.
    // Gecko: gfx/src/nsRect.h nsRect::SaturatingUnion
    #[must_use]
    pub fn SaturatingUnion(&self, aRect: nsRect) -> nsRect {
        if self.IsEmpty() {
            aRect
        } else if aRect.IsEmpty() {
            *self
        } else {
            self.SaturatingUnionEdges(aRect)
        }
    }

    // Gecko: gfx/src/nsRect.h nsRect::SaturatingUnionEdges
    #[must_use]
    pub fn SaturatingUnionEdges(&self, aRect: nsRect) -> nsRect {
        let mut resultX: nscoord = std::cmp::min(aRect.X(), self.x);
        let mut w: i64 = std::cmp::max(aRect.X() as i64 + aRect.Width() as i64, self.x as i64 + self.width as i64)
            - resultX as i64;
        if w > nscoord_MAX as i64 {
            // Clamp huge negative x to nscoord_MIN / 2 and try again.
            resultX = std::cmp::max(resultX, nscoord_MIN / 2);
            w = std::cmp::max(aRect.X() as i64 + aRect.Width() as i64, self.x as i64 + self.width as i64)
                - resultX as i64;
            if w > nscoord_MAX as i64 {
                w = nscoord_MAX as i64;
            }
        }

        let mut resultY: nscoord = std::cmp::min(aRect.y, self.y);
        let mut h: i64 = std::cmp::max(aRect.Y() as i64 + aRect.Height() as i64, self.y as i64 + self.height as i64)
            - resultY as i64;
        if h > nscoord_MAX as i64 {
            // Clamp huge negative y to nscoord_MIN / 2 and try again.
            resultY = std::cmp::max(resultY, nscoord_MIN / 2);
            h = std::cmp::max(aRect.Y() as i64 + aRect.Height() as i64, self.y as i64 + self.height as i64)
                - resultY as i64;
            if h > nscoord_MAX as i64 {
                h = nscoord_MAX as i64;
            }
        }
        nsRect::new(resultX, resultY, w as nscoord, h as nscoord)
    }

    // Make all nsRect Union methods be saturating.
    // Gecko: gfx/src/nsRect.h nsRect::UnionEdges
    #[must_use]
    pub fn UnionEdges(&self, aRect: nsRect) -> nsRect {
        self.SaturatingUnionEdges(aRect)
    }
    // Gecko: gfx/src/nsRect.h nsRect::Union
    #[must_use]
    pub fn Union(&self, aRect: nsRect) -> nsRect {
        self.SaturatingUnion(aRect)
    }
    // Gecko: gfx/src/nsRect.h nsRect::UnsafeUnion
    #[must_use]
    pub fn UnsafeUnion(&self, aRect: nsRect) -> nsRect {
        // return Super::Union(aRect);
        self.Super__Union(aRect)
    }
    // Gecko: gfx/src/nsRect.h nsRect::UnionRect
    pub fn UnionRect(&mut self, aRect1: nsRect, aRect2: nsRect) {
        *self = aRect1.Union(aRect2);
    }

    // [[nodiscard]] nsRect Intersect(const nsRect& aRect) const;
    // The `#ifdef USE_NEON` / `USE_SSE42` IntersectEngine fast paths of nsRect.cpp are NOT PORTED (the scalar
    // fallback below is what runs without them).
    // Gecko: gfx/src/nsRect.cpp nsRect::Intersect
    #[must_use]
    pub fn Intersect(&self, aRect: nsRect) -> nsRect {
        let mut result = nsRect::default();

        result.x = std::cmp::max::<i32>(self.x, aRect.x);
        result.y = std::cmp::max::<i32>(self.y, aRect.y);
        result.width = std::cmp::min::<i32>(
            self.x - result.x + self.width,
            aRect.x - result.x + aRect.width,
        );
        result.height = std::cmp::min::<i32>(
            self.y - result.y + self.height,
            aRect.y - result.y + aRect.height,
        );

        if result.width < 0 || result.height < 0 {
            result.SizeTo__nscoord_nscoord(0, 0);
        }
        result
    }

    // bool IntersectRect(const nsRect& aRect1, const nsRect& aRect2);
    // (same NOT PORTED SIMD fast paths as Intersect)
    // Gecko: gfx/src/nsRect.cpp nsRect::IntersectRect
    pub fn IntersectRect(&mut self, aRect1: nsRect, aRect2: nsRect) -> bool {
        let newX: i32 = std::cmp::max::<i32>(aRect1.x, aRect2.x);
        let newY: i32 = std::cmp::max::<i32>(aRect1.y, aRect2.y);
        self.width = std::cmp::min::<i32>(
            aRect1.x - newX + aRect1.width,
            aRect2.x - newX + aRect2.width,
        );
        self.height = std::cmp::min::<i32>(
            aRect1.y - newY + aRect1.height,
            aRect2.y - newY + aRect2.height,
        );
        self.x = newX;
        self.y = newY;
        if self.width <= 0 || self.height <= 0 {
            self.SizeTo__nscoord_nscoord(0, 0);
            return false;
        }
        true
    }

    // Return whether this rect's right or bottom edge overflow int32.
    // (mozilla::CheckedInt<int32_t> addition: valid iff the sum fits in int32)
    // Gecko: gfx/src/nsRect.cpp nsRect::Overflows
    pub fn Overflows(&self) -> bool {
        let xMost: Option<i32> = self.x.checked_add(self.width);
        let yMost: Option<i32> = self.y.checked_add(self.height);
        xMost.is_none() || yMost.is_none()
    }

    /**
     * Return this rect scaled to a different appunits per pixel (APP) ratio.
     * In the RoundOut version we make the rect the smallest rect containing the
     * unrounded result. In the RoundIn version we make the rect the largest rect
     * contained in the unrounded result.
     * @param aFromAPP the APP to scale from
     * @param aToAPP the APP to scale to
     * @note this can turn an empty rectangle into a non-empty rectangle
     */
    // Gecko: gfx/src/nsRect.h nsRect::ScaleToOtherAppUnitsRoundOut
    #[must_use]
    pub fn ScaleToOtherAppUnitsRoundOut(&self, aFromAPP: i32, aToAPP: i32) -> nsRect {
        if aFromAPP == aToAPP {
            return *self;
        }

        let mut rect = nsRect::default();
        rect.SetBox(
            NSToCoordFloor__float(NSCoordScale(self.x, aFromAPP, aToAPP)),
            NSToCoordFloor__float(NSCoordScale(self.y, aFromAPP, aToAPP)),
            NSToCoordCeil__float(NSCoordScale(self.XMost(), aFromAPP, aToAPP)),
            NSToCoordCeil__float(NSCoordScale(self.YMost(), aFromAPP, aToAPP)),
        );
        rect
    }

    // Gecko: gfx/src/nsRect.h nsRect::ScaleToOtherAppUnitsRoundIn
    #[must_use]
    pub fn ScaleToOtherAppUnitsRoundIn(&self, aFromAPP: i32, aToAPP: i32) -> nsRect {
        if aFromAPP == aToAPP {
            return *self;
        }

        let mut rect = nsRect::default();
        rect.SetBox(
            NSToCoordCeil__float(NSCoordScale(self.x, aFromAPP, aToAPP)),
            NSToCoordCeil__float(NSCoordScale(self.y, aFromAPP, aToAPP)),
            NSToCoordFloor__float(NSCoordScale(self.XMost(), aFromAPP, aToAPP)),
            NSToCoordFloor__float(NSCoordScale(self.YMost(), aFromAPP, aToAPP)),
        );
        rect
    }

    // NOT PORTED: pixel rounding, not needed for layout (they return mozilla::gfx::IntRect).
    // The declarations, then the bodies, as in the header; the SSE2 branches (`#if !defined(ANDROID) &&
    // defined(__SSE2__)`) are omitted from the quoted bodies since only the portable `#else` branch would be
    // translated:
    //
    // [[nodiscard]] inline mozilla::gfx::IntRect ScaleToNearestPixels(
    //     float aXScale, float aYScale, nscoord aAppUnitsPerPixel) const;
    //
    // [[nodiscard]] inline mozilla::gfx::IntRect ToNearestPixels(
    //     nscoord aAppUnitsPerPixel) const;
    //
    // // Note: this can turn an empty rectangle into a non-empty rectangle
    // [[nodiscard]] inline mozilla::gfx::IntRect ScaleToOutsidePixels(
    //     float aXScale, float aYScale, nscoord aAppUnitsPerPixel) const;
    //
    // // Note: this can turn an empty rectangle into a non-empty rectangle
    // [[nodiscard]] inline mozilla::gfx::IntRect ToOutsidePixels(
    //     nscoord aAppUnitsPerPixel) const;
    //
    // [[nodiscard]] inline mozilla::gfx::IntRect ScaleToInsidePixels(
    //     float aXScale, float aYScale, nscoord aAppUnitsPerPixel) const;
    //
    // [[nodiscard]] inline mozilla::gfx::IntRect ToInsidePixels(
    //     nscoord aAppUnitsPerPixel) const;
    //
    // // scale the rect but round to preserve centers
    // inline mozilla::gfx::IntRect nsRect::ScaleToNearestPixels(
    //     float aXScale, float aYScale, nscoord aAppUnitsPerPixel) const {
    //   mozilla::gfx::IntRect rect;
    //   rect.SetNonEmptyBox(
    //       NSToIntRoundUp(NSAppUnitsToFloatPixels(x, aAppUnitsPerPixel) * aXScale),
    //       NSToIntRoundUp(NSAppUnitsToFloatPixels(y, aAppUnitsPerPixel) * aYScale),
    //       NSToIntRoundUp(NSAppUnitsToFloatPixels(XMost(), aAppUnitsPerPixel) *
    //                      aXScale),
    //       NSToIntRoundUp(NSAppUnitsToFloatPixels(YMost(), aAppUnitsPerPixel) *
    //                      aYScale));
    //   return rect;
    // }
    //
    // // scale the rect but round to smallest containing rect
    // inline mozilla::gfx::IntRect nsRect::ScaleToOutsidePixels(
    //     float aXScale, float aYScale, nscoord aAppUnitsPerPixel) const {
    //   mozilla::gfx::IntRect rect;
    //   rect.SetNonEmptyBox(
    //       NSToIntFloor(NSAppUnitsToFloatPixels(x, float(aAppUnitsPerPixel)) *
    //                    aXScale),
    //       NSToIntFloor(NSAppUnitsToFloatPixels(y, float(aAppUnitsPerPixel)) *
    //                    aYScale),
    //       NSToIntCeil(NSAppUnitsToFloatPixels(XMost(), float(aAppUnitsPerPixel)) *
    //                   aXScale),
    //       NSToIntCeil(NSAppUnitsToFloatPixels(YMost(), float(aAppUnitsPerPixel)) *
    //                   aYScale));
    //   return rect;
    // }
    //
    // // scale the rect but round to largest contained rect
    // inline mozilla::gfx::IntRect nsRect::ScaleToInsidePixels(
    //     float aXScale, float aYScale, nscoord aAppUnitsPerPixel) const {
    //   mozilla::gfx::IntRect rect;
    //   rect.SetNonEmptyBox(
    //       NSToIntCeil(NSAppUnitsToFloatPixels(x, float(aAppUnitsPerPixel)) *
    //                   aXScale),
    //       NSToIntCeil(NSAppUnitsToFloatPixels(y, float(aAppUnitsPerPixel)) *
    //                   aYScale),
    //       NSToIntFloor(NSAppUnitsToFloatPixels(XMost(), float(aAppUnitsPerPixel)) *
    //                    aXScale),
    //       NSToIntFloor(NSAppUnitsToFloatPixels(YMost(), float(aAppUnitsPerPixel)) *
    //                    aYScale));
    //   return rect;
    // }
    //
    // inline mozilla::gfx::IntRect nsRect::ToNearestPixels(
    //     nscoord aAppUnitsPerPixel) const {
    //   return ScaleToNearestPixels(1.0f, 1.0f, aAppUnitsPerPixel);
    // }
    //
    // inline mozilla::gfx::IntRect nsRect::ToOutsidePixels(
    //     nscoord aAppUnitsPerPixel) const {
    //   return ScaleToOutsidePixels(1.0f, 1.0f, aAppUnitsPerPixel);
    // }
    //
    // inline mozilla::gfx::IntRect nsRect::ToInsidePixels(
    //     nscoord aAppUnitsPerPixel) const {
    //   return ScaleToInsidePixels(1.0f, 1.0f, aAppUnitsPerPixel);
    // }

    // This is here only to keep IPDL-generated code happy. DO NOT USE.
    // bool operator==(const nsRect& aRect) const { return IsEqualEdges(aRect); }
    // (see `impl PartialEq for nsRect` below)

    // Gecko: gfx/src/nsRect.h nsRect::RemoveResolution
    #[must_use]
    pub fn RemoveResolution(&self, aResolution: f32) -> nsRect {
        MOZ_ASSERT!(aResolution > 0.0f32);
        let mut rect = nsRect::default();
        rect.MoveTo__nscoord_nscoord(
            NSToCoordRound__float(NSCoordToFloat(self.x) / aResolution),
            NSToCoordRound__float(NSCoordToFloat(self.y) / aResolution),
        );
        // A 1x1 rect indicates we are just hit testing a point, so pass down a 1x1
        // rect as well instead of possibly rounding the width or height to zero.
        if self.width == 1 && self.height == 1 {
            rect.SizeTo__nscoord_nscoord(1, 1);
        } else {
            rect.SizeTo__nscoord_nscoord(
                NSToCoordCeil__float(NSCoordToFloat(self.width) / aResolution),
                NSToCoordCeil__float(NSCoordToFloat(self.height) / aResolution),
            );
        }

        rect
    }

    // mozilla::Maybe<nsRect> is Option<nsRect>.
    // Gecko: gfx/src/nsRect.h nsRect::EdgeInclusiveIntersection
    #[must_use]
    pub fn EdgeInclusiveIntersection(&self, aOther: nsRect) -> Option<nsRect> {
        let left: nscoord = std::cmp::max(self.x, aOther.x);
        let top: nscoord = std::cmp::max(self.y, aOther.y);
        let right: nscoord = std::cmp::min(self.XMost(), aOther.XMost());
        let bottom: nscoord = std::cmp::min(self.YMost(), aOther.YMost());
        if left > right || top > bottom {
            return None;
        }
        Some(nsRect::new(left, top, right - left, bottom - top))
    }
}

// This is here only to keep IPDL-generated code happy. DO NOT USE.
// bool operator==(const nsRect& aRect) const { return IsEqualEdges(aRect); }
// Gecko: gfx/src/nsRect.h nsRect::operator==
impl PartialEq for nsRect {
    fn eq(&self, aRect: &nsRect) -> bool {
        self.IsEqualEdges(*aRect)
    }
}

// NOT PORTED: pixel rounding, not needed for layout
// const mozilla::gfx::IntRect& GetMaxSizedIntRect();
// const mozilla::gfx::IntRect& GetMaxSizedIntRect() {
//   static const mozilla::gfx::IntRect r(0, 0, INT32_MAX, INT32_MAX);
//   return r;
// }
//
// // app units are integer multiples of pixels, so no rounding needed
// template <class units>
// nsRect ToAppUnits(const mozilla::gfx::IntRectTyped<units>& aRect,
//                   nscoord aAppUnitsPerPixel) {
//   return nsRect(NSIntPixelsToAppUnits(aRect.X(), aAppUnitsPerPixel),
//                 NSIntPixelsToAppUnits(aRect.Y(), aAppUnitsPerPixel),
//                 NSIntPixelsToAppUnits(aRect.Width(), aAppUnitsPerPixel),
//                 NSIntPixelsToAppUnits(aRect.Height(), aAppUnitsPerPixel));
// }

// struct nsRectCornerRadii final
//     : public mozilla::gfx::BaseRectCornerRadii<nscoord, nsSize, nsMargin> {
//   using BaseRectCornerRadii::BaseRectCornerRadii;
// };
// Gecko: gfx/src/nsRect.h nsRectCornerRadii
BaseRectCornerRadii!(nsRectCornerRadii, nscoord, nsSize, nsMargin);

// ---------------------------------------------------------------------------------------------------------
// Tests: gfx/tests/gtest/TestRect.cpp (the parts that use ported types: nsRect). TestCoord.cpp only tests
// AuCoord / AuCoord64 (NOT PORTED), so there is nothing to translate from it.
//
// Skipped parts of TestRect.cpp: the static_asserts on CSSIntSize / CSSCoord (Units types), the `Gfx.Logical`
// test (LogicalRect lives in WritingModes.rs, which this file does not depend on), `Gfx.nsIntRect`,
// `Gfx.gfxRect`, TestFiniteGfx, `Gfx.nsRectAbsolute`, `Gfx.IntRectAbsolute` (types not ported). The gtests
// TestMoveInsideAndClamp / ClampPoint / SafeMoveBy run on IntRect / Rect there; the same cases are run on
// nsRect here (BaseRect code is shared), as `gfx_MoveInsideAndClamp_nsRect`, `gfx_ClampPoint_nsRect`,
// `gfx_SafeMoveBy_nsRect` (test names carry a `gfx_` prefix so `cargo test --lib gfx` selects them) (not in the gtest file under these names).
// The gtest templates over RectType are instantiated for nsRect only.
// ---------------------------------------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    type RectType = nsRect;
    type UnitType = nscoord;

    fn TestRectConstructors() -> bool {
        // Create a rectangle
        let mut rect1 = RectType::new(10, 20, 30, 40);

        // Make sure the rectangle was properly initialized
        assert!(
            rect1.IsEqualRect(10, 20, 30, 40) && rect1.IsEqualXY(10, 20) && rect1.IsEqualSize(30, 40),
            "[1] Make sure the rectangle was properly initialized with constructor"
        );

        // Create a second rect using the copy constructor
        let rect2 = rect1;

        // Make sure the rectangle was properly initialized
        assert!(
            rect2.IsEqualEdges(rect1)
                && rect2.IsEqualXY(rect1.X(), rect1.Y())
                && rect2.IsEqualSize(rect1.Width(), rect1.Height()),
            "[2] Make sure the rectangle was properly initialized with copy constructor"
        );

        assert!(
            !rect1.IsEmpty()
                && !rect1.IsZeroArea()
                && rect1.IsFinite()
                && !rect2.IsEmpty()
                && !rect2.IsZeroArea()
                && rect2.IsFinite(),
            "[3] These rectangles are not empty and are finite"
        );

        rect1.SetRect__nscoord_nscoord_nscoord_nscoord(1, 2, 30, 40);
        assert!(
            rect1.X() == 1 && rect1.Y() == 2 && rect1.Width() == 30 && rect1.Height() == 40 && rect1.XMost() == 31
                && rect1.YMost() == 42
        );

        rect1.SetRectX(11, 50);
        assert!(
            rect1.X() == 11 && rect1.Y() == 2 && rect1.Width() == 50 && rect1.Height() == 40 && rect1.XMost() == 61
                && rect1.YMost() == 42
        );

        rect1.SetRectY(22, 60);
        assert!(
            rect1.X() == 11 && rect1.Y() == 22 && rect1.Width() == 50 && rect1.Height() == 60 && rect1.XMost() == 61
                && rect1.YMost() == 82
        );

        rect1.SetBox(1, 2, 31, 42);
        assert!(
            rect1.X() == 1 && rect1.Y() == 2 && rect1.Width() == 30 && rect1.Height() == 40 && rect1.XMost() == 31
                && rect1.YMost() == 42
        );

        rect1.SetBoxX(11, 61);
        assert!(
            rect1.X() == 11 && rect1.Y() == 2 && rect1.Width() == 50 && rect1.Height() == 40 && rect1.XMost() == 61
                && rect1.YMost() == 42
        );

        rect1.SetBoxY(22, 82);
        assert!(
            rect1.X() == 11 && rect1.Y() == 22 && rect1.Width() == 50 && rect1.Height() == 60 && rect1.XMost() == 61
                && rect1.YMost() == 82
        );

        rect1.SetRect__nscoord_nscoord_nscoord_nscoord(1, 2, 30, 40);
        assert!(
            rect1.X() == 1 && rect1.Y() == 2 && rect1.Width() == 30 && rect1.Height() == 40 && rect1.XMost() == 31
                && rect1.YMost() == 42
        );

        rect1.MoveByX(10);
        assert!(
            rect1.X() == 11 && rect1.Y() == 2 && rect1.Width() == 30 && rect1.Height() == 40 && rect1.XMost() == 41
                && rect1.YMost() == 42
        );

        rect1.MoveByY(20);
        assert!(
            rect1.X() == 11 && rect1.Y() == 22 && rect1.Width() == 30 && rect1.Height() == 40 && rect1.XMost() == 41
                && rect1.YMost() == 62
        );

        true
    }

    fn TestEqualityOperator() -> bool {
        let mut rect1 = RectType::new(10, 20, 30, 40);
        let mut rect2 = rect1;

        // Test the equality operator
        assert!(rect1 == rect2, "[1] Test the equality operator");

        assert!(!(!rect1.IsEqualInterior(rect2)), "[2] Test the inequality operator");

        // Make sure that two empty rects are equal
        rect1.SetEmpty();
        rect2.SetEmpty();
        assert!(rect1 == rect2, "[3] Make sure that two empty rects are equal");

        true
    }

    fn TestContainment() -> bool {
        let rect1 = RectType::new(10, 10, 50, 50);

        // Test the point containment methods
        //

        // Basic test of a point in the middle of the rect
        assert!(
            rect1.Contains__nsPoint(rect1.Center())
                && rect1.ContainsX(rect1.Center().x)
                && rect1.ContainsY(rect1.Center().y),
            "[1] Basic test of a point in the middle of the rect"
        );

        // Test against a point at the left/top edges
        assert!(
            rect1.Contains__nscoord_nscoord(rect1.X(), rect1.Y()) && rect1.ContainsX(rect1.X()) && rect1.ContainsY(rect1.Y()),
            "[2] Test against a point at the left/top edges"
        );

        // Test against a point at the right/bottom extents
        assert!(
            !(rect1.Contains__nscoord_nscoord(rect1.XMost(), rect1.YMost())
                || rect1.ContainsX(rect1.XMost())
                || rect1.ContainsY(rect1.YMost())),
            "[3] Test against a point at the right/bottom extents"
        );

        // Test the rect containment methods
        //
        let mut rect2 = rect1;

        // Test against a rect that's the same as rect1
        assert!(!(!rect1.Contains__nsRect(rect2)), "[4] Test against a rect that's the same as rect1");

        // Test against a rect whose left edge (only) is outside of rect1
        rect2.MoveByX(-1);
        assert!(
            !rect1.Contains__nsRect(rect2),
            "[5] Test against a rect whose left edge (only) is outside of rect1"
        );
        rect2.MoveByX(1);

        // Test against a rect whose top edge (only) is outside of rect1
        rect2.MoveByY(-1);
        assert!(
            !rect1.Contains__nsRect(rect2),
            "[6] Test against a rect whose top edge (only) is outside of rect1"
        );
        rect2.MoveByY(1);

        // Test against a rect whose right edge (only) is outside of rect1
        rect2.MoveByX(1);
        assert!(
            !rect1.Contains__nsRect(rect2),
            "[7] Test against a rect whose right edge (only) is outside of rect1"
        );
        rect2.MoveByX(-1);

        // Test against a rect whose bottom edge (only) is outside of rect1
        rect2.MoveByY(1);
        assert!(
            !rect1.Contains__nsRect(rect2),
            "[8] Test against a rect whose bottom edge (only) is outside of rect1"
        );
        rect2.MoveByY(-1);

        // Test rects approaching numeric limits can contain rects
        let rectLarge = RectType::new(10, 10, UnitType::MAX, UnitType::MAX);
        assert!(
            rectLarge.Contains__nsRect(rect2),
            "[9] Test rect at numeric limits against a smaller rect"
        );

        true
    }

    // Test the method that returns a boolean result but doesn't return a
    // a rectangle
    fn TestIntersects() -> bool {
        let rect1 = RectType::new(10, 10, 50, 50);
        let mut rect2 = rect1;

        // Test against a rect that's the same as rect1
        assert!(!(!rect1.Intersects(rect2)), "[1] Test against a rect that's the same as rect1");

        // Test against a rect that's enclosed by rect1
        rect2.Inflate__nscoord_nscoord(-1, -1);
        assert!(
            !(!rect1.Contains__nsRect(rect2) || !rect1.Intersects(rect2)),
            "[2] Test against a rect that's enclosed by rect1"
        );
        rect2.Inflate__nscoord_nscoord(1, 1);

        // Make sure inflate and deflate worked correctly
        assert!(rect1.IsEqualInterior(rect2), "[3] Make sure inflate and deflate worked correctly");

        // Test against a rect that overlaps the left edge of rect1
        rect2.MoveByX(-1);
        assert!(!(!rect1.Intersects(rect2)), "[4] Test against a rect that overlaps the left edge of rect1");
        rect2.MoveByX(1);

        // Test against a rect that's outside of rect1 on the left
        rect2.MoveByX(-rect2.Width());
        assert!(!rect1.Intersects(rect2), "[5] Test against a rect that's outside of rect1 on the left");
        rect2.MoveByX(rect2.Width());

        // Test against a rect that overlaps the top edge of rect1
        rect2.MoveByY(-1);
        assert!(!(!rect1.Intersects(rect2)), "[6] Test against a rect that overlaps the top edge of rect1");
        rect2.MoveByY(1);

        // Test against a rect that's outside of rect1 on the top
        rect2.MoveByY(-rect2.Height());
        assert!(!rect1.Intersects(rect2), "[7] Test against a rect that's outside of rect1 on the top");
        rect2.MoveByY(rect2.Height());

        // Test against a rect that overlaps the right edge of rect1
        rect2.MoveByX(1);
        assert!(!(!rect1.Intersects(rect2)), "[8] Test against a rect that overlaps the right edge of rect1");
        rect2.MoveByX(-1);

        // Test against a rect that's outside of rect1 on the right
        rect2.MoveByX(rect2.Width());
        assert!(!rect1.Intersects(rect2), "[9] Test against a rect that's outside of rect1 on the right");
        rect2.MoveByX(-rect2.Width());

        // Test against a rect that overlaps the bottom edge of rect1
        rect2.MoveByY(1);
        assert!(!(!rect1.Intersects(rect2)), "[10] Test against a rect that overlaps the bottom edge of rect1");
        rect2.MoveByY(-1);

        // Test against a rect that's outside of rect1 on the bottom
        rect2.MoveByY(rect2.Height());
        assert!(!rect1.Intersects(rect2), "[11] Test against a rect that's outside of rect1 on the bottom");
        rect2.MoveByY(-rect2.Height());

        true
    }

    // Test the method that returns a boolean result and an intersection rect
    fn TestIntersection() -> bool {
        let mut rect1 = RectType::new(10, 10, 50, 50);
        let mut rect2 = rect1;
        let mut dest = RectType::default();

        // Test against a rect that's the same as rect1
        assert!(
            !(!dest.IntersectRect(rect1, rect2) || !(dest.IsEqualInterior(rect1))),
            "[1] Test against a rect that's the same as rect1"
        );

        // Test against a rect that's enclosed by rect1
        rect2.Inflate__nscoord_nscoord(-1, -1);
        assert!(
            !(!dest.IntersectRect(rect1, rect2) || !(dest.IsEqualInterior(rect2))),
            "[2] Test against a rect that's enclosed by rect1"
        );
        rect2.Inflate__nscoord_nscoord(1, 1);

        // Test against a rect that overlaps the left edge of rect1
        rect2.MoveByX(-1);
        assert!(
            !(!dest.IntersectRect(rect1, rect2)
                || !(dest.IsEqualInterior(RectType::new(rect1.X(), rect1.Y(), rect1.Width() - 1, rect1.Height())))),
            "[3] Test against a rect that overlaps the left edge of rect1"
        );
        rect2.MoveByX(1);

        // Test against a rect that's outside of rect1 on the left
        rect2.MoveByX(-rect2.Width());
        assert!(
            !dest.IntersectRect(rect1, rect2),
            "[4] Test against a rect that's outside of rect1 on the left"
        );
        // Make sure an empty rect is returned
        assert!(dest.IsEmpty() && dest.IsZeroArea(), "[4] Make sure an empty rect is returned");
        assert!(dest.IsFinite(), "[4b] Should be finite");
        rect2.MoveByX(rect2.Width());

        // Test against a rect that overlaps the top edge of rect1
        rect2.MoveByY(-1);
        assert!(
            !(!dest.IntersectRect(rect1, rect2)
                || !(dest.IsEqualInterior(RectType::new(rect1.X(), rect1.Y(), rect1.Width(), rect1.Height() - 1)))),
            "[5] Test against a rect that overlaps the top edge of rect1"
        );
        assert!(dest.IsFinite(), "[5b] Should be finite");
        rect2.MoveByY(1);

        // Test against a rect that's outside of rect1 on the top
        rect2.MoveByY(-rect2.Height());
        assert!(
            !dest.IntersectRect(rect1, rect2),
            "[6] Test against a rect that's outside of rect1 on the top"
        );
        // Make sure an empty rect is returned
        assert!(dest.IsEmpty() && dest.IsZeroArea(), "[6] Make sure an empty rect is returned");
        assert!(dest.IsFinite(), "[6b] Should be finite");
        rect2.MoveByY(rect2.Height());

        // Test against a rect that overlaps the right edge of rect1
        rect2.MoveByX(1);
        assert!(
            !(!dest.IntersectRect(rect1, rect2)
                || !(dest.IsEqualInterior(RectType::new(rect1.X() + 1, rect1.Y(), rect1.Width() - 1, rect1.Height())))),
            "[7] Test against a rect that overlaps the right edge of rect1"
        );
        rect2.MoveByX(-1);

        // Test against a rect that's outside of rect1 on the right
        rect2.MoveByX(rect2.Width());
        assert!(
            !dest.IntersectRect(rect1, rect2),
            "[8] Test against a rect that's outside of rect1 on the right"
        );
        // Make sure an empty rect is returned
        assert!(dest.IsEmpty() && dest.IsZeroArea(), "[8] Make sure an empty rect is returned");
        assert!(dest.IsFinite(), "[8b] Should be finite");
        rect2.MoveByX(-rect2.Width());

        // Test against a rect that overlaps the bottom edge of rect1
        rect2.MoveByY(1);
        assert!(
            !(!dest.IntersectRect(rect1, rect2)
                || !(dest.IsEqualInterior(RectType::new(rect1.X(), rect1.Y() + 1, rect1.Width(), rect1.Height() - 1)))),
            "[9] Test against a rect that overlaps the bottom edge of rect1"
        );
        assert!(dest.IsFinite(), "[9b] Should be finite");
        rect2.MoveByY(-1);

        // Test against a rect that's outside of rect1 on the bottom
        rect2.MoveByY(rect2.Height());
        assert!(
            !dest.IntersectRect(rect1, rect2),
            "[10] Test against a rect that's outside of rect1 on the bottom"
        );
        // Make sure an empty rect is returned
        assert!(dest.IsEmpty() && dest.IsZeroArea(), "[10] Make sure an empty rect is returned");
        assert!(dest.IsFinite(), "[10b] Should be finite");
        rect2.MoveByY(-rect2.Height());

        // Test against a rect with zero width or height
        rect1.SetRect__nscoord_nscoord_nscoord_nscoord(100, 100, 100, 100);
        rect2.SetRect__nscoord_nscoord_nscoord_nscoord(150, 100, 0, 100);
        assert!(
            !dest.IntersectRect(rect1, rect2) && dest.IsEmpty() && dest.IsZeroArea(),
            "[11] Intersection of rects with zero width or height should be empty"
        );
        assert!(dest.IsFinite(), "[11b] Should be finite");

        // Tests against a rect with negative width or height
        //

        // Test against a rect with negative width
        rect1.SetRect__nscoord_nscoord_nscoord_nscoord(100, 100, 100, 100);
        rect2.SetRect__nscoord_nscoord_nscoord_nscoord(100, 100, -100, 100);
        assert!(
            !dest.IntersectRect(rect1, rect2) && dest.IsEmpty() && dest.IsZeroArea(),
            "[12] Intersection of rects with negative width or height should be empty"
        );
        assert!(dest.IsFinite(), "[12b] Should be finite");

        // Those two rects exactly overlap in some way...
        // but we still want to return an empty rect
        rect1.SetRect__nscoord_nscoord_nscoord_nscoord(100, 100, 100, 100);
        rect2.SetRect__nscoord_nscoord_nscoord_nscoord(200, 200, -100, -100);
        assert!(
            !dest.IntersectRect(rect1, rect2) && dest.IsEmpty() && dest.IsZeroArea(),
            "[13] Intersection of rects with negative width or height should be empty"
        );
        assert!(dest.IsFinite(), "[13b] Should be finite");

        // Test against two identical rects with negative height
        rect1.SetRect__nscoord_nscoord_nscoord_nscoord(100, 100, 100, -100);
        rect2.SetRect__nscoord_nscoord_nscoord_nscoord(100, 100, 100, -100);
        assert!(
            !dest.IntersectRect(rect1, rect2) && dest.IsEmpty() && dest.IsZeroArea(),
            "[14] Intersection of rects with negative width or height should be empty"
        );
        assert!(dest.IsFinite(), "[14b] Should be finite");

        true
    }

    fn TestUnion() -> bool {
        let mut rect1 = RectType::default();
        let mut rect2 = RectType::new(10, 10, 50, 50);
        let mut dest = RectType::default();

        // Check the case where the receiver is an empty rect
        rect1.SetEmpty();
        dest.UnionRect(rect1, rect2);
        assert!(
            !dest.IsEmpty() && !dest.IsZeroArea() && dest.IsEqualInterior(rect2),
            "[1] Check the case where the receiver is an empty rect"
        );
        assert!(dest.IsFinite(), "[1b] Should be finite");

        // Check the case where the source rect is an empty rect
        rect1 = rect2;
        rect2.SetEmpty();
        dest.UnionRect(rect1, rect2);
        assert!(
            !dest.IsEmpty() && !dest.IsZeroArea() && dest.IsEqualInterior(rect1),
            "[2] Check the case where the source rect is an empty rect"
        );
        assert!(dest.IsFinite(), "[2b] Should be finite");

        // Test the case where both rects are empty
        rect1.SetEmpty();
        rect2.SetEmpty();
        dest.UnionRect(rect1, rect2);
        assert!(dest.IsEmpty() && dest.IsZeroArea(), "[3] Test the case where both rects are empty");
        assert!(dest.IsFinite(), "[3b] Should be finite");

        // Test union case where the two rects don't overlap at all
        rect1.SetRect__nscoord_nscoord_nscoord_nscoord(10, 10, 50, 50);
        rect2.SetRect__nscoord_nscoord_nscoord_nscoord(100, 100, 50, 50);
        dest.UnionRect(rect1, rect2);
        assert!(
            !dest.IsEmpty()
                && !dest.IsZeroArea()
                && (dest.IsEqualInterior(RectType::new(
                    rect1.X(),
                    rect1.Y(),
                    rect2.XMost() - rect1.X(),
                    rect2.YMost() - rect1.Y()
                ))),
            "[4] Test union case where the two rects don't overlap at all"
        );
        assert!(dest.IsFinite(), "[4b] Should be finite");

        // Test union case where the two rects overlap
        rect1.SetRect__nscoord_nscoord_nscoord_nscoord(30, 30, 50, 50);
        rect2.SetRect__nscoord_nscoord_nscoord_nscoord(10, 10, 50, 50);
        dest.UnionRect(rect1, rect2);
        assert!(
            !dest.IsEmpty()
                && !dest.IsZeroArea()
                && (dest.IsEqualInterior(RectType::new(
                    rect2.X(),
                    rect2.Y(),
                    rect1.XMost() - rect2.X(),
                    rect1.YMost() - rect2.Y()
                ))),
            "[5] Test union case where the two rects overlap"
        );
        assert!(dest.IsFinite(), "[5b] Should be finite");

        true
    }

    fn TestUnionEmptyRects() {
        let rect1 = RectType::new(10, 10, 0, 50);
        let rect2 = RectType::new(5, 5, 40, 0);
        assert!(rect1.IsEmpty() && rect2.IsEmpty());

        let dest = rect1.Union(rect2);
        assert!(
            dest.IsEmpty() && dest.IsEqualEdges(rect2),
            "Test the case where both rects are empty, and the result is the same value passing into Union()"
        );
    }

    // We want to test nsRect values that are still in range but where
    // the implementation is at risk of overflowing
    fn TestBug1135677() -> bool {
        let rect1 = RectType::new(1073741344, 1073741344, 1073756696, 1073819936);
        let rect2 = RectType::new(1073741820, 1073741820, 14400, 77640);
        let dest: RectType;

        dest = rect1.Intersect(rect2);

        assert!(
            dest.IsEqualRect(1073741820, 1073741820, 14400, 77640),
            "[1] Operation should not overflow internally."
        );

        true
    }

    fn TestSetWH() -> bool {
        let mut rect = RectType::new(1, 2, 3, 4);
        assert!(rect.IsEqualRect(1, 2, 3, 4));
        rect.SetWidth(13);
        assert!(rect.IsEqualRect(1, 2, 13, 4));
        rect.SetHeight(14);
        assert!(rect.IsEqualRect(1, 2, 13, 14));
        rect.SizeTo__nscoord_nscoord(23, 24);
        assert!(rect.IsEqualRect(1, 2, 23, 24));
        true
    }

    fn TestSwap() -> bool {
        let mut rect = RectType::new(1, 2, 3, 4);
        assert!(rect.IsEqualRect(1, 2, 3, 4));
        rect.Swap();
        assert!(rect.IsEqualRect(2, 1, 4, 3));
        true
    }

    // TEST(Gfx, nsRect)
    #[test]
    fn gfx_nsRect() {
        TestRectConstructors();
        TestEqualityOperator();
        TestContainment();
        TestIntersects();
        TestIntersection();
        TestUnion();
        TestUnionEmptyRects();
        TestBug1135677();
        TestSetWH();
        TestSwap();
    }

    fn TestMoveInsideAndClamp(aSrc: nsRect, aTarget: nsRect, aExpected: nsRect) {
        // Test the implementation in BaseRect (x/y/width/height representation)
        let result = aSrc.MoveInsideAndClamp(aTarget);
        assert!(
            result.IsEqualEdges(aExpected),
            "Source {:?} Target {:?} Expected {:?} Actual {:?}",
            aSrc,
            aTarget,
            aExpected,
            result
        );
    }

    // Adapted from TEST(Gfx, MoveInsideAndClamp): same cases, nsRect instead of IntRect.
    #[test]
    fn gfx_MoveInsideAndClamp_nsRect() {
        TestMoveInsideAndClamp(nsRect::new(0, 0, 10, 10), nsRect::new(1, -1, 10, 10), nsRect::new(1, -1, 10, 10));
        TestMoveInsideAndClamp(nsRect::new(0, 0, 10, 10), nsRect::new(-1, -1, 12, 5), nsRect::new(0, -1, 10, 5));
        TestMoveInsideAndClamp(nsRect::new(0, 0, 10, 10), nsRect::new(10, 11, 10, 0), nsRect::new(10, 11, 10, 0));
        TestMoveInsideAndClamp(nsRect::new(0, 0, 10, 10), nsRect::new(-10, -1, 10, 0), nsRect::new(-10, -1, 10, 0));
        TestMoveInsideAndClamp(nsRect::new(0, 0, 0, 0), nsRect::new(10, -10, 10, 10), nsRect::new(10, 0, 0, 0));
    }

    // Adapted from TEST(Gfx, ClampPoint): same cases, nsRect / nsPoint instead of IntRect / IntPoint.
    #[test]
    fn gfx_ClampPoint_nsRect() {
        /* Various bounds */
        let Square = nsRect::new(0, 0, 10, 10);
        assert_eq!(Square.ClampPoint(nsPoint::new(-5, -5)), nsPoint::new(0, 0));
        assert_eq!(Square.ClampPoint(nsPoint::new(-5, 5)), nsPoint::new(0, 5));
        assert_eq!(Square.ClampPoint(nsPoint::new(-5, 15)), nsPoint::new(0, 10));

        assert_eq!(Square.ClampPoint(nsPoint::new(5, -5)), nsPoint::new(5, 0));
        assert_eq!(Square.ClampPoint(nsPoint::new(5, 5)), nsPoint::new(5, 5));
        assert_eq!(Square.ClampPoint(nsPoint::new(5, 15)), nsPoint::new(5, 10));

        assert_eq!(Square.ClampPoint(nsPoint::new(15, -5)), nsPoint::new(10, 0));
        assert_eq!(Square.ClampPoint(nsPoint::new(15, 5)), nsPoint::new(10, 5));
        assert_eq!(Square.ClampPoint(nsPoint::new(15, 15)), nsPoint::new(10, 10));

        /* Special case of empty rect */
        let Empty = nsRect::new(0, 0, -1, -1);
        assert!(Empty.IsEmpty());
        assert_eq!(Empty.ClampPoint(nsPoint::new(-1, -1)), nsPoint::new(0, 0));
        assert_eq!(Empty.ClampPoint(nsPoint::new(-1, 1)), nsPoint::new(0, 0));
        assert_eq!(Empty.ClampPoint(nsPoint::new(1, -1)), nsPoint::new(0, 0));
        assert_eq!(Empty.ClampPoint(nsPoint::new(1, 1)), nsPoint::new(0, 0));
    }

    // Adapted from the IntRect half of TEST(Gfx, SafeMoveBy).
    #[test]
    fn gfx_SafeMoveBy_nsRect() {
        let mut intRect = nsRect::new(0, 0, 10, 10);
        intRect.SafeMoveByX(10);
        intRect.SafeMoveByY(10);
        assert_eq!(intRect, nsRect::new(10, 10, 10, 10));

        intRect = nsRect::new(0, 0, 10, 10);
        intRect.SafeMoveByX(-10);
        intRect.SafeMoveByY(-10);
        assert_eq!(intRect, nsRect::new(-10, -10, 10, 10));
    }

    // ---- Tests below are not from the gtest files; written from the documented behaviour. ----

    #[test]
    fn gfx_operators_and_overloads() {
        let mut r = nsRect::new(10, 20, 30, 40);
        assert_eq!(r + nsPoint::new(1, 2), nsRect::new(11, 22, 30, 40));
        assert_eq!(r - nsPoint::new(1, 2), nsRect::new(9, 18, 30, 40));
        assert_eq!(r + nsSize::new(1, 2), nsRect::new(10, 20, 31, 42));
        assert_eq!(r - nsSize::new(1, 2), nsRect::new(10, 20, 29, 38));
        // nsRect - nsRect -> nsMargin(top, right, bottom, left)
        let m = nsRect::new(0, 0, 100, 100) - nsRect::new(10, 20, 30, 40);
        assert_eq!(m, nsMargin::new(20, 60, 40, 10));
        r.Inflate__nsMargin(nsMargin::new(1, 2, 3, 4));
        assert_eq!(r, nsRect::new(6, 19, 36, 44));
        r.Deflate__nsMargin(nsMargin::new(1, 2, 3, 4));
        assert_eq!(r, nsRect::new(10, 20, 30, 40));
        r.Inflate__nscoord(5);
        assert_eq!(r, nsRect::new(5, 15, 40, 50));
        r.Deflate__nscoord(5);
        r.Deflate__nsSize(nsSize::new(100, 100));
        assert!(r.IsEqualEdges(nsRect::new(110, 120, 0, 0)));
        assert_eq!(nsRect::new(0, 0, 10, 10).Center(), nsPoint::new(5, 5));
        assert_eq!(nsRect::new(1, 2, 3, 4).BottomRight(), nsPoint::new(4, 6));
        assert_eq!(nsRect::new(1, 2, 3, 4).Edge(eSideRight), 4);
        assert_eq!(nsRect::new(1, 2, 3, 4).AtCorner(eCornerBottomLeft), nsPoint::new(1, 6));
        assert_eq!(nsRect::new(1, 2, 3, 4).Size(), nsSize::new(3, 4));
        assert_eq!(nsRect::new(0, 0, 10, 10).DistanceTo(nsPoint::new(15, -3)), nsPoint::new(5, 3));
    }

    #[test]
    fn gfx_xmost_saturates() {
        let r = nsRect::new(i32::MAX - 1, 0, 10, 10);
        assert_eq!(r.XMost(), i32::MAX);
        assert!(r.Overflows());
        assert!(!nsRect::new(0, 0, 10, 10).Overflows());
    }

    #[test]
    fn gfx_saturating_union() {
        let a = nsRect::new(nscoord_MIN, 0, nscoord_MAX, 10);
        let b = nsRect::new(nscoord_MAX - 10, 0, 5, 10);
        let u = a.SaturatingUnion(b);
        assert_eq!(u.width, nscoord_MAX);
        let c = nsRect::new(10, 10, 5, 5).UnionEdges(nsRect::new(0, 0, 0, 0));
        assert!(c.IsEqualEdges(nsRect::new(0, 0, 15, 15)));
        assert!(nsRect::new(0, 0, 10, 10).UnsafeUnion(nsRect::new(20, 20, 10, 10)).IsEqualEdges(nsRect::new(0, 0, 30, 30)));
    }

    #[test]
    fn gfx_edge_inclusive_intersection() {
        let a = nsRect::new(0, 0, 10, 10);
        assert!(a.EdgeInclusiveIntersection(nsRect::new(10, 0, 5, 5)).unwrap().IsEqualEdges(nsRect::new(10, 0, 0, 5)));
        assert!(a.EdgeInclusiveIntersection(nsRect::new(11, 0, 5, 5)).is_none());
    }

    #[test]
    fn gfx_scale_to_other_app_units_and_resolution() {
        let r = nsRect::new(10, 10, 25, 25);
        // 60 -> 30: halves
        assert_eq!(r.ScaleToOtherAppUnitsRoundOut(60, 30), nsRect::new(5, 5, 13, 13));
        assert_eq!(r.ScaleToOtherAppUnitsRoundIn(60, 30), nsRect::new(5, 5, 12, 12));
        assert_eq!(r.ScaleToOtherAppUnitsRoundOut(60, 60), r);
        assert_eq!(nsRect::new(10, 10, 1, 1).RemoveResolution(2.0), nsRect::new(5, 5, 1, 1));
        assert_eq!(nsRect::new(10, 10, 5, 5).RemoveResolution(2.0), nsRect::new(5, 5, 3, 3));
        assert_eq!(nsPoint::new(10, 20).ScaleToOtherAppUnits(60, 30), nsPoint::new(5, 10));
        assert_eq!(nsSize::new(10, 20).ScaleToOtherAppUnits(30, 60), nsSize::new(20, 40));
    }

    #[test]
    fn gfx_point_size_margin() {
        let p = nsPoint::new(3, 4);
        assert_eq!(p + nsPoint::new(1, 1), nsPoint::new(4, 5));
        assert_eq!(-p, nsPoint::new(-3, -4));
        assert_eq!(p * 2, nsPoint::new(6, 8));
        assert_eq!(p.Length(), 5.0);
        assert_eq!(p.DotProduct(nsPoint::new(2, 1)), 10);
        let mut q = p;
        q += nsPoint::new(1, 1);
        q.MoveBy(1, 1);
        q.MoveTo(0, 0);
        assert_eq!(q, nsPoint::default());

        let s = nsSize::new(4, 6);
        assert_eq!(s * 2, nsSize::new(8, 12));
        assert_eq!(2 * s, nsSize::new(8, 12));
        assert_eq!(s * nsSize::new(2, 3), nsSize::new(8, 18));
        assert!(nsSize::new(1, 1) < nsSize::new(1, 2));
        assert!(!(nsSize::new(1, 3) <= nsSize::new(2, 2)));
        assert_eq!(Min__nsSize_nsSize(s, nsSize::new(5, 1)), nsSize::new(4, 1));
        assert_eq!(Max__nsSize_nsSize(s, nsSize::new(5, 1)), nsSize::new(5, 6));
        assert!(nsSize::new(0, 5).IsEmpty());

        let mut m = nsMargin::new(1, 2, 3, 4);
        assert_eq!(m.LeftRight(), 6);
        assert_eq!(m.TopBottom(), 4);
        assert_eq!(m.Side(eSideBottom), 3);
        *m.Side_mut(eSideLeft) = 9;
        assert_eq!(m.left, 9);
        m.ApplySkipSides(Sides::new(SideBits::eTop) | SideBits::eRight);
        assert_eq!(m, nsMargin::new(0, 0, 3, 9));
        assert_eq!(-m, nsMargin::new(0, 0, -3, -9));
        assert!(nsMargin::default().IsAllZero());
        assert!(Sides::new(SideBits::eAll).Contains(SideBits::eLeftRight));
    }

    #[test]
    fn gfx_corner_radii() {
        let mut r = nsRectCornerRadii::new__nscoord(10);
        assert!(r.AreRadiiSame() && r.AreShapesAllRound() && !r.IsEmpty());
        assert!(nsRectCornerRadii::default().IsEmpty());
        assert!(nsRectCornerRadii::default().AreShapesAllRound());
        r[eCornerTopLeft] = nsSize::new(1, 2);
        assert_eq!(r[eCornerTopLeftX], 1);
        assert_eq!(r[eCornerTopLeftY], 2);
        r[eCornerBottomRightY] = 7;
        assert_eq!(r.BottomRight().height, 7);
        assert!(!r.AreRadiiSame());
        let mut q = nsRectCornerRadii::new__nscoord_nscoord_nscoord_nscoord(4, 5, 6, 7);
        assert_eq!(*q.BottomLeft(), nsSize::new(7, 7));
        q.AdjustOutwards(nsMargin::new(1, 1, 1, 1));
        assert_eq!(*q.TopLeft(), nsSize::new(5, 5));
        q.AdjustInwards(nsMargin::new(10, 10, 10, 10));
        assert!(q.IsEmpty());
        assert_ne!(nsRectCornerRadii::new__nscoord(1), nsRectCornerRadii::new__nscoord(2));
    }
}
