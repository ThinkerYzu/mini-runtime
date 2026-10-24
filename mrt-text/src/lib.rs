//! Fonts and text shaping (text phase 1a). Firefox's HarfBuzz is vendored and compiled by build.rs; the
//! types here follow the Gecko classes they stand in for (gfxFontEntry, gfxFont, nsFontMetrics) and
//! name their Gecko source in a `// Gecko:` line, as in ../mrt-layout/CONVENTIONS.md.
#![allow(non_snake_case, non_camel_case_types, non_upper_case_globals, dead_code)]

#[path = "harfbuzz.rs"]
pub mod harfbuzz;
#[path = "gfx/thebes/gfxFcPlatformFontList.rs"]
mod gfxFcPlatformFontList_rs;
#[path = "gfx/thebes/gfxFont.rs"]
mod gfxFont_rs;
#[cfg(target_os = "android")]
#[path = "gfx/thebes/AndroidFontTable.rs"]
mod AndroidFontTable_rs;
#[path = "gfx/src/nsFontMetrics.rs"]
mod nsFontMetrics_rs;

pub use gfxFcPlatformFontList_rs::*;
pub use gfxFont_rs::*;
pub use nsFontMetrics_rs::*;

/// Same type and unit as nscoord in mrt-layout (`gfx/src/nsCoord.h`): app units, 60 per CSS px.
pub type nscoord = i32;
pub const AppUnitsPerCSSPixel: i32 = 60;
