// Translated from layout/tables/nsTableColFrame.h and nsTableColFrame.cpp (GECKO_REV).
//
// Rust shape (see CONVENTIONS.md and the top of nsBlockFrame.rs): `nsTableColFrame` is a `Copy`
// handle over a frame of type TableCol that derefs to nsIFrame (the C++ class derives from
// nsSplittableFrame). The data members live in one heap block per frame behind a frame property
// (`nsTableColFrame_Members`, `Members()`), created on first use. The virtual methods of the class
// (Init, DidSetComputedStyle, Reflow, ...) are `match` arms on the frame type in the adapter layer;
// this file has the nsTableColFrame overrides under their own names.
//
// NOT PORTED: painting (BuildDisplayList), invalidation (InvalidateFrame, InvalidateFrameWithRect,
// InvalidateFrameForRemoval), frame dumps (GetFrameName, Dump), the border-collapse damage
// computation (the part of DidSetComputedStyle that needs nsTableFrame::BCRecalcNeeded is kept as
// written and calls it), refcount logging.
use crate::*;
use mrt_style::style::properties::ComputedValues;
use mrt_style::style::servo_arc::Arc as ServoArc;
use std::ops::Deref;

// Gecko: layout/tables/nsTableColFrame.cpp COL_TYPE_BITS, COL_TYPE_OFFSET
// (NS_FRAME_STATE_BIT(28) | NS_FRAME_STATE_BIT(29) | NS_FRAME_STATE_BIT(30) | NS_FRAME_STATE_BIT(31))
const COL_TYPE_BITS: nsFrameState = (1 << 28) | (1 << 29) | (1 << 30) | (1 << 31);
const COL_TYPE_OFFSET: u32 = 28;

// Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::eWIDTH_SOURCE_NONE, ...
// (an anonymous enum)
pub const nsTableColFrame_eWIDTH_SOURCE_NONE: i32 = 0; // no cell has contributed to the width style
pub const nsTableColFrame_eWIDTH_SOURCE_CELL: i32 = 1; // a cell specified a width
pub const nsTableColFrame_eWIDTH_SOURCE_CELL_WITH_SPAN: i32 = 2; // a cell implicitly specified a width via colspan

// ADAPTER: the members of the C++ class that are plain data (see the top of the file).
// Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::mMinCoord ... mHasSpecifiedCoord
pub struct nsTableColFrame_Members {
    pub mMinCoord: nscoord,
    pub mPrefCoord: nscoord,
    pub mSpanMinCoord: nscoord,  // XXX...
    pub mSpanPrefCoord: nscoord, // XXX...
    pub mPrefPercent: f32,
    pub mSpanPrefPercent: f32, // XXX...
    // ...XXX the four members marked above could be allocated as part of
    // a separate array allocated only during
    // BasicTableLayoutStrategy::ComputeColumnIntrinsicISizes (and only
    // when colspans were present).
    pub mFinalISize: nscoord,

    // the index of the column with respect to the whole table (starting at 0)
    // it should never be smaller then the start column index of the parent
    // colgroup
    pub mColIndex: u32,

    // border widths of the inner half of the border only
    pub mIStartBorderWidth: nscoord,
    pub mIEndBorderWidth: nscoord,

    pub mHasSpecifiedCoord: bool,
}

// Gecko: layout/tables/nsTableColFrame.cpp (the frame properties; ids 7500..7549 are this file's)
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(ColMembersProperty, usize, 7500);
// ADAPTER: the value of `StyleTable()->mXSpan` (see GetSpan); set by the frame constructor.
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(ColSpanProperty, i32, 7501);

// Gecko: layout/tables/nsTableColFrame.h nsTableColFrame
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsTableColFrame(pub nsIFrame);

impl Deref for nsTableColFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

impl nsTableColFrame {
    // ADAPTER: static_cast<nsTableColFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsTableColFrame {
        nsTableColFrame(aFrame)
    }

    // ADAPTER: do_QueryFrame<nsTableColFrame>(frame): the frame is of type TableCol.
    pub fn do_QueryFrame(aFrame: nsIFrame) -> Option<nsTableColFrame> {
        if aFrame.Type() == LayoutFrameType::TableCol {
            Some(nsTableColFrame(aFrame))
        } else {
            None
        }
    }

    // ADAPTER: this frame's data members (see nsTableColFrame_Members). Created on first use with
    // the values the C++ constructor gives them (before the calls in its body, which NS_NewTableColFrame makes).
    fn Members(&self) -> &'static mut nsTableColFrame_Members {
        let p = match self.0.GetProperty(ColMembersProperty()) {
            Some(p) => p,
            None => {
                let p = Box::into_raw(Box::new(nsTableColFrame_Members {
                    mMinCoord: 0,
                    mPrefCoord: 0,
                    mSpanMinCoord: 0,
                    mSpanPrefCoord: 0,
                    mPrefPercent: 0.0,
                    mSpanPrefPercent: 0.0,
                    mFinalISize: 0,
                    mColIndex: 0,
                    mIStartBorderWidth: 0,
                    mIEndBorderWidth: 0,
                    mHasSpecifiedCoord: false,
                })) as usize;
                self.0.SetOwnedPointerProperty::<nsTableColFrame_Members>(ColMembersProperty(), p);
                p
            }
        };
        unsafe { &mut *(p as *mut nsTableColFrame_Members) }
    }
}

// Gecko: layout/tables/nsTableColFrame.cpp NS_NewTableColFrame
// ADAPTER: frames are made by whoever builds the frame tree (the frame constructor, a test) with
// FrameTree::AddFrame; this gives one the class and runs the body of the constructor
// nsTableColFrame::nsTableColFrame.
pub fn NS_NewTableColFrame(aFrame: nsIFrame) -> nsTableColFrame {
    aFrame.SetType(LayoutFrameType::TableCol);
    let col = nsTableColFrame(aFrame);
    // (the member initializers are in Members())
    col.Members();
    col.SetColType(nsTableColType::eColContent);
    col.ResetIntrinsics();
    col.ResetSpanIntrinsics();
    col.ResetFinalISize();
    col
}

// Gecko: layout/tables/nsTableColFrame.h (the inline members, in declaration order) and nsTableColFrame.cpp
impl nsTableColFrame {
    // Gecko: layout/tables/nsTableColFrame.cpp nsTableColFrame::GetColType
    pub fn GetColType(&self) -> nsTableColType {
        match (self.GetStateBits() & COL_TYPE_BITS) >> COL_TYPE_OFFSET {
            0 => nsTableColType::eColContent,
            1 => nsTableColType::eColAnonymousCol,
            2 => nsTableColType::eColAnonymousColGroup,
            _ => nsTableColType::eColAnonymousCell,
        }
    }

    // Gecko: layout/tables/nsTableColFrame.cpp nsTableColFrame::SetColType
    pub fn SetColType(&self, aType: nsTableColType) {
        NS_ASSERTION!(
            aType != nsTableColType::eColAnonymousCol
                || (self.GetPrevContinuation().is_some()
                    && self.GetPrevContinuation().unwrap().GetNextContinuation() == Some(self.0)
                    && self.GetPrevContinuation().unwrap().GetNextSibling() == Some(self.0)),
            "spanned content cols must be continuations"
        );
        let type_: u64 = (aType as u64) - (nsTableColType::eColContent as u64);
        self.RemoveStateBits(COL_TYPE_BITS);
        self.AddStateBits(type_ << COL_TYPE_OFFSET);
    }

    // nsIFrame overrides
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::Init
    pub fn Init(&self, aContent: Option<nsIContent>, aParent: nsIFrame, aPrevInFlow: Option<nsIFrame>) {
        // nsSplittableFrame::Init(aContent, aParent, aPrevInFlow);
        // ADAPTER: nsSplittableFrame::Init has no translation of its own yet; the TEMP stub of
        // nsContainerFrame_Init (nsBlockFrame.rs) stands in, as everywhere in this crate.
        self.nsContainerFrame_Init(aContent, aParent, aPrevInFlow);
        if aPrevInFlow.is_none() {
            // mWritingMode = GetTableFrame()->GetWritingMode();
            let wm = self.GetTableFrame().GetWritingMode();
            WithFrameTree(|t| t.mFrames[self.0.0 as usize].mWritingMode = wm);
        }
    }

    /** @see nsIFrame::DidSetComputedStyle */
    // Gecko: layout/tables/nsTableColFrame.cpp nsTableColFrame::DidSetComputedStyle
    // ADAPTER: the old style is the Stylo style (the frame's `Style()` is already the new one).
    pub fn DidSetComputedStyle(&self, aOldComputedStyle: Option<ServoArc<ComputedValues>>) {
        // nsSplittableFrame::DidSetComputedStyle(aOldComputedStyle);

        let Some(aOldComputedStyle) = aOldComputedStyle else {
            // avoid this on init
            return;
        };

        let tableFrame = self.GetTableFrame();
        if tableFrame.IsBorderCollapse() && tableFrame.BCRecalcNeeded(&aOldComputedStyle, &self.Style()) {
            let damageArea = TableArea::new(self.GetColIndex(), 0, 1, tableFrame.GetRowCount());
            tableFrame.AddBCDamageArea(&damageArea);
        }
    }

    // Gecko: layout/tables/nsTableColFrame.cpp nsTableColFrame::Reflow
    pub fn Reflow(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.MarkInReflow();
        // DO_GLOBAL_REFLOW_COUNT("nsTableColFrame");
        // NOT PORTED: reflow counting.
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");
        aDesiredSize.ClearSize();
        if IsVisibilityCollapse(self.0) {
            self.GetTableFrame().SetNeedToCollapse(true);
        }
    }

    // Gecko: layout/tables/nsTableColFrame.cpp nsTableColFrame::BuildDisplayList
    // NOT PORTED: painting. (In Gecko this asserts: "Cols don't paint themselves".)

    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::GetTableColGroupFrame
    pub fn GetTableColGroupFrame(&self) -> nsTableColGroupFrame {
        let parent = self.GetParent();
        MOZ_ASSERT!(parent.is_some() && parent.unwrap().IsTableColGroupFrame());
        nsTableColGroupFrame::from_frame(parent.unwrap())
    }

    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::GetTableFrame
    pub fn GetTableFrame(&self) -> nsTableFrame {
        self.GetTableColGroupFrame().GetTableFrame()
    }

    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::GetColIndex
    pub fn GetColIndex(&self) -> i32 {
        self.Members().mColIndex as i32
    }

    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::SetColIndex
    pub fn SetColIndex(&self, aColIndex: i32) {
        self.Members().mColIndex = aColIndex as u32;
    }

    // Gecko: layout/tables/nsTableColFrame.cpp nsTableColFrame::GetNextCol
    pub fn GetNextCol(&self) -> Option<nsTableColFrame> {
        let mut childFrame = self.GetNextSibling();
        while let Some(child) = childFrame {
            if child.IsTableColFrame() {
                return Some(nsTableColFrame(child));
            }
            childFrame = child.GetNextSibling();
        }
        None
    }

    /** return the number of the columns the col represents.  always >= 1 */
    // Gecko: layout/tables/nsTableColFrame.cpp nsTableColFrame::GetSpan
    // ADAPTER: `StyleTable()->mXSpan` is Gecko's internal `-x-span` property (the `<col span>`
    // presentation attribute), which is not in the servo configuration of Stylo. The frame constructor
    // gives the span with SetSpan(); the default is the property's initial value, 1.
    pub fn GetSpan(&self) -> i32 {
        self.0.GetProperty(ColSpanProperty()).unwrap_or(1)
    }

    // ADAPTER: sets the value GetSpan() returns (see there).
    pub fn SetSpan(&self, aSpan: i32) {
        self.0.SetProperty(ColSpanProperty(), aSpan);
    }

    /** convenience method, calls into cellmap */
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::Count
    // NOT PORTED: declared in the header but never defined in Gecko.

    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::GetIStartBorderWidth
    pub fn GetIStartBorderWidth(&self) -> nscoord {
        self.Members().mIStartBorderWidth
    }
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::GetIEndBorderWidth
    pub fn GetIEndBorderWidth(&self) -> nscoord {
        self.Members().mIEndBorderWidth
    }
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::SetIStartBorderWidth
    pub fn SetIStartBorderWidth(&self, aWidth: nscoord) {
        self.Members().mIStartBorderWidth = aWidth;
    }
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::SetIEndBorderWidth
    pub fn SetIEndBorderWidth(&self, aWidth: nscoord) {
        self.Members().mIEndBorderWidth = aWidth;
    }

    // Gecko: layout/tables/nsTableColFrame.cpp nsTableColFrame::Dump
    // NOT PORTED: debug dump.

    /**
     * Restore the default values of the intrinsic widths, so that we can
     * re-accumulate intrinsic widths from the cells in the column.
     */
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::ResetIntrinsics
    pub fn ResetIntrinsics(&self) {
        let m = self.Members();
        m.mMinCoord = 0;
        m.mPrefCoord = 0;
        m.mPrefPercent = 0.0;
        m.mHasSpecifiedCoord = false;
    }

    /**
     * Restore the default value of the preferred percentage width (the
     * only intrinsic width used by FixedTableLayoutStrategy.
     */
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::ResetPrefPercent
    pub fn ResetPrefPercent(&self) {
        self.Members().mPrefPercent = 0.0;
    }

    /**
     * Restore the default values of the temporary buffer for
     * spanning-cell intrinsic widths (as we process spanning cells).
     */
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::ResetSpanIntrinsics
    pub fn ResetSpanIntrinsics(&self) {
        let m = self.Members();
        m.mSpanMinCoord = 0;
        m.mSpanPrefCoord = 0;
        m.mSpanPrefPercent = 0.0;
    }

    /**
     * Add the widths for a cell or column element, or the contribution of
     * the widths from a column-spanning cell:
     * @param aMinCoord The minimum intrinsic width
     * @param aPrefCoord The preferred intrinsic width or, if there is a
     *   specified non-percentage width, max(specified width, minimum intrinsic
     *   width).
     * @param aHasSpecifiedCoord Whether there is a specified
     *   non-percentage width.
     *
     * Note that the implementation of this functions is a bit tricky
     * since mPrefCoord means different things depending on
     * whether mHasSpecifiedCoord is true (and likewise for aPrefCoord and
     * aHasSpecifiedCoord).  If mHasSpecifiedCoord is false, then
     * all widths added had aHasSpecifiedCoord false and mPrefCoord is the
     * largest of the pref widths.  But if mHasSpecifiedCoord is true,
     * then mPrefCoord is the largest of (1) the pref widths for cells
     * with aHasSpecifiedCoord true and (2) the min widths for cells with
     * aHasSpecifiedCoord false.
     */
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::AddCoords
    pub fn AddCoords(&self, aMinCoord: nscoord, mut aPrefCoord: nscoord, aHasSpecifiedCoord: bool) {
        NS_ASSERTION!(aMinCoord <= aPrefCoord, "intrinsic widths out of order");

        let m = self.Members();
        if aHasSpecifiedCoord && !m.mHasSpecifiedCoord {
            m.mPrefCoord = m.mMinCoord;
            m.mHasSpecifiedCoord = true;
        }
        if !aHasSpecifiedCoord && m.mHasSpecifiedCoord {
            aPrefCoord = aMinCoord; // NOTE: modifying argument
        }

        if aMinCoord > m.mMinCoord {
            m.mMinCoord = aMinCoord;
        }
        if aPrefCoord > m.mPrefCoord {
            m.mPrefCoord = aPrefCoord;
        }

        NS_ASSERTION!(m.mMinCoord <= m.mPrefCoord, "min larger than pref");
    }

    /**
     * Add a percentage width specified on a cell or column element or the
     * contribution to this column of a percentage width specified on a
     * column-spanning cell.
     */
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::AddPrefPercent
    pub fn AddPrefPercent(&self, aPrefPercent: f32) {
        let m = self.Members();
        if aPrefPercent > m.mPrefPercent {
            m.mPrefPercent = aPrefPercent;
        }
    }

    /**
     * Get the largest minimum intrinsic width for this column.
     */
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::GetMinCoord
    pub fn GetMinCoord(&self) -> nscoord {
        self.Members().mMinCoord
    }
    /**
     * Get the largest preferred width for this column, or, if there were
     * any specified non-percentage widths (see GetHasSpecifiedCoord), the
     * largest minimum intrinsic width or specified width.
     */
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::GetPrefCoord
    pub fn GetPrefCoord(&self) -> nscoord {
        self.Members().mPrefCoord
    }
    /**
     * Get whether there were any specified widths contributing to this
     * column.
     */
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::GetHasSpecifiedCoord
    pub fn GetHasSpecifiedCoord(&self) -> bool {
        self.Members().mHasSpecifiedCoord
    }

    /**
     * Get the largest specified percentage width contributing to this
     * column (returns 0 if there were none).
     */
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::GetPrefPercent
    pub fn GetPrefPercent(&self) -> f32 {
        self.Members().mPrefPercent
    }

    /**
     * Like AddCoords, but into a temporary buffer used for groups of
     * column-spanning cells.
     */
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::AddSpanCoords
    pub fn AddSpanCoords(&self, aSpanMinCoord: nscoord, mut aSpanPrefCoord: nscoord, aSpanHasSpecifiedCoord: bool) {
        NS_ASSERTION!(aSpanMinCoord <= aSpanPrefCoord, "intrinsic widths out of order");

        let m = self.Members();
        if !aSpanHasSpecifiedCoord && m.mHasSpecifiedCoord {
            aSpanPrefCoord = aSpanMinCoord; // NOTE: modifying argument
        }

        if aSpanMinCoord > m.mSpanMinCoord {
            m.mSpanMinCoord = aSpanMinCoord;
        }
        if aSpanPrefCoord > m.mSpanPrefCoord {
            m.mSpanPrefCoord = aSpanPrefCoord;
        }

        NS_ASSERTION!(m.mSpanMinCoord <= m.mSpanPrefCoord, "min larger than pref");
    }

    /*
     * Accumulate percentage widths on column spanning cells into
     * temporary variables.
     */
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::AddSpanPrefPercent
    pub fn AddSpanPrefPercent(&self, aSpanPrefPercent: f32) {
        let m = self.Members();
        if aSpanPrefPercent > m.mSpanPrefPercent {
            m.mSpanPrefPercent = aSpanPrefPercent;
        }
    }

    /*
     * Accumulate the temporary variables for column spanning cells into
     * the primary variables.
     */
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::AccumulateSpanIntrinsics
    pub fn AccumulateSpanIntrinsics(&self) {
        let m = self.Members();
        let (spanMin, spanPref, hasSpecified, spanPct) = (m.mSpanMinCoord, m.mSpanPrefCoord, m.mHasSpecifiedCoord, m.mSpanPrefPercent);
        self.AddCoords(spanMin, spanPref, hasSpecified);
        self.AddPrefPercent(spanPct);
    }

    // Used to adjust a column's pref percent so that the table's total
    // never exceeeds 100% (by only allowing percentages to be used,
    // starting at the first column, until they reach 100%).
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::AdjustPrefPercent
    pub fn AdjustPrefPercent(&self, aTableTotalPercent: &mut f32) {
        let m = self.Members();
        let allowed = 1.0f32 - *aTableTotalPercent;
        if m.mPrefPercent > allowed {
            m.mPrefPercent = allowed;
        }
        *aTableTotalPercent += m.mPrefPercent;
    }

    // The final width of the column.
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::ResetFinalISize
    pub fn ResetFinalISize(&self) {
        self.Members().mFinalISize = nscoord_MIN; // so we detect that it changed
    }
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::SetFinalISize
    pub fn SetFinalISize(&self, aFinalISize: nscoord) {
        self.Members().mFinalISize = aFinalISize;
    }
    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::GetFinalISize
    pub fn GetFinalISize(&self) -> nscoord {
        self.Members().mFinalISize
    }

    // Gecko: layout/tables/nsTableColFrame.h nsTableColFrame::InvalidateFrame, InvalidateFrameWithRect,
    // InvalidateFrameForRemoval and nsTableColFrame.cpp nsTableColFrame::InvalidateFrame,
    // InvalidateFrameWithRect
    // NOT PORTED: invalidation is painting.

    // nsTableColFrame::~nsTableColFrame
    // ADAPTER: frees the member block (see nsTableColFrame_Members); called when the frame is destroyed.
    pub fn Destroy(&self, aContext: &mut FrameDestroyContext) {
        self.nsSplittableFrame_Destroy(aContext);
        if let Some(p) = self.0.GetProperty(ColMembersProperty()) {
            drop(unsafe { Box::from_raw(p as *mut nsTableColFrame_Members) });
            self.0.RemoveProperty(ColMembersProperty());
        }
    }
}

// ADAPTER: `StyleVisibility()->mVisible == StyleVisibility::Collapse` (there is no nsStyleVisibility
// wrapper yet); also used by nsTableColGroupFrame.rs.
pub fn IsVisibilityCollapse(aFrame: nsIFrame) -> bool {
    use mrt_style::style::properties::longhands::visibility::computed_value::T as StyleVisibilityT;
    aFrame.Style().get_inherited_box().clone_visibility() == StyleVisibilityT::Collapse
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BlockReflowState_test_support::*;

    // A col frame with no parent (the accessors below do not need the table).
    fn col() -> nsTableColFrame {
        let f = frames(r#"<div id=c style="display:table-column"></div>"#, &[("c", None)], 10);
        NS_NewTableColFrame(f[0])
    }

    // Written from the member comments of nsTableColFrame.h: a new col has empty intrinsics and an
    // unset final inline-size.
    #[test]
    fn a_new_col_has_empty_intrinsics_and_unknown_final_isize() {
        let c = col();
        assert_eq!(c.GetMinCoord(), 0);
        assert_eq!(c.GetPrefCoord(), 0);
        assert_eq!(c.GetPrefPercent(), 0.0);
        assert!(!c.GetHasSpecifiedCoord());
        assert_eq!(c.GetFinalISize(), nscoord_MIN);
        assert_eq!(c.GetColType(), nsTableColType::eColContent);
        assert_eq!(c.GetSpan(), 1);
        assert_eq!(c.GetColIndex(), 0);
    }

    #[test]
    fn col_index_and_border_widths_are_stored() {
        let c = col();
        c.SetColIndex(7);
        c.SetIStartBorderWidth(3);
        c.SetIEndBorderWidth(4);
        assert_eq!(c.GetColIndex(), 7);
        assert_eq!(c.GetIStartBorderWidth(), 3);
        assert_eq!(c.GetIEndBorderWidth(), 4);
    }

    // AddCoords: largest min and largest pref win.
    #[test]
    fn add_coords_keeps_the_largest_min_and_pref() {
        let c = col();
        c.AddCoords(10, 30, false);
        c.AddCoords(20, 25, false);
        assert_eq!((c.GetMinCoord(), c.GetPrefCoord()), (20, 30));
        assert!(!c.GetHasSpecifiedCoord());
    }

    // The comment of AddCoords: once a specified width is seen, mPrefCoord is the largest of the pref
    // widths of the specified cells and the min widths of the others.
    #[test]
    fn add_coords_with_a_specified_width_ignores_pref_of_unspecified_cells() {
        let c = col();
        c.AddCoords(10, 100, false);
        c.AddCoords(5, 50, true);
        // mPrefCoord restarts from the min width (10) when the first specified width arrives.
        assert!(c.GetHasSpecifiedCoord());
        assert_eq!((c.GetMinCoord(), c.GetPrefCoord()), (10, 50));
        // an unspecified cell now contributes its min width only
        c.AddCoords(60, 500, false);
        assert_eq!((c.GetMinCoord(), c.GetPrefCoord()), (60, 60));
    }

    #[test]
    fn pref_percent_is_the_largest_and_can_be_reset() {
        let c = col();
        c.AddPrefPercent(0.25);
        c.AddPrefPercent(0.1);
        assert_eq!(c.GetPrefPercent(), 0.25);
        c.ResetPrefPercent();
        assert_eq!(c.GetPrefPercent(), 0.0);
    }

    // AdjustPrefPercent keeps the table's total at 100%, starting from the first column.
    #[test]
    fn adjust_pref_percent_limits_the_total_to_100_percent() {
        let a = col();
        let b = col();
        a.AddPrefPercent(0.75);
        b.AddPrefPercent(0.5);
        let mut total = 0.0f32;
        a.AdjustPrefPercent(&mut total);
        b.AdjustPrefPercent(&mut total);
        assert_eq!(a.GetPrefPercent(), 0.75);
        assert_eq!(b.GetPrefPercent(), 0.25);
        assert_eq!(total, 1.0);
    }

    // The spanning cells' contribution is collected apart and added by AccumulateSpanIntrinsics.
    #[test]
    fn span_intrinsics_are_accumulated_then_reset() {
        let c = col();
        c.AddCoords(10, 20, false);
        c.AddSpanCoords(15, 40, false);
        c.AddSpanPrefPercent(0.5);
        assert_eq!((c.GetMinCoord(), c.GetPrefCoord()), (10, 20));
        c.AccumulateSpanIntrinsics();
        c.ResetSpanIntrinsics();
        assert_eq!((c.GetMinCoord(), c.GetPrefCoord()), (15, 40));
        assert_eq!(c.GetPrefPercent(), 0.5);
        c.AccumulateSpanIntrinsics();
        assert_eq!((c.GetMinCoord(), c.GetPrefCoord()), (15, 40));
    }

    #[test]
    fn reset_intrinsics_clears_everything_but_the_final_isize() {
        let c = col();
        c.AddCoords(10, 20, true);
        c.AddPrefPercent(0.5);
        c.SetFinalISize(33);
        c.ResetIntrinsics();
        assert_eq!((c.GetMinCoord(), c.GetPrefCoord(), c.GetPrefPercent(), c.GetHasSpecifiedCoord()), (0, 0, 0.0, false));
        assert_eq!(c.GetFinalISize(), 33);
    }

    #[test]
    fn col_type_is_kept_in_the_state_bits() {
        let c = col();
        c.SetColType(nsTableColType::eColAnonymousCell);
        assert_eq!(c.GetColType(), nsTableColType::eColAnonymousCell);
        c.SetColType(nsTableColType::eColAnonymousColGroup);
        assert_eq!(c.GetColType(), nsTableColType::eColAnonymousColGroup);
    }
}
