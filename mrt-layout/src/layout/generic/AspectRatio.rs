/* The aspect ratio of a box, in a "width / height" format. */
// Translated from layout/generic/AspectRatio.h and AspectRatio.cpp (GECKO_REV), plus
// AspectRatio::ConvertToWritingMode, which Gecko defines at the end of WritingModes.h (the
// WritingModes.rs translation kept it as a comment, "to be moved when AspectRatio is translated").
use crate::*;

// Gecko: layout/generic/AspectRatio.h mozilla::UseBoxSizing
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum UseBoxSizing {
    // The aspect ratio works with content box dimensions always.
    #[default]
    No,
    // The aspect ratio works with the dimensions of the box specified by
    // box-sizing.
    Yes,
}

// Gecko: layout/generic/AspectRatio.h mozilla::AspectRatio
// NOT PORTED: `friend struct IPC::ParamTraits<mozilla::AspectRatio>` (IPC).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct AspectRatio {
    // 0.0f represents no aspect ratio.
    mRatio: f32,
    mUseBoxSizing: UseBoxSizing,
}

impl Default for AspectRatio {
    // Gecko: layout/generic/AspectRatio.h AspectRatio::AspectRatio()
    fn default() -> AspectRatio {
        AspectRatio { mRatio: 0.0, mUseBoxSizing: UseBoxSizing::No }
    }
}

impl AspectRatio {
    // Gecko: layout/generic/AspectRatio.h AspectRatio::AspectRatio(float, UseBoxSizing)
    // (the C++ default for aUseBoxSizing is UseBoxSizing::No)
    pub fn new(aRatio: f32, aUseBoxSizing: UseBoxSizing) -> AspectRatio {
        AspectRatio { mRatio: if aRatio < 0.0 { 0.0 } else { aRatio }, mUseBoxSizing: aUseBoxSizing }
    }

    // Gecko: layout/generic/AspectRatio.h AspectRatio::FromSize(float, float, UseBoxSizing)
    pub fn FromSize__float_float_UseBoxSizing(aWidth: f32, aHeight: f32, aUseBoxSizing: UseBoxSizing) -> AspectRatio {
        if aWidth == 0.0 || aHeight == 0.0 {
            // For the degenerate ratio, we don't care about which box sizing we are
            // using, so using default constructor is fine.
            return AspectRatio::default();
        }
        let ratio = aWidth / aHeight;
        if !ratio.is_finite() {
            return AspectRatio::default();
        }
        AspectRatio::new(ratio, aUseBoxSizing)
    }

    // NOT PORTED: template <typename T, typename Sub, typename Coord>
    // static AspectRatio FromSize(const gfx::BaseSize<T, Sub, Coord>& aSize) { return FromSize(aSize.Width(), aSize.Height()); }
    // (callers use FromSize__float_float_UseBoxSizing directly)

    // Gecko: layout/generic/AspectRatio.h AspectRatio::operator bool
    pub fn operator_bool(&self) -> bool {
        self.mRatio != 0.0
    }

    // Gecko: layout/generic/AspectRatio.h AspectRatio::ApplyTo
    pub fn ApplyTo(&self, aCoord: nscoord) -> nscoord {
        MOZ_ASSERT!(self.operator_bool());
        NSCoordSaturatingNonnegativeMultiply(aCoord, self.mRatio)
    }

    // Gecko: layout/generic/AspectRatio.h AspectRatio::ApplyToFloat
    pub fn ApplyToFloat(&self, aFloat: f32) -> f32 {
        MOZ_ASSERT!(self.operator_bool());
        self.mRatio * aFloat
    }

    // Inverts the ratio, in order to get the height / width ratio.
    // Gecko: layout/generic/AspectRatio.h AspectRatio::Inverted
    pub fn Inverted(&self) -> AspectRatio {
        if !self.operator_bool() {
            return AspectRatio::default();
        }
        // Clamp to a small epsilon, in case mRatio is absurdly large & produces
        // 0.0f in the division here (so that valid ratios always generate other
        // valid ratios when inverted).
        let inverse = 1.0f32 / self.mRatio;
        AspectRatio::new(if f32::EPSILON < inverse { inverse } else { f32::EPSILON }, self.mUseBoxSizing)
    }

    // Gecko: layout/generic/WritingModes.h AspectRatio::ConvertToWritingMode
    pub fn ConvertToWritingMode(&self, aWM: WritingMode) -> AspectRatio {
        if aWM.IsVertical() { self.Inverted() } else { *self }
    }

    /**
     * This method computes the ratio-dependent size by the ratio-determining size
     * and aspect-ratio (i.e. preferred aspect ratio). Basically this function
     * will be used in the calculation of 'auto' sizes when the preferred
     * aspect ratio is not 'auto'.
     *
     * @param aRatioDependentAxis  The ratio depenedent axis of the box.
     * @param aWM  The writing mode of the box.
     * @param aRatioDetermingSize  The content-box size on the ratio determining
     *                             axis. Basically, we use this size and |mRatio|
     *                             to compute the size on the ratio-dependent
     *                             axis.
     * @param aContentBoxSizeToBoxSizingAdjust  The border padding box size
     *                                          adjustment. We need this because
     *                                          aspect-ratio should take the
     *                                          box-sizing into account if its
     *                                          style is '<ratio>'. If its style
     *                                          is 'auto & <ratio>', we should use
     *                                          content-box dimensions always.
     *                                          If the callers want the ratio to
     *                                          apply to the content-box size, we
     *                                          should pass a zero LogicalSize.
     *                                          If mUseBoxSizing is No, we ignore
     *                                          this parameter because we should
     *                                          use content box dimensions always.
     *
     * The return value is the content-box size on the ratio-dependent axis.
     * Plese see the definition of the ratio-dependent axis and the
     * ratio-determining axis in the spec:
     * https://drafts.csswg.org/css-sizing-4/#aspect-ratio
     */
    // Gecko: layout/generic/AspectRatio.cpp AspectRatio::ComputeRatioDependentSize
    pub fn ComputeRatioDependentSize(&self, aRatioDependentAxis: LogicalAxis, aWM: WritingMode, aRatioDeterminingSize: nscoord, aContentBoxSizeToBoxSizingAdjust: LogicalSize) -> nscoord {
        MOZ_ASSERT!(self.operator_bool(), "Infinite or zero ratio may have undefined behavior when computing the size");
        let boxSizingAdjust = if self.mUseBoxSizing == UseBoxSizing::No { LogicalSize::new__WritingMode(aWM) } else { aContentBoxSizeToBoxSizingAdjust };
        if aRatioDependentAxis == LogicalAxis::Inline {
            self.ConvertToWritingMode(aWM).ApplyTo(aRatioDeterminingSize + boxSizingAdjust.BSize(aWM)) - boxSizingAdjust.ISize(aWM)
        } else {
            self.ConvertToWritingMode(aWM).Inverted().ApplyTo(aRatioDeterminingSize + boxSizingAdjust.ISize(aWM)) - boxSizingAdjust.BSize(aWM)
        }
    }
}

// Gecko: layout/generic/AspectRatio.h AspectRatio::operator<
// (C++ asserts that both have the same mUseBoxSizing; same here.)
impl PartialOrd for AspectRatio {
    fn partial_cmp(&self, aOther: &AspectRatio) -> Option<std::cmp::Ordering> {
        MOZ_ASSERT!(self.mUseBoxSizing == aOther.mUseBoxSizing, "Do not compare AspectRatio if their mUseBoxSizing are different.");
        self.mRatio.partial_cmp(&aOther.mRatio)
    }
}
