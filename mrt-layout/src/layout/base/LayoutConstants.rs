/* constants used throughout the Layout module */
// Translated from layout/base/LayoutConstants.h (GECKO_REV).
use crate::*;

/**
 * Constant used to indicate an unconstrained size.
 *
 * NOTE: The constants defined in this file are semantically used as symbolic
 *       values, so user should not depend on the underlying numeric values. If
 *       new specific use cases arise, define a new constant here.
 */
// Gecko: layout/base/LayoutConstants.h NS_UNCONSTRAINEDSIZE
pub const NS_UNCONSTRAINEDSIZE: nscoord = nscoord_MAX;

// NS_AUTOOFFSET is assumed to have the same value as NS_UNCONSTRAINEDSIZE.
// Gecko: layout/base/LayoutConstants.h NS_AUTOOFFSET
pub const NS_AUTOOFFSET: nscoord = NS_UNCONSTRAINEDSIZE;

// +1 is to avoid clamped huge margin values being processed as auto margins
// Gecko: layout/base/LayoutConstants.h NS_AUTOMARGIN
pub const NS_AUTOMARGIN: nscoord = NS_UNCONSTRAINEDSIZE + 1;

// Gecko: layout/base/LayoutConstants.h NS_INTRINSIC_ISIZE_UNKNOWN
pub const NS_INTRINSIC_ISIZE_UNKNOWN: nscoord = nscoord_MIN;

/**
 * Bit-flags to pass to various functions that compute sizes like
 * nsIFrame::ComputeSize().
 */
// Gecko: layout/base/LayoutConstants.h mozilla::ComputeSizeFlag
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum ComputeSizeFlag {
    /**
     * Set if the frame is in a context where non-replaced blocks should
     * shrink-wrap (e.g., it's floating, absolutely positioned, or
     * inline-block).
     */
    ShrinkWrap,

    /**
     * Set if this is a grid measuring reflow, to prevent stretching.
     */
    IsGridMeasuringReflow,

    /**
     * Indicates that we should clamp the margin-box min-size to the given CB
     * size.  This is used for implementing the grid area clamping here:
     * https://drafts.csswg.org/css-grid/#min-size-auto
     */
    IClampMarginBoxMinSize, // clamp in our inline axis
    BClampMarginBoxMinSize, // clamp in our block axis

    /**
     * The frame is stretching (per CSS Box Alignment) and doesn't have an
     * Automatic Minimum Size in the indicated axis.
     * (may be used for both flex/grid items, but currently only used for Grid)
     * https://drafts.csswg.org/css-grid/#min-size-auto
     * https://drafts.csswg.org/css-align-3/#valdef-justify-self-stretch
     */
    IApplyAutoMinSize, // Only has an effect when the ShrinkWrap bit is unset.
}
impl EnumSetValue for ComputeSizeFlag {
    fn BitFor(self) -> u32 {
        1 << (self as u32)
    }
}
// Gecko: layout/base/LayoutConstants.h mozilla::ComputeSizeFlags
pub type ComputeSizeFlags = EnumSet<ComputeSizeFlag>;

// NOT PORTED: kFallbackIntrinsicWidthInPixels, kFallbackIntrinsicHeightInPixels,
// kFallbackIntrinsicSizeInPixels, kFallbackIntrinsicWidth, kFallbackIntrinsicHeight,
// kFallbackIntrinsicSize: only replaced elements (images, canvas) use them and replaced
// elements are not translated yet (CSSIntCoord / CSSIntSize are not in this crate either).

/**
 * This is used in some nsLayoutUtils functions.
 * Declared here so that fewer files need to include nsLayoutUtils.h.
 */
// Gecko: layout/base/LayoutConstants.h mozilla::IntrinsicISizeType
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IntrinsicISizeType {
    MinISize,
    PrefISize,
}

// NOT PORTED: enum class ContentRelevancyReason / ContentRelevancy (content-visibility: auto
// relevance, not part of this runtime).
