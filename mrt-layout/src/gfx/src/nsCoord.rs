// Gecko: gfx/src/nsCoord.h
use crate::*;

// NOT PORTED: refcount/gfx includes ("mozilla/gfx/Coord.h").
//
// The NS_lround / NS_lroundf helpers that NSToIntRound needs live in xpcom/ds/nsMathUtils.h, which is not
// ported as a file; the two tiny functions are written out here, right before their first user.

// Gecko: xpcom/ds/nsMathUtils.h NS_lround
pub fn NS_lround(aNum: f64) -> i32 {
    if aNum >= 0.0 { (aNum + 0.5) as i32 } else { (aNum - 0.5) as i32 }
}

// Gecko: xpcom/ds/nsMathUtils.h NS_lroundf
pub fn NS_lroundf(aNum: f32) -> i32 {
    if aNum >= 0.0f32 { (aNum + 0.5f32) as i32 } else { (aNum - 0.5f32) as i32 }
}

/*
 * Basic type used for the geometry classes.
 *
 * Normally all coordinates are maintained in an app unit coordinate
 * space. An app unit is 1/60th of a CSS device pixel, which is, in turn
 * an integer number of device pixels, such at the CSS DPI is as close to
 * 96dpi as possible.
 */

// Gecko: gfx/src/nsCoord.h nscoord
pub type nscoord = i32;
// Gecko: gfx/src/nsCoord.h nscoord_MAX
pub const nscoord_MAX: nscoord = (1 << 30) - 1;
// Gecko: gfx/src/nsCoord.h nscoord_MIN
pub const nscoord_MIN: nscoord = -nscoord_MAX;

// NOT PORTED: AuCoord / AuCoord64 and AppUnit (need mozilla/gfx/Coord.h: IntCoordTyped), not needed for layout.
// namespace mozilla {
// struct AppUnit {};
//
// // Declare AppUnit as a coordinate system tag.
// template <>
// struct IsPixel<AppUnit> : std::true_type {};
//
// namespace detail {
// template <typename Rep>
// struct AuCoordImpl : public gfx::IntCoordTyped<AppUnit, Rep> {
//   using Super = gfx::IntCoordTyped<AppUnit, Rep>;
//
//   constexpr AuCoordImpl() : Super() {}
//   constexpr MOZ_IMPLICIT AuCoordImpl(Rep aValue) : Super(aValue) {}
//   constexpr MOZ_IMPLICIT AuCoordImpl(Super aValue) : Super(aValue) {}
//
//   template <typename F>
//   static AuCoordImpl FromRound(F aValue) {
//     // Note: aValue is *not* rounding to nearest integer if it is negative. See
//     // https://bugzilla.mozilla.org/show_bug.cgi?id=410748#c14
//     return AuCoordImpl(std::floor(aValue + 0.5f));
//   }
//
//   template <typename F>
//   static AuCoordImpl FromTruncate(F aValue) {
//     return AuCoordImpl(std::trunc(aValue));
//   }
//
//   template <typename F>
//   static AuCoordImpl FromCeil(F aValue) {
//     return AuCoordImpl(std::ceil(aValue));
//   }
//
//   template <typename F>
//   static AuCoordImpl FromFloor(F aValue) {
//     return AuCoordImpl(std::floor(aValue));
//   }
//
//   // Note: this returns the result of the operation, without modifying the
//   // original value.
//   [[nodiscard]] AuCoordImpl ToMinMaxClamped() const {
//     return std::clamp(this->value, kMin, kMax);
//   }
//
//   static constexpr Rep kMax = nscoord_MAX;
//   static constexpr Rep kMin = nscoord_MIN;
// };
// }  // namespace detail
//
// using AuCoord = detail::AuCoordImpl<int32_t>;
// using AuCoord64 = detail::AuCoordImpl<int64_t>;
//
// }  // namespace mozilla

/**
 * Divide aSpace by aN.  Assign the resulting quotient to aQuotient and
 * return the remainder.
 */
// Gecko: gfx/src/nsCoord.h NSCoordDivRem
pub fn NSCoordDivRem(aSpace: nscoord, aN: usize, aQuotient: &mut nscoord) -> nscoord {
    // div_t result = div(aSpace, aN);  (C div takes int, so aN is converted to int)
    let n = aN as i32;
    let quot = aSpace / n;
    let rem = aSpace % n;
    *aQuotient = quot as nscoord;
    rem as nscoord
}

// Gecko: gfx/src/nsCoord.h NSCoordMulDiv
pub fn NSCoordMulDiv(aMult1: nscoord, aMult2: nscoord, aDiv: nscoord) -> nscoord {
    (aMult1 as i64 * aMult2 as i64 / aDiv as i64) as nscoord
}

// Gecko: gfx/src/nsCoord.h NSToCoordRound
pub fn NSToCoordRound__float(aValue: f32) -> nscoord {
    (aValue + 0.5f32).floor() as nscoord
}

// Gecko: gfx/src/nsCoord.h NSToCoordRound
pub fn NSToCoordRound__double(aValue: f64) -> nscoord {
    (aValue + 0.5f32 as f64).floor() as nscoord
}

// Gecko: gfx/src/nsCoord.h NSToCoordRoundWithClamp
pub fn NSToCoordRoundWithClamp__float(aValue: f32) -> nscoord {
    // Bounds-check before converting out of float, to avoid overflow
    if aValue >= nscoord_MAX as f32 {
        return nscoord_MAX;
    }
    if aValue <= nscoord_MIN as f32 {
        return nscoord_MIN;
    }
    NSToCoordRound__float(aValue)
}

// Gecko: gfx/src/nsCoord.h NSToCoordRoundWithClamp
pub fn NSToCoordRoundWithClamp__double(aValue: f64) -> nscoord {
    // Bounds-check before converting out of double, to avoid overflow
    if aValue >= nscoord_MAX as f64 {
        return nscoord_MAX;
    }
    if aValue <= nscoord_MIN as f64 {
        return nscoord_MIN;
    }
    NSToCoordRound__double(aValue)
}

/**
 * Returns aCoord * aScale, capping the product to nscoord_MAX or nscoord_MIN as
 * appropriate for the signs of aCoord and aScale.  If requireNotNegative is
 * true, this method will enforce that aScale is not negative; use that
 * parametrization to get a check of that fact in debug builds.
 */
// Gecko: gfx/src/nsCoord.h _nscoordSaturatingMultiply
pub fn _nscoordSaturatingMultiply(aCoord: nscoord, aScale: f32, requireNotNegative: bool) -> nscoord {
    if requireNotNegative {
        MOZ_ASSERT!(aScale >= 0.0f32, "negative scaling factors must be handled manually");
    }
    let product: f32 = aCoord as f32 * aScale;
    if if requireNotNegative { aCoord > 0 } else { (aCoord > 0) == (aScale > 0.0) } {
        // std::min<float>((float)nscoord_MAX, product)  ==  (product < MAX) ? product : MAX
        let max = nscoord_MAX as f32;
        return NSToCoordRoundWithClamp__float(if product < max { product } else { max });
    }
    // std::max<float>((float)nscoord_MIN, product)  ==  (MIN < product) ? product : MIN
    let min = nscoord_MIN as f32;
    NSToCoordRoundWithClamp__float(if min < product { product } else { min })
}

/**
 * Returns aCoord * aScale, capping the product to nscoord_MAX or nscoord_MIN as
 * appropriate for the sign of aCoord.  This method requires aScale to not be
 * negative; use this method when you know that aScale should never be
 * negative to get a sanity check of that invariant in debug builds.
 */
// Gecko: gfx/src/nsCoord.h NSCoordSaturatingNonnegativeMultiply
pub fn NSCoordSaturatingNonnegativeMultiply(aCoord: nscoord, aScale: f32) -> nscoord {
    _nscoordSaturatingMultiply(aCoord, aScale, true)
}

/**
 * Returns aCoord * aScale, capping the product to nscoord_MAX or nscoord_MIN as
 * appropriate for the signs of aCoord and aScale.
 */
// Gecko: gfx/src/nsCoord.h NSCoordSaturatingMultiply
pub fn NSCoordSaturatingMultiply(aCoord: nscoord, aScale: f32) -> nscoord {
    _nscoordSaturatingMultiply(aCoord, aScale, false)
}

/**
 * Returns a + b, capping the sum to nscoord_MAX.
 *
 * This function assumes that neither argument is nscoord_MIN.
 */
// Gecko: gfx/src/nsCoord.h NSCoordSaturatingAdd
pub fn NSCoordSaturatingAdd(a: nscoord, b: nscoord) -> nscoord {
    if a == nscoord_MAX || b == nscoord_MAX {
        // infinity + anything = anything + infinity = infinity
        nscoord_MAX
    } else {
        // a + b = a + b
        // Cap the result, just in case we're dealing with numbers near nscoord_MAX
        std::cmp::min(nscoord_MAX, a + b)
    }
}

/**
 * Returns a - b, gracefully handling cases involving nscoord_MAX.
 * This function assumes that neither argument is nscoord_MIN.
 *
 * The behavior is as follows:
 *
 *  a)  infinity - infinity -> infMinusInfResult
 *  b)  N - infinity        -> 0  (unexpected -- triggers NOTREACHED)
 *  c)  infinity - N        -> infinity
 *  d)  N1 - N2             -> N1 - N2
 */
// Gecko: gfx/src/nsCoord.h NSCoordSaturatingSubtract
pub fn NSCoordSaturatingSubtract(a: nscoord, b: nscoord, infMinusInfResult: nscoord) -> nscoord {
    if b == nscoord_MAX {
        if a == nscoord_MAX {
            // case (a)
            infMinusInfResult
        } else {
            // case (b)
            0
        }
    } else {
        if a == nscoord_MAX {
            // case (c) for integers
            nscoord_MAX
        } else {
            // case (d) for integers
            // Cap the result, in case we're dealing with numbers near nscoord_MAX
            std::cmp::min(nscoord_MAX, a - b)
        }
    }
}

// Gecko: gfx/src/nsCoord.h NSCoordToFloat
pub fn NSCoordToFloat(aCoord: nscoord) -> f32 {
    aCoord as f32
}

/*
 * Coord Rounding Functions
 */
// Gecko: gfx/src/nsCoord.h NSToCoordFloor
pub fn NSToCoordFloor__float(aValue: f32) -> nscoord {
    aValue.floor() as nscoord
}

// Gecko: gfx/src/nsCoord.h NSToCoordFloor
pub fn NSToCoordFloor__double(aValue: f64) -> nscoord {
    aValue.floor() as nscoord
}

// Gecko: gfx/src/nsCoord.h NSToCoordFloorClamped
pub fn NSToCoordFloorClamped(aValue: f32) -> nscoord {
    // Bounds-check before converting out of float, to avoid overflow
    if aValue >= nscoord_MAX as f32 {
        return nscoord_MAX;
    }
    if aValue <= nscoord_MIN as f32 {
        return nscoord_MIN;
    }
    NSToCoordFloor__float(aValue)
}

// Gecko: gfx/src/nsCoord.h NSToCoordCeil
pub fn NSToCoordCeil__float(aValue: f32) -> nscoord {
    aValue.ceil() as nscoord
}

// Gecko: gfx/src/nsCoord.h NSToCoordCeil
pub fn NSToCoordCeil__double(aValue: f64) -> nscoord {
    aValue.ceil() as nscoord
}

// Gecko: gfx/src/nsCoord.h NSToCoordCeilClamped
pub fn NSToCoordCeilClamped(aValue: f64) -> nscoord {
    // Bounds-check before converting out of double, to avoid overflow
    if aValue >= nscoord_MAX as f64 {
        return nscoord_MAX;
    }
    if aValue <= nscoord_MIN as f64 {
        return nscoord_MIN;
    }
    NSToCoordCeil__double(aValue)
}

// The NSToCoordTrunc* functions remove the fractional component of
// aValue, and are thus equivalent to NSToCoordFloor* for positive
// values and NSToCoordCeil* for negative values.

// Gecko: gfx/src/nsCoord.h NSToCoordTrunc
pub fn NSToCoordTrunc__float(aValue: f32) -> nscoord {
    // There's no need to use truncf() since it matches the default
    // rules for float to integer conversion.
    aValue as nscoord
}

// Gecko: gfx/src/nsCoord.h NSToCoordTrunc
pub fn NSToCoordTrunc__double(aValue: f64) -> nscoord {
    // There's no need to use trunc() since it matches the default
    // rules for float to integer conversion.
    aValue as nscoord
}

// Gecko: gfx/src/nsCoord.h NSToCoordTruncClamped
pub fn NSToCoordTruncClamped__float(aValue: f32) -> nscoord {
    // Bounds-check before converting out of float, to avoid overflow
    if aValue >= nscoord_MAX as f32 {
        return nscoord_MAX;
    }
    if aValue <= nscoord_MIN as f32 {
        return nscoord_MIN;
    }
    NSToCoordTrunc__float(aValue)
}

// Gecko: gfx/src/nsCoord.h NSToCoordTruncClamped
pub fn NSToCoordTruncClamped__double(aValue: f64) -> nscoord {
    // Bounds-check before converting out of double, to avoid overflow
    // (the C++ compares against float(nscoord_MAX) here, kept as is)
    if aValue >= (nscoord_MAX as f32) as f64 {
        return nscoord_MAX;
    }
    if aValue <= (nscoord_MIN as f32) as f64 {
        return nscoord_MIN;
    }
    NSToCoordTrunc__double(aValue)
}

/*
 * Int Rounding Functions
 */
// Gecko: gfx/src/nsCoord.h NSToIntFloor
pub fn NSToIntFloor(aValue: f32) -> i32 {
    aValue.floor() as i32
}

// Gecko: gfx/src/nsCoord.h NSToIntCeil
pub fn NSToIntCeil(aValue: f32) -> i32 {
    aValue.ceil() as i32
}

// Gecko: gfx/src/nsCoord.h NSToIntRound
pub fn NSToIntRound__float(aValue: f32) -> i32 {
    NS_lroundf(aValue)
}

// Gecko: gfx/src/nsCoord.h NSToIntRound
pub fn NSToIntRound__double(aValue: f64) -> i32 {
    NS_lround(aValue)
}

// Gecko: gfx/src/nsCoord.h NSToIntRoundUp
pub fn NSToIntRoundUp(aValue: f64) -> i32 {
    (aValue + 0.5).floor() as i32
}

/*
 * App Unit/Pixel conversions
 */
// Gecko: gfx/src/nsCoord.h NSFloatPixelsToAppUnits
pub fn NSFloatPixelsToAppUnits(aPixels: f32, aAppUnitsPerPixel: f32) -> nscoord {
    NSToCoordRoundWithClamp__float(aPixels * aAppUnitsPerPixel)
}

// Gecko: gfx/src/nsCoord.h NSDoublePixelsToAppUnits
pub fn NSDoublePixelsToAppUnits(aPixels: f64, aAppUnitsPerPixel: f64) -> nscoord {
    NSToCoordRoundWithClamp__double(aPixels * aAppUnitsPerPixel)
}

// Gecko: gfx/src/nsCoord.h NSIntPixelsToAppUnits
pub fn NSIntPixelsToAppUnits(aPixels: i32, aAppUnitsPerPixel: i32) -> nscoord {
    // The cast to nscoord makes sure we don't overflow if we ever change
    // nscoord to float
    let r: nscoord = aPixels * (aAppUnitsPerPixel as nscoord);
    r
}

// Gecko: gfx/src/nsCoord.h NSAppUnitsToFloatPixels
pub fn NSAppUnitsToFloatPixels(aAppUnits: nscoord, aAppUnitsPerPixel: f32) -> f32 {
    (aAppUnits as f32) / aAppUnitsPerPixel
}

// Gecko: gfx/src/nsCoord.h NSAppUnitsToDoublePixels
pub fn NSAppUnitsToDoublePixels(aAppUnits: nscoord, aAppUnitsPerPixel: f64) -> f64 {
    (aAppUnits as f64) / aAppUnitsPerPixel
}

// Gecko: gfx/src/nsCoord.h NSAppUnitsToIntPixels
pub fn NSAppUnitsToIntPixels(aAppUnits: nscoord, aAppUnitsPerPixel: f32) -> i32 {
    NSToIntRound__float((aAppUnits as f32) / aAppUnitsPerPixel)
}

// Gecko: gfx/src/nsCoord.h NSCoordScale
pub fn NSCoordScale(aCoord: nscoord, aFromAPP: i32, aToAPP: i32) -> f32 {
    (NSCoordToFloat(aCoord) * aToAPP as f32) / aFromAPP as f32
}

/// handy constants
// Gecko: gfx/src/nsCoord.h TWIPS_PER_POINT_INT (#define)
pub const TWIPS_PER_POINT_INT: i32 = 20;
// Gecko: gfx/src/nsCoord.h TWIPS_PER_POINT_FLOAT (#define)
pub const TWIPS_PER_POINT_FLOAT: f32 = 20.0f32;
// Gecko: gfx/src/nsCoord.h POINTS_PER_INCH_INT (#define)
pub const POINTS_PER_INCH_INT: i32 = 72;
// Gecko: gfx/src/nsCoord.h POINTS_PER_INCH_FLOAT (#define)
pub const POINTS_PER_INCH_FLOAT: f32 = 72.0f32;
// Gecko: gfx/src/nsCoord.h CM_PER_INCH_FLOAT (#define)
pub const CM_PER_INCH_FLOAT: f32 = 2.54f32;
// Gecko: gfx/src/nsCoord.h MM_PER_INCH_FLOAT (#define)
pub const MM_PER_INCH_FLOAT: f32 = 25.4f32;

/*
 * Twips/unit conversions
 */
// Gecko: gfx/src/nsCoord.h NSUnitsToTwips
pub fn NSUnitsToTwips(aValue: f32, aPointsPerUnit: f32) -> f32 {
    aValue * aPointsPerUnit * TWIPS_PER_POINT_FLOAT
}

// Gecko: gfx/src/nsCoord.h NSTwipsToUnits
pub fn NSTwipsToUnits(aTwips: f32, aUnitsPerPoint: f32) -> f32 {
    aTwips * (aUnitsPerPoint / TWIPS_PER_POINT_FLOAT)
}

/// Unit conversion macros
//@{
// The C++ function-like macros become functions with the same names.
// Gecko: gfx/src/nsCoord.h NS_POINTS_TO_TWIPS (#define)
pub fn NS_POINTS_TO_TWIPS(x: f32) -> f32 {
    NSUnitsToTwips(x, 1.0f32)
}
// Gecko: gfx/src/nsCoord.h NS_INCHES_TO_TWIPS (#define)
pub fn NS_INCHES_TO_TWIPS(x: f32) -> f32 {
    NSUnitsToTwips(x, POINTS_PER_INCH_FLOAT) // 72 points per inch
}

// Gecko: gfx/src/nsCoord.h NS_MILLIMETERS_TO_TWIPS (#define)
pub fn NS_MILLIMETERS_TO_TWIPS(x: f32) -> f32 {
    NSUnitsToTwips(x, POINTS_PER_INCH_FLOAT * 0.03937f32)
}

// Gecko: gfx/src/nsCoord.h NS_POINTS_TO_INT_TWIPS (#define)
pub fn NS_POINTS_TO_INT_TWIPS(x: f32) -> i32 {
    NSToIntRound__float(NS_POINTS_TO_TWIPS(x))
}
// Gecko: gfx/src/nsCoord.h NS_INCHES_TO_INT_TWIPS (#define)
pub fn NS_INCHES_TO_INT_TWIPS(x: f32) -> i32 {
    NSToIntRound__float(NS_INCHES_TO_TWIPS(x))
}

// Gecko: gfx/src/nsCoord.h NS_TWIPS_TO_INCHES (#define)
pub fn NS_TWIPS_TO_INCHES(x: f32) -> f32 {
    NSTwipsToUnits(x, 1.0f32 / POINTS_PER_INCH_FLOAT)
}

// Gecko: gfx/src/nsCoord.h NS_TWIPS_TO_MILLIMETERS (#define)
pub fn NS_TWIPS_TO_MILLIMETERS(x: f32) -> f32 {
    NSTwipsToUnits(x, 1.0f32 / (POINTS_PER_INCH_FLOAT * 0.03937f32))
}
//@}

// TestCoord.cpp (gfx/tests/gtest) only tests AuCoord / AuCoord64, which are NOT PORTED above, so it is
// skipped. The tests below are written from the documented behaviour of the helpers (not a gtest translation).
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gfx_rounding() {
        // Round is floor(x + 0.5), not round-to-nearest for negatives.
        assert_eq!(NSToCoordRound__float(5.5), 6);
        assert_eq!(NSToCoordRound__float(-2.5), -2);
        assert_eq!(NSToCoordRound__double(-2.5), -2);
        assert_eq!(NSToCoordFloor__float(-2.7), -3);
        assert_eq!(NSToCoordCeil__double(-2.7), -2);
        assert_eq!(NSToCoordTrunc__float(-2.7), -2);
        assert_eq!(NSToIntRound__float(-2.5), -3);
        assert_eq!(NSToIntRound__double(2.5), 3);
        assert_eq!(NSToIntRoundUp(-2.5), -2);
    }

    #[test]
    fn gfx_clamping() {
        assert_eq!(NSToCoordRoundWithClamp__float(1e20), nscoord_MAX);
        assert_eq!(NSToCoordRoundWithClamp__double(-1e20), nscoord_MIN);
        assert_eq!(NSToCoordFloorClamped(1e20), nscoord_MAX);
        assert_eq!(NSToCoordCeilClamped(-1e20), nscoord_MIN);
        assert_eq!(NSToCoordTruncClamped__float(7.9), 7);
    }

    #[test]
    fn gfx_saturating() {
        assert_eq!(NSCoordSaturatingAdd(nscoord_MAX, 5), nscoord_MAX);
        assert_eq!(NSCoordSaturatingAdd(3, 5), 8);
        assert_eq!(NSCoordSaturatingSubtract(nscoord_MAX, nscoord_MAX, 7), 7);
        assert_eq!(NSCoordSaturatingSubtract(5, nscoord_MAX, 7), 0);
        assert_eq!(NSCoordSaturatingSubtract(nscoord_MAX, 5, 7), nscoord_MAX);
        assert_eq!(NSCoordSaturatingSubtract(9, 5, 7), 4);
        assert_eq!(NSCoordSaturatingMultiply(nscoord_MAX, 4.0), nscoord_MAX);
        assert_eq!(NSCoordSaturatingMultiply(nscoord_MAX, -4.0), nscoord_MIN);
        assert_eq!(NSCoordSaturatingNonnegativeMultiply(10, 0.5), 5);
    }

    #[test]
    fn gfx_div_rem_and_mul_div() {
        let mut q = 0;
        assert_eq!(NSCoordDivRem(17, 5, &mut q), 2);
        assert_eq!(q, 3);
        assert_eq!(NSCoordMulDiv(1 << 30, 4, 8), 1 << 29);
        assert_eq!(NSCoordScale(60, 60, 30), 30.0);
        assert_eq!(NSIntPixelsToAppUnits(3, 60), 180);
        assert_eq!(NSAppUnitsToIntPixels(90, 60.0), 2);
    }
}
