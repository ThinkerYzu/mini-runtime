// Gecko: layout/style/nsStyleTransformMatrix.h and nsStyleTransformMatrix.cpp: the reference box
// of a transformed frame and the matrix of its computed `translate`, `rotate`, `scale` and
// `transform`.
// ADAPTER: the matrix of each transform function is Stylo's (ToMatrix::to_3d_matrix, which follows
// MatrixForTransformFunction: the same functions, combined in the same order, the trigonometry in
// double) instead of a translation of MatrixForTransformFunction and its Process* helpers. One
// device pixel is one CSS px here (AppUnitsPerDevPixel is AppUnitsPerCSSPixel), so ReadTransforms'
// scale to device pixels is 1. No zoom (the Servo build has none).
// NOT PORTED: SVG reference boxes (no SVG layout), `transform-box: stroke-box` (Gecko leaves the
// box empty too), `offset-path` (motion paths: ResolveMotionPath).
use crate::*;
use mrt_style::style::values::computed::transform::TransformOperation;
use mrt_style::style::values::computed::{Length, LengthPercentage};
use mrt_style::style::values::generics::transform::{Rotate, Scale, ToMatrix, Transform, Translate};
use mrt_style::style::values::specified::transform::TransformBox;

// Gecko: layout/style/nsStyleTransformMatrix.h TransformReferenceBox: the box a transform's
// percentages and its origin are relative to, in the frame's own app units.
// ADAPTER: computed when made (Gecko computes it on first use, EnsureDimensionsAreCached).
#[derive(Clone, Copy, Debug)]
pub struct TransformReferenceBox {
    mFrame: nsIFrame,
    mBox: nsRect,
}

impl TransformReferenceBox {
    // Gecko: TransformReferenceBox(const nsIFrame*) and EnsureDimensionsAreCached
    pub fn new(aFrame: nsIFrame) -> TransformReferenceBox {
        TransformReferenceBox::new__nsIFrame_nsSize(aFrame, aFrame.GetSize())
    }

    // ADAPTER: the box of `aFrame` as if its size were `aSize`: nsIFrame::FinishAndStoreOverflow
    // gives the frame its new size for the time of the call (SetSize) before Gecko reads the box.
    pub fn new__nsIFrame_nsSize(aFrame: nsIFrame, aSize: nsSize) -> TransformReferenceBox {
        // For elements with associated CSS layout box, the used value for fill-box is
        // content-box and for stroke-box and view-box is border-box.
        // https://drafts.csswg.org/css-transforms-1/#transform-box
        let mBox = match aFrame.Style().get_box().transform_box {
            TransformBox::FillBox | TransformBox::ContentBox => {
                // GetContentRectRelativeToSelf, with the size given
                let bp = aFrame.GetUsedBorderAndPadding();
                let mut r = nsRect::new(0, 0, aSize.width, aSize.height);
                r.Deflate__nsMargin(bp);
                r
            }
            // TODO: Implement this in the following patches.
            TransformBox::StrokeBox => nsRect::default(),
            // (no UNIFIED_CONTINUATIONS: the frame's own rectangle, at the origin)
            TransformBox::ViewBox | TransformBox::BorderBox => nsRect::new(0, 0, aSize.width, aSize.height),
        };
        TransformReferenceBox { mFrame: aFrame, mBox }
    }

    pub fn X(&self) -> nscoord {
        self.mBox.x
    }
    pub fn Y(&self) -> nscoord {
        self.mBox.y
    }
    pub fn Width(&self) -> nscoord {
        self.mBox.width
    }
    pub fn Height(&self) -> nscoord {
        self.mBox.height
    }
    pub fn IsEmpty(&self) -> bool {
        self.mBox.IsEmpty()
    }
}

pub struct nsStyleTransformMatrix;

impl nsStyleTransformMatrix {
    // Gecko: nsStyleTransformMatrix::Convert2DPosition(const LengthPercentage&, const
    // LengthPercentage&, TransformReferenceBox&): a position in CSS px, its percentages of the
    // reference box's size.
    pub fn Convert2DPosition(aX: &LengthPercentage, aY: &LengthPercentage, aRefBox: &TransformReferenceBox) -> (f32, f32) {
        let css = |v: nscoord| v as f32 / AppUnitsPerCSSPixel() as f32;
        (aX.resolve(Length::new(css(aRefBox.Width()))).px(), aY.resolve(Length::new(css(aRefBox.Height()))).px())
    }

    // Gecko: nsStyleTransformMatrix::ReadTransforms(const StyleTranslate&, const StyleRotate&, const
    // StyleScale&, const ResolvedMotionPathData*, const StyleTransform&, TransformReferenceBox&,
    // float, Zoomed): translate, then rotate, then scale, then the transform functions in order, as
    // one matrix in device pixels. A function Stylo cannot resolve (none: the reference box is given)
    // leaves the identity.
    // ADAPTER: the individual properties are the equivalent transform functions (ProcessTranslate is
    // ProcessTranslate3D, ProcessRotate RotateZ or ProcessRotate3D, ProcessScale ProcessScaleHelper)
    // ahead of the list, which Stylo's components_to_transform_3d_matrix multiplies as Gecko's
    // MatrixForTransformFunction does; percentages in translations are of the reference box's size
    // (ProcessTranslatePart: 0 when the box is empty).
    pub fn ReadTransforms(aFrame: nsIFrame, aRefBox: &TransformReferenceBox) -> Matrix4x4 {
        let style = aFrame.Style();
        let b = style.get_box();
        let mut ops: Vec<TransformOperation> = Vec::with_capacity(b.transform.0.len() + 3);
        match &b.translate {
            Translate::None => {}
            Translate::Translate(x, y, z) => ops.push(TransformOperation::Translate3D(x.clone(), y.clone(), *z)),
        }
        match &b.rotate {
            Rotate::None => {}
            Rotate::Rotate(angle) => ops.push(TransformOperation::RotateZ(*angle)),
            Rotate::Rotate3D(x, y, z, angle) => ops.push(TransformOperation::Rotate3D(*x, *y, *z, *angle)),
        }
        match &b.scale {
            Scale::None => {}
            Scale::Scale(x, y, z) => ops.push(TransformOperation::Scale3D(*x, *y, *z)),
        }
        // NOT PORTED: the motion path (offset-path)
        ops.extend(b.transform.0.iter().cloned());

        let css = |v: nscoord| Length::new(if aRefBox.IsEmpty() { 0.0 } else { v as f32 / AppUnitsPerCSSPixel() as f32 });
        let refBox = euclid::default::Rect::new(euclid::point2(Length::new(0.0), Length::new(0.0)), euclid::size2(css(aRefBox.Width()), css(aRefBox.Height())));
        let Ok((m, _)) = Transform::components_to_transform_3d_matrix(&ops, Some(&refBox)) else {
            return Matrix4x4::default();
        };
        Matrix4x4::new(m.m11, m.m12, m.m13, m.m14, m.m21, m.m22, m.m23, m.m24, m.m31, m.m32, m.m33, m.m34, m.m41, m.m42, m.m43, m.m44)
    }
}
