/* Numeric helpers */
// Translated from gfx/2d/NumericTools.h (GECKO_REV).
use crate::*;

// Gecko: gfx/2d/NumericTools.h mozilla::gfx::RoundDownToMultiple
// Same as int32_t(floor(double(x) / aMultiplier)) * aMultiplier,
// but faster.
pub fn RoundDownToMultiple(x: i32, aMultiplier: i32) -> i32 {
    // We don't use float division + floor because that's hard for the compiler
    // to optimize.
    let r#mod = x % aMultiplier;
    if x > 0 {
        return x - r#mod;
    }
    if r#mod != 0 { x - aMultiplier - r#mod } else { x }
}

// Gecko: gfx/2d/NumericTools.h mozilla::gfx::RoundUpToMultiple
// Returns the smallest multiple of aMultiplied that's >= x.
// Same as int32_t(ceil(double(x) / aMultiplier)) * aMultiplier,
// but faster.
pub fn RoundUpToMultiple(x: i32, aMultiplier: i32) -> i32 {
    let r#mod = x % aMultiplier;
    if x > 0 {
        return if r#mod != 0 { x + aMultiplier - r#mod } else { x };
    }
    x - r#mod
}

// Gecko: gfx/2d/NumericTools.h mozilla::gfx::RoundToMultiple
pub fn RoundToMultiple(x: i32, aMultiplier: i32) -> i32 {
    RoundDownToMultiple(x + aMultiplier / 2, aMultiplier)
}

#[cfg(test)]
mod tests {
    use super::*;

    // written from the documented behaviour (the comments above); Gecko has no gtest for these
    #[test]
    fn rounding_to_a_multiple() {
        assert_eq!(RoundDownToMultiple(130, 60), 120);
        assert_eq!(RoundDownToMultiple(-1, 60), -60);
        assert_eq!(RoundDownToMultiple(-60, 60), -60);
        assert_eq!(RoundUpToMultiple(121, 60), 180);
        assert_eq!(RoundUpToMultiple(120, 60), 120);
        assert_eq!(RoundToMultiple(89, 60), 60);
        assert_eq!(RoundToMultiple(90, 60), 120);
    }
}
