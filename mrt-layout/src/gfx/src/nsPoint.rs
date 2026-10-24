// Gecko: gfx/src/nsPoint.h
use crate::*;

// NOT PORTED: includes (mozilla/gfx/Point.h).

// nsIntPoint represents a point in one of the types of pixels.
// Uses of nsIntPoint should eventually be converted to CSSIntPoint,
// LayoutDeviceIntPoint, etc. (see layout/base/Units.h).
// NOT PORTED: typedef mozilla::gfx::IntPoint nsIntPoint;  (IntPoint is NOT PORTED, see Rect.rs)

// nsPoint represents a point in app units.

// struct nsPoint : public mozilla::gfx::BasePoint<nscoord, nsPoint>
// Gecko: gfx/src/nsPoint.h nsPoint
BasePoint!(nsPoint, nscoord);

impl nsPoint {
    // typedef mozilla::gfx::BasePoint<nscoord, nsPoint> Super;
    //
    // nsPoint() = default;                          -> Default::default()
    // nsPoint(const nsPoint& aPoint) = default;     -> Copy
    // nsPoint(nscoord aX, nscoord aY) : Super(aX, aY) {}   -> new(aX, aY), from BasePoint!

    // NOT PORTED: pixel rounding, not needed for layout
    // inline nsIntPoint ScaleToNearestPixels(float aXScale, float aYScale,
    //                                        nscoord aAppUnitsPerPixel) const;
    // inline nsIntPoint ToNearestPixels(nscoord aAppUnitsPerPixel) const;
    // inline nsIntPoint nsPoint::ScaleToNearestPixels(
    //     float aXScale, float aYScale, nscoord aAppUnitsPerPixel) const {
    //   return nsIntPoint(
    //       NSToIntRoundUp(NSAppUnitsToDoublePixels(x, aAppUnitsPerPixel) * aXScale),
    //       NSToIntRoundUp(NSAppUnitsToDoublePixels(y, aAppUnitsPerPixel) * aYScale));
    // }
    //
    // inline nsIntPoint nsPoint::ToNearestPixels(nscoord aAppUnitsPerPixel) const {
    //   return ScaleToNearestPixels(1.0f, 1.0f, aAppUnitsPerPixel);
    // }

    /**
     * Return this point scaled to a different appunits per pixel (APP) ratio.
     * @param aFromAPP the APP to scale from
     * @param aToAPP the APP to scale to
     */
    // Gecko: gfx/src/nsPoint.h nsPoint::ScaleToOtherAppUnits
    #[must_use]
    pub fn ScaleToOtherAppUnits(&self, aFromAPP: i32, aToAPP: i32) -> nsPoint {
        if aFromAPP != aToAPP {
            let mut point = nsPoint::default();
            point.x = NSToCoordRound__float(NSCoordScale(self.x, aFromAPP, aToAPP));
            point.y = NSToCoordRound__float(NSCoordScale(self.y, aFromAPP, aToAPP));
            return point;
        }
        *self
    }

    // Gecko: gfx/src/nsPoint.h nsPoint::RemoveResolution
    #[must_use]
    pub fn RemoveResolution(&self, resolution: f32) -> nsPoint {
        if resolution != 1.0f32 {
            let mut point = nsPoint::default();
            point.x = NSToCoordRound__float(NSCoordToFloat(self.x) / resolution);
            point.y = NSToCoordRound__float(NSCoordToFloat(self.y) / resolution);
            return point;
        }
        *self
    }

    // Gecko: gfx/src/nsPoint.h nsPoint::ApplyResolution
    #[must_use]
    pub fn ApplyResolution(&self, resolution: f32) -> nsPoint {
        if resolution != 1.0f32 {
            let mut point = nsPoint::default();
            point.x = NSToCoordRound__float(NSCoordToFloat(self.x) * resolution);
            point.y = NSToCoordRound__float(NSCoordToFloat(self.y) * resolution);
            return point;
        }
        *self
    }
}

// NOT PORTED: pixel rounding, not needed for layout (needs nsIntPoint = IntPoint)
// inline nsPoint ToAppUnits(const nsIntPoint& aPoint, nscoord aAppUnitsPerPixel);
// // app units are integer multiples of pixels, so no rounding needed
// inline nsPoint ToAppUnits(const nsIntPoint& aPoint, nscoord aAppUnitsPerPixel) {
//   return nsPoint(NSIntPixelsToAppUnits(aPoint.x, aAppUnitsPerPixel),
//                  NSIntPixelsToAppUnits(aPoint.y, aAppUnitsPerPixel));
// }
