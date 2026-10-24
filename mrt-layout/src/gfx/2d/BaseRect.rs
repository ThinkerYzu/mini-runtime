// Gecko: gfx/2d/BaseRect.h
use crate::*;

// NOT PORTED: mozilla/gfx/ScaleFactors2D.h (BaseScaleFactors2D), used only by the Scale(BaseScaleFactors2D)
// overload below.

/**
 * Rectangles have two interpretations: a set of (zero-size) points,
 * and a rectangular area of the plane. Most rectangle operations behave
 * the same no matter what interpretation is being used, but some operations
 * differ:
 * -- Equality tests behave differently. When a rectangle represents an area,
 * all zero-width and zero-height rectangles are equal to each other since they
 * represent the empty area. But when a rectangle represents a set of
 * mathematical points, zero-width and zero-height rectangles can be unequal.
 * -- The union operation can behave differently. When rectangles represent
 * areas, taking the union of a zero-width or zero-height rectangle with
 * another rectangle can just ignore the empty rectangle. But when rectangles
 * represent sets of mathematical points, we may need to extend the latter
 * rectangle to include the points of a zero-width or zero-height rectangle.
 *
 * To ensure that these interpretations are explicitly disambiguated, we
 * deny access to the == and != operators and require use of IsEqualEdges and
 * IsEqualInterior instead. Similarly we provide separate Union and UnionEdges
 * methods.
 *
 * Do not use this class directly. Subclass it, pass that subclass as the
 * Sub parameter, and only use that subclass.
 */
// BaseRect<T, Sub, Point, SizeT, MarginT> as a macro: `BaseRect!(Sub, T, Point, SizeT, MarginT)` defines the
// struct `Sub` (with the base class's members x, y, width, height) and the base class's methods on it.
//
// Only the nscoord instantiation exists (BaseRect!(nsRect, nscoord, nsPoint, nsSize, nsMargin)), so the
// overload suffixes below spell the instantiated type names (`Contains__nsPoint`, `Inflate__nsMargin`,
// `SetRect__nsPoint_nsSize`), not the template parameter names.
//
// Methods that the subclass hides in C++ (nsRect::Intersect, IntersectRect, Union, UnionEdges, UnionRect) and
// that the subclass still reaches as `Super::X` are emitted here as `Super__X`; the subclass file defines
// the plain names, exactly like the C++ subclass does. The private deleted operator== / operator!= are not
// defined (the subclass defines PartialEq).
// Gecko: gfx/2d/BaseRect.h BaseRect
#[macro_export]
macro_rules! BaseRect {
    ($Sub:ident, $T:ty, $Point:ident, $SizeT:ident, $MarginT:ident) => {
        #[derive(Clone, Copy, Debug, Default)]
        pub struct $Sub {
            pub x: $T,
            pub y: $T,
            pub width: $T,
            pub height: $T,
        }

        impl $Sub {
            // Constructors
            // BaseRect() : x(0), y(0), width(0), height(0) {}   -> Default::default()
            // BaseRect(const Point& aOrigin, const SizeT& aSize)
            // Gecko: gfx/2d/BaseRect.h BaseRect::BaseRect
            pub fn new__nsPoint_nsSize(aOrigin: $Point, aSize: $SizeT) -> $Sub {
                $Sub { x: aOrigin.x, y: aOrigin.y, width: aSize.width, height: aSize.height }
            }
            // BaseRect(T aX, T aY, T aWidth, T aHeight)
            // Gecko: gfx/2d/BaseRect.h BaseRect::BaseRect
            pub fn new(aX: $T, aY: $T, aWidth: $T, aHeight: $T) -> $Sub {
                $Sub { x: aX, y: aY, width: aWidth, height: aHeight }
            }

            // Emptiness. An empty rect is one that has no area, i.e. its height or width
            // is <= 0.  Zero rect is the one with height and width set to zero.  Note
            // that SetEmpty() may change a rectangle that identified as IsEmpty().
            // Gecko: gfx/2d/BaseRect.h BaseRect::IsZeroArea
            #[inline(always)]
            pub fn IsZeroArea(&self) -> bool {
                self.height == 0 || self.width == 0
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::IsEmpty
            #[inline(always)]
            pub fn IsEmpty(&self) -> bool {
                self.height <= 0 || self.width <= 0
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::SetEmpty
            pub fn SetEmpty(&mut self) {
                self.width = 0;
                self.height = 0;
            }

            // "Finite" means not inf and not NaN
            // (an integer is always finite)
            // Gecko: gfx/2d/BaseRect.h BaseRect::IsFinite
            pub fn IsFinite(&self) -> bool {
                true
            }

            // Returns true if this rectangle contains the interior of aRect. Always
            // returns true if aRect is empty, and always returns false is aRect is
            // nonempty but this rect is empty.
            // Gecko: gfx/2d/BaseRect.h BaseRect::Contains
            pub fn Contains__nsRect(&self, aRect: $Sub) -> bool {
                aRect.IsEmpty()
                    || (self.x <= aRect.x
                        && aRect.XMost() <= self.XMost()
                        && self.y <= aRect.y
                        && aRect.YMost() <= self.YMost())
            }
            // Returns true if this rectangle contains the point. Points are considered
            // in the rectangle if they are on the left or top edge, but outside if they
            // are on the right or bottom edge.
            // Gecko: gfx/2d/BaseRect.h BaseRect::Contains
            #[inline(always)]
            pub fn Contains__nscoord_nscoord(&self, aX: $T, aY: $T) -> bool {
                self.x <= aX && aX < self.XMost() && self.y <= aY && aY < self.YMost()
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::ContainsX
            #[inline(always)]
            pub fn ContainsX(&self, aX: $T) -> bool {
                self.x <= aX && aX < self.XMost()
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::ContainsY
            #[inline(always)]
            pub fn ContainsY(&self, aY: $T) -> bool {
                self.y <= aY && aY < self.YMost()
            }
            // Returns true if this rectangle contains the point. Points are considered
            // in the rectangle if they are on the left or top edge, but outside if they
            // are on the right or bottom edge.
            // Gecko: gfx/2d/BaseRect.h BaseRect::Contains
            pub fn Contains__nsPoint(&self, aPoint: $Point) -> bool {
                self.Contains__nscoord_nscoord(aPoint.x, aPoint.y)
            }

            // Returns true if this rectangle contains the point, considering points on
            // all edges of the rectangle to be contained (as compared to Contains()
            // which only includes points on the top & left but not bottom & right edges).
            // Gecko: gfx/2d/BaseRect.h BaseRect::ContainsInclusively
            #[inline(always)]
            pub fn ContainsInclusively(&self, aPoint: $Point) -> bool {
                self.x <= aPoint.x && aPoint.x <= self.XMost() && self.y <= aPoint.y && aPoint.y <= self.YMost()
            }

            // Intersection. Returns TRUE if the receiver's area has non-empty
            // intersection with aRect's area, and FALSE otherwise.
            // Always returns false if aRect is empty or 'this' is empty.
            // Gecko: gfx/2d/BaseRect.h BaseRect::Intersects
            pub fn Intersects(&self, aRect: $Sub) -> bool {
                !self.IsEmpty()
                    && !aRect.IsEmpty()
                    && self.x < aRect.XMost()
                    && aRect.x < self.XMost()
                    && self.y < aRect.YMost()
                    && aRect.y < self.YMost()
            }
            // Returns the rectangle containing the intersection of the points
            // (including edges) of *this and aRect. If there are no points in that
            // intersection, returns an empty rectangle with x/y set to the std::max of
            // the x/y of *this and aRect.
            //
            // Intersection with an empty Rect may not produce an empty Rect if overflow
            // occurs. e.g. {INT_MIN, 0, 0, 20} Intersect { 5000, 0, 500, 20 } gives:
            // the non-emtpy {5000, 0, 500, 20 } instead of {5000, 0, 0, 0}
            // (hidden by nsRect::Intersect, see the comment above the macro)
            // Gecko: gfx/2d/BaseRect.h BaseRect::Intersect
            #[must_use]
            pub fn Super__Intersect(&self, aRect: $Sub) -> $Sub {
                let mut result = $Sub::default();
                result.x = std::cmp::max::<$T>(self.x, aRect.x);
                result.y = std::cmp::max::<$T>(self.y, aRect.y);
                result.width = std::cmp::min::<$T>(
                    self.x - result.x + self.width,
                    aRect.x - result.x + aRect.width,
                );
                result.height = std::cmp::min::<$T>(
                    self.y - result.y + self.height,
                    aRect.y - result.y + aRect.height,
                );
                // See bug 1457110, this function expects to -only- size to 0,0 if the
                // width/height is explicitly negative.
                if result.width < 0 || result.height < 0 {
                    result.SizeTo__nscoord_nscoord(0, 0);
                }
                result
            }

            // Gives the same results as Intersect() but handles integer overflow
            // better. This comes at a tiny cost in performance.
            // e.g. {INT_MIN, 0, 0, 20} Intersect { 5000, 0, 500, 20 } gives:
            // {5000, 0, 0, 0}
            // Gecko: gfx/2d/BaseRect.h BaseRect::SafeIntersect
            #[must_use]
            pub fn SafeIntersect(&self, aRect: $Sub) -> $Sub {
                let mut result = $Sub::default();
                result.x = std::cmp::max::<$T>(self.x, aRect.x);
                result.y = std::cmp::max::<$T>(self.y, aRect.y);
                let right: $T = std::cmp::min::<$T>(self.x + self.width, aRect.x + aRect.width);
                let bottom: $T = std::cmp::min::<$T>(self.y + self.height, aRect.y + aRect.height);
                // See bug 1457110, this function expects to -only- size to 0,0 if the
                // width/height is explicitly negative.
                if right < result.x || bottom < result.y {
                    result.width = 0;
                    result.height = 0;
                } else {
                    result.width = right - result.x;
                    result.height = bottom - result.y;
                }
                result
            }

            // Sets *this to be the rectangle containing the intersection of the points
            // (including edges) of *this and aRect. If there are no points in that
            // intersection, sets *this to be an empty rectangle with x/y set to the
            // std::max of the x/y of *this and aRect.
            //
            // 'this' can be the same object as either aRect1 or aRect2
            // (hidden by nsRect::IntersectRect, see the comment above the macro)
            // Gecko: gfx/2d/BaseRect.h BaseRect::IntersectRect
            pub fn Super__IntersectRect(&mut self, aRect1: $Sub, aRect2: $Sub) -> bool {
                let newX: $T = std::cmp::max::<$T>(aRect1.x, aRect2.x);
                let newY: $T = std::cmp::max::<$T>(aRect1.y, aRect2.y);
                self.width = std::cmp::min::<$T>(
                    aRect1.x - newX + aRect1.width,
                    aRect2.x - newX + aRect2.width,
                );
                self.height = std::cmp::min::<$T>(
                    aRect1.y - newY + aRect1.height,
                    aRect2.y - newY + aRect2.height,
                );
                self.x = newX;
                self.y = newY;
                if self.width <= 0 || self.height <= 0 {
                    self.SizeTo__nscoord_nscoord(0, 0);
                    return false;
                }
                true
            }

            // Returns the smallest rectangle that contains both the area of both
            // this and aRect. Thus, empty input rectangles are ignored.
            // Note: if both rectangles are empty, returns aRect.
            // WARNING! This is not safe against overflow, prefer using SafeUnion instead
            // when dealing with int-based rects.
            // (hidden by nsRect::Union, see the comment above the macro)
            // Gecko: gfx/2d/BaseRect.h BaseRect::Union
            #[must_use]
            pub fn Super__Union(&self, aRect: $Sub) -> $Sub {
                if self.IsEmpty() {
                    aRect
                } else if aRect.IsEmpty() {
                    *self
                } else {
                    self.Super__UnionEdges(aRect)
                }
            }
            // Returns the smallest rectangle that contains both the points (including
            // edges) of both aRect1 and aRect2.
            // Thus, empty input rectangles are allowed to affect the result.
            // WARNING! This is not safe against overflow, prefer using SafeUnionEdges
            // instead when dealing with int-based rects.
            // (hidden by nsRect::UnionEdges, see the comment above the macro)
            // Gecko: gfx/2d/BaseRect.h BaseRect::UnionEdges
            #[must_use]
            pub fn Super__UnionEdges(&self, aRect: $Sub) -> $Sub {
                let mut result = $Sub::default();
                result.x = std::cmp::min(self.x, aRect.x);
                result.y = std::cmp::min(self.y, aRect.y);
                result.width = std::cmp::max(self.XMost(), aRect.XMost()) - result.x;
                result.height = std::cmp::max(self.YMost(), aRect.YMost()) - result.y;
                result
            }
            // Computes the smallest rectangle that contains both the area of both
            // aRect1 and aRect2, and fills 'this' with the result.
            // Thus, empty input rectangles are ignored.
            // If both rectangles are empty, sets 'this' to aRect2.
            //
            // 'this' can be the same object as either aRect1 or aRect2
            // (hidden by nsRect::UnionRect, see the comment above the macro)
            // Gecko: gfx/2d/BaseRect.h BaseRect::UnionRect
            pub fn Super__UnionRect(&mut self, aRect1: $Sub, aRect2: $Sub) {
                *self = aRect1.Union(aRect2);
            }

            // Gecko: gfx/2d/BaseRect.h BaseRect::OrWith
            pub fn OrWith(&mut self, aRect1: $Sub) {
                let thisRect = *self;
                self.Super__UnionRect(thisRect, aRect1);
            }

            // Computes the smallest rectangle that contains both the points (including
            // edges) of both aRect1 and aRect2.
            // Thus, empty input rectangles are allowed to affect the result.
            //
            // 'this' can be the same object as either aRect1 or aRect2
            // Gecko: gfx/2d/BaseRect.h BaseRect::UnionRectEdges
            pub fn UnionRectEdges(&mut self, aRect1: $Sub, aRect2: $Sub) {
                *self = aRect1.UnionEdges(aRect2);
            }

            // Expands the rect to include the point
            // Gecko: gfx/2d/BaseRect.h BaseRect::ExpandToEnclose
            pub fn ExpandToEnclose(&mut self, aPoint: $Point) {
                if aPoint.x < self.x {
                    self.width = self.XMost() - aPoint.x;
                    self.x = aPoint.x;
                } else if aPoint.x > self.XMost() {
                    self.width = aPoint.x - self.x;
                }
                if aPoint.y < self.y {
                    self.height = self.YMost() - aPoint.y;
                    self.y = aPoint.y;
                } else if aPoint.y > self.YMost() {
                    self.height = aPoint.y - self.y;
                }
            }

            // Gecko: gfx/2d/BaseRect.h BaseRect::SetRect
            #[inline(always)]
            pub fn SetRect__nscoord_nscoord_nscoord_nscoord(&mut self, aX: $T, aY: $T, aWidth: $T, aHeight: $T) {
                self.x = aX;
                self.y = aY;
                self.width = aWidth;
                self.height = aHeight;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::SetRectX
            #[inline(always)]
            pub fn SetRectX(&mut self, aX: $T, aWidth: $T) {
                self.x = aX;
                self.width = aWidth;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::SetRectY
            #[inline(always)]
            pub fn SetRectY(&mut self, aY: $T, aHeight: $T) {
                self.y = aY;
                self.height = aHeight;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::SetBox
            #[inline(always)]
            pub fn SetBox(&mut self, aX: $T, aY: $T, aXMost: $T, aYMost: $T) {
                self.x = aX;
                self.y = aY;
                self.width = aXMost - aX;
                self.height = aYMost - aY;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::SetNonEmptyBox
            #[inline(always)]
            pub fn SetNonEmptyBox(&mut self, aX: $T, aY: $T, aXMost: $T, aYMost: $T) {
                self.x = aX;
                self.y = aY;
                self.width = std::cmp::max(0, aXMost - aX);
                self.height = std::cmp::max(0, aYMost - aY);
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::SetBoxX
            #[inline(always)]
            pub fn SetBoxX(&mut self, aX: $T, aXMost: $T) {
                self.x = aX;
                self.width = aXMost - aX;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::SetBoxY
            #[inline(always)]
            pub fn SetBoxY(&mut self, aY: $T, aYMost: $T) {
                self.y = aY;
                self.height = aYMost - aY;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::SetRect
            pub fn SetRect__nsPoint_nsSize(&mut self, aPt: $Point, aSize: $SizeT) {
                self.SetRect__nscoord_nscoord_nscoord_nscoord(aPt.x, aPt.y, aSize.width, aSize.height);
            }
            // T* out-parameters are &mut.
            // Gecko: gfx/2d/BaseRect.h BaseRect::GetRect
            #[inline(always)]
            pub fn GetRect(&self, aX: &mut $T, aY: &mut $T, aWidth: &mut $T, aHeight: &mut $T) {
                *aX = self.x;
                *aY = self.y;
                *aWidth = self.width;
                *aHeight = self.height;
            }

            // Gecko: gfx/2d/BaseRect.h BaseRect::MoveTo
            #[inline(always)]
            pub fn MoveTo__nscoord_nscoord(&mut self, aX: $T, aY: $T) {
                self.x = aX;
                self.y = aY;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::MoveToX
            #[inline(always)]
            pub fn MoveToX(&mut self, aX: $T) {
                self.x = aX;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::MoveToY
            #[inline(always)]
            pub fn MoveToY(&mut self, aY: $T) {
                self.y = aY;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::MoveTo
            #[inline(always)]
            pub fn MoveTo__nsPoint(&mut self, aPoint: $Point) {
                self.x = aPoint.x;
                self.y = aPoint.y;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::MoveBy
            #[inline(always)]
            pub fn MoveBy__nscoord_nscoord(&mut self, aDx: $T, aDy: $T) {
                self.x += aDx;
                self.y += aDy;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::MoveByX
            #[inline(always)]
            pub fn MoveByX(&mut self, aDx: $T) {
                self.x += aDx;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::MoveByY
            #[inline(always)]
            pub fn MoveByY(&mut self, aDy: $T) {
                self.y += aDy;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::MoveBy
            #[inline(always)]
            pub fn MoveBy__nsPoint(&mut self, aPoint: $Point) {
                self.x += aPoint.x;
                self.y += aPoint.y;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::SizeTo
            #[inline(always)]
            pub fn SizeTo__nscoord_nscoord(&mut self, aWidth: $T, aHeight: $T) {
                self.width = aWidth;
                self.height = aHeight;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::SizeTo
            #[inline(always)]
            pub fn SizeTo__nsSize(&mut self, aSize: $SizeT) {
                self.width = aSize.width;
                self.height = aSize.height;
            }

            // Variant of MoveBy that ensures that even after translation by a point that
            // the rectangle coordinates will still fit within numeric limits. The origin
            // and size will be clipped within numeric limits to ensure this.
            // Gecko: gfx/2d/BaseRect.h BaseRect::SafeMoveByX
            pub fn SafeMoveByX(&mut self, aDx: $T) {
                let x2: $T = self.XMost();
                if aDx >= 0 {
                    let limit: $T = <$T>::MAX;
                    self.x = if limit - aDx < self.x { limit } else { self.x + aDx };
                    self.width = (if limit - aDx < x2 { limit } else { x2 + aDx }) - self.x;
                } else {
                    let limit: $T = <$T>::MIN; // std::numeric_limits<T>::lowest()
                    self.x = if limit - aDx > self.x { limit } else { self.x + aDx };
                    self.width = (if limit - aDx > x2 { limit } else { x2 + aDx }) - self.x;
                }
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::SafeMoveByY
            pub fn SafeMoveByY(&mut self, aDy: $T) {
                let y2: $T = self.YMost();
                if aDy >= 0 {
                    let limit: $T = <$T>::MAX;
                    self.y = if limit - aDy < self.y { limit } else { self.y + aDy };
                    self.height = (if limit - aDy < y2 { limit } else { y2 + aDy }) - self.y;
                } else {
                    let limit: $T = <$T>::MIN; // std::numeric_limits<T>::lowest()
                    self.y = if limit - aDy > self.y { limit } else { self.y + aDy };
                    self.height = (if limit - aDy > y2 { limit } else { y2 + aDy }) - self.y;
                }
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::SafeMoveBy
            pub fn SafeMoveBy__nscoord_nscoord(&mut self, aDx: $T, aDy: $T) {
                self.SafeMoveByX(aDx);
                self.SafeMoveByY(aDy);
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::SafeMoveBy
            pub fn SafeMoveBy__nsPoint(&mut self, aPoint: $Point) {
                self.SafeMoveBy__nscoord_nscoord(aPoint.x, aPoint.y);
            }

            // Gecko: gfx/2d/BaseRect.h BaseRect::Inflate
            pub fn Inflate__nscoord(&mut self, aD: $T) {
                self.Inflate__nscoord_nscoord(aD, aD);
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::Inflate
            pub fn Inflate__nscoord_nscoord(&mut self, aDx: $T, aDy: $T) {
                self.x -= aDx;
                self.y -= aDy;
                self.width += 2 * aDx;
                self.height += 2 * aDy;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::Inflate
            pub fn Inflate__nsMargin(&mut self, aMargin: $MarginT) {
                self.x -= aMargin.left;
                self.y -= aMargin.top;
                self.width += aMargin.LeftRight();
                self.height += aMargin.TopBottom();
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::Inflate
            pub fn Inflate__nsSize(&mut self, aSize: $SizeT) {
                self.Inflate__nscoord_nscoord(aSize.width, aSize.height);
            }

            // Gecko: gfx/2d/BaseRect.h BaseRect::Deflate
            pub fn Deflate__nscoord(&mut self, aD: $T) {
                self.Deflate__nscoord_nscoord(aD, aD);
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::Deflate
            pub fn Deflate__nscoord_nscoord(&mut self, aDx: $T, aDy: $T) {
                self.x += aDx;
                self.y += aDy;
                self.width = std::cmp::max(0 as $T, self.width - 2 * aDx);
                self.height = std::cmp::max(0 as $T, self.height - 2 * aDy);
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::Deflate
            pub fn Deflate__nsMargin(&mut self, aMargin: $MarginT) {
                self.x += aMargin.left;
                self.y += aMargin.top;
                self.width = std::cmp::max(0 as $T, self.width - aMargin.LeftRight());
                self.height = std::cmp::max(0 as $T, self.height - aMargin.TopBottom());
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::Deflate
            pub fn Deflate__nsSize(&mut self, aSize: $SizeT) {
                self.Deflate__nscoord_nscoord(aSize.width, aSize.height);
            }

            // Return true if the rectangles contain the same set of points, including
            // points on the edges.
            // Use when we care about the exact x/y/width/height values being
            // equal (i.e. we care about differences in empty rectangles).
            // Gecko: gfx/2d/BaseRect.h BaseRect::IsEqualEdges
            pub fn IsEqualEdges(&self, aRect: $Sub) -> bool {
                self.x == aRect.x && self.y == aRect.y && self.width == aRect.width && self.height == aRect.height
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::IsEqualRect
            #[inline(always)]
            pub fn IsEqualRect(&self, aX: $T, aY: $T, aW: $T, aH: $T) -> bool {
                self.x == aX && self.y == aY && self.width == aW && self.height == aH
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::IsEqualXY
            #[inline(always)]
            pub fn IsEqualXY(&self, aX: $T, aY: $T) -> bool {
                self.x == aX && self.y == aY
            }

            // Gecko: gfx/2d/BaseRect.h BaseRect::IsEqualSize
            #[inline(always)]
            pub fn IsEqualSize(&self, aW: $T, aH: $T) -> bool {
                self.width == aW && self.height == aH
            }

            // Return true if the rectangles contain the same area of the plane.
            // Use when we do not care about differences in empty rectangles.
            // Gecko: gfx/2d/BaseRect.h BaseRect::IsEqualInterior
            pub fn IsEqualInterior(&self, aRect: $Sub) -> bool {
                self.IsEqualEdges(aRect) || (self.IsEmpty() && aRect.IsEmpty())
            }

            // Helpers for accessing the vertices
            // Gecko: gfx/2d/BaseRect.h BaseRect::TopLeft
            pub fn TopLeft(&self) -> $Point {
                $Point::new(self.x, self.y)
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::TopRight
            pub fn TopRight(&self) -> $Point {
                $Point::new(self.XMost(), self.y)
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::BottomLeft
            pub fn BottomLeft(&self) -> $Point {
                $Point::new(self.x, self.YMost())
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::BottomRight
            pub fn BottomRight(&self) -> $Point {
                $Point::new(self.XMost(), self.YMost())
            }
            // (the C++ `MOZ_CRASH("GFX: Incomplete switch")` after the switch is unreachable: Rust matches are exhaustive)
            // Gecko: gfx/2d/BaseRect.h BaseRect::AtCorner
            pub fn AtCorner(&self, aCorner: Corner) -> $Point {
                match aCorner {
                    eCornerTopLeft => self.TopLeft(),
                    eCornerTopRight => self.TopRight(),
                    eCornerBottomRight => self.BottomRight(),
                    eCornerBottomLeft => self.BottomLeft(),
                }
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::CCWCorner
            pub fn CCWCorner(&self, side: Side) -> $Point {
                match side {
                    eSideTop => self.TopLeft(),
                    eSideRight => self.TopRight(),
                    eSideBottom => self.BottomRight(),
                    eSideLeft => self.BottomLeft(),
                }
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::CWCorner
            pub fn CWCorner(&self, side: Side) -> $Point {
                match side {
                    eSideTop => self.TopRight(),
                    eSideRight => self.BottomRight(),
                    eSideBottom => self.BottomLeft(),
                    eSideLeft => self.TopLeft(),
                }
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::Center
            pub fn Center(&self) -> $Point {
                $Point::new(self.x, self.y) + $Point::new(self.width, self.height) / 2
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::Size
            pub fn Size(&self) -> $SizeT {
                $SizeT::new(self.width, self.height)
            }

            // Gecko: gfx/2d/BaseRect.h BaseRect::Area
            pub fn Area(&self) -> $T {
                self.width * self.height
            }

            // Helper methods for computing the extents
            // Gecko: gfx/2d/BaseRect.h BaseRect::X
            #[inline(always)]
            pub fn X(&self) -> $T {
                self.x
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::Y
            #[inline(always)]
            pub fn Y(&self) -> $T {
                self.y
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::Width
            #[inline(always)]
            pub fn Width(&self) -> $T {
                self.width
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::Height
            #[inline(always)]
            pub fn Height(&self) -> $T {
                self.height
            }

            // For an integral T, `(Saturate<T>(x) + width).value()`: mozilla::Saturate clamps at the limits
            // of T, which is what saturating_add does.
            // Gecko: gfx/2d/BaseRect.h BaseRect::XMost
            #[inline(always)]
            pub fn XMost(&self) -> $T {
                self.x.saturating_add(self.width)
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::YMost
            #[inline(always)]
            pub fn YMost(&self) -> $T {
                self.y.saturating_add(self.height)
            }

            // Set width and height. SizeTo() sets them together.
            // Gecko: gfx/2d/BaseRect.h BaseRect::SetWidth
            #[inline(always)]
            pub fn SetWidth(&mut self, aWidth: $T) {
                self.width = aWidth;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::SetHeight
            #[inline(always)]
            pub fn SetHeight(&mut self, aHeight: $T) {
                self.height = aHeight;
            }

            // Get the coordinate of the edge on the given side.
            // Gecko: gfx/2d/BaseRect.h BaseRect::Edge
            pub fn Edge(&self, aSide: Side) -> $T {
                match aSide {
                    eSideTop => self.Y(),
                    eSideRight => self.XMost(),
                    eSideBottom => self.YMost(),
                    eSideLeft => self.X(),
                }
            }

            // Moves one edge of the rect without moving the opposite edge.
            // Gecko: gfx/2d/BaseRect.h BaseRect::SetLeftEdge
            pub fn SetLeftEdge(&mut self, aX: $T) {
                self.width = self.XMost() - aX;
                self.x = aX;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::SetRightEdge
            pub fn SetRightEdge(&mut self, aXMost: $T) {
                self.width = aXMost - self.x;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::SetTopEdge
            pub fn SetTopEdge(&mut self, aY: $T) {
                self.height = self.YMost() - aY;
                self.y = aY;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::SetBottomEdge
            pub fn SetBottomEdge(&mut self, aYMost: $T) {
                self.height = aYMost - self.y;
            }
            // Gecko: gfx/2d/BaseRect.h BaseRect::Swap
            pub fn Swap(&mut self) {
                std::mem::swap(&mut self.x, &mut self.y);
                std::mem::swap(&mut self.width, &mut self.height);
            }

            // Round the rectangle edges to integer coordinates, such that the rounded
            // rectangle has the same set of pixel centers as the original rectangle.
            // Edges at offset 0.5 round up.
            // Suitable for most places where integral device coordinates
            // are needed, but note that any translation should be applied first to
            // avoid pixel rounding errors.
            // Note that this is *not* rounding to nearest integer if the values are
            // negative. They are always rounding as floor(n + 0.5). See
            // https://bugzilla.mozilla.org/show_bug.cgi?id=410748#c14 If you need similar
            // method which is using NS_round(), you should create new
            // |RoundAwayFromZero()| method.
            // (for an integral T the C++ `T(X()) + 0.5f` is a float addition, kept here)
            // Gecko: gfx/2d/BaseRect.h BaseRect::Round
            pub fn Round(&mut self) {
                let x0: $T = ((self.X() as f32 + 0.5f32).floor()) as $T;
                let y0: $T = ((self.Y() as f32 + 0.5f32).floor()) as $T;
                let x1: $T = ((self.XMost() as f32 + 0.5f32).floor()) as $T;
                let y1: $T = ((self.YMost() as f32 + 0.5f32).floor()) as $T;

                self.x = x0;
                self.y = y0;

                self.width = x1 - x0;
                self.height = y1 - y0;
            }

            // Snap the rectangle edges to integer coordinates, such that the
            // original rectangle contains the resulting rectangle.
            // Gecko: gfx/2d/BaseRect.h BaseRect::RoundIn
            pub fn RoundIn(&mut self) {
                let x0: $T = ((self.X() as f64).ceil()) as $T;
                let y0: $T = ((self.Y() as f64).ceil()) as $T;
                let x1: $T = ((self.XMost() as f64).floor()) as $T;
                let y1: $T = ((self.YMost() as f64).floor()) as $T;

                self.x = x0;
                self.y = y0;

                self.width = x1 - x0;
                self.height = y1 - y0;
            }

            // Snap the rectangle edges to integer coordinates, such that the
            // resulting rectangle contains the original rectangle.
            // Gecko: gfx/2d/BaseRect.h BaseRect::RoundOut
            pub fn RoundOut(&mut self) {
                let x0: $T = ((self.X() as f64).floor()) as $T;
                let y0: $T = ((self.Y() as f64).floor()) as $T;
                let x1: $T = ((self.XMost() as f64).ceil()) as $T;
                let y1: $T = ((self.YMost() as f64).ceil()) as $T;

                self.x = x0;
                self.y = y0;

                self.width = x1 - x0;
                self.height = y1 - y0;
            }

            // NOT PORTED: needs BaseScaleFactors2D (gfx/ScaleFactors2D.h), not needed for layout.
            // // Scale 'this' by aScale.xScale and aScale.yScale without doing any rounding.
            // template <class Src, class Dst>
            // void Scale(const BaseScaleFactors2D<Src, Dst, T>& aScale) {
            //   Scale(aScale.xScale, aScale.yScale);
            // }
            // Scale 'this' by aScale without doing any rounding.
            // Gecko: gfx/2d/BaseRect.h BaseRect::Scale
            pub fn Scale__nscoord(&mut self, aScale: $T) {
                self.Scale__nscoord_nscoord(aScale, aScale);
            }
            // Scale 'this' by aXScale and aYScale, without doing any rounding.
            // Gecko: gfx/2d/BaseRect.h BaseRect::Scale
            pub fn Scale__nscoord_nscoord(&mut self, aXScale: $T, aYScale: $T) {
                self.x = self.x * aXScale;
                self.y = self.y * aYScale;
                self.width = self.width * aXScale;
                self.height = self.height * aYScale;
            }
            // Scale 'this' by aScale, converting coordinates to integers so that the
            // result is the smallest integer-coordinate rectangle containing the
            // unrounded result. Note: this can turn an empty rectangle into a non-empty
            // rectangle
            // Gecko: gfx/2d/BaseRect.h BaseRect::ScaleRoundOut
            pub fn ScaleRoundOut__double(&mut self, aScale: f64) {
                self.ScaleRoundOut__double_double(aScale, aScale);
            }
            // Scale 'this' by aXScale and aYScale, converting coordinates to integers so
            // that the result is the smallest integer-coordinate rectangle containing the
            // unrounded result.
            // Note: this can turn an empty rectangle into a non-empty rectangle
            // Gecko: gfx/2d/BaseRect.h BaseRect::ScaleRoundOut
            pub fn ScaleRoundOut__double_double(&mut self, aXScale: f64, aYScale: f64) {
                let right: $T = ((self.XMost() as f64) * aXScale).ceil() as $T;
                let bottom: $T = ((self.YMost() as f64) * aYScale).ceil() as $T;
                self.x = ((self.x as f64) * aXScale).floor() as $T;
                self.y = ((self.y as f64) * aYScale).floor() as $T;
                self.width = right - self.x;
                self.height = bottom - self.y;
            }
            // Scale 'this' by aScale, converting coordinates to integers so that the
            // result is the largest integer-coordinate rectangle contained by the
            // unrounded result.
            // Gecko: gfx/2d/BaseRect.h BaseRect::ScaleRoundIn
            pub fn ScaleRoundIn__double(&mut self, aScale: f64) {
                self.ScaleRoundIn__double_double(aScale, aScale);
            }
            // Scale 'this' by aXScale and aYScale, converting coordinates to integers so
            // that the result is the largest integer-coordinate rectangle contained by
            // the unrounded result.
            // Gecko: gfx/2d/BaseRect.h BaseRect::ScaleRoundIn
            pub fn ScaleRoundIn__double_double(&mut self, aXScale: f64, aYScale: f64) {
                let right: $T = ((self.XMost() as f64) * aXScale).floor() as $T;
                let bottom: $T = ((self.YMost() as f64) * aYScale).floor() as $T;
                self.x = ((self.x as f64) * aXScale).ceil() as $T;
                self.y = ((self.y as f64) * aYScale).ceil() as $T;
                self.width = std::cmp::max::<$T>(0, right - self.x);
                self.height = std::cmp::max::<$T>(0, bottom - self.y);
            }
            // Scale 'this' by 1/aScale, converting coordinates to integers so that the
            // result is the smallest integer-coordinate rectangle containing the
            // unrounded result. Note: this can turn an empty rectangle into a non-empty
            // rectangle
            // Gecko: gfx/2d/BaseRect.h BaseRect::ScaleInverseRoundOut
            pub fn ScaleInverseRoundOut__double(&mut self, aScale: f64) {
                self.ScaleInverseRoundOut__double_double(aScale, aScale);
            }
            // Scale 'this' by 1/aXScale and 1/aYScale, converting coordinates to integers
            // so that the result is the smallest integer-coordinate rectangle containing
            // the unrounded result. Note: this can turn an empty rectangle into a
            // non-empty rectangle
            // Gecko: gfx/2d/BaseRect.h BaseRect::ScaleInverseRoundOut
            pub fn ScaleInverseRoundOut__double_double(&mut self, aXScale: f64, aYScale: f64) {
                let right: $T = ((self.XMost() as f64) / aXScale).ceil() as $T;
                let bottom: $T = ((self.YMost() as f64) / aYScale).ceil() as $T;
                self.x = ((self.x as f64) / aXScale).floor() as $T;
                self.y = ((self.y as f64) / aYScale).floor() as $T;
                self.width = right - self.x;
                self.height = bottom - self.y;
            }
            // Scale 'this' by 1/aScale, converting coordinates to integers so that the
            // result is the largest integer-coordinate rectangle contained by the
            // unrounded result.
            // Gecko: gfx/2d/BaseRect.h BaseRect::ScaleInverseRoundIn
            pub fn ScaleInverseRoundIn__double(&mut self, aScale: f64) {
                self.ScaleInverseRoundIn__double_double(aScale, aScale);
            }
            // Scale 'this' by 1/aXScale and 1/aYScale, converting coordinates to integers
            // so that the result is the largest integer-coordinate rectangle contained by
            // the unrounded result.
            // Gecko: gfx/2d/BaseRect.h BaseRect::ScaleInverseRoundIn
            pub fn ScaleInverseRoundIn__double_double(&mut self, aXScale: f64, aYScale: f64) {
                let right: $T = ((self.XMost() as f64) / aXScale).floor() as $T;
                let bottom: $T = ((self.YMost() as f64) / aYScale).floor() as $T;
                self.x = ((self.x as f64) / aXScale).ceil() as $T;
                self.y = ((self.y as f64) / aYScale).ceil() as $T;
                self.width = std::cmp::max::<$T>(0, right - self.x);
                self.height = std::cmp::max::<$T>(0, bottom - self.y);
            }

            /**
             * Clamp aPoint to this rectangle. It is allowed to end up on any
             * edge of the rectangle.
             * Return the rectangle as a point if the rectangle is empty.
             */
            // (`using Coord = decltype(aPoint.x)` is T here)
            // Gecko: gfx/2d/BaseRect.h BaseRect::ClampPoint
            #[must_use]
            pub fn ClampPoint(&self, aPoint: $Point) -> $Point {
                $Point::new(
                    std::cmp::max(self.x, std::cmp::min(self.XMost(), aPoint.x)),
                    std::cmp::max(self.y, std::cmp::min(self.YMost(), aPoint.y)),
                )
            }

            /**
             * Translate this rectangle to be inside aRect. If it doesn't fit inside
             * aRect then the dimensions that don't fit will be shrunk so that they
             * do fit. The resulting rect is returned.
             */
            // Gecko: gfx/2d/BaseRect.h BaseRect::MoveInsideAndClamp
            #[must_use]
            pub fn MoveInsideAndClamp(&self, aRect: $Sub) -> $Sub {
                let mut rect = $Sub::new(
                    std::cmp::max(aRect.x, self.x),
                    std::cmp::max(aRect.y, self.y),
                    std::cmp::min(aRect.width, self.width),
                    std::cmp::min(aRect.height, self.height),
                );
                rect.x = std::cmp::min(rect.XMost(), aRect.XMost()) - rect.width;
                rect.y = std::cmp::min(rect.YMost(), aRect.YMost()) - rect.height;
                rect
            }

            // Returns the largest rectangle that can be represented with 32-bit
            // signed integers, centered around a point at 0,0.  As BaseRect's represent
            // the dimensions as a top-left point with a width and height, the width
            // and height will be the largest positive 32-bit value.  The top-left
            // position coordinate is divided by two to center the rectangle around a
            // point at 0,0.
            // Gecko: gfx/2d/BaseRect.h BaseRect::MaxIntRect
            pub fn MaxIntRect() -> $Sub {
                $Sub::new(
                    (-(i32::MAX as f64) * 0.5) as $T,
                    (-(i32::MAX as f64) * 0.5) as $T,
                    i32::MAX as $T,
                    i32::MAX as $T,
                )
            }

            // Returns a point representing the distance, along each dimension, of the
            // given point from this rectangle. The distance along a dimension is defined
            // as zero if the point is within the bounds of the rectangle in that
            // dimension; otherwise, it's the distance to the closer endpoint of the
            // rectangle in that dimension.
            // Gecko: gfx/2d/BaseRect.h BaseRect::DistanceTo
            pub fn DistanceTo(&self, aPoint: $Point) -> $Point {
                $Point::new(
                    Self::DistanceFromInterval(aPoint.x, self.x, self.XMost()),
                    Self::DistanceFromInterval(aPoint.y, self.y, self.YMost()),
                )
            }

            // NOT PORTED: friend std::ostream& operator<<  (the struct derives Debug instead)

            // private:
            // Do not use the default operator== or operator!= !
            // Use IsEqualEdges or IsEqualInterior explicitly.
            // (not defined; nsRect defines PartialEq itself)

            // Helper function for DistanceTo() that computes the distance of a
            // coordinate along one dimension from an interval in that dimension.
            // Gecko: gfx/2d/BaseRect.h BaseRect::DistanceFromInterval
            fn DistanceFromInterval(aCoord: $T, aIntervalStart: $T, aIntervalEnd: $T) -> $T {
                if aCoord < aIntervalStart {
                    return aIntervalStart - aCoord;
                }
                if aCoord > aIntervalEnd {
                    return aCoord - aIntervalEnd;
                }
                0
            }
        }

        // friend Sub operator+(Sub aSub, const Point& aPoint)
        // Gecko: gfx/2d/BaseRect.h BaseRect::operator+
        impl std::ops::Add<$Point> for $Sub {
            type Output = $Sub;
            fn add(mut self, aPoint: $Point) -> $Sub {
                self += aPoint;
                self
            }
        }
        // Gecko: gfx/2d/BaseRect.h BaseRect::operator-
        impl std::ops::Sub<$Point> for $Sub {
            type Output = $Sub;
            fn sub(mut self, aPoint: $Point) -> $Sub {
                self -= aPoint;
                self
            }
        }
        // Gecko: gfx/2d/BaseRect.h BaseRect::operator+
        impl std::ops::Add<$SizeT> for $Sub {
            type Output = $Sub;
            fn add(mut self, aSize: $SizeT) -> $Sub {
                self += aSize;
                self
            }
        }
        // Gecko: gfx/2d/BaseRect.h BaseRect::operator-
        impl std::ops::Sub<$SizeT> for $Sub {
            type Output = $Sub;
            fn sub(mut self, aSize: $SizeT) -> $Sub {
                self -= aSize;
                self
            }
        }
        // Gecko: gfx/2d/BaseRect.h BaseRect::operator+=
        impl std::ops::AddAssign<$Point> for $Sub {
            fn add_assign(&mut self, aPoint: $Point) {
                self.MoveBy__nsPoint(aPoint);
            }
        }
        // Gecko: gfx/2d/BaseRect.h BaseRect::operator-=
        impl std::ops::SubAssign<$Point> for $Sub {
            fn sub_assign(&mut self, aPoint: $Point) {
                self.MoveBy__nsPoint(-aPoint);
            }
        }
        // Gecko: gfx/2d/BaseRect.h BaseRect::operator+=
        impl std::ops::AddAssign<$SizeT> for $Sub {
            fn add_assign(&mut self, aSize: $SizeT) {
                self.width += aSize.width;
                self.height += aSize.height;
            }
        }
        // Gecko: gfx/2d/BaseRect.h BaseRect::operator-=
        impl std::ops::SubAssign<$SizeT> for $Sub {
            fn sub_assign(&mut self, aSize: $SizeT) {
                self.width -= aSize.width;
                self.height -= aSize.height;
            }
        }
        // Find difference as a Margin
        // Gecko: gfx/2d/BaseRect.h BaseRect::operator-
        impl std::ops::Sub<$Sub> for $Sub {
            type Output = $MarginT;
            fn sub(self, aRect: $Sub) -> $MarginT {
                $MarginT::new(
                    aRect.y - self.y,
                    self.XMost() - aRect.XMost(),
                    self.YMost() - aRect.YMost(),
                    aRect.x - self.x,
                )
            }
        }
    };
}
