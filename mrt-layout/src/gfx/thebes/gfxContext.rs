// ADAPTER (not a translation of gfx/thebes/gfxContext.h): the rendering context that Gecko passes
// to reflow code for text measurement. Layout in this crate measures text through the font stage
// (not translated yet), so the context carries nothing; it exists so that the signatures that take
// a `gfxContext*` keep their shape.
use crate::*;

#[derive(Clone, Copy, Debug, Default)]
pub struct gfxContext;
