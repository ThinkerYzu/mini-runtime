// Gecko: gfx/src/nsMargin.h
use crate::*;

// struct nsMargin : public mozilla::gfx::BaseMargin<nscoord, nsMargin>
// Gecko: gfx/src/nsMargin.h nsMargin
BaseMargin!(nsMargin, nscoord);

impl nsMargin {
    // typedef mozilla::gfx::BaseMargin<nscoord, nsMargin> Super;

    // Constructors
    // nsMargin() = default;                           -> Default::default()
    // nsMargin(const nsMargin& aMargin) = default;    -> Copy
    // nsMargin(nscoord aTop, nscoord aRight, nscoord aBottom, nscoord aLeft)   -> new(), from BaseMargin!
}

// NOT PORTED: typedef mozilla::gfx::IntMargin nsIntMargin;  (IntMargin is NOT PORTED, see Rect.rs)
