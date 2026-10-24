// Gecko: gfx/src/nsSize.h
use crate::*;

// NOT PORTED: includes (mozilla/gfx/Point.h).

// Maximum allowable size
// Gecko: gfx/src/nsSize.h NS_MAXSIZE
pub const NS_MAXSIZE: nscoord = nscoord_MAX;

// NOT PORTED: typedef mozilla::gfx::IntSize nsIntSize;  (IntSize is NOT PORTED, see Rect.rs)

// struct nsSize : public mozilla::gfx::BaseSize<nscoord, nsSize>
// Gecko: gfx/src/nsSize.h nsSize
BaseSize!(nsSize, nscoord);

impl nsSize {
    // typedef mozilla::gfx::BaseSize<nscoord, nsSize> Super;
    //
    // constexpr nsSize() = default;                                  -> Default::default()
    // constexpr nsSize(nscoord aWidth, nscoord aHeight) : Super(aWidth, aHeight) {}   -> new(), from BaseSize!

    // NOT PORTED: pixel rounding, not needed for layout
    // mozilla::gfx::IntSize ScaleToNearestPixels(float aXScale, float aYScale,
    //                                            nscoord aAppUnitsPerPixel) const {
    //   return {NSToIntRoundUp(NSAppUnitsToDoublePixels(width, aAppUnitsPerPixel) *
    //                          aXScale),
    //           NSToIntRoundUp(NSAppUnitsToDoublePixels(height, aAppUnitsPerPixel) *
    //                          aYScale)};
    // }
    //
    // mozilla::gfx::IntSize ToNearestPixels(nscoord aAppUnitsPerPixel) const {
    //   return ScaleToNearestPixels(1.0f, 1.0f, aAppUnitsPerPixel);
    // }

    /**
     * Return this size scaled to a different appunits per pixel (APP) ratio.
     * @param aFromAPP the APP to scale from
     * @param aToAPP the APP to scale to
     */
    // Gecko: gfx/src/nsSize.h nsSize::ScaleToOtherAppUnits
    #[must_use]
    pub fn ScaleToOtherAppUnits(&self, aFromAPP: i32, aToAPP: i32) -> nsSize {
        if aFromAPP != aToAPP {
            let mut size = nsSize::default();
            size.width = NSToCoordRound__float(NSCoordScale(self.width, aFromAPP, aToAPP));
            size.height = NSToCoordRound__float(NSCoordScale(self.height, aFromAPP, aToAPP));
            return size;
        }
        *self
    }
}

// The friend functions Min / Max of BaseSize<nscoord, nsSize> (found by ADL in C++) as free functions;
// BaseSize!'s associated functions hold the bodies.
// Gecko: gfx/2d/BaseSize.h BaseSize::Min  (friend, nsSize instantiation)
pub fn Min__nsSize_nsSize(aA: nsSize, aB: nsSize) -> nsSize {
    nsSize::Min(aA, aB)
}
// Gecko: gfx/2d/BaseSize.h BaseSize::Max  (friend, nsSize instantiation)
pub fn Max__nsSize_nsSize(aA: nsSize, aB: nsSize) -> nsSize {
    nsSize::Max(aA, aB)
}

// NOT PORTED: pixel rounding, not needed for layout (needs mozilla::gfx::IntSize)
// inline nsSize IntSizeToAppUnits(mozilla::gfx::IntSize aSize,
//                                 nscoord aAppUnitsPerPixel) {
//   return nsSize(NSIntPixelsToAppUnits(aSize.width, aAppUnitsPerPixel),
//                 NSIntPixelsToAppUnits(aSize.height, aAppUnitsPerPixel));
// }
