/* struct containing the output from nsIFrame::Reflow */
// Translated from layout/generic/ReflowOutput.h and ReflowOutput.cpp (GECKO_REV).
use crate::*;

// NOT PORTED: includes (mozilla/EnumeratedRange.h, see AllOverflowTypes below).

//----------------------------------------------------------------------

// struct ReflowInput;

// enum class OverflowType : uint8_t { Ink, Scrollable };
// Gecko: layout/generic/ReflowOutput.h OverflowType
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[repr(u8)]
pub enum OverflowType {
    Ink,
    Scrollable,
}

// constexpr auto AllOverflowTypes() {
//   return MakeInclusiveEnumeratedRange(OverflowType::Ink,
//                                       OverflowType::Scrollable);
// }
// TEMPORARY: mfbt/EnumeratedRange.h is not translated; the range is an array, which iterates the
// same way (`for t in AllOverflowTypes()`).
// Gecko: layout/generic/ReflowOutput.h AllOverflowTypes
pub fn AllOverflowTypes() -> [OverflowType; 2] {
    [OverflowType::Ink, OverflowType::Scrollable]
}

// Flags controlling how a child's overflow areas are unioned into a parent's
// overflow areas.
MOZ_ENUM_CLASS_BITS! {
    // Gecko: layout/generic/ReflowOutput.h OverflowAreaUnionFlags
    pub struct OverflowAreaUnionFlags: u8 {
        None = 0,
        // Treat the frame as if it were a scroll container: bypass the contain:layout
        // check so the scrollable overflow of children still contributes.
        AsIfScrolled = 1 << 0,
        // The child being considered is absolutely positioned. The child's overflow
        // is unioned via OverflowAreas::UnionWithAbsoluteOverflowAreas (which skips
        // empty overflow rects).
        ChildIsAbsPos = 1 << 1,
    }
}

// Gecko: layout/generic/ReflowOutput.cpp IsValidOverflowRect
fn IsValidOverflowRect(aRect: nsRect) -> bool {
    // The reason we can't simply use `nsRect::IsEmpty` is that any one dimension
    // being zero is considered empty by it - On the other hand, an overflow rect
    // is valid if it has non-negative dimensions and at least one of them is
    // non-zero.
    aRect.Size() != nsSize::new(0, 0) && aRect.Width() >= 0 && aRect.Height() >= 0
}

// Gecko: layout/generic/ReflowOutput.h OverflowAreas
#[derive(Clone, Copy, Debug, Default)]
pub struct OverflowAreas {
    mInk: nsRect,
    mScrollable: nsRect,
}

impl OverflowAreas {
    // nsRect& InkOverflow() { return mInk; }
    // Gecko: layout/generic/ReflowOutput.h OverflowAreas::InkOverflow (non-const)
    pub fn InkOverflow_mut(&mut self) -> &mut nsRect {
        &mut self.mInk
    }
    // Gecko: layout/generic/ReflowOutput.h OverflowAreas::InkOverflow
    pub fn InkOverflow(&self) -> &nsRect {
        &self.mInk
    }

    // Gecko: layout/generic/ReflowOutput.h OverflowAreas::ScrollableOverflow (non-const)
    pub fn ScrollableOverflow_mut(&mut self) -> &mut nsRect {
        &mut self.mScrollable
    }
    // Gecko: layout/generic/ReflowOutput.h OverflowAreas::ScrollableOverflow
    pub fn ScrollableOverflow(&self) -> &nsRect {
        &self.mScrollable
    }

    // Gecko: layout/generic/ReflowOutput.h OverflowAreas::Overflow (non-const)
    pub fn Overflow_mut(&mut self, aType: OverflowType) -> &mut nsRect {
        if aType == OverflowType::Ink { self.InkOverflow_mut() } else { self.ScrollableOverflow_mut() }
    }
    // Gecko: layout/generic/ReflowOutput.h OverflowAreas::Overflow
    pub fn Overflow(&self, aType: OverflowType) -> &nsRect {
        if aType == OverflowType::Ink { self.InkOverflow() } else { self.ScrollableOverflow() }
    }

    // OverflowAreas() = default;  -> Default::default()

    // Gecko: layout/generic/ReflowOutput.h OverflowAreas::OverflowAreas
    pub fn new(aInkOverflow: nsRect, aScrollableOverflow: nsRect) -> OverflowAreas {
        OverflowAreas { mInk: aInkOverflow, mScrollable: aScrollableOverflow }
    }

    // bool operator!=(const OverflowAreas&) const = default;  -> PartialEq provides `!=`

    // OverflowAreas operator+(const nsPoint& aPoint) const  -> impl Add<nsPoint> below
    // OverflowAreas& operator+=(const nsPoint& aPoint)      -> impl AddAssign<nsPoint> below

    // Gecko: layout/generic/ReflowOutput.h OverflowAreas::Clear
    pub fn Clear(&mut self) {
        self.SetAllTo(nsRect::default());
    }

    // Applies overflow clipping (for e.g. overflow: clip) as needed to both our
    // overflow rects.
    // Gecko: layout/generic/ReflowOutput.h OverflowAreas::ApplyClipping
    pub fn ApplyClipping(&mut self, aBounds: nsRect, aClipAxes: PhysicalAxes, aOverflowMargin: nsMargin) {
        OverflowAreas::ApplyOverflowClippingOnRect(self.InkOverflow_mut(), aBounds, aClipAxes, aOverflowMargin);
        OverflowAreas::ApplyOverflowClippingOnRect(self.ScrollableOverflow_mut(), aBounds, aClipAxes, aOverflowMargin);
    }

    // Gets the overflow clipping rect for a given element given a rect to clip,
    // the frame bounds, a set of axes, and the overflow margin.
    /* static */
    // Gecko: layout/generic/ReflowOutput.cpp OverflowAreas::GetOverflowClipRect
    pub fn GetOverflowClipRect(aRectToClip: nsRect, aBounds: nsRect, aClipAxes: PhysicalAxes, aOverflowMargin: nsMargin) -> nsRect {
        let mut inflatedBounds = aBounds;
        inflatedBounds.Inflate__nsMargin(aOverflowMargin);
        let mut clip = aRectToClip;
        if aClipAxes.contains(PhysicalAxis::Vertical) {
            clip.y = inflatedBounds.y;
            clip.height = inflatedBounds.height;
        }
        if aClipAxes.contains(PhysicalAxis::Horizontal) {
            clip.x = inflatedBounds.x;
            clip.width = inflatedBounds.width;
        }
        clip
    }

    // Applies the overflow clipping to a given overflow rect, given the frame
    // bounds, and the physical axes on which to apply the overflow clip.
    /* static */
    // Gecko: layout/generic/ReflowOutput.cpp OverflowAreas::ApplyOverflowClippingOnRect
    pub fn ApplyOverflowClippingOnRect(aOverflowRect: &mut nsRect, aBounds: nsRect, aClipAxes: PhysicalAxes, aOverflowMargin: nsMargin) {
        *aOverflowRect = aOverflowRect.Intersect(OverflowAreas::GetOverflowClipRect(*aOverflowRect, aBounds, aClipAxes, aOverflowMargin));
    }

    // Mutates |this| by unioning both overflow areas with |aOther|.
    // Gecko: layout/generic/ReflowOutput.cpp OverflowAreas::UnionWith
    pub fn UnionWith(&mut self, aOther: &OverflowAreas) {
        if IsValidOverflowRect(*aOther.InkOverflow()) {
            let ink = *self.InkOverflow();
            self.InkOverflow_mut().UnionRect(ink, *aOther.InkOverflow());
        }
        if IsValidOverflowRect(*aOther.ScrollableOverflow()) {
            let scrollable = *self.ScrollableOverflow();
            self.ScrollableOverflow_mut().UnionRect(scrollable, *aOther.ScrollableOverflow());
        }
    }

    // Mutates |this| by unioning both overflow areas with |aOther|, which is
    // assumed to be overflow areas of an absolutely positioned frame.
    // Gecko: layout/generic/ReflowOutput.cpp OverflowAreas::UnionWithAbsoluteOverflowAreas
    pub fn UnionWithAbsoluteOverflowAreas(&mut self, aOther: &OverflowAreas) {
        // Note(dshin, Bug 2025540): This is different from
        // `OverflowAreas::UnionWith`, which adds overflows as long as at least one
        // axis has a non-zero size, is explicitly avoided here. This is not
        // explicitly specified in spec, but not being strict here can lead to
        // surprises when the inset value causes the abspos frame to lie outside its
        // containing block.
        for t in AllOverflowTypes() {
            let kidOverflow = aOther.Overflow(t);
            if kidOverflow.IsEmpty() {
                continue;
            }
            let overflow = self.Overflow_mut(t);
            let current = *overflow;
            overflow.UnionRect(current, *kidOverflow);
        }
    }

    // Mutates |this| by unioning both overflow areas with |aRect|.
    // Gecko: layout/generic/ReflowOutput.cpp OverflowAreas::UnionAllWith
    pub fn UnionAllWith(&mut self, aRect: nsRect) {
        if !IsValidOverflowRect(aRect) {
            // Same as `UnionWith()` - avoid losing information.
            return;
        }
        let ink = *self.InkOverflow();
        self.InkOverflow_mut().UnionRect(ink, aRect);
        let scrollable = *self.ScrollableOverflow();
        self.ScrollableOverflow_mut().UnionRect(scrollable, aRect);
    }

    // Mutates |this| by setting both overflow areas to |aRect|.
    // Gecko: layout/generic/ReflowOutput.cpp OverflowAreas::SetAllTo
    pub fn SetAllTo(&mut self, aRect: nsRect) {
        *self.InkOverflow_mut() = aRect;
        *self.ScrollableOverflow_mut() = aRect;
    }
}

// Gecko: layout/generic/ReflowOutput.h OverflowAreas::operator==
impl PartialEq for OverflowAreas {
    fn eq(&self, aOther: &OverflowAreas) -> bool {
        // Scrollable overflow is a point-set rectangle and ink overflow
        // is a pixel-set rectangle.
        self.InkOverflow().IsEqualInterior(*aOther.InkOverflow()) && self.ScrollableOverflow().IsEqualEdges(*aOther.ScrollableOverflow())
    }
}

// Gecko: layout/generic/ReflowOutput.h OverflowAreas::operator+
impl std::ops::Add<nsPoint> for OverflowAreas {
    type Output = OverflowAreas;
    fn add(self, aPoint: nsPoint) -> OverflowAreas {
        let mut result = self;
        result += aPoint;
        result
    }
}

// Gecko: layout/generic/ReflowOutput.h OverflowAreas::operator+=
impl std::ops::AddAssign<nsPoint> for OverflowAreas {
    fn add_assign(&mut self, aPoint: nsPoint) {
        self.mInk += aPoint;
        self.mScrollable += aPoint;
    }
}

/**
 * CollapsingMargin represents a vertical collapsing margin between
 * blocks as described in section 8.3.1 of CSS2.
 * <https://www.w3.org/TR/CSS22/box.html#collapsing-margins>
 *
 * All adjacent vertical margins collapse, and the resulting margin is
 * the sum of the largest positive margin included and the smallest (most
 * negative) negative margin included.
 */
// Gecko: layout/generic/ReflowOutput.h CollapsingMargin
// (operator== / operator!= are `= default`: PartialEq derive)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CollapsingMargin {
    // The largest positive margin included.
    mMostPos: nscoord,

    // The smallest negative margin included.
    mMostNeg: nscoord,
}

impl Default for CollapsingMargin {
    fn default() -> CollapsingMargin {
        CollapsingMargin { mMostPos: 0, mMostNeg: 0 }
    }
}

impl CollapsingMargin {
    // Gecko: layout/generic/ReflowOutput.h CollapsingMargin::Include
    pub fn Include__nscoord(&mut self, aCoord: nscoord) {
        if aCoord > self.mMostPos {
            self.mMostPos = aCoord;
        } else if aCoord < self.mMostNeg {
            self.mMostNeg = aCoord;
        }
    }

    // Gecko: layout/generic/ReflowOutput.h CollapsingMargin::Include
    pub fn Include__CollapsingMargin(&mut self, aOther: &CollapsingMargin) {
        if aOther.mMostPos > self.mMostPos {
            self.mMostPos = aOther.mMostPos;
        }
        if aOther.mMostNeg < self.mMostNeg {
            self.mMostNeg = aOther.mMostNeg;
        }
    }

    // Gecko: layout/generic/ReflowOutput.h CollapsingMargin::Zero
    pub fn Zero(&mut self) {
        self.mMostPos = 0;
        self.mMostNeg = 0;
    }

    // Gecko: layout/generic/ReflowOutput.h CollapsingMargin::IsZero
    pub fn IsZero(&self) -> bool {
        self.mMostPos == 0 && self.mMostNeg == 0
    }

    // Gecko: layout/generic/ReflowOutput.h CollapsingMargin::Get
    pub fn Get(&self) -> nscoord {
        self.mMostPos + self.mMostNeg
    }
}

/**
 * ReflowOutput is initialized by a parent frame as a parameter passing to
 * Reflow() to allow a child frame to return its desired size and alignment
 * information.
 *
 * ReflowOutput's constructor usually takes a parent frame's WritingMode (or
 * ReflowInput) because it is more convenient for the parent frame to use the
 * stored Size() after reflowing the child frame. However, it can actually
 * accept any WritingMode (or ReflowInput) because SetSize() knows how to
 * convert a size in any writing mode to the stored writing mode.
 *
 * @see nsIFrame::Reflow() for more information.
 */
// Gecko: layout/generic/ReflowOutput.h ReflowOutput
#[derive(Clone, Debug)]
pub struct ReflowOutput {
    // Metrics that _exactly_ enclose the text to allow precise MathML placements.
    pub mBoundingMetrics: nsBoundingMetrics, // [OUT]

    // Carried out block-end margin values. This is the collapsed
    // (generational) block-end margin value.
    pub mCarriedOutBEndMargin: CollapsingMargin,

    // For frames that have content that overflow their content area
    // (HasOverflowAreas() is true) these rectangles represent the total
    // area of the frame including visible overflow, i.e., don't include
    // overflowing content that is hidden.  The rects are in the local
    // coordinate space of the frame, and should be at least as big as the
    // desired size. If there is no content that overflows, then the
    // overflow area is identical to the desired size and should be {0, 0,
    // width, height}.
    pub mOverflowAreas: OverflowAreas,

    // Desired size of a frame's border-box.
    mSize: LogicalSize,

    // Baseline (in block direction), or the default value ASK_FOR_BASELINE.
    mBlockStartAscent: nscoord,

    mWritingMode: WritingMode,
}

impl ReflowOutput {
    // It's only meaningful to consider "ascent" on the block-start side of the
    // frame, so no need to pass a writing mode argument
    // enum { ASK_FOR_BASELINE = nscoord_MAX };
    pub const ASK_FOR_BASELINE: nscoord = nscoord_MAX;

    // explicit ReflowOutput(WritingMode aWritingMode)
    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::ReflowOutput
    pub fn new__WritingMode(aWritingMode: WritingMode) -> ReflowOutput {
        ReflowOutput {
            mBoundingMetrics: nsBoundingMetrics::default(),
            mCarriedOutBEndMargin: CollapsingMargin::default(),
            mOverflowAreas: OverflowAreas::default(),
            mSize: LogicalSize::new__WritingMode(aWritingMode),
            mBlockStartAscent: ReflowOutput::ASK_FOR_BASELINE,
            mWritingMode: aWritingMode,
        }
    }

    // A convenient constructor to get WritingMode in ReflowInput.
    // Gecko: layout/generic/ReflowOutput.cpp ReflowOutput::ReflowOutput
    pub fn new__ReflowInput(aReflowInput: &ReflowInput) -> ReflowOutput {
        ReflowOutput::new__WritingMode(aReflowInput.GetWritingMode())
    }

    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::ISize
    pub fn ISize(&self, aWritingMode: WritingMode) -> nscoord {
        self.mSize.ISize(aWritingMode)
    }
    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::BSize
    pub fn BSize(&self, aWritingMode: WritingMode) -> nscoord {
        self.mSize.BSize(aWritingMode)
    }
    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::Size
    pub fn Size(&self, aWritingMode: WritingMode) -> LogicalSize {
        self.mSize.ConvertTo(aWritingMode, self.mWritingMode)
    }

    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::ISize (non-const)
    pub fn ISize_mut(&mut self, aWritingMode: WritingMode) -> &mut nscoord {
        self.mSize.ISize_mut(aWritingMode)
    }
    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::BSize (non-const)
    pub fn BSize_mut(&mut self, aWritingMode: WritingMode) -> &mut nscoord {
        self.mSize.BSize_mut(aWritingMode)
    }

    // Set inline and block size from a LogicalSize, converting to our
    // writing mode as necessary.
    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::SetSize
    pub fn SetSize(&mut self, aWM: WritingMode, aSize: &LogicalSize) {
        self.mSize = aSize.ConvertTo(self.mWritingMode, aWM);
    }

    // Set both inline and block size to zero -- no need for a writing mode!
    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::ClearSize
    pub fn ClearSize(&mut self) {
        self.mSize.SizeTo(self.mWritingMode, 0, 0);
    }

    // Width and Height are physical dimensions, independent of writing mode.
    // Accessing these is slightly more expensive than accessing the logical
    // dimensions; as far as possible, client code should work purely with logical
    // dimensions.
    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::Width
    pub fn Width(&self) -> nscoord {
        self.mSize.Width(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::Height
    pub fn Height(&self) -> nscoord {
        self.mSize.Height(self.mWritingMode)
    }
    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::Width (non-const)
    pub fn Width_mut(&mut self) -> &mut nscoord {
        if self.mWritingMode.IsVertical() { self.mSize.BSize_mut(self.mWritingMode) } else { self.mSize.ISize_mut(self.mWritingMode) }
    }
    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::Height (non-const)
    pub fn Height_mut(&mut self) -> &mut nscoord {
        if self.mWritingMode.IsVertical() { self.mSize.ISize_mut(self.mWritingMode) } else { self.mSize.BSize_mut(self.mWritingMode) }
    }

    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::PhysicalSize
    pub fn PhysicalSize(&self) -> nsSize {
        self.mSize.GetPhysicalSize(self.mWritingMode)
    }

    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::BlockStartAscent
    pub fn BlockStartAscent(&self) -> nscoord {
        self.mBlockStartAscent
    }
    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::SetBlockStartAscent
    pub fn SetBlockStartAscent(&mut self, aAscent: nscoord) {
        self.mBlockStartAscent = aAscent;
    }

    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::InkOverflow (non-const)
    pub fn InkOverflow_mut(&mut self) -> &mut nsRect {
        self.mOverflowAreas.InkOverflow_mut()
    }
    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::InkOverflow
    pub fn InkOverflow(&self) -> &nsRect {
        self.mOverflowAreas.InkOverflow()
    }
    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::ScrollableOverflow (non-const)
    pub fn ScrollableOverflow_mut(&mut self) -> &mut nsRect {
        self.mOverflowAreas.ScrollableOverflow_mut()
    }
    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::ScrollableOverflow
    pub fn ScrollableOverflow(&self) -> &nsRect {
        self.mOverflowAreas.ScrollableOverflow()
    }

    // Set all of mOverflowAreas to (0, 0, width, height).
    // Gecko: layout/generic/ReflowOutput.cpp ReflowOutput::SetOverflowAreasToDesiredBounds
    pub fn SetOverflowAreasToDesiredBounds(&mut self) {
        let bounds = nsRect::new(0, 0, self.Width(), self.Height());
        self.mOverflowAreas.SetAllTo(bounds);
    }

    // Union all of mOverflowAreas with (0, 0, width, height).
    // Gecko: layout/generic/ReflowOutput.cpp ReflowOutput::UnionOverflowAreasWithDesiredBounds
    pub fn UnionOverflowAreasWithDesiredBounds(&mut self) {
        let bounds = nsRect::new(0, 0, self.Width(), self.Height());
        self.mOverflowAreas.UnionAllWith(bounds);
    }

    // Gecko: layout/generic/ReflowOutput.h ReflowOutput::GetWritingMode
    pub fn GetWritingMode(&self) -> WritingMode {
        self.mWritingMode
    }
}

// Tests: there is no gtest for ReflowOutput in the Firefox tree; these are written from the
// behaviour documented in the comments of ReflowOutput.h / ReflowOutput.cpp.
#[cfg(test)]
mod tests {
    use super::*;

    fn horizontal() -> WritingMode {
        WritingMode::default()
    }

    #[test]
    fn UnionWithIgnoresInvalidRects() {
        let mut a = OverflowAreas::new(nsRect::new(0, 0, 10, 10), nsRect::new(0, 0, 10, 10));
        // Zero-size and negative rects are not valid overflow rects: no change.
        a.UnionWith(&OverflowAreas::new(nsRect::new(50, 50, 0, 0), nsRect::new(50, 50, -5, 3)));
        assert!(*a.InkOverflow() == nsRect::new(0, 0, 10, 10));
        assert!(*a.ScrollableOverflow() == nsRect::new(0, 0, 10, 10));
    }

    #[test]
    fn UnionWithUnionsValidRects() {
        let mut a = OverflowAreas::new(nsRect::new(0, 0, 10, 10), nsRect::new(0, 0, 10, 10));
        a.UnionWith(&OverflowAreas::new(nsRect::new(20, 0, 5, 5), nsRect::new(-5, -5, 1, 1)));
        assert!(*a.InkOverflow() == nsRect::new(0, 0, 25, 10));
        assert!(*a.ScrollableOverflow() == nsRect::new(-5, -5, 15, 15));
    }

    #[test]
    fn UnionWithAbsoluteSkipsEmptyRects() {
        let mut a = OverflowAreas::new(nsRect::new(0, 0, 10, 10), nsRect::new(0, 0, 10, 10));
        // Unlike UnionWith, a rect with one zero dimension is skipped.
        a.UnionWithAbsoluteOverflowAreas(&OverflowAreas::new(nsRect::new(20, 0, 0, 5), nsRect::new(0, 20, 10, 10)));
        assert!(*a.InkOverflow() == nsRect::new(0, 0, 10, 10));
        assert!(*a.ScrollableOverflow() == nsRect::new(0, 0, 10, 30));
    }

    #[test]
    fn UnionAllWithAndSetAllToAndClear() {
        let mut a = OverflowAreas::default();
        a.SetAllTo(nsRect::new(1, 2, 3, 4));
        assert!(*a.InkOverflow() == nsRect::new(1, 2, 3, 4) && *a.ScrollableOverflow() == nsRect::new(1, 2, 3, 4));
        a.UnionAllWith(nsRect::new(0, 0, 0, 0));
        assert!(*a.InkOverflow() == nsRect::new(1, 2, 3, 4));
        a.UnionAllWith(nsRect::new(10, 10, 5, 5));
        assert!(*a.ScrollableOverflow() == nsRect::new(1, 2, 14, 13));
        a.Clear();
        assert!(*a.InkOverflow() == nsRect::default() && *a.ScrollableOverflow() == nsRect::default());
    }

    #[test]
    fn OverflowAreasEqualityAndOffset() {
        let a = OverflowAreas::new(nsRect::new(0, 0, 10, 10), nsRect::new(0, 0, 20, 20));
        let b = a + nsPoint::new(5, 6);
        assert!(*b.InkOverflow() == nsRect::new(5, 6, 10, 10));
        assert!(*b.ScrollableOverflow() == nsRect::new(5, 6, 20, 20));
        assert!(a != b);
        assert!(a == a);
    }

    #[test]
    fn OverflowByType() {
        let mut a = OverflowAreas::new(nsRect::new(0, 0, 1, 1), nsRect::new(0, 0, 2, 2));
        assert!(*a.Overflow(OverflowType::Ink) == nsRect::new(0, 0, 1, 1));
        assert!(*a.Overflow(OverflowType::Scrollable) == nsRect::new(0, 0, 2, 2));
        *a.Overflow_mut(OverflowType::Scrollable) = nsRect::new(0, 0, 3, 3);
        assert!(*a.ScrollableOverflow() == nsRect::new(0, 0, 3, 3));
        assert_eq!(AllOverflowTypes(), [OverflowType::Ink, OverflowType::Scrollable]);
    }

    #[test]
    fn OverflowClipRect() {
        let bounds = nsRect::new(0, 0, 100, 50);
        let toClip = nsRect::new(-10, -10, 300, 300);
        let mut v = PhysicalAxes::new();
        v += PhysicalAxis::Vertical;
        assert!(OverflowAreas::GetOverflowClipRect(toClip, bounds, v, nsMargin::new(0, 0, 0, 0)) == nsRect::new(-10, 0, 300, 50));
        assert!(OverflowAreas::GetOverflowClipRect(toClip, bounds, kPhysicalAxesBoth, nsMargin::new(1, 2, 3, 4)) == nsRect::new(-4, -1, 106, 54));
        let mut r = toClip;
        OverflowAreas::ApplyOverflowClippingOnRect(&mut r, bounds, kPhysicalAxesBoth, nsMargin::new(0, 0, 0, 0));
        assert!(r == bounds);
        let mut areas = OverflowAreas::new(toClip, toClip);
        areas.ApplyClipping(bounds, kPhysicalAxesBoth, nsMargin::new(0, 0, 0, 0));
        assert!(*areas.InkOverflow() == bounds && *areas.ScrollableOverflow() == bounds);
    }

    #[test]
    fn OverflowAreaUnionFlagsBits() {
        let f = OverflowAreaUnionFlags::AsIfScrolled | OverflowAreaUnionFlags::ChildIsAbsPos;
        assert!((f & OverflowAreaUnionFlags::AsIfScrolled).to_bool());
        assert!((f & OverflowAreaUnionFlags::ChildIsAbsPos).to_bool());
        assert!(!OverflowAreaUnionFlags::None.to_bool());
    }

    #[test]
    fn CollapsingMarginIncludes() {
        let mut m = CollapsingMargin::default();
        assert!(m.IsZero());
        m.Include__nscoord(10);
        m.Include__nscoord(4);
        m.Include__nscoord(-3);
        m.Include__nscoord(-1);
        assert_eq!(m.Get(), 7);
        let mut o = CollapsingMargin::default();
        o.Include__nscoord(20);
        o.Include__nscoord(-8);
        m.Include__CollapsingMargin(&o);
        assert_eq!(m.Get(), 12);
        assert!(m == o);
        assert!(m != CollapsingMargin::default());
        m.Zero();
        assert!(m.IsZero());
    }

    #[test]
    fn ReflowOutputSize() {
        let wm = horizontal();
        let mut o = ReflowOutput::new__WritingMode(wm);
        assert_eq!(o.BlockStartAscent(), ReflowOutput::ASK_FOR_BASELINE);
        o.SetSize(wm, &LogicalSize::new(wm, 30, 40));
        assert_eq!((o.ISize(wm), o.BSize(wm), o.Width(), o.Height()), (30, 40, 30, 40));
        assert!(o.PhysicalSize() == nsSize::new(30, 40));
        *o.Width_mut() = 31;
        *o.Height_mut() = 41;
        assert_eq!((o.ISize(wm), o.BSize(wm)), (31, 41));
        *o.ISize_mut(wm) = 5;
        *o.BSize_mut(wm) = 6;
        assert_eq!((o.Width(), o.Height()), (5, 6));
        o.SetBlockStartAscent(3);
        assert_eq!(o.BlockStartAscent(), 3);
        o.ClearSize();
        assert_eq!((o.Width(), o.Height()), (0, 0));
        assert!(o.GetWritingMode() == wm);
    }

    // new__ReflowInput is covered by the ReflowInput tests (a real ReflowInput needs a frame tree).

    #[test]
    fn ReflowOutputOverflowToDesiredBounds() {
        let wm = horizontal();
        let mut o = ReflowOutput::new__WritingMode(wm);
        o.SetSize(wm, &LogicalSize::new(wm, 30, 40));
        o.SetOverflowAreasToDesiredBounds();
        assert!(*o.InkOverflow() == nsRect::new(0, 0, 30, 40));
        assert!(*o.ScrollableOverflow() == nsRect::new(0, 0, 30, 40));
        *o.InkOverflow_mut() = nsRect::new(-5, 0, 10, 10);
        o.UnionOverflowAreasWithDesiredBounds();
        assert!(*o.InkOverflow() == nsRect::new(-5, 0, 35, 40));
        assert!(*o.ScrollableOverflow() == nsRect::new(0, 0, 30, 40));
    }
}
