// Gecko: gfx/2d/Matrix.h and Matrix.cpp, the part of Matrix4x4 that CSS transforms need in layout
// (nsIFrame::GetTransformMatrix, nsLayoutUtils::TransformFrameRectToAncestor, nsDisplayTransform's
// hit test and TransformRect), with the float points and rectangles it takes (gfx/2d/Point.h
// Point4DTyped, gfx/2d/Rect.h RectTyped).
// ADAPTER: units are not typed (Matrix4x4Typed's SourceUnits and TargetUnits are UnknownUnits);
// Matrix4x4Flagged, which only skips work for identity and translation matrices, is Matrix4x4. The
// functions templated on the point's float type are generic over MatrixFloat (f32 for Gecko's Point and
// Rect, f64 for PointDouble and RectDouble). NOT PORTED: the 2D BaseMatrix (Is2D's out-parameter is
// not needed), rotations, scales, skews and the decompositions (Stylo builds the matrices,
// nsStyleTransformMatrix.rs).
use crate::*;
use std::ops::{Add, Div, Mul, Neg, Sub};

// ADAPTER: what Gecko's templates over `F` (float or double) use of it (gfxFloat is taken: thebes'
// double).
pub trait MatrixFloat: Copy + PartialOrd + Add<Output = Self> + Sub<Output = Self> + Mul<Output = Self> + Div<Output = Self> + Neg<Output = Self> + From<f32> {
    const MAX: Self;
}
impl MatrixFloat for f32 {
    const MAX: f32 = f32::MAX;
}
impl MatrixFloat for f64 {
    const MAX: f64 = f64::MAX;
}

// Gecko: gfx/2d/Point.h Point4DTyped
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point4DTyped<F> {
    pub x: F,
    pub y: F,
    pub z: F,
    pub w: F,
}

impl<F: MatrixFloat> Point4DTyped<F> {
    pub fn new(x: F, y: F, z: F, w: F) -> Self {
        Point4DTyped { x, y, z, w }
    }

    // Gecko: Point4DTyped::HasPositiveWCoord
    pub fn HasPositiveWCoord(&self) -> bool {
        self.w > F::from(0.0)
    }

    // Gecko: Point4DTyped::As2DPoint
    pub fn As2DPoint(&self) -> (F, F) {
        (self.x / self.w, self.y / self.w)
    }

    // Gecko: BasePoint4D::DotProduct
    fn DotProduct(&self, aPoint: &Self) -> F {
        self.x * aPoint.x + self.y * aPoint.y + self.z * aPoint.z + self.w * aPoint.w
    }

    // Gecko: BasePoint4D's operator+, operator- and operator*(F)
    fn Plus(&self, aPoint: &Self) -> Self {
        Self::new(self.x + aPoint.x, self.y + aPoint.y, self.z + aPoint.z, self.w + aPoint.w)
    }
    fn Minus(&self, aPoint: &Self) -> Self {
        Self::new(self.x - aPoint.x, self.y - aPoint.y, self.z - aPoint.z, self.w - aPoint.w)
    }
    fn Times(&self, aScale: F) -> Self {
        Self::new(self.x * aScale, self.y * aScale, self.z * aScale, self.w * aScale)
    }
}

// Gecko: gfx/2d/Rect.h RectTyped (Rect is RectTyped<UnknownUnits, float>, RectDouble the double one):
// the members the transforms use.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct gfxRectTyped<F> {
    pub x: F,
    pub y: F,
    pub width: F,
    pub height: F,
}

impl<F: MatrixFloat> gfxRectTyped<F> {
    pub fn new(x: F, y: F, width: F, height: F) -> Self {
        gfxRectTyped { x, y, width, height }
    }
    pub fn X(&self) -> F {
        self.x
    }
    pub fn Y(&self) -> F {
        self.y
    }
    pub fn XMost(&self) -> F {
        self.x + self.width
    }
    pub fn YMost(&self) -> F {
        self.y + self.height
    }

    // Gecko: BaseRect::ClampPoint
    pub fn ClampPoint(&self, aPoint: (F, F)) -> (F, F) {
        let clamp = |v: F, lo: F, hi: F| if v < lo { lo } else if v > hi { hi } else { v };
        (clamp(aPoint.0, self.x, self.XMost()), clamp(aPoint.1, self.y, self.YMost()))
    }
}

// Gecko: gfx/2d/Matrix.h ComputePerspectivePlaneIntercept
fn ComputePerspectivePlaneIntercept<F: MatrixFloat>(aFirst: &Point4DTyped<F>, aSecond: &Point4DTyped<F>) -> Point4DTyped<F> {
    // This function will always return a point with a w value of 0.
    // The X, Y, and Z components will point towards an infinite vanishing
    // point.

    // We want to interpolate aFirst and aSecond to find the point intersecting
    // with the w=0 plane.

    // Since we know what we want the w component to be, we can rearrange the
    // interpolation equation and solve for t.
    let t = -aFirst.w / (aSecond.w - aFirst.w);

    // Use t to find the remainder of the components
    aFirst.Plus(&aSecond.Minus(aFirst).Times(t))
}

// Gecko: gfx/2d/Matrix.cpp IntersectPolygon
// ADAPTER: the destination buffer is a Vec (Gecko's has kTransformAndClipRectMaxVerts points).
fn IntersectPolygon<F: MatrixFloat>(aPoints: &[Point4DTyped<F>], aPlaneNormal: &Point4DTyped<F>) -> Vec<Point4DTyped<F>> {
    let mut aDestBuffer = Vec::new();
    if aPoints.is_empty() {
        return aDestBuffer;
    }
    let zero = F::from(0.0);
    let one = F::from(1.0);

    // Iterate over the polygon edges. In each iteration the current edge
    // is the edge from *prevPoint to point. If the two end points lie on
    // different sides of the plane, we have an intersection. Otherwise,
    // the edge is either completely "inside" the half-space created by
    // the clipping plane, and we add curPoint, or it is completely
    // "outside", and we discard curPoint. This loop can create duplicated
    // points in the polygon.
    let mut prevPoint = &aPoints[aPoints.len() - 1];
    let mut prevDot = aPlaneNormal.DotProduct(prevPoint);
    for curPoint in aPoints {
        let curDot = aPlaneNormal.DotProduct(curPoint);

        if (curDot >= zero) != (prevDot >= zero) {
            // An intersection with the clipping plane has been detected.
            // Interpolate to find the intersecting curPoint and emit it.
            let t = -prevDot / (curDot - prevDot);
            aDestBuffer.push(curPoint.Times(t).Plus(&prevPoint.Times(one - t)));
            if aDestBuffer.len() >= Matrix4x4::kTransformAndClipRectMaxVerts {
                break;
            }
        }

        if curDot >= zero {
            // Emit any source points that are on the positive side of the
            // clipping plane.
            aDestBuffer.push(*curPoint);
            if aDestBuffer.len() >= Matrix4x4::kTransformAndClipRectMaxVerts {
                break;
            }
        }

        prevPoint = curPoint;
        prevDot = curDot;
    }

    aDestBuffer
}

// Gecko: gfx/2d/Matrix.h Matrix4x4Typed<UnknownUnits, UnknownUnits, float>: row vectors, a point p
// maps to p * M, so `A * B` applies A first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix4x4 {
    pub _11: f32,
    pub _12: f32,
    pub _13: f32,
    pub _14: f32,
    pub _21: f32,
    pub _22: f32,
    pub _23: f32,
    pub _24: f32,
    pub _31: f32,
    pub _32: f32,
    pub _33: f32,
    pub _34: f32,
    pub _41: f32,
    pub _42: f32,
    pub _43: f32,
    pub _44: f32,
}

impl Default for Matrix4x4 {
    fn default() -> Self {
        Matrix4x4::new(1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0)
    }
}

impl Matrix4x4 {
    pub const kTransformAndClipRectMaxVerts: usize = 32;

    #[allow(clippy::too_many_arguments)]
    pub fn new(a11: f32, a12: f32, a13: f32, a14: f32, a21: f32, a22: f32, a23: f32, a24: f32, a31: f32, a32: f32, a33: f32, a34: f32, a41: f32, a42: f32, a43: f32, a44: f32) -> Self {
        Matrix4x4 {
            _11: a11,
            _12: a12,
            _13: a13,
            _14: a14,
            _21: a21,
            _22: a22,
            _23: a23,
            _24: a24,
            _31: a31,
            _32: a32,
            _33: a33,
            _34: a34,
            _41: a41,
            _42: a42,
            _43: a43,
            _44: a44,
        }
    }

    pub fn IsIdentity(&self) -> bool {
        *self == Matrix4x4::default()
    }

    // Gecko: Matrix4x4Typed::Is2D
    // Returns true if the matrix is isomorphic to a 2D affine transformation.
    pub fn Is2D(&self) -> bool {
        if self._13 != 0.0 || self._14 != 0.0 || self._23 != 0.0 || self._24 != 0.0 || self._31 != 0.0 || self._32 != 0.0 || self._33 != 1.0 || self._34 != 0.0 || self._43 != 0.0 || self._44 != 1.0 {
            return false;
        }
        true
    }

    // Gecko: Matrix4x4Typed::PreservesAxisAlignedRectangles of the 2D matrix (BaseMatrix): the
    // matrix maps axis-aligned rectangles to axis-aligned rectangles
    pub fn PreservesAxisAlignedRectangles(&self) -> bool {
        (self._11 == 0.0 && self._22 == 0.0) || (self._21 == 0.0 && self._12 == 0.0)
    }

    // Gecko: Matrix4x4Typed::ProjectTo2D
    pub fn ProjectTo2D(&mut self) -> &mut Self {
        self._31 = 0.0;
        self._32 = 0.0;
        self._13 = 0.0;
        self._23 = 0.0;
        self._33 = 1.0;
        self._43 = 0.0;
        self._34 = 0.0;
        // Some matrices, such as those derived from perspective transforms,
        // can modify _44 from 1, while leaving the rest of the fourth column
        // (_14, _24) at 0. In this case, after resetting the third row and
        // third column above, the value of _44 functions only to scale the
        // coordinate transform divide by W. The matrix can be converted to
        // a true 2D matrix by normalizing out the scaling effect of _44 on
        // the remaining components ahead of time.
        if self._14 == 0.0 && self._24 == 0.0 && self._44 != 1.0 && self._44 != 0.0 {
            let scale = 1.0 / self._44;
            self._11 *= scale;
            self._12 *= scale;
            self._21 *= scale;
            self._22 *= scale;
            self._41 *= scale;
            self._42 *= scale;
            self._44 = 1.0;
        }
        self
    }

    // Gecko: Matrix4x4Typed::ProjectPoint
    pub fn ProjectPoint<F: MatrixFloat>(&self, aPoint: (F, F)) -> Point4DTyped<F> {
        let m = |v: f32| F::from(v);
        // Find a value for z that will transform to 0.

        // The transformed value of z is computed as:
        // z' = aPoint.x * _13 + aPoint.y * _23 + z * _33 + _43;

        // Solving for z when z' = 0 gives us:
        let z = -(aPoint.0 * m(self._13) + aPoint.1 * m(self._23) + m(self._43)) / m(self._33);

        // Compute the transformed point
        self.TransformPoint(Point4DTyped::new(aPoint.0, aPoint.1, z, F::from(1.0)))
    }

    // Gecko: Matrix4x4Typed::ProjectRectBounds
    pub fn ProjectRectBounds<F: MatrixFloat>(&self, aRect: gfxRectTyped<F>, aClip: gfxRectTyped<F>) -> gfxRectTyped<F> {
        // This function must never return std::numeric_limits<Float>::max() or any
        // other arbitrary large value in place of inifinity.  This often occurs
        // when aRect is an inversed projection matrix or when aRect is transformed
        // to be partly behind and in front of the camera (w=0 plane in homogenous
        // coordinates) - See Bug 1035611

        // (The rest of Gecko's comment, Matrix.h lines 752 to 772, explains why
        // the bounds are clamped to aClip here rather than left infinite.)
        let points = [
            self.ProjectPoint((aRect.X(), aRect.Y())),
            self.ProjectPoint((aRect.XMost(), aRect.Y())),
            self.ProjectPoint((aRect.XMost(), aRect.YMost())),
            self.ProjectPoint((aRect.X(), aRect.YMost())),
        ];

        let mut min_x = F::MAX;
        let mut min_y = F::MAX;
        let mut max_x = -F::MAX;
        let mut max_y = -F::MAX;
        let zero = F::from(0.0);

        for i in 0..4 {
            // Only use points that exist above the w=0 plane
            if points[i].HasPositiveWCoord() {
                let point2d = aClip.ClampPoint(points[i].As2DPoint());
                min_x = if point2d.0 < min_x { point2d.0 } else { min_x };
                max_x = if point2d.0 > max_x { point2d.0 } else { max_x };
                min_y = if point2d.1 < min_y { point2d.1 } else { min_y };
                max_y = if point2d.1 > max_y { point2d.1 } else { max_y };
            }

            let next = if i == 3 { 0 } else { i + 1 };
            if points[i].HasPositiveWCoord() != points[next].HasPositiveWCoord() {
                // If the line between two points crosses the w=0 plane, then
                // interpolate to find the point of intersection with the w=0 plane and
                // use that instead.
                let intercept = ComputePerspectivePlaneIntercept(&points[i], &points[next]);
                // Since intercept.w will always be 0 here, we interpret x,y,z as a
                // direction towards an infinite vanishing point.
                if intercept.x < zero {
                    min_x = aClip.X();
                } else if intercept.x > zero {
                    max_x = aClip.XMost();
                }
                if intercept.y < zero {
                    min_y = aClip.Y();
                } else if intercept.y > zero {
                    max_y = aClip.YMost();
                }
            }
        }

        if max_x < min_x || max_y < min_y {
            return gfxRectTyped::new(zero, zero, zero, zero);
        }

        gfxRectTyped::new(min_x, min_y, max_x - min_x, max_y - min_y)
    }

    // Gecko: Matrix4x4Typed::TransformAndClipBounds
    // TransformAndClipBounds transforms aRect as a bounding box, while clipping
    // the transformed bounds to the extents of aClip.
    pub fn TransformAndClipBounds<F: MatrixFloat>(&self, aRect: gfxRectTyped<F>, aClip: gfxRectTyped<F>) -> gfxRectTyped<F> {
        let verts = self.TransformAndClipRect(aRect, aClip);

        let mut min_x = F::MAX;
        let mut min_y = F::MAX;
        let mut max_x = -F::MAX;
        let mut max_y = -F::MAX;
        for v in &verts {
            min_x = if v.0 < min_x { v.0 } else { min_x };
            max_x = if v.0 > max_x { v.0 } else { max_x };
            min_y = if v.1 < min_y { v.1 } else { min_y };
            max_y = if v.1 > max_y { v.1 } else { max_y };
        }

        let zero = F::from(0.0);
        if max_x < min_x || max_y < min_y {
            return gfxRectTyped::new(zero, zero, zero, zero);
        }

        gfxRectTyped::new(min_x, min_y, max_x - min_x, max_y - min_y)
    }

    // Gecko: Matrix4x4Typed::TransformAndClipRect
    // TransformAndClipRect projects a rectangle and clips against view frustum
    // clipping planes in homogenous space so that its projected vertices are
    // constrained within the 2d rectangle passed in aClip.
    // (ADAPTER: the vertices are returned; fewer than 3 mean aRect is not visible within aClip.)
    pub fn TransformAndClipRect<F: MatrixFloat>(&self, aRect: gfxRectTyped<F>, aClip: gfxRectTyped<F>) -> Vec<(F, F)> {
        let (zero, one) = (F::from(0.0), F::from(1.0));
        let P4D = Point4DTyped::new;
        // The initial polygon is made up by the corners of aRect in homogenous
        // space, mapped into the destination space of this transform.
        let rectCorners = [
            self.TransformPoint(P4D(aRect.X(), aRect.Y(), zero, one)),
            self.TransformPoint(P4D(aRect.XMost(), aRect.Y(), zero, one)),
            self.TransformPoint(P4D(aRect.XMost(), aRect.YMost(), zero, one)),
            self.TransformPoint(P4D(aRect.X(), aRect.YMost(), zero, one)),
        ];

        // Cut off pieces of the polygon that are outside of aClip (the "view
        // frustrum"), by consecutively intersecting the polygon with the half space
        // induced by the clipping plane for each side of aClip.
        // View frustum clipping planes are described as normals originating from
        // the 0,0,0,0 origin.
        let mut polygon = IntersectPolygon(&rectCorners, &P4D(one, zero, zero, -aClip.X()));
        polygon = IntersectPolygon(&polygon, &P4D(-one, zero, zero, aClip.XMost()));
        polygon = IntersectPolygon(&polygon, &P4D(zero, one, zero, -aClip.Y()));
        polygon = IntersectPolygon(&polygon, &P4D(zero, -one, zero, aClip.YMost()));

        let mut aVerts: Vec<(F, F)> = Vec::new();
        for srcPoint in &polygon {
            let p = if srcPoint.w == zero {
                // If a point lies on the intersection of the clipping planes at
                // (0,0,0,0), we must avoid a division by zero w component.
                (zero, zero)
            } else {
                srcPoint.As2DPoint()
            };
            // Emit only unique points
            if aVerts.last() != Some(&p) {
                aVerts.push(p);
            }
        }
        aVerts
    }

    // Gecko: Matrix4x4Typed::TransformPoint(const Point4DTyped&)
    pub fn TransformPoint<F: MatrixFloat>(&self, aPoint: Point4DTyped<F>) -> Point4DTyped<F> {
        let m = |v: f32| F::from(v);
        Point4DTyped::new(
            aPoint.x * m(self._11) + aPoint.y * m(self._21) + aPoint.z * m(self._31) + aPoint.w * m(self._41),
            aPoint.x * m(self._12) + aPoint.y * m(self._22) + aPoint.z * m(self._32) + aPoint.w * m(self._42),
            aPoint.x * m(self._13) + aPoint.y * m(self._23) + aPoint.z * m(self._33) + aPoint.w * m(self._43),
            aPoint.x * m(self._14) + aPoint.y * m(self._24) + aPoint.z * m(self._34) + aPoint.w * m(self._44),
        )
    }

    // Gecko: Matrix4x4Typed::PreTranslate(T, T, T)
    // Add a translation to this matrix, applied before the existing transformation.
    pub fn PreTranslate(&mut self, aX: f32, aY: f32, aZ: f32) -> &mut Self {
        self._41 += aX * self._11 + aY * self._21 + aZ * self._31;
        self._42 += aX * self._12 + aY * self._22 + aZ * self._32;
        self._43 += aX * self._13 + aY * self._23 + aZ * self._33;
        self._44 += aX * self._14 + aY * self._24 + aZ * self._34;

        self
    }

    // Gecko: Matrix4x4Typed::PostTranslate(T, T, T)
    // Similar to PreTranslate, but the translation is applied -after- this
    // matrix's existing transformation instead of before it.
    pub fn PostTranslate(&mut self, aX: f32, aY: f32, aZ: f32) -> &mut Self {
        self._11 += self._14 * aX;
        self._21 += self._24 * aX;
        self._31 += self._34 * aX;
        self._41 += self._44 * aX;
        self._12 += self._14 * aY;
        self._22 += self._24 * aY;
        self._32 += self._34 * aY;
        self._42 += self._44 * aY;
        self._13 += self._14 * aZ;
        self._23 += self._24 * aZ;
        self._33 += self._34 * aZ;
        self._43 += self._44 * aZ;

        self
    }

    // Gecko: Matrix4x4Typed::ChangeBasis(T, T, T)
    pub fn ChangeBasis(&mut self, aX: f32, aY: f32, aZ: f32) -> &mut Self {
        // Translate to the origin before applying this matrix
        self.PreTranslate(-aX, -aY, -aZ);

        // Translate back into position after applying this matrix
        self.PostTranslate(aX, aY, aZ);

        self
    }

    // Gecko: Matrix4x4Typed::IsSingular
    pub fn IsSingular(&self) -> bool {
        self.Determinant() == 0.0
    }

    // Gecko: Matrix4x4Typed::Determinant
    pub fn Determinant(&self) -> f32 {
        let s = self;
        s._14 * s._23 * s._32 * s._41 - s._13 * s._24 * s._32 * s._41 - s._14 * s._22 * s._33 * s._41 + s._12 * s._24 * s._33 * s._41 + s._13 * s._22 * s._34 * s._41 - s._12 * s._23 * s._34 * s._41
            - s._14 * s._23 * s._31 * s._42
            + s._13 * s._24 * s._31 * s._42
            + s._14 * s._21 * s._33 * s._42
            - s._11 * s._24 * s._33 * s._42
            - s._13 * s._21 * s._34 * s._42
            + s._11 * s._23 * s._34 * s._42
            + s._14 * s._22 * s._31 * s._43
            - s._12 * s._24 * s._31 * s._43
            - s._14 * s._21 * s._32 * s._43
            + s._11 * s._24 * s._32 * s._43
            + s._12 * s._21 * s._34 * s._43
            - s._11 * s._22 * s._34 * s._43
            - s._13 * s._22 * s._31 * s._44
            + s._12 * s._23 * s._31 * s._44
            + s._13 * s._21 * s._32 * s._44
            - s._11 * s._23 * s._32 * s._44
            - s._12 * s._21 * s._33 * s._44
            + s._11 * s._22 * s._33 * s._44
    }

    // Gecko: Matrix4x4Typed::Invert
    pub fn Invert(&mut self) -> bool {
        let det = self.Determinant();
        if det == 0.0 {
            return false;
        }
        let s = *self;
        let mut r = Matrix4x4::default();
        r._11 = s._23 * s._34 * s._42 - s._24 * s._33 * s._42 + s._24 * s._32 * s._43 - s._22 * s._34 * s._43 - s._23 * s._32 * s._44 + s._22 * s._33 * s._44;
        r._12 = s._14 * s._33 * s._42 - s._13 * s._34 * s._42 - s._14 * s._32 * s._43 + s._12 * s._34 * s._43 + s._13 * s._32 * s._44 - s._12 * s._33 * s._44;
        r._13 = s._13 * s._24 * s._42 - s._14 * s._23 * s._42 + s._14 * s._22 * s._43 - s._12 * s._24 * s._43 - s._13 * s._22 * s._44 + s._12 * s._23 * s._44;
        r._14 = s._14 * s._23 * s._32 - s._13 * s._24 * s._32 - s._14 * s._22 * s._33 + s._12 * s._24 * s._33 + s._13 * s._22 * s._34 - s._12 * s._23 * s._34;
        r._21 = s._24 * s._33 * s._41 - s._23 * s._34 * s._41 - s._24 * s._31 * s._43 + s._21 * s._34 * s._43 + s._23 * s._31 * s._44 - s._21 * s._33 * s._44;
        r._22 = s._13 * s._34 * s._41 - s._14 * s._33 * s._41 + s._14 * s._31 * s._43 - s._11 * s._34 * s._43 - s._13 * s._31 * s._44 + s._11 * s._33 * s._44;
        r._23 = s._14 * s._23 * s._41 - s._13 * s._24 * s._41 - s._14 * s._21 * s._43 + s._11 * s._24 * s._43 + s._13 * s._21 * s._44 - s._11 * s._23 * s._44;
        r._24 = s._13 * s._24 * s._31 - s._14 * s._23 * s._31 + s._14 * s._21 * s._33 - s._11 * s._24 * s._33 - s._13 * s._21 * s._34 + s._11 * s._23 * s._34;
        r._31 = s._22 * s._34 * s._41 - s._24 * s._32 * s._41 + s._24 * s._31 * s._42 - s._21 * s._34 * s._42 - s._22 * s._31 * s._44 + s._21 * s._32 * s._44;
        r._32 = s._14 * s._32 * s._41 - s._12 * s._34 * s._41 - s._14 * s._31 * s._42 + s._11 * s._34 * s._42 + s._12 * s._31 * s._44 - s._11 * s._32 * s._44;
        r._33 = s._12 * s._24 * s._41 - s._14 * s._22 * s._41 + s._14 * s._21 * s._42 - s._11 * s._24 * s._42 - s._12 * s._21 * s._44 + s._11 * s._22 * s._44;
        r._34 = s._14 * s._22 * s._31 - s._12 * s._24 * s._31 - s._14 * s._21 * s._32 + s._11 * s._24 * s._32 + s._12 * s._21 * s._34 - s._11 * s._22 * s._34;
        r._41 = s._23 * s._32 * s._41 - s._22 * s._33 * s._41 - s._23 * s._31 * s._42 + s._21 * s._33 * s._42 + s._22 * s._31 * s._43 - s._21 * s._32 * s._43;
        r._42 = s._12 * s._33 * s._41 - s._13 * s._32 * s._41 + s._13 * s._31 * s._42 - s._11 * s._33 * s._42 - s._12 * s._31 * s._43 + s._11 * s._32 * s._43;
        r._43 = s._13 * s._22 * s._41 - s._12 * s._23 * s._41 - s._13 * s._21 * s._42 + s._11 * s._23 * s._42 + s._12 * s._21 * s._43 - s._11 * s._22 * s._43;
        r._44 = s._12 * s._23 * s._31 - s._13 * s._22 * s._31 + s._13 * s._21 * s._32 - s._11 * s._23 * s._32 - s._12 * s._21 * s._33 + s._11 * s._22 * s._33;
        for v in [
            &mut r._11, &mut r._12, &mut r._13, &mut r._14, &mut r._21, &mut r._22, &mut r._23, &mut r._24, &mut r._31, &mut r._32, &mut r._33, &mut r._34, &mut r._41, &mut r._42, &mut r._43, &mut r._44,
        ] {
            *v /= det;
        }
        *self = r;

        true
    }

    // Gecko: Matrix4x4Typed::MaybeInverse
    pub fn MaybeInverse(&self) -> Option<Matrix4x4> {
        let mut clone = *self;
        if clone.Invert() { Some(clone) } else { None }
    }
}

// Gecko: Matrix4x4Typed::operator*
impl Mul for Matrix4x4 {
    type Output = Matrix4x4;
    fn mul(self, aMatrix: Matrix4x4) -> Matrix4x4 {
        let (s, a) = (self, aMatrix);
        Matrix4x4 {
            _11: s._11 * a._11 + s._12 * a._21 + s._13 * a._31 + s._14 * a._41,
            _21: s._21 * a._11 + s._22 * a._21 + s._23 * a._31 + s._24 * a._41,
            _31: s._31 * a._11 + s._32 * a._21 + s._33 * a._31 + s._34 * a._41,
            _41: s._41 * a._11 + s._42 * a._21 + s._43 * a._31 + s._44 * a._41,
            _12: s._11 * a._12 + s._12 * a._22 + s._13 * a._32 + s._14 * a._42,
            _22: s._21 * a._12 + s._22 * a._22 + s._23 * a._32 + s._24 * a._42,
            _32: s._31 * a._12 + s._32 * a._22 + s._33 * a._32 + s._34 * a._42,
            _42: s._41 * a._12 + s._42 * a._22 + s._43 * a._32 + s._44 * a._42,
            _13: s._11 * a._13 + s._12 * a._23 + s._13 * a._33 + s._14 * a._43,
            _23: s._21 * a._13 + s._22 * a._23 + s._23 * a._33 + s._24 * a._43,
            _33: s._31 * a._13 + s._32 * a._23 + s._33 * a._33 + s._34 * a._43,
            _43: s._41 * a._13 + s._42 * a._23 + s._43 * a._33 + s._44 * a._43,
            _14: s._11 * a._14 + s._12 * a._24 + s._13 * a._34 + s._14 * a._44,
            _24: s._21 * a._14 + s._22 * a._24 + s._23 * a._34 + s._24 * a._44,
            _34: s._31 * a._14 + s._32 * a._24 + s._33 * a._34 + s._34 * a._44,
            _44: s._41 * a._14 + s._42 * a._24 + s._43 * a._34 + s._44 * a._44,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A translation, a scale about an origin and their product map points as Gecko's row vectors do.
    #[test]
    fn TranslateScaleAndProduct() {
        let mut t = Matrix4x4::default();
        t.PostTranslate(10.0, 20.0, 0.0);
        let p = t.TransformPoint(Point4DTyped::new(1.0f32, 2.0, 0.0, 1.0));
        assert_eq!((p.x, p.y), (11.0, 22.0));
        let mut s = Matrix4x4::default();
        s._11 = 2.0;
        s._22 = 2.0;
        s.ChangeBasis(5.0, 5.0, 0.0);
        let q = s.TransformPoint(Point4DTyped::new(5.0f32, 6.0, 0.0, 1.0));
        assert_eq!((q.x, q.y), (5.0, 7.0));
        // s then t
        let st = s * t;
        let r = st.TransformPoint(Point4DTyped::new(5.0f32, 6.0, 0.0, 1.0));
        assert_eq!((r.x, r.y), (15.0, 27.0));
        let inv = st.MaybeInverse().unwrap();
        let back = inv.ProjectPoint((15.0f32, 27.0)).As2DPoint();
        assert_eq!(back, (5.0, 6.0));
        let b = st.TransformAndClipBounds(gfxRectTyped::new(0.0f64, 0.0, 10.0, 10.0), gfxRectTyped::new(-1e9, -1e9, 2e9, 2e9));
        assert_eq!(b, gfxRectTyped::new(5.0, 15.0, 20.0, 20.0));
    }
}
