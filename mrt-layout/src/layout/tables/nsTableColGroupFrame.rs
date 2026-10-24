// Translated from layout/tables/nsTableColGroupFrame.h and nsTableColGroupFrame.cpp (GECKO_REV).
//
// Rust shape (see CONVENTIONS.md and the top of nsBlockFrame.rs): `nsTableColGroupFrame` is a
// `Copy` handle over a frame of type TableColGroup that derefs to nsIFrame (the C++ class derives
// from nsContainerFrame, whose members are nsIFrame's here). The data members (mColCount,
// mStartColIndex) live in one heap block per frame behind a frame property. What is virtual in C++
// (SetInitialChildList, AppendFrames, InsertFrames, RemoveFrame, Reflow, DidSetComputedStyle,
// GetLogicalSkipSides, Init) is a `match` on the frame type in the adapter layer; this file has the
// nsTableColGroupFrame overrides under their own names.
//
// NOT PORTED: painting (BuildDisplayList), invalidation (InvalidateFrame, InvalidateFrameWithRect,
// InvalidateFrameForRemoval, tableFrame->InvalidateFrameSubtree()), frame dumps (GetFrameName, Dump),
// refcount logging.
use crate::*;
use mrt_style::style::properties::ComputedValues;
use mrt_style::style::servo_arc::Arc as ServoArc;
use std::ops::Deref;

// Gecko: layout/tables/nsTableColGroupFrame.cpp COLGROUP_SYNTHETIC_BIT (NS_FRAME_STATE_BIT(30))
const COLGROUP_SYNTHETIC_BIT: nsFrameState = 1 << 30;

// ADAPTER: the data members of the C++ class (see the top of the file).
pub struct nsTableColGroupFrame_Members {
    // Gecko: nsTableColGroupFrame::mColCount
    pub mColCount: i32,
    // the starting column index this col group represents. Must be >= 0.
    // Gecko: nsTableColGroupFrame::mStartColIndex
    pub mStartColIndex: i32,
}

// Gecko: layout/tables/nsTableColGroupFrame.cpp (the frame properties; ids 7550..7599 are this file's)
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(ColGroupMembersProperty, usize, 7550);
// ADAPTER: the value of `StyleTable()->mXSpan` (see GetSpan); set by the frame constructor.
NS_DECLARE_FRAME_PROPERTY_DELETABLE!(ColGroupSpanProperty, i32, 7551);

/**
 * nsTableColGroupFrame
 * data structure to maintain information about a single table cell's frame
 */
// Gecko: layout/tables/nsTableColGroupFrame.h nsTableColGroupFrame
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsTableColGroupFrame(pub nsIFrame);

impl Deref for nsTableColGroupFrame {
    type Target = nsIFrame;
    fn deref(&self) -> &nsIFrame {
        &self.0
    }
}

impl nsTableColGroupFrame {
    // ADAPTER: static_cast<nsTableColGroupFrame*>(frame)
    pub fn from_frame(aFrame: nsIFrame) -> nsTableColGroupFrame {
        nsTableColGroupFrame(aFrame)
    }

    // ADAPTER: do_QueryFrame<nsTableColGroupFrame>(frame): the frame is of type TableColGroup.
    // (NS_QUERYFRAME_HEAD(nsTableColGroupFrame): NS_QUERYFRAME_ENTRY(nsTableColGroupFrame))
    pub fn do_QueryFrame(aFrame: nsIFrame) -> Option<nsTableColGroupFrame> {
        if aFrame.Type() == LayoutFrameType::TableColGroup {
            Some(nsTableColGroupFrame(aFrame))
        } else {
            None
        }
    }

    // ADAPTER: this frame's data members (see nsTableColGroupFrame_Members). Created on first use
    // with the values of the constructor (`mColCount(0), mStartColIndex(0)`).
    fn Members(&self) -> &'static mut nsTableColGroupFrame_Members {
        let p = match self.0.GetProperty(ColGroupMembersProperty()) {
            Some(p) => p,
            None => {
                let p = Box::into_raw(Box::new(nsTableColGroupFrame_Members { mColCount: 0, mStartColIndex: 0 })) as usize;
                self.0.SetOwnedPointerProperty::<nsTableColGroupFrame_Members>(ColGroupMembersProperty(), p);
                p
            }
        };
        unsafe { &mut *(p as *mut nsTableColGroupFrame_Members) }
    }
}

/**
 * instantiate a new instance of nsTableRowFrame.
 *
 * @param aPresShell the pres shell for this frame
 *
 * @return           the frame that was created
 */
// Gecko: layout/tables/nsTableColGroupFrame.cpp NS_NewTableColGroupFrame
// ADAPTER: frames are made by whoever builds the frame tree (the frame constructor, a test) with
// FrameTree::AddFrame; this gives one the class.
pub fn NS_NewTableColGroupFrame(aFrame: nsIFrame) -> nsTableColGroupFrame {
    aFrame.SetType(LayoutFrameType::TableColGroup);
    let colGroup = nsTableColGroupFrame(aFrame);
    colGroup.Members();
    colGroup
}

// Gecko: layout/tables/nsTableColGroupFrame.cpp ResetColIndexOfGroup
fn ResetColIndexOfGroup(aGroup: nsTableColGroupFrame, aFirstColIndex: i32, aStartColFrame: Option<nsIFrame>, aCurIndex: &mut i32) {
    // reset the starting col index for the first cg only if we should reset
    // the whole colgroup (aStartColFrame defaults to nullptr) or if
    // aFirstColIndex is smaller than the existing starting col index
    if *aCurIndex != aFirstColIndex || *aCurIndex < aGroup.GetStartColumnIndex() || aStartColFrame.is_none() {
        aGroup.SetStartColumnIndex(*aCurIndex);
    }
    let mut colFrame = aStartColFrame;
    if colFrame.is_none() || *aCurIndex != aFirstColIndex {
        colFrame = aGroup.GetChildList(FrameChildListID::Principal).FirstChild();
    }
    while let Some(col) = colFrame {
        if col.IsTableColFrame() {
            nsTableColFrame::from_frame(col).SetColIndex(*aCurIndex);
            *aCurIndex += 1;
        }
        colFrame = col.GetNextSibling();
    }
}

impl nsTableColGroupFrame {
    // nsIFrame overrides
    // Gecko: layout/tables/nsTableColGroupFrame.h nsTableColGroupFrame::Init
    pub fn Init(&self, aContent: Option<nsIContent>, aParent: nsIFrame, aPrevInFlow: Option<nsIFrame>) {
        self.nsContainerFrame_Init(aContent, aParent, aPrevInFlow);
        if aPrevInFlow.is_none() {
            // mWritingMode = GetTableFrame()->GetWritingMode();
            let wm = self.GetTableFrame().GetWritingMode();
            WithFrameTree(|t| t.mFrames[self.0.0 as usize].mWritingMode = wm);
        }
    }

    // Gecko: layout/tables/nsTableColGroupFrame.h nsTableColGroupFrame::GetTableFrame
    pub fn GetTableFrame(&self) -> nsTableFrame {
        let parent = self.GetParent();
        MOZ_ASSERT!(parent.is_some() && parent.unwrap().IsTableFrame());
        MOZ_ASSERT!(parent.unwrap().GetPrevInFlow().is_none(), "Col group should always be in a first-in-flow table frame");
        nsTableFrame::from_frame(parent.unwrap())
    }

    // Gets the synthetic colgroup of our table.
    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::GetSyntheticColGroup
    pub fn GetSyntheticColGroup(&self) -> Option<nsTableColGroupFrame> {
        self.GetTableFrame().GetSyntheticColGroup()
    }

    // Gecko: layout/tables/nsTableColGroupFrame.h nsTableColGroupFrame::BuildDisplayList
    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::BuildDisplayList
    // NOT PORTED: painting. (Colgroup backgrounds are drawn by nsTableFrame.)

    /** A colgroup can be caused by three things:
     * 1) An element with table-column-group display
     * 2) An element with a table-column display without a
     *    table-column-group parent
     * 3) Cells that are not in a column (and hence get an anonymous
     *    column and colgroup).
     *
     * In practice, we don't need to differentiate between cases (1) and (2),
     * because they both correspond to table-column-group boxes in the spec and
     * hence have observably identical behavior.  Case three is flagged as a
     * synthetic colgroup, because it may need to have different behavior in some
     * cases.
     */
    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::IsSynthetic
    pub fn IsSynthetic(&self) -> bool {
        self.HasAnyStateBits(COLGROUP_SYNTHETIC_BIT)
    }

    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::SetIsSynthetic
    pub fn SetIsSynthetic(&self) {
        self.AddStateBits(COLGROUP_SYNTHETIC_BIT);
    }

    /** Real in this context are colgroups that come from an element
     * with table-column-group display or wrap around columns that
     * come from an element with table-column display. Colgroups
     * that are the result of wrapping cells in an anonymous
     * column and colgroup are not considered real here.
     * @param aTableFrame - the table parent of the colgroups
     * @return the last real colgroup
     */
    // Gecko: layout/tables/nsTableColGroupFrame.h nsTableColGroupFrame::GetLastRealColGroup
    // NOT PORTED: declared in the header but never defined in Gecko.

    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::ResetColIndices
    pub fn ResetColIndices(aFirstFrame: Option<nsIFrame>, aSyntheticColGroup: Option<nsTableColGroupFrame>, aFirstColIndex: i32, aStartColFrame: Option<nsIFrame>) {
        let mut colIndex = aFirstColIndex;
        let mut cur = aFirstFrame;
        while let Some(frame) = cur {
            if let Some(colGroupFrame) = nsTableColGroupFrame::do_QueryFrame(frame) {
                if colGroupFrame.IsSynthetic() {
                    MOZ_ASSERT!(Some(colGroupFrame) == aSyntheticColGroup);
                } else {
                    ResetColIndexOfGroup(colGroupFrame, aFirstColIndex, aStartColFrame, &mut colIndex);
                }
            }
            cur = frame.GetNextSibling();
        }
        if let Some(syntheticColGroup) = aSyntheticColGroup {
            ResetColIndexOfGroup(syntheticColGroup, aFirstColIndex, aStartColFrame, &mut colIndex);
        }
    }

    /** Add column frames to the table storages: colframe cache and cellmap
     * this doesn't change the mFrames of the colgroup frame.
     * @param aFirstColIndex - the index at which aFirstFrame should be inserted
     *                         into the colframe cache.
     * @param aResetSubsequentColIndices - the indices of the col frames
     *                                     after the insertion might need
     *                                     an update
     * @param aCols - an iterator that can be used to iterate over the col
     *                frames to be added.  Once this is done, the frames on the
     *                sbling chain of its .get() at that point will still need
     *                their col indices updated.
     * @result            - if there is no table frame or the table frame is not
     *                      the first in flow it will return an error
     */
    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::AddColsToTable
    pub fn AddColsToTable(&self, aFirstColIndex: i32, aResetSubsequentColIndices: bool, aCols: &nsFrameList_Slice) -> nsresult {
        let tableFrame = self.GetTableFrame();

        // tableFrame->InvalidateFrameSubtree();
        // NOT PORTED: invalidation is painting.

        // set the col indices of the col frames and and add col info to the table
        let mut colIndex = aFirstColIndex;

        // XXX: We cannot use range-based for loop because InsertCol() can destroy the
        // nsTableColFrame in the slice we're traversing! Need to check the validity
        // of *colIter.
        let mut colIter = aCols.begin();
        let colIterEnd = aCols.end();
        while colIter.operator_star().is_some() && colIter != colIterEnd {
            let colFrame = nsTableColFrame::from_frame(colIter.operator_star().unwrap());
            colFrame.SetColIndex(colIndex);
            self.Members().mColCount += 1;
            tableFrame.InsertCol(colFrame, colIndex);
            colIndex += 1;
            colIter.operator_inc();
        }

        while colIter.operator_star().is_some() {
            let colFrame = nsTableColFrame::from_frame(colIter.operator_star().unwrap());
            colFrame.SetColIndex(colIndex);
            colIndex += 1;
            colIter.operator_inc();
        }

        // We have already set the colindex for all the colframes in this
        // colgroup that come after the first inserted colframe, but there could
        // be other colgroups following this one and their colframes need
        // correct colindices too.
        if aResetSubsequentColIndices && !self.IsSynthetic() {
            nsTableColGroupFrame::ResetColIndices(self.GetNextSibling(), self.GetSyntheticColGroup(), colIndex, None);
        }

        NS_OK
    }

    // don't set mColCount here, it is done in AddColsToTable
    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::SetInitialChildList
    pub fn SetInitialChildList(&self, aListID: FrameChildListID, aChildList: nsFrameList) {
        MOZ_ASSERT!(self.mFrames().IsEmpty(), "unexpected second call to SetInitialChildList");
        MOZ_ASSERT!(aListID == FrameChildListID::Principal, "unexpected child list");
        #[cfg(debug_assertions)]
        {
            for f in &aChildList {
                MOZ_ASSERT!(f.GetParent() == Some(self.0), "Unexpected parent");
            }
        }
        if aChildList.IsEmpty() {
            self.GetTableFrame().AppendAnonymousColFrames__nsTableColGroupFrame_int32_t_nsTableColType_bool(*self, self.GetSpan(), nsTableColType::eColAnonymousColGroup, false);
            return;
        }

        self.mFrames().AppendFrames(Some(self.0), aChildList);
    }

    /* virtual */
    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::DidSetComputedStyle
    // ADAPTER: the old style is the Stylo style (the frame's `Style()` is already the new one).
    pub fn DidSetComputedStyle(&self, aOldComputedStyle: Option<ServoArc<ComputedValues>>) {
        self.nsContainerFrame_DidSetComputedStyle(None);

        let Some(aOldComputedStyle) = aOldComputedStyle else {
            // avoid this on init
            return;
        };

        let tableFrame = self.GetTableFrame();
        if tableFrame.IsBorderCollapse() && tableFrame.BCRecalcNeeded(&aOldComputedStyle, &self.Style()) {
            let colCount = self.GetColCount();
            if colCount == 0 {
                return; // this is a degenerated colgroup
            }
            let damageArea = TableArea::new(self.GetFirstColumn().unwrap().GetColIndex(), 0, colCount, tableFrame.GetRowCount());
            tableFrame.AddBCDamageArea(&damageArea);
        }
    }

    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::AppendFrames
    pub fn AppendFrames(&self, aListID: FrameChildListID, aFrameList: nsFrameList) {
        NS_ASSERTION!(aListID == FrameChildListID::Principal, "unexpected child list");

        let mut col = self.GetFirstColumn();
        let mut nextCol: Option<nsTableColFrame>;
        while col.is_some() && col.unwrap().GetColType() == nsTableColType::eColAnonymousColGroup {
            // this colgroup spans one or more columns but now that there is a
            // real column below, spanned anonymous columns should be removed,
            // since the HTML spec says to ignore the span of a colgroup if it
            // has content columns in it.
            nextCol = col.unwrap().GetNextCol();
            let mut context = FrameDestroyContext::default();
            self.RemoveFrame(&mut context, FrameChildListID::Principal, col.unwrap().0);
            col = nextCol;
        }

        // Our next colframe should be an eColContent.  We've removed all the
        // eColAnonymousColGroup colframes, eColAnonymousCol colframes always follow
        // eColContent ones, and eColAnonymousCell colframes only appear in a
        // synthetic colgroup, which never gets AppendFrames() called on it.
        MOZ_ASSERT!(col.is_none() || col.unwrap().GetColType() == nsTableColType::eColContent, "What's going on with our columns?");

        let newFrames = self.mFrames().AppendFrames(Some(self.0), aFrameList);
        self.InsertColsReflow(self.GetStartColumnIndex() + self.Members().mColCount, &newFrames);
    }

    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::InsertFrames
    pub fn InsertFrames(&self, aListID: FrameChildListID, mut aPrevFrame: Option<nsIFrame>, aPrevFrameLine: Option<&nsLineList_iterator>, aFrameList: nsFrameList) {
        NS_ASSERTION!(aListID == FrameChildListID::Principal, "unexpected child list");
        NS_ASSERTION!(aPrevFrame.is_none() || aPrevFrame.unwrap().GetParent() == Some(self.0), "inserting after sibling frame with different parent");

        let mut col = self.GetFirstColumn();
        let mut nextCol: Option<nsTableColFrame>;
        while col.is_some() && col.unwrap().GetColType() == nsTableColType::eColAnonymousColGroup {
            // this colgroup spans one or more columns but now that there is a
            // real column below, spanned anonymous columns should be removed,
            // since the HTML spec says to ignore the span of a colgroup if it
            // has content columns in it.
            nextCol = col.unwrap().GetNextCol();
            if Some(col.unwrap().0) == aPrevFrame {
                // This can happen when we're being appended to
                NS_ASSERTION!(nextCol.is_none() || nextCol.unwrap().GetColType() != nsTableColType::eColAnonymousColGroup, "Inserting in the middle of our anonymous cols?");
                // We'll want to insert at the beginning
                aPrevFrame = None;
            }
            let mut context = FrameDestroyContext::default();
            self.RemoveFrame(&mut context, FrameChildListID::Principal, col.unwrap().0);
            col = nextCol;
        }

        // Our next colframe should be an eColContent.  We've removed all the
        // eColAnonymousColGroup colframes, eColAnonymousCol colframes always follow
        // eColContent ones, and eColAnonymousCell colframes only appear in a
        // synthetic colgroup, which never gets InsertFrames() called on it.
        MOZ_ASSERT!(col.is_none() || col.unwrap().GetColType() == nsTableColType::eColContent, "What's going on with our columns?");

        // (LastContinuation() is LastInFlow() for the frames that are not text frames; this
        // crate has LastContinuation only on nsTextFrame.)
        NS_ASSERTION!(aPrevFrame.is_none() || aPrevFrame.unwrap() == aPrevFrame.unwrap().LastInFlow(), "Prev frame should be last in continuation chain");
        NS_ASSERTION!(
            aPrevFrame.is_none() || self.GetNextColumn(aPrevFrame).is_none() || self.GetNextColumn(aPrevFrame).unwrap().GetColType() != nsTableColType::eColAnonymousCol,
            "Shouldn't be inserting before a spanned colframe"
        );

        let newFrames = self.mFrames().InsertFrames(Some(self.0), aPrevFrame, aFrameList);
        let prevFrame = nsTableFrame::GetFrameAtOrBefore(self.0, aPrevFrame, LayoutFrameType::TableCol);

        let colIndex = match prevFrame {
            Some(prev) => nsTableColFrame::from_frame(prev).GetColIndex() + 1,
            None => self.GetStartColumnIndex(),
        };
        self.InsertColsReflow(colIndex, &newFrames);
    }

    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::InsertColsReflow
    fn InsertColsReflow(&self, aColIndex: i32, aCols: &nsFrameList_Slice) {
        self.AddColsToTable(aColIndex, true, aCols);

        PresShell_FrameNeedsReflow(self.0, IntrinsicDirty::FrameAndAncestors, NS_FRAME_HAS_DIRTY_CHILDREN);
    }

    /** remove the column aChild from the column group, if requested renumber
     * the subsequent columns in this column group and all following column
     * groups. see also ResetColIndices for this
     * @param aChild       - the column frame that needs to be removed
     * @param aResetSubsequentColIndices - if true the columns that follow
     *                                     after aChild will be reenumerated
     */
    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::RemoveChild
    pub fn RemoveChild(&self, aContext: &mut FrameDestroyContext, aChild: nsTableColFrame, aResetSubsequentColIndices: bool) {
        let mut colIndex = 0;
        let mut nextChild: Option<nsIFrame> = None;
        if aResetSubsequentColIndices {
            colIndex = aChild.GetColIndex();
            nextChild = aChild.GetNextSibling();
        }
        self.mFrames().DestroyFrame(aContext, aChild.0);
        self.Members().mColCount -= 1;
        if aResetSubsequentColIndices {
            if nextChild.is_some() {
                // reset inside this and all following colgroups
                nsTableColGroupFrame::ResetColIndices(Some(self.0), self.GetSyntheticColGroup(), colIndex, nextChild);
            } else if !self.IsSynthetic() {
                nsTableColGroupFrame::ResetColIndices(self.GetNextSibling(), self.GetSyntheticColGroup(), colIndex, None);
            }
        }

        PresShell_FrameNeedsReflow(self.0, IntrinsicDirty::FrameAndAncestors, NS_FRAME_HAS_DIRTY_CHILDREN);
    }

    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::RemoveFrame
    pub fn RemoveFrame(&self, aContext: &mut FrameDestroyContext, aListID: FrameChildListID, aOldFrame: nsIFrame) {
        NS_ASSERTION!(aListID == FrameChildListID::Principal, "unexpected child list");

        // if (!aOldFrame) { return; }
        // (aOldFrame cannot be null here: it is a handle.)
        let mut contentRemoval = false;

        if aOldFrame.IsTableColFrame() {
            let colFrame = nsTableColFrame::from_frame(aOldFrame);
            if colFrame.GetColType() == nsTableColType::eColContent {
                contentRemoval = true;
                // Remove any anonymous column frames this <col> produced via a colspan
                let mut col = colFrame.GetNextCol();
                let mut nextCol: Option<nsTableColFrame>;
                while col.is_some() && col.unwrap().GetColType() == nsTableColType::eColAnonymousCol {
                    nextCol = col.unwrap().GetNextCol();
                    self.RemoveFrame(aContext, FrameChildListID::Principal, col.unwrap().0);
                    col = nextCol;
                }
            }

            let colIndex = colFrame.GetColIndex();
            // The RemoveChild call handles calling FrameNeedsReflow on us.
            self.RemoveChild(aContext, colFrame, true);

            let tableFrame = self.GetTableFrame();
            tableFrame.RemoveCol(colIndex, true, true);
            if self.mFrames().IsEmpty() && contentRemoval && !self.IsSynthetic() {
                tableFrame.AppendAnonymousColFrames__nsTableColGroupFrame_int32_t_nsTableColType_bool(*self, self.GetSpan(), nsTableColType::eColAnonymousColGroup, true);
            }
        } else {
            self.mFrames().DestroyFrame(aContext, aOldFrame);
        }
    }

    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::GetLogicalSkipSides
    pub fn GetLogicalSkipSides(&self) -> LogicalSides {
        let mWritingMode = self.GetWritingMode();
        let mut skip = LogicalSides::new__WritingMode(mWritingMode);
        if self.StyleBorder().mBoxDecorationBreak() == StyleBoxDecorationBreak::Clone {
            return skip;
        }

        if self.GetPrevInFlow().is_some() {
            skip += LogicalSide::BStart;
        }
        if self.GetNextInFlow().is_some() {
            skip += LogicalSide::BEnd;
        }
        skip
    }

    /** reflow of a column group is a trivial matter of reflowing
     * the col group's children (columns), and setting this frame
     * to 0-size.  Since tables are row-centric, column group frames
     * don't play directly in the rendering game.  They do however
     * maintain important state that effects table and cell layout.
     */
    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::Reflow
    pub fn Reflow(&self, aPresContext: &nsPresContext, aDesiredSize: &mut ReflowOutput, aReflowInput: &ReflowInput, aStatus: &mut nsReflowStatus) {
        self.MarkInReflow();
        // DO_GLOBAL_REFLOW_COUNT("nsTableColGroupFrame");
        // NOT PORTED: reflow counting.
        MOZ_ASSERT!(aStatus.IsEmpty(), "Caller should pass a fresh reflow status!");
        NS_ASSERTION!(self.GetContent().is_some(), "bad state -- null content for frame");
        if IsVisibilityCollapse(self.0) {
            self.GetTableFrame().SetNeedToCollapse(true);
        }

        let wm = self.GetWritingMode();
        let mut kidFrameIter = self.mFrames().FirstChild();
        while let Some(kidFrame) = kidFrameIter {
            // Give the child frame a chance to reflow, even though we know it'll have 0
            // size
            let mut kidSize = ReflowOutput::new__ReflowInput(aReflowInput);
            let kidReflowInput = ReflowInput::new__nsPresContext_ReflowInput_nsIFrame_LogicalSize_Maybe_InitFlags_StyleSizeOverrides_ComputeSizeFlags_AnchorPosResolutionCache(
                aPresContext,
                aReflowInput,
                kidFrame,
                LogicalSize::new__WritingMode(kidFrame.GetWritingMode()),
                None,
                EnumSet::default(),
                StyleSizeOverrides::default(),
                ComputeSizeFlags::default(),
                std::ptr::null_mut(),
            );
            let dummyPos = LogicalPoint::new__WritingMode(wm);
            let dummyContainerSize = nsSize::new(0, 0);
            let mut status = nsReflowStatus::new();
            nsContainerFrame::from_frame(self.0).ReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags_nsReflowStatus_nsOverflowContinuationTracker(
                kidFrame,
                aPresContext,
                &mut kidSize,
                &kidReflowInput,
                wm,
                dummyPos,
                dummyContainerSize,
                nsIFrame_ReflowChildFlags::Default,
                &mut status,
                None,
            );
            nsContainerFrame::FinishReflowChild__nsIFrame_nsPresContext_ReflowOutput_ReflowInput_WritingMode_LogicalPoint_nsSize_ReflowChildFlags(
                kidFrame,
                aPresContext,
                &kidSize,
                Some(&kidReflowInput),
                wm,
                dummyPos,
                dummyContainerSize,
                nsIFrame_ReflowChildFlags::Default,
            );
            kidFrameIter = kidFrame.GetNextSibling();
        }

        aDesiredSize.ClearSize();
    }

    /** first column on the child list */
    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::GetFirstColumn
    pub fn GetFirstColumn(&self) -> Option<nsTableColFrame> {
        self.GetNextColumn(None)
    }

    /** next sibling to aChildFrame that is a column frame, first column frame
     * in the column group if aChildFrame is null
     */
    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::GetNextColumn
    pub fn GetNextColumn(&self, aChildFrame: Option<nsIFrame>) -> Option<nsTableColFrame> {
        let mut result: Option<nsTableColFrame> = None;
        let mut childFrame = aChildFrame;
        if childFrame.is_none() {
            childFrame = self.mFrames().FirstChild();
        } else {
            childFrame = childFrame.unwrap().GetNextSibling();
        }
        while let Some(child) = childFrame {
            if StyleDisplay::TableColumn == child.StyleDisplay().mDisplay() {
                result = Some(nsTableColFrame::from_frame(child));
                break;
            }
            childFrame = child.GetNextSibling();
        }
        result
    }

    /** helper method to get the span attribute for this colgroup */
    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::GetSpan
    // ADAPTER: `StyleTable()->mXSpan` is Gecko's internal `-x-span` property (the `<colgroup span>`
    // presentation attribute), which is not in the servo configuration of Stylo. The frame constructor
    // gives the span with SetSpan(); the default is the property's initial value, 1.
    pub fn GetSpan(&self) -> i32 {
        self.0.GetProperty(ColGroupSpanProperty()).unwrap_or(1)
    }

    // ADAPTER: sets the value GetSpan() returns (see there).
    pub fn SetSpan(&self, aSpan: i32) {
        self.0.SetProperty(ColGroupSpanProperty(), aSpan);
    }

    // Gecko: layout/tables/nsTableColGroupFrame.h nsTableColGroupFrame::GetStartColumnIndex
    // (a plain getter, defined inline in the header)
    /** @return - the position of the first column in this colgroup in the table
     * colframe cache.
     */
    pub fn GetStartColumnIndex(&self) -> i32 {
        self.Members().mStartColIndex
    }

    /** set the position of the first column in this colgroup in the table
     * colframe cache.
     */
    // Gecko: layout/tables/nsTableColGroupFrame.h nsTableColGroupFrame::SetStartColumnIndex
    pub fn SetStartColumnIndex(&self, aIndex: i32) {
        self.Members().mStartColIndex = aIndex;
    }

    /** returns the number of columns represented by this group.
     * if there are col children, count them (taking into account the span of
     * each) else, check my own span attribute.
     */
    // Gecko: layout/tables/nsTableColGroupFrame.h nsTableColGroupFrame::GetColCount
    pub fn GetColCount(&self) -> i32 {
        self.Members().mColCount
    }

    /** provide access to the mFrames list
     */
    // Gecko: layout/tables/nsTableColGroupFrame.h nsTableColGroupFrame::GetWritableChildList
    pub fn GetWritableChildList(&self) -> &'static mut nsFrameList {
        self.mFrames().as_static_mut()
    }

    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::InvalidateFrame,
    // InvalidateFrameWithRect and nsTableColGroupFrame.h InvalidateFrameForRemoval
    // NOT PORTED: invalidation is painting.

    // Gecko: layout/tables/nsTableColGroupFrame.cpp nsTableColGroupFrame::Dump
    // NOT PORTED: debug dump.

    // ADAPTER: frees the member block (see nsTableColGroupFrame_Members); called when the frame is destroyed.
    pub fn Destroy(&self, aContext: &mut FrameDestroyContext) {
        self.nsContainerFrame_Destroy(aContext);
        if let Some(p) = self.0.GetProperty(ColGroupMembersProperty()) {
            drop(unsafe { Box::from_raw(p as *mut nsTableColGroupFrame_Members) });
            self.0.RemoveProperty(ColGroupMembersProperty());
        }
    }
}

// ---------------------------------------------------------------------------------------------
// TEMP stub for B: nothing (the colgroup calls only nsTableFrame and nsTableColFrame).
// ---------------------------------------------------------------------------------------------
