/* Helper structs or classes used throughout the Layout module */
// Translated from layout/base/LayoutStructs.h (GECKO_REV).
use crate::*;

/**
 * A set of StyleSizes used as an input parameter to various functions that
 * compute sizes like nsIFrame::ComputeSize(). If any of the member fields has a
 * value, the function may use the value instead of retrieving it from the
 * frame's style.
 *
 * The logical sizes are assumed to be in the associated frame's writing-mode.
 */
// Gecko: layout/base/LayoutStructs.h mozilla::StyleSizeOverrides
#[derive(Clone, Default)]
pub struct StyleSizeOverrides {
    pub mStyleISize: Option<StyleSize>,
    pub mStyleBSize: Option<StyleSize>,
    pub mAspectRatio: Option<AspectRatio>,

    // By default, table wrapper frame considers the size overrides applied to
    // itself, so it creates any length size overrides for inner table frame by
    // subtracting the area occupied by the caption and border & padding according
    // to box-sizing.
    //
    // When this flag is true, table wrapper frame is required to apply the size
    // overrides to the inner table frame directly, without any modification,
    // which is useful for flex container to override the inner table frame's
    // preferred main size with 'flex-basis'.
    //
    // Note: if mStyleISize is a LengthPercentage, the inner table frame will
    // comply with the inline-size override without enforcing its min-content
    // inline-size in nsTableFrame::ComputeSize(). This is necessary so that small
    // flex-basis values like 'flex-basis:1%' can be resolved correctly; the
    // flexbox layout algorithm does still explicitly clamp to min-sizes *at a
    // later step*, after the flex-basis has been resolved -- so this flag won't
    // actually produce any user-visible tables whose final inline size is smaller
    // than their min-content inline size.
    pub mApplyOverridesVerbatim: bool,
}

impl StyleSizeOverrides {
    // Gecko: layout/base/LayoutStructs.h StyleSizeOverrides::HasAnyOverrides
    pub fn HasAnyOverrides(&self) -> bool {
        self.mStyleISize.is_some() || self.mStyleBSize.is_some()
    }
    // Gecko: layout/base/LayoutStructs.h StyleSizeOverrides::HasAnyLengthOverrides
    pub fn HasAnyLengthOverrides(&self) -> bool {
        self.mStyleISize.as_ref().map_or(false, |s| s.ConvertsToLength()) || self.mStyleBSize.as_ref().map_or(false, |s| s.ConvertsToLength())
    }
}

// Defines container types that force fragmentation boundaries.
// This supports CSS fragmentation behavior such as 'break-before: page'.
// Gecko: layout/base/LayoutStructs.h mozilla::BreakType
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum BreakType {
    #[default]
    Auto,
    Column,
    Page,
}
