// Translated from gfx/src/nsBoundingMetrics.h (GECKO_REV).
use crate::*;

// NOT PORTED: includes (<algorithm>).

/* Struct used for accurate measurements of a string, in order to
 * allow precise positioning when processing MathML.  This is in its
 * own header file because some very-widely-included headers need it
 * but not the rest of nsFontMetrics, or vice versa.
 */

// Gecko: gfx/src/nsBoundingMetrics.h nsBoundingMetrics
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct nsBoundingMetrics {
    ///////////
    // Metrics that _exactly_ enclose the text:

    // The character coordinate system is the one used on X Windows:
    // 1. The origin is located at the intersection of the baseline
    //    with the left of the character's cell.
    // 2. All horizontal bearings are oriented from left to right.
    // 2. All horizontal bearings are oriented from left to right.
    // 3. The ascent is oriented from bottom to top (being 0 at the orgin).
    // 4. The descent is oriented from top to bottom (being 0 at the origin).

    // Note that Win32/Mac/PostScript use a different convention for
    // the descent (all vertical measurements are oriented from bottom
    // to top on these palatforms). Make sure to flip the sign of the
    // descent on these platforms for cross-platform compatibility.

    // Any of the following member variables listed here can have
    // positive or negative value.
    pub leftBearing: nscoord,
    /* The horizontal distance from the origin of the drawing
    operation to the left-most part of the drawn string. */
    pub rightBearing: nscoord,
    /* The horizontal distance from the origin of the drawing
    operation to the right-most part of the drawn string.
    The _exact_ width of the string is therefore:
    rightBearing - leftBearing */
    pub ascent: nscoord,
    /* The vertical distance from the origin of the drawing
    operation to the top-most part of the drawn string. */
    pub descent: nscoord,
    /* The vertical distance from the origin of the drawing
    operation to the bottom-most part of the drawn string.
    The _exact_ height of the string is therefore:
    ascent + descent */
    pub width: nscoord,
    /* The horizontal distance from the origin of the drawing
    operation to the correct origin for drawing another string
    to follow the current one. Depending on the font, this
    could be greater than or less than the right bearing. */
}

impl nsBoundingMetrics {
    // nsBoundingMetrics() : leftBearing(0), rightBearing(0), ascent(0), descent(0), width(0) {}
    // -> Default::default() (all members are 0), and new().
    pub fn new() -> nsBoundingMetrics {
        nsBoundingMetrics { leftBearing: 0, rightBearing: 0, ascent: 0, descent: 0, width: 0 }
    }
}

impl Default for nsBoundingMetrics {
    fn default() -> nsBoundingMetrics {
        nsBoundingMetrics::new()
    }
}

// void operator+=(const nsBoundingMetrics& bm)
// Gecko: gfx/src/nsBoundingMetrics.h nsBoundingMetrics::operator+=
impl std::ops::AddAssign for nsBoundingMetrics {
    fn add_assign(&mut self, bm: nsBoundingMetrics) {
        if self.ascent + self.descent == 0 && self.rightBearing - self.leftBearing == 0 {
            self.ascent = bm.ascent;
            self.descent = bm.descent;
            self.leftBearing = self.width + bm.leftBearing;
            self.rightBearing = self.width + bm.rightBearing;
        } else {
            if self.ascent < bm.ascent {
                self.ascent = bm.ascent;
            }
            if self.descent < bm.descent {
                self.descent = bm.descent;
            }
            self.leftBearing = std::cmp::min(self.leftBearing, self.width + bm.leftBearing);
            self.rightBearing = std::cmp::max(self.rightBearing, self.width + bm.rightBearing);
        }
        self.width += bm.width;
    }
}

// Tests: there is no gtest for nsBoundingMetrics; written from the behaviour of operator+= above.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn DefaultIsZero() {
        let m = nsBoundingMetrics::default();
        assert_eq!((m.leftBearing, m.rightBearing, m.ascent, m.descent, m.width), (0, 0, 0, 0, 0));
    }

    #[test]
    fn AddAssignToEmptyTakesOtherAtWidth() {
        let mut a = nsBoundingMetrics::default();
        a.width = 10;
        let b = nsBoundingMetrics { leftBearing: 1, rightBearing: 5, ascent: 7, descent: 2, width: 6 };
        a += b;
        assert_eq!((a.leftBearing, a.rightBearing, a.ascent, a.descent, a.width), (11, 15, 7, 2, 16));
    }

    #[test]
    fn AddAssignToNonEmptyWidens() {
        let mut a = nsBoundingMetrics { leftBearing: 2, rightBearing: 8, ascent: 5, descent: 1, width: 10 };
        let b = nsBoundingMetrics { leftBearing: -4, rightBearing: 12, ascent: 3, descent: 4, width: 6 };
        a += b;
        assert_eq!((a.leftBearing, a.rightBearing, a.ascent, a.descent, a.width), (2, 22, 5, 4, 16));
    }
}
