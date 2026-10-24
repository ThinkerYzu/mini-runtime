// Translated from layout/generic/nsLineBox.h and nsLineBox.cpp (GECKO_REV).
/* representation of one line within a block frame, a CSS line box */
//
// Notes on the Rust shape (see CONVENTIONS.md):
//  - `nsLineBox*` is `*mut nsLineBox` (Gecko shares and replaces these pointers, and the lines are
//    linked into an intrusive list). `nsLineBox` is allocated with `Box` by `NS_NewLineBox` (the
//    returned pointer is the Box's) and freed by `Destroy()`. `nsIFrame*` is `Option<nsIFrame>`.
//  - Const-ness (accessor scheme of WritingModes.rs): the const overload keeps the plain name and the
//    non-const one gets `_mut`. So `nsLineList::begin()` returns a const_iterator (a read-only
//    view: `*it` is `&nsLineBox`) and `begin_mut()` returns the `iterator` that mutating code
//    uses, likewise `end`, `rbegin`, `rend`, `front` and `back`. Iterators dereference to `nsLineBox`
//    (`it.IsDirty()` is `it->IsDirty()`), and `it.get()` is `it.get()`.
//  - The overloads of `begin`/`rbegin` that take a line are `begin_mut__nsLineBox` and
//    `rbegin_mut__nsLineBox`; `splice` is `splice__iterator_nsLineList`,
//    `splice__iterator_nsLineList_iterator` and `splice__iterator_nsLineList_iterator_iterator`.
//  - The C++ unions (`mFrames`/`mChildCount`, `mAllFlags`/`mFlags`, `mData`/`mBlockData`/`mInlineData`)
//    are separate fields; the discriminating flag decides which one is meaningful, as in C++.
//    `mData` is an enum over the two kinds of extra data, with accessors named like the members.
//  - NOT PORTED (whole file): arena allocation through PresShell (the PresShell arguments of
//    NS_NewLineBox and Destroy are dropped), the DEBUG_FRAME_DUMP code (List, UsedClearToString,
//    ListFloats, LastChild), nsWindowSizes (AddSizeOfExcludingThis), and refcount logging
//    (MOZ_COUNT_CTOR / MOZ_COUNT_DTOR).
use crate::*;
use std::collections::HashSet;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

// NOT PORTED: includes (nsILineIterator.h comes through lib.rs, nsTHashSet.h is std::collections::HashSet).

// class nsLineBox; class nsWindowSizes; namespace mozilla { class PresShell; }

// Gecko: layout/generic/nsIFrame.h LineListIterator
// (`using LineListIterator = GenericLineListIterator<nsLineLink, false>;` is declared in nsIFrame.h)
pub type LineListIterator = GenericLineListIterator<nsLineLink, false>;

// Gecko: layout/generic/nsLineBox.cpp ctorCount (static int32_t, DEBUG only)
#[cfg(debug_assertions)]
static ctorCount: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

// NOT PORTED: `const uint32_t nsLineBox::kMinChildCountForHashtable;` (the out-of-class
// definition of a static const member; the Rust associated const needs none).

/**
 * Function to create a line box and initialize it with a single frame.
 * The allocation is infallible.
 * If the frame was moved from another line then you're responsible
 * for notifying that line using NoteFrameRemoved().  Alternatively,
 * it's better to use the next function that does that for you in an
 * optimal way.
 */
// Gecko: layout/generic/nsLineBox.cpp NS_NewLineBox(PresShell*, nsIFrame*, bool)
// NOT PORTED: the PresShell argument (arena allocation).
pub fn NS_NewLineBox__nsIFrame_bool(aFrame: Option<nsIFrame>, aIsBlock: bool) -> *mut nsLineBox {
    Box::into_raw(Box::new(nsLineBox::new(aFrame, 1, aIsBlock)))
}

/**
 * Function to create a line box and initialize it with aCount frames
 * that are currently on aFromLine.  The allocation is infallible.
 */
// Gecko: layout/generic/nsLineBox.cpp NS_NewLineBox(PresShell*, nsLineBox*, nsIFrame*, int32_t)
// NOT PORTED: the PresShell argument (arena allocation).
pub fn NS_NewLineBox__nsLineBox_nsIFrame_int32_t(aFromLine: *mut nsLineBox, aFrame: Option<nsIFrame>, aCount: i32) -> *mut nsLineBox {
    let newLine = Box::into_raw(Box::new(nsLineBox::new(aFrame, aCount, false)));
    unsafe {
        (*newLine).NoteFramesMovedFrom(aFromLine);
        (*newLine).mContainerSize = (*aFromLine).mContainerSize;
    }
    newLine
}

/**
 * Users must have the class that is to be part of the list inherit
 * from nsLineLink.  If they want to be efficient, it should be the
 * first base class.  (This was originally nsCLink in a templatized
 * nsCList, but it's still useful separately.)
 */

// class nsLineList;

// Gecko: layout/generic/nsLineBox.h nsLineLink
// (`#[repr(C)]` and the first field of nsLineBox, so that `static_cast<nsLineBox*>(link)` is a pointer cast)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct nsLineLink {
    // template <typename Link, bool> friend class GenericLineListIterator; friend class nsLineList;
    // (the fields are private to this file, which holds all of those classes)
    _mNext: *mut nsLineLink, // or head
    _mPrev: *mut nsLineLink, // or tail
}

impl Default for nsLineLink {
    fn default() -> nsLineLink {
        nsLineLink { _mNext: std::ptr::null_mut(), _mPrev: std::ptr::null_mut() }
    }
}

/**
 * The nsLineBox class represents a horizontal line of frames. It contains
 * enough state to support incremental reflow of the frames, event handling
 * for the frames, and rendering of the frames.
 */
// Gecko: layout/generic/nsLineBox.h nsLineBox
// (class nsLineBox final : public nsLineLink; the base is the first field and nsLineBox derefs to it)
#[repr(C)]
pub struct nsLineBox {
    mLineLink: nsLineLink,

    // public:
    pub mFirstChild: Option<nsIFrame>,

    pub mWritingMode: WritingMode,

    // Physical size. Use only for physical <-> logical coordinate conversion.
    pub mContainerSize: nsSize,

    // private:
    mBounds: LogicalRect,

    // mFlags.mHasHashedFrames says which one to use
    // union {
    //   nsTHashSet<nsIFrame*>* mFrames;
    //   uint32_t mChildCount;
    // };
    mFrames: Option<Box<HashSet<nsIFrame>>>,
    mChildCount: u32,

    // protected:
    mAscent: nscoord, // see |SetAscent| / |GetAscent|
    // union { uint32_t mAllFlags; FlagBits mFlags; };
    // (the static_assert on the size of FlagBits is NOT PORTED; mAllFlags(0) is FlagBits::default())
    mFlags: nsLineBox_FlagBits,
    // union { ExtraData* mData; ExtraBlockData* mBlockData; ExtraInlineData* mInlineData; };
    mData: Option<Box<nsLineBox_Data>>,
}

impl Deref for nsLineBox {
    type Target = nsLineLink;
    fn deref(&self) -> &nsLineLink {
        &self.mLineLink
    }
}
impl DerefMut for nsLineBox {
    fn deref_mut(&mut self) -> &mut nsLineLink {
        &mut self.mLineLink
    }
}

// Gecko: layout/generic/nsLineBox.h nsLineBox::FlagBits
#[derive(Clone, Copy, Debug)]
pub struct nsLineBox_FlagBits {
    pub mDirty: bool,
    pub mPreviousMarginDirty: bool,
    pub mHasClearance: bool,
    pub mBlock: bool,
    pub mImpactedByFloat: bool,
    pub mLineWrapped: bool,
    pub mInvalidateTextRuns: bool,
    // default 0 = means that the opt potentially applies to this line.
    // 1 = never skip reflowing this line for a resize reflow
    pub mResizeReflowOptimizationDisabled: bool,
    pub mEmptyCacheValid: bool,
    pub mEmptyCacheState: bool,
    // mHasMarker indicates that this is an inline line whose block's
    // ::marker is adjacent to this line and non-empty.
    pub mHasMarker: bool,
    // Indicates that this line *may* have a placeholder for a float
    // that was pushed to a later column or page.
    pub mHadFloatPushed: bool,
    pub mHasHashedFrames: bool,
    // Indicates that this line is the one identified by an ancestor block
    // with -webkit-line-clamp on its legacy flex container, and that subsequent
    // lines under that block are "clamped" away, and therefore we need to
    // render a 'text-overflow: ellipsis'-like marker in this line.  At most one
    // line in the set of lines found by LineClampLineIterator for a given
    // block will have this flag set.
    pub mHasLineClampEllipsis: bool,
    // Has this line moved to a different fragment of the block since
    // the last time it was reflowed?
    pub mMovedFragments: bool,
    // mHasForcedLineBreakAfter indicates that this *inline* line has a
    // break-after status due to a float clearance or ending with <br>. A block
    // line shouldn't set this bit.
    //
    // Note: This bit is unrelated to CSS break-after property because it is all
    // about line break-after for inline-level boxes.
    pub mHasForcedLineBreakAfter: bool,
    // mFloatClearType indicates that there's a float clearance before a block
    // line, or after an inline line.
    pub mFloatClearType: UsedClear,
}

// mAllFlags(0)
impl Default for nsLineBox_FlagBits {
    fn default() -> nsLineBox_FlagBits {
        nsLineBox_FlagBits {
            mDirty: false,
            mPreviousMarginDirty: false,
            mHasClearance: false,
            mBlock: false,
            mImpactedByFloat: false,
            mLineWrapped: false,
            mInvalidateTextRuns: false,
            mResizeReflowOptimizationDisabled: false,
            mEmptyCacheValid: false,
            mEmptyCacheState: false,
            mHasMarker: false,
            mHadFloatPushed: false,
            mHasHashedFrames: false,
            mHasLineClampEllipsis: false,
            mMovedFragments: false,
            mHasForcedLineBreakAfter: false,
            mFloatClearType: UsedClear::None,
        }
    }
}

// Gecko: layout/generic/nsLineBox.h nsLineBox::ExtraData
pub struct nsLineBox_ExtraData {
    pub mOverflowAreas: OverflowAreas,
    // Union of the margin-boxes of our in-flow children (only children,
    // *not* their descendants). This is part of a special contribution to
    // the scrollable overflow of a scrolled block; as such, this is only
    // emplaced if our block is a scrolled frame (and we have in-flow children,
    // and floats, which are considered in-flow for scrollable overflow).
    pub mInFlowChildBounds: Option<nsRect>,
}

impl nsLineBox_ExtraData {
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ExtraData::ExtraData
    pub fn new(aBounds: nsRect) -> nsLineBox_ExtraData {
        nsLineBox_ExtraData { mOverflowAreas: OverflowAreas::new(aBounds, aBounds), mInFlowChildBounds: None }
    }
}

// Gecko: layout/generic/nsLineBox.h nsLineBox::ExtraBlockData (struct ExtraBlockData : public ExtraData)
pub struct nsLineBox_ExtraBlockData {
    mExtraData: nsLineBox_ExtraData,
    pub mCarriedOutBEndMargin: CollapsingMargin,
}

impl nsLineBox_ExtraBlockData {
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ExtraBlockData::ExtraBlockData
    pub fn new(aBounds: nsRect) -> nsLineBox_ExtraBlockData {
        nsLineBox_ExtraBlockData { mExtraData: nsLineBox_ExtraData::new(aBounds), mCarriedOutBEndMargin: CollapsingMargin::default() }
    }
}
impl Deref for nsLineBox_ExtraBlockData {
    type Target = nsLineBox_ExtraData;
    fn deref(&self) -> &nsLineBox_ExtraData {
        &self.mExtraData
    }
}
impl DerefMut for nsLineBox_ExtraBlockData {
    fn deref_mut(&mut self) -> &mut nsLineBox_ExtraData {
        &mut self.mExtraData
    }
}

// Gecko: layout/generic/nsLineBox.h nsLineBox::ExtraInlineData (struct ExtraInlineData : public ExtraData)
pub struct nsLineBox_ExtraInlineData {
    mExtraData: nsLineBox_ExtraData,
    pub mFloatEdgeIStart: nscoord,
    pub mFloatEdgeIEnd: nscoord,
    pub mFloats: Vec<nsIFrame>,
}

impl nsLineBox_ExtraInlineData {
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ExtraInlineData::ExtraInlineData
    pub fn new(aBounds: nsRect) -> nsLineBox_ExtraInlineData {
        nsLineBox_ExtraInlineData { mExtraData: nsLineBox_ExtraData::new(aBounds), mFloatEdgeIStart: nscoord_MIN, mFloatEdgeIEnd: nscoord_MIN, mFloats: Vec::new() }
    }
}
impl Deref for nsLineBox_ExtraInlineData {
    type Target = nsLineBox_ExtraData;
    fn deref(&self) -> &nsLineBox_ExtraData {
        &self.mExtraData
    }
}
impl DerefMut for nsLineBox_ExtraInlineData {
    fn deref_mut(&mut self) -> &mut nsLineBox_ExtraData {
        &mut self.mExtraData
    }
}

// ADAPTER: the union of `ExtraData* mData`, `ExtraBlockData* mBlockData` and `ExtraInlineData*
// mInlineData`. Which one is stored follows IsBlock(), as in C++. `mData->x` is `Data().x`;
// `mBlockData->x` and `mInlineData->x` are `BlockData().x` and `InlineData().x`.
pub enum nsLineBox_Data {
    Block(nsLineBox_ExtraBlockData),
    Inline(nsLineBox_ExtraInlineData),
}

impl Deref for nsLineBox_Data {
    type Target = nsLineBox_ExtraData;
    fn deref(&self) -> &nsLineBox_ExtraData {
        match self {
            nsLineBox_Data::Block(b) => b,
            nsLineBox_Data::Inline(i) => i,
        }
    }
}
impl DerefMut for nsLineBox_Data {
    fn deref_mut(&mut self) -> &mut nsLineBox_ExtraData {
        match self {
            nsLineBox_Data::Block(b) => b,
            nsLineBox_Data::Inline(i) => i,
        }
    }
}

impl nsLineBox {
    // Add a hash table for fast lookup when the line has more frames than this.
    // Gecko: layout/generic/nsLineBox.h nsLineBox::kMinChildCountForHashtable
    const kMinChildCountForHashtable: u32 = 200;

    // PLDHashTable::kDefaultInitialLength
    const kDefaultInitialLength: u32 = 4;

    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::nsLineBox
    fn new(aFrame: Option<nsIFrame>, aCount: i32, aIsBlock: bool) -> nsLineBox {
        let mut this = nsLineBox {
            mLineLink: nsLineLink::default(),
            mFirstChild: aFrame,
            mWritingMode: WritingMode::default(),
            mContainerSize: nsSize::new(-1, -1),
            mBounds: LogicalRect::new__WritingMode(WritingMode::default()), // mBounds will be initialized with the correct
            // writing mode when it is set
            mFrames: None,
            mChildCount: 0,
            mAscent: 0,
            mFlags: nsLineBox_FlagBits::default(),
            mData: None,
        };
        // Assert that the union elements chosen for initialisation are at
        // least as large as all other elements in their respective unions, so
        // as to ensure that no parts are missed.
        // static_assert(sizeof(mFrames) >= sizeof(mChildCount), "nsLineBox init #1");
        // static_assert(sizeof(mAllFlags) >= sizeof(mFlags), "nsLineBox init #2");
        // static_assert(sizeof(mData) >= sizeof(mBlockData), "nsLineBox init #3");
        // static_assert(sizeof(mData) >= sizeof(mInlineData), "nsLineBox init #4");
        // NOT PORTED: the static_asserts (the unions are separate fields here).

        // MOZ_COUNT_CTOR(nsLineBox);  NOT PORTED: refcount logging
        #[cfg(debug_assertions)]
        {
            ctorCount.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            NS_ASSERTION!(!aIsBlock || aCount == 1, "Blocks must have exactly one child");
            let mut f = aFrame;
            let mut n = aCount;
            while n > 0 {
                NS_ASSERTION!(aIsBlock == f.unwrap().IsBlockOutside(), "wrong kind of child frame");
                f = f.unwrap().GetNextSibling();
                n -= 1;
            }
        }
        this.mChildCount = aCount as u32;
        this.MarkDirty();
        this.mFlags.mBlock = aIsBlock;
        this
    }

    // Infallible overloaded new operator. Uses an arena (which comes from the
    // presShell) to perform the allocation.
    // void* operator new(size_t sz, mozilla::PresShell* aPresShell);
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::operator new
    // NOT PORTED: arena allocation (NS_NewLineBox uses Box).

    // void operator delete(void* aPtr, size_t sz) = delete;
    // Use these functions to allocate and destroy line boxes
    // friend nsLineBox* NS_NewLineBox(...);  (both overloads, defined above)

    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::Destroy
    // NOT PORTED: the PresShell argument. `delete this` of a Box allocated by NS_NewLineBox.
    pub fn Destroy(&mut self) {
        unsafe {
            drop(Box::from_raw(self as *mut nsLineBox));
        }
    }

    // mBlock bit
    // Gecko: layout/generic/nsLineBox.h nsLineBox::IsBlock
    pub fn IsBlock(&self) -> bool {
        self.mFlags.mBlock
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::IsInline
    pub fn IsInline(&self) -> bool {
        !self.mFlags.mBlock
    }

    // mDirty bit
    // Gecko: layout/generic/nsLineBox.h nsLineBox::MarkDirty
    pub fn MarkDirty(&mut self) {
        self.mFlags.mDirty = true;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ClearDirty
    pub fn ClearDirty(&mut self) {
        self.mFlags.mDirty = false;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::IsDirty
    pub fn IsDirty(&self) -> bool {
        self.mFlags.mDirty
    }

    // mPreviousMarginDirty bit
    // Gecko: layout/generic/nsLineBox.h nsLineBox::MarkPreviousMarginDirty
    pub fn MarkPreviousMarginDirty(&mut self) {
        self.mFlags.mPreviousMarginDirty = true;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ClearPreviousMarginDirty
    pub fn ClearPreviousMarginDirty(&mut self) {
        self.mFlags.mPreviousMarginDirty = false;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::IsPreviousMarginDirty
    pub fn IsPreviousMarginDirty(&self) -> bool {
        self.mFlags.mPreviousMarginDirty
    }

    // mHasClearance bit
    // Gecko: layout/generic/nsLineBox.h nsLineBox::SetHasClearance
    pub fn SetHasClearance(&mut self) {
        self.mFlags.mHasClearance = true;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ClearHasClearance
    pub fn ClearHasClearance(&mut self) {
        self.mFlags.mHasClearance = false;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::HasClearance
    pub fn HasClearance(&self) -> bool {
        self.mFlags.mHasClearance
    }

    // mImpactedByFloat bit
    // Gecko: layout/generic/nsLineBox.h nsLineBox::SetLineIsImpactedByFloat
    pub fn SetLineIsImpactedByFloat(&mut self, aValue: bool) {
        self.mFlags.mImpactedByFloat = aValue;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::IsImpactedByFloat
    pub fn IsImpactedByFloat(&self) -> bool {
        self.mFlags.mImpactedByFloat
    }

    // mLineWrapped bit
    // Gecko: layout/generic/nsLineBox.h nsLineBox::SetLineWrapped
    pub fn SetLineWrapped(&mut self, aOn: bool) {
        self.mFlags.mLineWrapped = aOn;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::IsLineWrapped
    pub fn IsLineWrapped(&self) -> bool {
        self.mFlags.mLineWrapped
    }

    // mInvalidateTextRuns bit
    // Gecko: layout/generic/nsLineBox.h nsLineBox::SetInvalidateTextRuns
    pub fn SetInvalidateTextRuns(&mut self, aOn: bool) {
        self.mFlags.mInvalidateTextRuns = aOn;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::GetInvalidateTextRuns
    pub fn GetInvalidateTextRuns(&self) -> bool {
        self.mFlags.mInvalidateTextRuns
    }

    // mResizeReflowOptimizationDisabled bit
    // Gecko: layout/generic/nsLineBox.h nsLineBox::DisableResizeReflowOptimization
    pub fn DisableResizeReflowOptimization(&mut self) {
        self.mFlags.mResizeReflowOptimizationDisabled = true;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::EnableResizeReflowOptimization
    pub fn EnableResizeReflowOptimization(&mut self) {
        self.mFlags.mResizeReflowOptimizationDisabled = false;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ResizeReflowOptimizationDisabled
    pub fn ResizeReflowOptimizationDisabled(&self) -> bool {
        self.mFlags.mResizeReflowOptimizationDisabled
    }

    // mHasMarker bit
    // Gecko: layout/generic/nsLineBox.h nsLineBox::SetHasMarker
    pub fn SetHasMarker(&mut self) {
        self.mFlags.mHasMarker = true;
        self.InvalidateCachedIsEmpty();
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ClearHasMarker
    pub fn ClearHasMarker(&mut self) {
        self.mFlags.mHasMarker = false;
        self.InvalidateCachedIsEmpty();
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::HasMarker
    pub fn HasMarker(&self) -> bool {
        self.mFlags.mHasMarker
    }

    // mHadFloatPushed bit
    // Gecko: layout/generic/nsLineBox.h nsLineBox::SetHadFloatPushed
    pub fn SetHadFloatPushed(&mut self) {
        self.mFlags.mHadFloatPushed = true;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ClearHadFloatPushed
    pub fn ClearHadFloatPushed(&mut self) {
        self.mFlags.mHadFloatPushed = false;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::HadFloatPushed
    pub fn HadFloatPushed(&self) -> bool {
        self.mFlags.mHadFloatPushed
    }

    // mHasLineClampEllipsis bit
    // Gecko: layout/generic/nsLineBox.h nsLineBox::SetHasLineClampEllipsis
    pub fn SetHasLineClampEllipsis(&mut self) {
        self.mFlags.mHasLineClampEllipsis = true;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ClearHasLineClampEllipsis
    pub fn ClearHasLineClampEllipsis(&mut self) {
        self.mFlags.mHasLineClampEllipsis = false;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::HasLineClampEllipsis
    pub fn HasLineClampEllipsis(&self) -> bool {
        self.mFlags.mHasLineClampEllipsis
    }

    // mMovedFragments bit
    // Gecko: layout/generic/nsLineBox.h nsLineBox::SetMovedFragments
    pub fn SetMovedFragments(&mut self) {
        self.mFlags.mMovedFragments = true;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ClearMovedFragments
    pub fn ClearMovedFragments(&mut self) {
        self.mFlags.mMovedFragments = false;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::MovedFragments
    pub fn MovedFragments(&self) -> bool {
        self.mFlags.mMovedFragments
    }

    /**
     * Take ownership of aFromLine's hash table and remove the frames that
     * stay on aFromLine from it, i.e. aFromLineNewCount frames starting with
     * mFirstChild.  This method is used to optimize moving a large number
     * of frames from one line to the next.
     */
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::StealHashTableFrom
    fn StealHashTableFrom(&mut self, aFromLine: *mut nsLineBox, aFromLineNewCount: u32) {
        let aFromLine = unsafe { &mut *aFromLine };
        MOZ_ASSERT!(!self.mFlags.mHasHashedFrames);
        MOZ_ASSERT!(self.GetChildCount() >= aFromLineNewCount as i32);
        self.mFrames = aFromLine.mFrames.take();
        self.mFlags.mHasHashedFrames = true;
        aFromLine.mFlags.mHasHashedFrames = false;
        aFromLine.mChildCount = aFromLineNewCount;
        // remove aFromLine's frames that aren't on this line
        let mut f = aFromLine.mFirstChild;
        let mut i = 0;
        while i < aFromLineNewCount {
            self.mFrames.as_mut().unwrap().remove(&f.unwrap());
            f = f.unwrap().GetNextSibling();
            i += 1;
        }
    }

    /**
     * Does the equivalent of this->NoteFrameAdded and aFromLine->NoteFrameRemoved
     * for each frame on this line, but in a optimized way.
     */
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::NoteFramesMovedFrom
    fn NoteFramesMovedFrom(&mut self, aFromLinePtr: *mut nsLineBox) {
        let aFromLine = unsafe { &mut *aFromLinePtr };
        let fromCount = aFromLine.GetChildCount() as u32;
        let toCount = self.GetChildCount() as u32;
        MOZ_ASSERT!(toCount <= fromCount, "moved more frames than aFromLine has");
        let fromNewCount = fromCount - toCount;
        if !aFromLine.mFlags.mHasHashedFrames {
            aFromLine.mChildCount = fromNewCount;
            MOZ_ASSERT!(toCount < nsLineBox::kMinChildCountForHashtable);
        } else if fromNewCount < nsLineBox::kMinChildCountForHashtable {
            // aFromLine has a hash table but will not have it after moving the frames
            // so this line can steal the hash table if it needs it.
            if toCount >= nsLineBox::kMinChildCountForHashtable {
                self.StealHashTableFrom(aFromLinePtr, fromNewCount);
            } else {
                aFromLine.mFrames = None;
                aFromLine.mFlags.mHasHashedFrames = false;
                aFromLine.mChildCount = fromNewCount;
            }
        } else {
            // aFromLine still needs a hash table.
            if toCount < nsLineBox::kMinChildCountForHashtable {
                // remove the moved frames from it
                let mut f = self.mFirstChild;
                let mut i = 0;
                while i < toCount {
                    aFromLine.mFrames.as_mut().unwrap().remove(&f.unwrap());
                    f = f.unwrap().GetNextSibling();
                    i += 1;
                }
            } else if toCount <= fromNewCount {
                // This line needs a hash table, allocate a hash table for it since that
                // means fewer hash ops.
                let mut f = self.mFirstChild;
                let mut i = 0;
                while i < toCount {
                    aFromLine.mFrames.as_mut().unwrap().remove(&f.unwrap()); // toCount RemoveEntry
                    f = f.unwrap().GetNextSibling();
                    i += 1;
                }
                self.SwitchToHashtable(); // toCount PutEntry
            } else {
                // This line needs a hash table, but it's fewer hash ops to steal
                // aFromLine's hash table and allocate a new hash table for that line.
                self.StealHashTableFrom(aFromLinePtr, fromNewCount); // fromNewCount RemoveEntry
                aFromLine.SwitchToHashtable(); // fromNewCount PutEntry
            }
        }
    }

    // Gecko: layout/generic/nsLineBox.h nsLineBox::SwitchToHashtable
    fn SwitchToHashtable(&mut self) {
        MOZ_ASSERT!(!self.mFlags.mHasHashedFrames);
        let mut count = self.GetChildCount() as u32;
        self.mFlags.mHasHashedFrames = true;
        let minLength = std::cmp::max(nsLineBox::kMinChildCountForHashtable, nsLineBox::kDefaultInitialLength);
        let mut frames = HashSet::with_capacity(std::cmp::max(count, minLength) as usize);
        let mut f = self.mFirstChild;
        while count > 0 {
            count -= 1;
            frames.insert(f.unwrap());
            f = f.unwrap().GetNextSibling();
        }
        self.mFrames = Some(Box::new(frames));
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::SwitchToCounter
    fn SwitchToCounter(&mut self) {
        MOZ_ASSERT!(self.mFlags.mHasHashedFrames);
        let count = self.GetChildCount() as u32;
        self.mFrames = None;
        self.mFlags.mHasHashedFrames = false;
        self.mChildCount = count;
    }

    // Gecko: layout/generic/nsLineBox.h nsLineBox::GetChildCount
    pub fn GetChildCount(&self) -> i32 {
        if self.mFlags.mHasHashedFrames { self.mFrames.as_ref().unwrap().len() as i32 } else { self.mChildCount as i32 }
    }

    // class ChildFrameIterator, class ChildFrameRange: see nsLineBox_ChildFrameIterator below.

    // Return a range over this line's child frames, suitable for use with a
    // range-based for loop.
    //
    // Example usage:
    //   for (nsIFrame* f : line->ChildFrames()) { ... }
    //
    // Note: use the range-based for loop only when the frame sibling chain in
    // this line is not mutated during traversal. If mutation is required, use
    // mFirstChild and GetChildCount() instead.
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ChildFrames
    pub fn ChildFrames(&self) -> nsLineBox_ChildFrameRange {
        nsLineBox_ChildFrameRange::new(self)
    }

    /**
     * Register that aFrame is now on this line.
     */
    // Gecko: layout/generic/nsLineBox.h nsLineBox::NoteFrameAdded
    pub fn NoteFrameAdded(&mut self, aFrame: nsIFrame) {
        if self.mFlags.mHasHashedFrames {
            self.mFrames.as_mut().unwrap().insert(aFrame);
        } else {
            self.mChildCount += 1;
            if self.mChildCount >= nsLineBox::kMinChildCountForHashtable {
                self.SwitchToHashtable();
            }
        }
    }

    /**
     * Register that aFrame is not on this line anymore.
     */
    // Gecko: layout/generic/nsLineBox.h nsLineBox::NoteFrameRemoved
    pub fn NoteFrameRemoved(&mut self, aFrame: nsIFrame) {
        MOZ_ASSERT!(self.GetChildCount() > 0);
        if self.mFlags.mHasHashedFrames {
            self.mFrames.as_mut().unwrap().remove(&aFrame);
            if (self.mFrames.as_ref().unwrap().len() as u32) < nsLineBox::kMinChildCountForHashtable {
                self.SwitchToCounter();
            }
        } else {
            self.mChildCount -= 1;
        }
    }

    // mHasForcedLineBreakAfter bit & mFloatClearType value
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ClearForcedLineBreak
    pub fn ClearForcedLineBreak(&mut self) {
        self.mFlags.mHasForcedLineBreakAfter = false;
        self.mFlags.mFloatClearType = UsedClear::None;
    }

    // Gecko: layout/generic/nsLineBox.h nsLineBox::HasFloatClearTypeBefore
    pub fn HasFloatClearTypeBefore(&self) -> bool {
        self.FloatClearTypeBefore() != UsedClear::None
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::SetFloatClearTypeBefore
    pub fn SetFloatClearTypeBefore(&mut self, aClearType: UsedClear) {
        MOZ_ASSERT!(self.IsBlock(), "Only block lines have break-before status!");
        MOZ_ASSERT!(aClearType != UsedClear::None, "Only UsedClear:Left/Right/Both are allowed before a line");
        self.mFlags.mFloatClearType = aClearType;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::FloatClearTypeBefore
    pub fn FloatClearTypeBefore(&self) -> UsedClear {
        if self.IsBlock() { self.mFlags.mFloatClearType } else { UsedClear::None }
    }

    // Gecko: layout/generic/nsLineBox.h nsLineBox::HasForcedLineBreakAfter
    pub fn HasForcedLineBreakAfter(&self) -> bool {
        MOZ_ASSERT!(self.IsInline() || !self.mFlags.mHasForcedLineBreakAfter, "A block line shouldn't set mHasForcedLineBreakAfter bit!");
        self.IsInline() && self.mFlags.mHasForcedLineBreakAfter
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::SetForcedLineBreakAfter
    pub fn SetForcedLineBreakAfter(&mut self, aClearType: UsedClear) {
        MOZ_ASSERT!(self.IsInline(), "Only inline lines have break-after status!");
        self.mFlags.mHasForcedLineBreakAfter = true;
        self.mFlags.mFloatClearType = aClearType;
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::HasFloatClearTypeAfter
    pub fn HasFloatClearTypeAfter(&self) -> bool {
        self.FloatClearTypeAfter() != UsedClear::None
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::FloatClearTypeAfter
    pub fn FloatClearTypeAfter(&self) -> UsedClear {
        if self.IsInline() { self.mFlags.mFloatClearType } else { UsedClear::None }
    }

    // mCarriedOutBEndMargin value
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::GetCarriedOutBEndMargin
    pub fn GetCarriedOutBEndMargin(&self) -> CollapsingMargin {
        NS_ASSERTION!(self.IsBlock(), "GetCarriedOutBEndMargin called on non-block line.");
        if self.IsBlock() && self.mData.is_some() { self.BlockData().unwrap().mCarriedOutBEndMargin } else { CollapsingMargin::default() }
    }

    // Returns true if the margin changed
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::SetCarriedOutBEndMargin
    pub fn SetCarriedOutBEndMargin(&mut self, aValue: CollapsingMargin) -> bool {
        let mut changed = false;
        if self.IsBlock() {
            if !aValue.IsZero() {
                if self.mData.is_none() {
                    self.mData = Some(Box::new(nsLineBox_Data::Block(nsLineBox_ExtraBlockData::new(self.GetPhysicalBounds()))));
                }
                let blockData = self.BlockData_mut().unwrap();
                changed = aValue != blockData.mCarriedOutBEndMargin;
                blockData.mCarriedOutBEndMargin = aValue;
            } else if self.mData.is_some() {
                let blockData = self.BlockData_mut().unwrap();
                changed = aValue != blockData.mCarriedOutBEndMargin;
                blockData.mCarriedOutBEndMargin = aValue;
                self.MaybeFreeData();
            }
        }
        changed
    }

    // ADAPTER: `mBlockData` and `mInlineData` of the union, as Options over the stored variant.
    // (`mData` itself is the field of that name; `mData->x` is `self.mData.as_ref().unwrap().x`.)
    fn BlockData(&self) -> Option<&nsLineBox_ExtraBlockData> {
        match self.mData.as_deref() {
            Some(nsLineBox_Data::Block(b)) => Some(b),
            _ => None,
        }
    }
    fn BlockData_mut(&mut self) -> Option<&mut nsLineBox_ExtraBlockData> {
        match self.mData.as_deref_mut() {
            Some(nsLineBox_Data::Block(b)) => Some(b),
            _ => None,
        }
    }
    fn InlineData(&self) -> Option<&nsLineBox_ExtraInlineData> {
        match self.mData.as_deref() {
            Some(nsLineBox_Data::Inline(i)) => Some(i),
            _ => None,
        }
    }
    fn InlineData_mut(&mut self) -> Option<&mut nsLineBox_ExtraInlineData> {
        match self.mData.as_deref_mut() {
            Some(nsLineBox_Data::Inline(i)) => Some(i),
            _ => None,
        }
    }

    // mFloats
    // Gecko: layout/generic/nsLineBox.h nsLineBox::HasFloats
    pub fn HasFloats(&self) -> bool {
        self.IsInline() && self.InlineData().is_some_and(|d| !d.mFloats.is_empty())
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::Floats
    pub fn Floats(&self) -> &Vec<nsIFrame> {
        MOZ_ASSERT!(self.HasFloats());
        &self.InlineData().unwrap().mFloats
    }
    // Append aFloats to mFloat. aFloats will be empty.
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::AppendFloats
    // (the nsTArray&& is taken by value; it is consumed, so "aFloats will be empty" holds)
    pub fn AppendFloats(&mut self, mut aFloats: Vec<nsIFrame>) {
        MOZ_ASSERT!(self.IsInline(), "block line can't have floats");
        if !self.IsInline() {
            return;
        }
        if !aFloats.is_empty() {
            if self.mData.is_some() {
                self.InlineData_mut().unwrap().mFloats.append(&mut aFloats);
            } else {
                let mut data = nsLineBox_ExtraInlineData::new(self.GetPhysicalBounds());
                data.mFloats = aFloats;
                self.mData = Some(Box::new(nsLineBox_Data::Inline(data)));
            }
        }
    }
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::ClearFloats
    pub fn ClearFloats(&mut self) {
        MOZ_ASSERT!(self.IsInline(), "block line can't have floats");
        if self.IsInline() && self.mData.is_some() {
            self.InlineData_mut().unwrap().mFloats.clear();
            self.MaybeFreeData();
        }
    }
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::RemoveFloat
    pub fn RemoveFloat(&mut self, aFrame: nsIFrame) -> bool {
        MOZ_ASSERT!(self.IsInline(), "block line can't have floats");
        if self.IsInline() && self.mData.is_some() {
            let floats = &mut self.InlineData_mut().unwrap().mFloats;
            // nsTArray::RemoveElement(aFrame): removes the first match
            if let Some(i) = floats.iter().position(|f| *f == aFrame) {
                floats.remove(i);
                // Note: the placeholder is part of the line's child list
                // and will be removed later.
                self.MaybeFreeData();
                return true;
            }
        }
        false
    }

    // The ink overflow area should never be used for things that affect layout.
    // The scrollable overflow area are permitted to affect layout for handling of
    // overflow and scrollbars.
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::SetOverflowAreas
    pub fn SetOverflowAreas(&mut self, aOverflowAreas: &OverflowAreas) {
        #[cfg(debug_assertions)]
        for otype in AllOverflowTypes() {
            NS_ASSERTION!(aOverflowAreas.Overflow(otype).width >= 0, "Illegal width for an overflow area!");
            NS_ASSERTION!(aOverflowAreas.Overflow(otype).height >= 0, "Illegal height for an overflow area!");
        }

        let bounds = self.GetPhysicalBounds();
        if !aOverflowAreas.InkOverflow().IsEqualInterior(bounds) || !aOverflowAreas.ScrollableOverflow().IsEqualEdges(bounds) {
            if self.mData.is_none() {
                if self.IsInline() {
                    self.mData = Some(Box::new(nsLineBox_Data::Inline(nsLineBox_ExtraInlineData::new(bounds))));
                } else {
                    self.mData = Some(Box::new(nsLineBox_Data::Block(nsLineBox_ExtraBlockData::new(bounds))));
                }
            }
            self.mData.as_mut().unwrap().mOverflowAreas = *aOverflowAreas;
        } else if self.mData.is_some() {
            // Store away new value so that MaybeFreeData compares against
            // the right value.
            self.mData.as_mut().unwrap().mOverflowAreas = *aOverflowAreas;
            self.MaybeFreeData();
        }
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::GetOverflowArea(OverflowType, WritingMode, const nsSize&)
    pub fn GetOverflowArea__OverflowType_WritingMode_nsSize(&self, aType: OverflowType, aWM: WritingMode, aContainerSize: nsSize) -> LogicalRect {
        LogicalRect::new__WritingMode_nsRect_nsSize(aWM, self.GetOverflowArea__OverflowType(aType), aContainerSize)
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::GetOverflowArea(OverflowType)
    pub fn GetOverflowArea__OverflowType(&self, aType: OverflowType) -> nsRect {
        match &self.mData {
            Some(data) => *data.mOverflowAreas.Overflow(aType),
            None => self.GetPhysicalBounds(),
        }
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::GetOverflowAreas
    pub fn GetOverflowAreas(&self) -> OverflowAreas {
        if let Some(data) = &self.mData {
            return data.mOverflowAreas;
        }
        let bounds = self.GetPhysicalBounds();
        OverflowAreas::new(bounds, bounds)
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::InkOverflowRect
    pub fn InkOverflowRect(&self) -> nsRect {
        self.GetOverflowArea__OverflowType(OverflowType::Ink)
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ScrollableOverflowRect
    pub fn ScrollableOverflowRect(&self) -> nsRect {
        self.GetOverflowArea__OverflowType(OverflowType::Scrollable)
    }

    // See comment on `mInFlowChildBounds`.
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::SetInFlowChildBounds
    pub fn SetInFlowChildBounds(&mut self, aInFlowChildBounds: Option<nsRect>) {
        if aInFlowChildBounds.is_some() {
            if self.mData.is_none() {
                let bounds = self.GetPhysicalBounds();
                if self.IsInline() {
                    self.mData = Some(Box::new(nsLineBox_Data::Inline(nsLineBox_ExtraInlineData::new(bounds))));
                } else {
                    self.mData = Some(Box::new(nsLineBox_Data::Block(nsLineBox_ExtraBlockData::new(bounds))));
                }
            }
            self.mData.as_mut().unwrap().mInFlowChildBounds = aInFlowChildBounds;
        } else if self.mData.is_some() {
            self.mData.as_mut().unwrap().mInFlowChildBounds = None;
            self.MaybeFreeData();
        }
    }
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::GetInFlowChildBounds
    pub fn GetInFlowChildBounds(&self) -> Option<nsRect> {
        match &self.mData {
            None => None,
            Some(data) => data.mInFlowChildBounds,
        }
    }

    // Gecko: layout/generic/nsLineBox.h nsLineBox::SlideBy
    pub fn SlideBy(&mut self, aDBCoord: nscoord, aContainerSize: nsSize) {
        NS_ASSERTION!(aContainerSize == self.mContainerSize || self.mContainerSize == nsSize::new(-1, -1), "container size doesn't match");
        self.mContainerSize = aContainerSize;
        *self.mBounds.BStart_mut(self.mWritingMode) += aDBCoord;
        if let Some(data) = &mut self.mData {
            // Use a null containerSize to convert vector from logical to physical.
            let nullContainerSize = nsSize::default();
            let physicalDelta = LogicalPoint::new(self.mWritingMode, 0, aDBCoord).GetPhysicalPoint(self.mWritingMode, nullContainerSize);
            for otype in AllOverflowTypes() {
                *data.mOverflowAreas.Overflow_mut(otype) += physicalDelta;
            }
            if let Some(b) = &mut data.mInFlowChildBounds {
                *b += physicalDelta;
            }
        }
    }

    // Container-size for the line is changing (and therefore if writing mode
    // was vertical-rl, the line will move physically; this is like SlideBy,
    // but it is the container size instead of the line's own logical coord
    // that is changing.
    // Gecko: layout/generic/nsLineBox.h nsLineBox::UpdateContainerSize
    pub fn UpdateContainerSize(&mut self, aNewContainerSize: nsSize) -> nsSize {
        NS_ASSERTION!(self.mContainerSize != nsSize::new(-1, -1), "container size not set");
        let delta = self.mContainerSize - aNewContainerSize;
        self.mContainerSize = aNewContainerSize;
        // this has a physical-coordinate effect only in vertical-rl mode
        if self.mWritingMode.IsVerticalRL() {
            if let Some(data) = &mut self.mData {
                let physicalDelta = nsPoint::new(-delta.width, 0);
                for otype in AllOverflowTypes() {
                    *data.mOverflowAreas.Overflow_mut(otype) += physicalDelta;
                }
                if let Some(b) = &mut data.mInFlowChildBounds {
                    *b += physicalDelta;
                }
            }
        }
        delta
    }

    // Gecko: layout/generic/nsLineBox.h nsLineBox::IndentBy
    pub fn IndentBy(&mut self, aDICoord: nscoord, aContainerSize: nsSize) {
        NS_ASSERTION!(aContainerSize == self.mContainerSize || self.mContainerSize == nsSize::new(-1, -1), "container size doesn't match");
        self.mContainerSize = aContainerSize;
        *self.mBounds.IStart_mut(self.mWritingMode) += aDICoord;
    }

    // Gecko: layout/generic/nsLineBox.h nsLineBox::ExpandBy
    pub fn ExpandBy(&mut self, aDISize: nscoord, aContainerSize: nsSize) {
        NS_ASSERTION!(aContainerSize == self.mContainerSize || self.mContainerSize == nsSize::new(-1, -1), "container size doesn't match");
        self.mContainerSize = aContainerSize;
        *self.mBounds.ISize_mut(self.mWritingMode) += aDISize;
    }

    /**
     * The logical ascent (distance from block-start to baseline) of the
     * linebox is the logical ascent of the anonymous inline box (for
     * which we don't actually create a frame) that wraps all the
     * consecutive inline children of a block.
     *
     * This is currently unused for block lines.
     */
    // Gecko: layout/generic/nsLineBox.h nsLineBox::GetLogicalAscent
    pub fn GetLogicalAscent(&self) -> nscoord {
        self.mAscent
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::SetLogicalAscent
    pub fn SetLogicalAscent(&mut self, aAscent: nscoord) {
        self.mAscent = aAscent;
    }

    // Gecko: layout/generic/nsLineBox.h nsLineBox::BStart
    pub fn BStart(&self) -> nscoord {
        self.mBounds.BStart(self.mWritingMode)
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::BSize
    pub fn BSize(&self) -> nscoord {
        self.mBounds.BSize(self.mWritingMode)
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::BEnd
    pub fn BEnd(&self) -> nscoord {
        self.mBounds.BEnd(self.mWritingMode)
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::IStart
    pub fn IStart(&self) -> nscoord {
        self.mBounds.IStart(self.mWritingMode)
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ISize
    pub fn ISize(&self) -> nscoord {
        self.mBounds.ISize(self.mWritingMode)
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::IEnd
    pub fn IEnd(&self) -> nscoord {
        self.mBounds.IEnd(self.mWritingMode)
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::SetBoundsEmpty
    pub fn SetBoundsEmpty(&mut self) {
        *self.mBounds.IStart_mut(self.mWritingMode) = 0;
        *self.mBounds.ISize_mut(self.mWritingMode) = 0;
        *self.mBounds.BStart_mut(self.mWritingMode) = 0;
        *self.mBounds.BSize_mut(self.mWritingMode) = 0;
    }

    // using DestroyContext = nsIFrame::DestroyContext;  (nsLineBox_DestroyContext, below the impl)

    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::DeleteLineList
    // NOT PORTED: the nsPresContext argument is kept but only the PresShell was read from it.
    pub fn DeleteLineList(aPresContext: nsPresContext, aLines: &mut nsLineList, aFrames: &mut nsFrameList, aContext: &mut nsLineBox_DestroyContext) {
        // Keep our line list and frame list up to date as we remove frames, in case
        // something wants to traverse the frame tree while we're destroying.
        //
        // Note: We delete the line list and the frames in each line in reverse order
        // to avoid unnecessary updates to the first-continuation and first-in-flow
        // cache. If we delete them from front to back, updating the cache has a
        // O(n^2) time complexity.
        while !aLines.empty() {
            let line = aLines.back_mut();
            let lineRef = unsafe { &mut *line };
            if lineRef.mFlags.mHasHashedFrames {
                lineRef.SwitchToCounter(); // Avoid expensive has table removals.
            }
            while lineRef.GetChildCount() > 0 {
                let child = aFrames.RemoveLastChild().unwrap();
                // MOZ_DIAGNOSTIC_ASSERT(child->PresContext() == aPresContext);
                // NOT PORTED: the assertion (there is one pres context per thread).
                lineRef.NoteFrameRemoved(child);
                child.Destroy(aContext);
            }
            MOZ_ASSERT!(line == aLines.back_mut(), "destroying child frames messed up our lines!");
            aLines.pop_back();
            lineRef.Destroy();
        }
    }

    // search from end to beginning of [aBegin, aEnd)
    // Returns true if it found the line and false if not.
    // Moves aEnd as it searches so that aEnd points to the resulting line.
    // aLastFrameBeforeEnd is the last frame before aEnd (so if aEnd is
    // the end of the line list, it's just the last frame in the frame
    // list).
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::RFindLineContaining
    pub fn RFindLineContaining(aFrame: nsIFrame, aBegin: &LineListIterator, aEnd: &mut LineListIterator, aLastFrameBeforeEnd: Option<nsIFrame>, aFrameIndexInLine: &mut i32) -> bool {
        let mut curFrame = aLastFrameBeforeEnd;
        while *aBegin != *aEnd {
            aEnd.operator_dec();
            NS_ASSERTION!(aEnd.LastChild() == curFrame, "Unexpected curFrame");
            if aEnd.mFlags.mHasHashedFrames && !aEnd.Contains(aFrame) {
                if let Some(first) = aEnd.mFirstChild {
                    curFrame = first.GetPrevSibling();
                }
                continue;
            }
            // i is the index of curFrame in aEnd
            let mut i = aEnd.GetChildCount() - 1;
            while i >= 0 {
                if curFrame == Some(aFrame) {
                    *aFrameIndexInLine = i;
                    return true;
                }
                i -= 1;
                curFrame = curFrame.unwrap().GetPrevSibling();
            }
            MOZ_ASSERT!(!aEnd.mFlags.mHasHashedFrames, "Contains lied to us!");
        }
        *aFrameIndexInLine = -1;
        false
    }

    // #ifdef DEBUG_FRAME_DUMP
    //   static const char* UsedClearToString(mozilla::UsedClear aClearType);
    //   void List(FILE* out, int32_t aIndent, nsIFrame::ListFlags aFlags = nsIFrame::ListFlags()) const;
    //   void List(FILE* out = stderr, const char* aPrefix = "", nsIFrame::ListFlags aFlags = nsIFrame::ListFlags()) const;
    //   nsIFrame* LastChild() const;
    // #endif
    // NOT PORTED: UsedClearToString and List (nsLineBox.cpp lines 105-216 at GECKO_REV; frame dumps
    // need nsIFrame::List, ListPtr and ConvertToString). LastChild is used by RFindLineContaining's
    // assertion, so it is ported:
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::LastChild
    pub fn LastChild(&self) -> Option<nsIFrame> {
        let mut frame = self.mFirstChild;
        let mut n = self.GetChildCount() - 1;
        loop {
            n -= 1;
            if n < 0 {
                break;
            }
            frame = frame.unwrap().GetNextSibling();
        }
        frame
    }

    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::AddSizeOfExcludingThis
    // NOT PORTED: nsWindowSizes (memory reporting).
    pub fn AddSizeOfExcludingThis(&self, aSizes: ()) {
        // if (mFlags.mHasHashedFrames) {
        //   aSizes.mLayoutFramePropertiesSize += mFrames->ShallowSizeOfIncludingThis(aSizes.mState.mMallocSizeOf);
        // }
        unimplemented!("NOT PORTED: nsWindowSizes")
    }

    // Find the index of aFrame within the line, starting search at the start.
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::IndexOf
    pub fn IndexOf(&self, aFrame: nsIFrame) -> i32 {
        let n = self.GetChildCount();
        let mut frame = self.mFirstChild;
        for i in 0..n {
            if frame == Some(aFrame) {
                return i;
            }
            frame = frame.unwrap().GetNextSibling();
        }
        -1
    }

    // Find the index of aFrame within the line, starting search from both ends
    // of the line and working inwards.
    // (Produces the same result as IndexOf, but with different performance
    // characteristics.)  The caller must provide the last frame in the line.
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::RLIndexOf
    pub fn RLIndexOf(&self, aFrame: nsIFrame, aLastFrameInLine: Option<nsIFrame>) -> i32 {
        let mut leftFrame = self.mFirstChild;
        let mut rightFrame = aLastFrameInLine;
        let mut leftIndex = 0;
        let mut rightIndex = self.GetChildCount() - 1;
        loop {
            if Some(aFrame) == rightFrame {
                return rightIndex;
            }
            if leftIndex == rightIndex {
                MOZ_ASSERT!(leftFrame == rightFrame, "caller provided incorrect last frame");
                break;
            }
            if Some(aFrame) == leftFrame {
                return leftIndex;
            }
            leftIndex += 1;
            if leftIndex == rightIndex {
                MOZ_ASSERT!(leftFrame.unwrap().GetNextSibling() == rightFrame, "caller provided incorrect last frame");
                break;
            }
            leftFrame = leftFrame.unwrap().GetNextSibling();
            rightFrame = rightFrame.unwrap().GetPrevSibling();
            rightIndex -= 1;
        }
        -1
    }

    // Gecko: layout/generic/nsLineBox.h nsLineBox::Contains
    pub fn Contains(&self, aFrame: nsIFrame) -> bool {
        if self.mFlags.mHasHashedFrames { self.mFrames.as_ref().unwrap().contains(&aFrame) } else { self.IndexOf(aFrame) >= 0 }
    }

    // whether the line box is "logically" empty (just like nsIFrame::IsEmpty)
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::IsEmpty
    pub fn IsEmpty(&self) -> bool {
        if self.IsBlock() {
            return self.mFirstChild.unwrap().IsEmpty();
        }

        for kid in self.ChildFrames() {
            if !kid.IsEmpty() {
                return false;
            }
        }
        if self.HasMarker() {
            return false;
        }
        true
    }

    // Call this only while in Reflow() for the block the line belongs
    // to, only between reflowing the line (or sliding it, if we skip
    // reflowing it) and the end of reflowing the block.
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::CachedIsEmpty
    pub fn CachedIsEmpty(&mut self) -> bool {
        if self.mFlags.mDirty {
            return self.IsEmpty();
        }

        if self.mFlags.mEmptyCacheValid {
            return self.mFlags.mEmptyCacheState;
        }

        let result;
        if self.IsBlock() {
            result = self.mFirstChild.unwrap().CachedIsEmpty();
        } else {
            let mut r = true;
            for kid in self.ChildFrames() {
                if !kid.CachedIsEmpty() {
                    r = false;
                    break;
                }
            }
            if self.HasMarker() {
                r = false;
            }
            result = r;
        }

        self.mFlags.mEmptyCacheValid = true;
        self.mFlags.mEmptyCacheState = result;
        result
    }

    // Gecko: layout/generic/nsLineBox.h nsLineBox::InvalidateCachedIsEmpty
    pub fn InvalidateCachedIsEmpty(&mut self) {
        self.mFlags.mEmptyCacheValid = false;
    }

    // For debugging purposes
    // Gecko: layout/generic/nsLineBox.h nsLineBox::IsValidCachedIsEmpty
    pub fn IsValidCachedIsEmpty(&self) -> bool {
        self.mFlags.mEmptyCacheValid
    }

    // #ifdef DEBUG
    //   static int32_t GetCtorCount();
    // #endif
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::GetCtorCount
    #[cfg(debug_assertions)]
    pub fn GetCtorCount() -> i32 {
        ctorCount.load(std::sync::atomic::Ordering::Relaxed)
    }

    // public:
    // Gecko: layout/generic/nsLineBox.h nsLineBox::GetBounds
    pub fn GetBounds(&self) -> &LogicalRect {
        &self.mBounds
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::GetPhysicalBounds
    pub fn GetPhysicalBounds(&self) -> nsRect {
        if self.mBounds.IsAllZero() {
            return nsRect::new(0, 0, 0, 0);
        }

        NS_ASSERTION!(self.mContainerSize != nsSize::new(-1, -1), "mContainerSize not initialized");
        self.mBounds.GetPhysicalRect(self.mWritingMode, self.mContainerSize)
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::SetBounds
    pub fn SetBounds(&mut self, aWritingMode: WritingMode, aIStart: nscoord, aBStart: nscoord, aISize: nscoord, aBSize: nscoord, aContainerSize: nsSize) {
        self.mWritingMode = aWritingMode;
        self.mContainerSize = aContainerSize;
        self.mBounds = LogicalRect::new(aWritingMode, aIStart, aBStart, aISize, aBSize);
    }

    // Gecko: layout/generic/nsLineBox.h nsLineBox::GetFloatEdges
    pub fn GetFloatEdges(&self, aStart: &mut nscoord, aEnd: &mut nscoord) -> bool {
        MOZ_ASSERT!(self.IsInline(), "block line can't have float edges");
        if let Some(d) = self.InlineData() {
            if d.mFloatEdgeIStart != nscoord_MIN {
                *aStart = d.mFloatEdgeIStart;
                *aEnd = d.mFloatEdgeIEnd;
                return true;
            }
        }
        false
    }
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::SetFloatEdges
    pub fn SetFloatEdges(&mut self, aStart: nscoord, aEnd: nscoord) {
        MOZ_ASSERT!(self.IsInline(), "block line can't have float edges");
        if self.mData.is_none() {
            self.mData = Some(Box::new(nsLineBox_Data::Inline(nsLineBox_ExtraInlineData::new(self.GetPhysicalBounds()))));
        }
        let d = self.InlineData_mut().unwrap();
        d.mFloatEdgeIStart = aStart;
        d.mFloatEdgeIEnd = aEnd;
    }
    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::ClearFloatEdges
    pub fn ClearFloatEdges(&mut self) {
        MOZ_ASSERT!(self.IsInline(), "block line can't have float edges");
        if let Some(d) = self.InlineData_mut() {
            d.mFloatEdgeIStart = nscoord_MIN;
            d.mFloatEdgeIEnd = nscoord_MIN;
        }
    }

    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::Cleanup
    fn Cleanup(&mut self) {
        if self.mData.is_some() {
            // if (IsBlock()) delete mBlockData; else delete mInlineData;
            self.mData = None;
        }
    }

    // Gecko: layout/generic/nsLineBox.cpp nsLineBox::MaybeFreeData
    fn MaybeFreeData(&mut self) {
        let bounds = self.GetPhysicalBounds();
        // If we have space allocated for additional data but no additional data to
        // represent, just delete it.
        let isInline = self.IsInline();
        if let Some(data) = &self.mData {
            if data.mOverflowAreas == OverflowAreas::new(bounds, bounds) && data.mInFlowChildBounds.is_none() {
                if isInline {
                    if self.InlineData().unwrap().mFloats.is_empty() {
                        self.mData = None;
                    }
                } else if self.BlockData().unwrap().mCarriedOutBEndMargin.IsZero() {
                    self.mData = None;
                }
            }
        }
    }
}

// Gecko: layout/generic/nsLineBox.h nsLineBox::DestroyContext
pub type nsLineBox_DestroyContext = nsIFrame_DestroyContext;

// Gecko: layout/generic/nsLineBox.cpp nsLineBox::~nsLineBox
impl Drop for nsLineBox {
    fn drop(&mut self) {
        // MOZ_COUNT_DTOR(nsLineBox);  NOT PORTED: refcount logging
        // if (MOZ_UNLIKELY(mFlags.mHasHashedFrames)) delete mFrames;
        // (the Box<HashSet> drops with the line)
        self.Cleanup();
    }
}

// An iterator over the child frames of a single line.
// Gecko: layout/generic/nsLineBox.h nsLineBox::ChildFrameIterator
// (`std::input_iterator_tag` stashing iterator; also a std Iterator over nsIFrame)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct nsLineBox_ChildFrameIterator {
    mCurrentFrame: Option<nsIFrame>,
    mRemainingChildCount: i32,
}

impl nsLineBox_ChildFrameIterator {
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ChildFrameIterator::ChildFrameIterator
    pub fn new(aFrame: Option<nsIFrame>, aRemaining: i32) -> nsLineBox_ChildFrameIterator {
        nsLineBox_ChildFrameIterator { mCurrentFrame: aFrame, mRemainingChildCount: aRemaining }
    }

    // Gecko: layout/generic/nsLineBox.h nsLineBox::ChildFrameIterator::operator*
    pub fn operator_star(&self) -> Option<nsIFrame> {
        self.mCurrentFrame
    }

    // Gecko: layout/generic/nsLineBox.h nsLineBox::ChildFrameIterator::operator++
    pub fn operator_inc(&mut self) -> &mut Self {
        MOZ_ASSERT!(self.mRemainingChildCount > 0);
        self.mRemainingChildCount -= 1;
        self.mCurrentFrame = if self.mRemainingChildCount > 0 { self.mCurrentFrame.unwrap().GetNextSibling() } else { None };
        self
    }
}

impl Iterator for nsLineBox_ChildFrameIterator {
    type Item = nsIFrame;
    fn next(&mut self) -> Option<nsIFrame> {
        let current = self.mCurrentFrame?;
        self.operator_inc();
        Some(current)
    }
}

// Gecko: layout/generic/nsLineBox.h nsLineBox::ChildFrameRange
pub struct nsLineBox_ChildFrameRange {
    // const nsLineBox* mLine; (only the begin and end iterators are needed, and they are values)
    mBegin: nsLineBox_ChildFrameIterator,
}

impl nsLineBox_ChildFrameRange {
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ChildFrameRange::ChildFrameRange
    pub fn new(aLine: &nsLineBox) -> nsLineBox_ChildFrameRange {
        let count = aLine.GetChildCount();
        nsLineBox_ChildFrameRange { mBegin: nsLineBox_ChildFrameIterator::new(if count > 0 { aLine.mFirstChild } else { None }, count) }
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ChildFrameRange::begin
    pub fn begin(&self) -> nsLineBox_ChildFrameIterator {
        self.mBegin
    }
    // Gecko: layout/generic/nsLineBox.h nsLineBox::ChildFrameRange::end
    pub fn end(&self) -> nsLineBox_ChildFrameIterator {
        nsLineBox_ChildFrameIterator::new(None, 0)
    }
}

impl IntoIterator for nsLineBox_ChildFrameRange {
    type Item = nsIFrame;
    type IntoIter = nsLineBox_ChildFrameIterator;
    fn into_iter(self) -> nsLineBox_ChildFrameIterator {
        self.begin()
    }
}

/**
 * A linked list type where the items in the list must inherit from
 * a link type to fuse allocations.
 *
 * API heavily based on the |list| class in the C++ standard.
 */

// ADAPTER: the `Link` template argument of GenericLineListIterator is `nsLineLink` or `const
// nsLineLink`. Rust has no `const T` as a type, so the const one is the marker type
// `const_nsLineLink` (it is never constructed). The trait gives `is_const` and the pointer types.
pub trait nsLineLinkKind {
    const is_const: bool;
}
impl nsLineLinkKind for nsLineLink {
    const is_const: bool = false;
}
// Gecko: `const nsLineLink` as the Link argument of GenericLineListIterator
pub struct const_nsLineLink;
impl nsLineLinkKind for const_nsLineLink {
    const is_const: bool = true;
}

// Gecko: layout/generic/nsLineBox.h GenericLineListIterator
// (`friend class nsLineList;` and the other-Link friend: the fields are private to this file)
pub struct GenericLineListIterator<Link: nsLineLinkKind, const IsReverse: bool> {
    mCurrent: *mut nsLineLink,
    #[cfg(debug_assertions)]
    mListLink: *mut nsLineLink, // the list's link, i.e., the end
    mMarker: PhantomData<Link>,
}

// Gecko: layout/generic/nsLineBox.h nsLineList::iterator, reverse_iterator, const_iterator,
// const_reverse_iterator
pub type nsLineList_iterator = GenericLineListIterator<nsLineLink, false>;
pub type nsLineList_reverse_iterator = GenericLineListIterator<nsLineLink, true>;
pub type nsLineList_const_iterator = GenericLineListIterator<const_nsLineLink, false>;
pub type nsLineList_const_reverse_iterator = GenericLineListIterator<const_nsLineLink, true>;

impl<Link: nsLineLinkKind, const IsReverse: bool> std::fmt::Debug for GenericLineListIterator<Link, IsReverse> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "GenericLineListIterator({:p})", self.mCurrent)
    }
}

// Auto generated copy-constructor OK.
impl<Link: nsLineLinkKind, const IsReverse: bool> Clone for GenericLineListIterator<Link, IsReverse> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<Link: nsLineLinkKind, const IsReverse: bool> Copy for GenericLineListIterator<Link, IsReverse> {}

// #ifdef DEBUG
//   GenericLineListIterator() : mListLink(nullptr) { memset(&mCurrent, 0xcd, sizeof(mCurrent)); }
// #else
//   // Auto generated default constructor OK.
// #endif
impl<Link: nsLineLinkKind, const IsReverse: bool> Default for GenericLineListIterator<Link, IsReverse> {
    fn default() -> Self {
        GenericLineListIterator {
            #[cfg(debug_assertions)]
            mCurrent: usize::MAX as *mut nsLineLink, // memset 0xcd (any invalid address)
            #[cfg(not(debug_assertions))]
            mCurrent: std::ptr::null_mut(),
            #[cfg(debug_assertions)]
            mListLink: std::ptr::null_mut(),
            mMarker: PhantomData,
        }
    }
}

impl<Link: nsLineLinkKind, const IsReverse: bool> GenericLineListIterator<Link, IsReverse> {
    // static constexpr bool is_const = std::is_const_v<Link>;
    pub const is_const: bool = Link::is_const;

    // template <typename OtherLink, bool OtherIsReverse>
    // self_type& operator=(const GenericLineListIterator<OtherLink, OtherIsReverse>& aOther)
    // Gecko: layout/generic/nsLineBox.h GenericLineListIterator::operator=
    pub fn operator_assign<OtherLink: nsLineLinkKind, const OtherIsReverse: bool>(&mut self, aOther: &GenericLineListIterator<OtherLink, OtherIsReverse>) -> &mut Self {
        self.mCurrent = aOther.mCurrent;
        #[cfg(debug_assertions)]
        {
            self.mListLink = aOther.mListLink;
        }
        self
    }

    // Gecko: layout/generic/nsLineBox.h GenericLineListIterator::SetPosition
    // (pointer is `nsLineBox*`, or `const nsLineBox*` for a const iterator; both are `*mut` here)
    pub fn SetPosition(&mut self, p: *mut nsLineBox) -> &mut Self {
        self.mCurrent = p as *mut nsLineLink;
        self
    }

    // Gecko: layout/generic/nsLineBox.h GenericLineListIterator::operator++()
    pub fn operator_inc(&mut self) -> &mut Self {
        self.mCurrent = unsafe { if IsReverse { (*self.mCurrent)._mPrev } else { (*self.mCurrent)._mNext } };
        self
    }

    // Gecko: layout/generic/nsLineBox.h GenericLineListIterator::operator++(int)
    pub fn operator_post_inc(&mut self) -> Self {
        let rv = *self;
        self.operator_inc();
        rv
    }

    // Gecko: layout/generic/nsLineBox.h GenericLineListIterator::operator--()
    pub fn operator_dec(&mut self) -> &mut Self {
        self.mCurrent = unsafe { if IsReverse { (*self.mCurrent)._mNext } else { (*self.mCurrent)._mPrev } };
        self
    }

    // Gecko: layout/generic/nsLineBox.h GenericLineListIterator::operator--(int)
    pub fn operator_post_dec(&mut self) -> Self {
        let rv = *self;
        self.operator_dec();
        rv
    }

    // The two `get()` overloads (non-const, const) return `pointer` (`nsLineBox*`, or `const
    // nsLineBox*` for a const iterator) and `const_pointer`. Here: `get()` is the const one and
    // returns `*const nsLineBox`; `get_mut()` is the non-const one and returns `*mut nsLineBox`.
    // Gecko: layout/generic/nsLineBox.h GenericLineListIterator::get (non-const)
    pub fn get_mut(&mut self) -> *mut nsLineBox {
        #[cfg(debug_assertions)]
        {
            MOZ_ASSERT!(!self.mListLink.is_null());
            MOZ_ASSERT!(self.mCurrent != self.mListLink, "running past end");
        }
        self.mCurrent as *mut nsLineBox
    }

    // Gecko: layout/generic/nsLineBox.h GenericLineListIterator::get (const)
    pub fn get(&self) -> *const nsLineBox {
        #[cfg(debug_assertions)]
        {
            MOZ_ASSERT!(!self.mListLink.is_null());
            MOZ_ASSERT!(self.mCurrent != self.mListLink, "running past end");
        }
        self.mCurrent as *const nsLineBox
    }

    // reference operator*(), pointer operator->(), operator pointer() (and the const ones):
    // `Deref` to nsLineBox (const), and `DerefMut` for a non-const iterator, below.

    // Gecko: layout/generic/nsLineBox.h GenericLineListIterator::next (non-const and const)
    pub fn next_(&self) -> Self {
        let mut copy = *self;
        copy.operator_inc();
        copy
    }

    // Gecko: layout/generic/nsLineBox.h GenericLineListIterator::prev (non-const and const)
    pub fn prev(&self) -> Self {
        let mut copy = *self;
        copy.operator_dec();
        copy
    }

    // #ifdef DEBUG
    // Gecko: layout/generic/nsLineBox.h GenericLineListIterator::IsInSameList
    #[cfg(debug_assertions)]
    pub fn IsInSameList(&self, aOther: &Self) -> bool {
        self.mListLink == aOther.mListLink
    }
}

// bool operator==(const self_type& aOther) const; bool operator!=(const self_type&) const = default;
impl<Link: nsLineLinkKind, const IsReverse: bool> PartialEq for GenericLineListIterator<Link, IsReverse> {
    fn eq(&self, aOther: &Self) -> bool {
        #[cfg(debug_assertions)]
        {
            MOZ_ASSERT!(!self.mListLink.is_null());
            MOZ_ASSERT!(self.mListLink == aOther.mListLink, "comparing iterators over different lists");
        }
        self.mCurrent == aOther.mCurrent
    }
}

// const_reference operator*() const / const_pointer operator->() const / operator const_pointer() const
impl<Link: nsLineLinkKind, const IsReverse: bool> Deref for GenericLineListIterator<Link, IsReverse> {
    type Target = nsLineBox;
    fn deref(&self) -> &nsLineBox {
        unsafe { &*self.get() }
    }
}

// reference operator*() / pointer operator->() / operator pointer(): non-const iterators only
impl<const IsReverse: bool> DerefMut for GenericLineListIterator<nsLineLink, IsReverse> {
    fn deref_mut(&mut self) -> &mut nsLineBox {
        unsafe { &mut *self.get_mut() }
    }
}

// Gecko: layout/generic/nsLineBox.h nsLineList
pub struct nsLineList {
    // private:
    // The sentinel link. It is boxed so that its address does not change when the nsLineList is
    // moved (in C++ the nsLineList is never moved).
    mLink: Box<nsLineLink>,
}

impl nsLineList {
    // Gecko: layout/generic/nsLineBox.h nsLineList::nsLineList
    pub fn new() -> nsLineList {
        // MOZ_COUNT_CTOR(nsLineList);  NOT PORTED: refcount logging
        let mut list = nsLineList { mLink: Box::new(nsLineLink::default()) };
        list.clear();
        list
    }

    // The address of mLink: `&mLink`.
    fn Link(&self) -> *mut nsLineLink {
        &*self.mLink as *const nsLineLink as *mut nsLineLink
    }

    // Gecko: layout/generic/nsLineBox.h nsLineList::begin() const
    pub fn begin(&self) -> nsLineList_const_iterator {
        let mut rv = nsLineList_const_iterator::default();
        rv.mCurrent = self.mLink._mNext;
        #[cfg(debug_assertions)]
        {
            rv.mListLink = self.Link();
        }
        rv
    }

    // Gecko: layout/generic/nsLineBox.h nsLineList::begin()
    pub fn begin_mut(&mut self) -> nsLineList_iterator {
        let mut rv = nsLineList_iterator::default();
        rv.mCurrent = self.mLink._mNext;
        #[cfg(debug_assertions)]
        {
            rv.mListLink = self.Link();
        }
        rv
    }

    // Gecko: layout/generic/nsLineBox.h nsLineList::begin(nsLineBox*)
    pub fn begin_mut__nsLineBox(&mut self, aLine: *mut nsLineBox) -> nsLineList_iterator {
        let mut rv = nsLineList_iterator::default();
        rv.mCurrent = aLine as *mut nsLineLink;
        #[cfg(debug_assertions)]
        {
            rv.mListLink = self.Link();
        }
        rv
    }

    // Gecko: layout/generic/nsLineBox.h nsLineList::end() const
    pub fn end(&self) -> nsLineList_const_iterator {
        let mut rv = nsLineList_const_iterator::default();
        rv.mCurrent = self.Link();
        #[cfg(debug_assertions)]
        {
            rv.mListLink = self.Link();
        }
        rv
    }

    // Gecko: layout/generic/nsLineBox.h nsLineList::end()
    pub fn end_mut(&mut self) -> nsLineList_iterator {
        let mut rv = nsLineList_iterator::default();
        rv.mCurrent = self.Link();
        #[cfg(debug_assertions)]
        {
            rv.mListLink = self.Link();
        }
        rv
    }

    // Gecko: layout/generic/nsLineBox.h nsLineList::rbegin() const
    pub fn rbegin(&self) -> nsLineList_const_reverse_iterator {
        let mut rv = nsLineList_const_reverse_iterator::default();
        rv.mCurrent = self.mLink._mPrev;
        #[cfg(debug_assertions)]
        {
            rv.mListLink = self.Link();
        }
        rv
    }

    // Gecko: layout/generic/nsLineBox.h nsLineList::rbegin()
    pub fn rbegin_mut(&mut self) -> nsLineList_reverse_iterator {
        let mut rv = nsLineList_reverse_iterator::default();
        rv.mCurrent = self.mLink._mPrev;
        #[cfg(debug_assertions)]
        {
            rv.mListLink = self.Link();
        }
        rv
    }

    // Gecko: layout/generic/nsLineBox.h nsLineList::rbegin(nsLineBox*)
    pub fn rbegin_mut__nsLineBox(&mut self, aLine: *mut nsLineBox) -> nsLineList_reverse_iterator {
        let mut rv = nsLineList_reverse_iterator::default();
        rv.mCurrent = aLine as *mut nsLineLink;
        #[cfg(debug_assertions)]
        {
            rv.mListLink = self.Link();
        }
        rv
    }

    // Gecko: layout/generic/nsLineBox.h nsLineList::rend() const
    pub fn rend(&self) -> nsLineList_const_reverse_iterator {
        let mut rv = nsLineList_const_reverse_iterator::default();
        rv.mCurrent = self.Link();
        #[cfg(debug_assertions)]
        {
            rv.mListLink = self.Link();
        }
        rv
    }

    // Gecko: layout/generic/nsLineBox.h nsLineList::rend()
    pub fn rend_mut(&mut self) -> nsLineList_reverse_iterator {
        let mut rv = nsLineList_reverse_iterator::default();
        rv.mCurrent = self.Link();
        #[cfg(debug_assertions)]
        {
            rv.mListLink = self.Link();
        }
        rv
    }

    // Gecko: layout/generic/nsLineBox.h nsLineList::empty
    pub fn empty(&self) -> bool {
        self.mLink._mNext == self.Link()
    }

    // NOTE: O(N).
    // Gecko: layout/generic/nsLineBox.h nsLineList::size
    pub fn size(&self) -> u32 {
        let mut count = 0;
        let mut cur = self.mLink._mNext as *const nsLineLink;
        while cur != self.Link() as *const nsLineLink {
            count += 1;
            cur = unsafe { (*cur)._mNext };
        }
        count
    }

    // Gecko: layout/generic/nsLineBox.h nsLineList::front() (non-const)
    pub fn front_mut(&mut self) -> *mut nsLineBox {
        NS_ASSERTION!(!self.empty(), "no element to return");
        self.mLink._mNext as *mut nsLineBox
    }

    // Gecko: layout/generic/nsLineBox.h nsLineList::front() const
    pub fn front(&self) -> *const nsLineBox {
        NS_ASSERTION!(!self.empty(), "no element to return");
        self.mLink._mNext as *const nsLineBox
    }

    // Gecko: layout/generic/nsLineBox.h nsLineList::back() (non-const)
    pub fn back_mut(&mut self) -> *mut nsLineBox {
        NS_ASSERTION!(!self.empty(), "no element to return");
        self.mLink._mPrev as *mut nsLineBox
    }

    // Gecko: layout/generic/nsLineBox.h nsLineList::back() const
    pub fn back(&self) -> *const nsLineBox {
        NS_ASSERTION!(!self.empty(), "no element to return");
        self.mLink._mPrev as *const nsLineBox
    }

    // Gecko: layout/generic/nsLineBox.h nsLineList::push_front
    pub fn push_front(&mut self, aNew: *mut nsLineBox) {
        let aNew = aNew as *mut nsLineLink;
        let mLink = self.Link();
        unsafe {
            (*aNew)._mNext = (*mLink)._mNext;
            (*(*mLink)._mNext)._mPrev = aNew;
            (*aNew)._mPrev = mLink;
            (*mLink)._mNext = aNew;
        }
    }

    // NOTE: leaves dangling next/prev pointers
    // Gecko: layout/generic/nsLineBox.h nsLineList::pop_front
    pub fn pop_front(&mut self) {
        NS_ASSERTION!(!self.empty(), "no element to pop");
        let mLink = self.Link();
        unsafe {
            let newFirst = (*(*mLink)._mNext)._mNext;
            (*newFirst)._mPrev = mLink;
            // mLink._mNext->_mNext = nullptr;
            // mLink._mNext->_mPrev = nullptr;
            (*mLink)._mNext = newFirst;
        }
    }

    // Gecko: layout/generic/nsLineBox.h nsLineList::push_back
    pub fn push_back(&mut self, aNew: *mut nsLineBox) {
        let aNew = aNew as *mut nsLineLink;
        let mLink = self.Link();
        unsafe {
            (*aNew)._mPrev = (*mLink)._mPrev;
            (*(*mLink)._mPrev)._mNext = aNew;
            (*aNew)._mNext = mLink;
            (*mLink)._mPrev = aNew;
        }
    }

    // NOTE: leaves dangling next/prev pointers
    // Gecko: layout/generic/nsLineBox.h nsLineList::pop_back
    pub fn pop_back(&mut self) {
        NS_ASSERTION!(!self.empty(), "no element to pop");
        let mLink = self.Link();
        unsafe {
            let newLast = (*(*mLink)._mPrev)._mPrev;
            (*newLast)._mNext = mLink;
            // mLink._mPrev->_mPrev = nullptr;
            // mLink._mPrev->_mNext = nullptr;
            (*mLink)._mPrev = newLast;
        }
    }

    // inserts x before position
    // Gecko: layout/generic/nsLineBox.h nsLineList::before_insert
    pub fn before_insert(&mut self, mut position: nsLineList_iterator, x: *mut nsLineBox) -> nsLineList_iterator {
        let x = x as *mut nsLineLink;
        // use |mCurrent| to prevent DEBUG_PASS_END assertions
        unsafe {
            (*x)._mPrev = (*position.mCurrent)._mPrev;
            (*x)._mNext = position.mCurrent;
            (*(*position.mCurrent)._mPrev)._mNext = x;
            (*position.mCurrent)._mPrev = x;
        }
        position.operator_dec();
        position
    }

    // inserts x after position
    // Gecko: layout/generic/nsLineBox.h nsLineList::after_insert
    pub fn after_insert(&mut self, mut position: nsLineList_iterator, x: *mut nsLineBox) -> nsLineList_iterator {
        let x = x as *mut nsLineLink;
        // use |mCurrent| to prevent DEBUG_PASS_END assertions
        unsafe {
            (*x)._mNext = (*position.mCurrent)._mNext;
            (*x)._mPrev = position.mCurrent;
            (*(*position.mCurrent)._mNext)._mPrev = x;
            (*position.mCurrent)._mNext = x;
        }
        position.operator_inc();
        position
    }

    // returns iterator pointing to after the element
    // NOTE: leaves dangling next/prev pointers (except in DEBUG build)
    // Gecko: layout/generic/nsLineBox.h nsLineList::erase
    pub fn erase(&mut self, mut position: nsLineList_iterator) -> nsLineList_iterator {
        let dead = position.get_mut() as *mut nsLineLink;
        unsafe {
            (*(*dead)._mPrev)._mNext = (*dead)._mNext;
            (*(*dead)._mNext)._mPrev = (*dead)._mPrev;
        }
        #[cfg(debug_assertions)]
        {
            position.operator_inc();
            let next = position;
            // memset(dead, 0, sizeof(*dead));
            unsafe {
                *dead = nsLineLink::default();
            }
            next
        }
        #[cfg(not(debug_assertions))]
        {
            position.operator_inc();
            position
        }
    }

    // Gecko: layout/generic/nsLineBox.h nsLineList::swap
    pub fn swap(&mut self, y: &mut nsLineList) {
        let tmp = *y.mLink;
        *y.mLink = *self.mLink;
        *self.mLink = tmp;

        if !self.empty() {
            unsafe {
                (*self.mLink._mNext)._mPrev = self.Link();
                (*self.mLink._mPrev)._mNext = self.Link();
            }
        }

        if !y.empty() {
            unsafe {
                (*y.mLink._mNext)._mPrev = y.Link();
                (*y.mLink._mPrev)._mNext = y.Link();
            }
        }
    }

    // NOTE:  leaves dangling next/prev pointers
    // Gecko: layout/generic/nsLineBox.h nsLineList::clear
    pub fn clear(&mut self) {
        let link = self.Link();
        self.mLink._mNext = link;
        self.mLink._mPrev = link;
    }

    // inserts the conts of x before position and makes x empty
    // Gecko: layout/generic/nsLineBox.h nsLineList::splice(iterator, self_type&)
    pub fn splice__iterator_nsLineList(&mut self, position: nsLineList_iterator, x: &mut nsLineList) {
        // use |mCurrent| to prevent DEBUG_PASS_END assertions
        unsafe {
            (*(*position.mCurrent)._mPrev)._mNext = x.mLink._mNext;
            (*x.mLink._mNext)._mPrev = (*position.mCurrent)._mPrev;
            (*x.mLink._mPrev)._mNext = position.mCurrent;
            (*position.mCurrent)._mPrev = x.mLink._mPrev;
        }
        x.clear();
    }

    // Inserts element *i from list x before position and removes
    // it from x.
    // Gecko: layout/generic/nsLineBox.h nsLineList::splice(iterator, self_type&, iterator)
    pub fn splice__iterator_nsLineList_iterator(&mut self, position: nsLineList_iterator, x: &mut nsLineList, mut i: nsLineList_iterator) {
        NS_ASSERTION!(!x.empty(), "Can't insert from empty list.");
        NS_ASSERTION!(position != i && position.mCurrent != i._mNext, "We don't check for this case.");

        let i = i.get_mut() as *mut nsLineLink;
        unsafe {
            // remove from |x|
            (*(*i)._mPrev)._mNext = (*i)._mNext;
            (*(*i)._mNext)._mPrev = (*i)._mPrev;

            // use |mCurrent| to prevent DEBUG_PASS_END assertions
            // link into |this|, before-side
            (*i)._mPrev = (*position.mCurrent)._mPrev;
            (*(*position.mCurrent)._mPrev)._mNext = i;

            // link into |this|, after-side
            (*i)._mNext = position.mCurrent;
            (*position.mCurrent)._mPrev = i;
        }
    }

    // Inserts elements in [|first|, |last|), which are in |x|,
    // into |this| before |position| and removes them from |x|.
    // Gecko: layout/generic/nsLineBox.h nsLineList::splice(iterator, self_type&, iterator, iterator)
    pub fn splice__iterator_nsLineList_iterator_iterator(&mut self, position: nsLineList_iterator, x: &mut nsLineList, mut first: nsLineList_iterator, mut last: nsLineList_iterator) {
        NS_ASSERTION!(!x.empty(), "Can't insert from empty list.");

        if first == last {
            return;
        }

        last.operator_dec(); // so we now want to move [first, last]
        let first = first.get_mut() as *mut nsLineLink;
        let last = last.get_mut() as *mut nsLineLink;
        unsafe {
            // remove from |x|
            (*(*first)._mPrev)._mNext = (*last)._mNext;
            (*(*last)._mNext)._mPrev = (*first)._mPrev;

            // use |mCurrent| to prevent DEBUG_PASS_END assertions
            // link into |this|, before-side
            (*first)._mPrev = (*position.mCurrent)._mPrev;
            (*(*position.mCurrent)._mPrev)._mNext = first;

            // link into |this|, after-side
            (*last)._mNext = position.mCurrent;
            (*position.mCurrent)._mPrev = last;
        }
    }
}

// Gecko: layout/generic/nsLineBox.h nsLineList::~nsLineList (MOZ_COUNTED_DTOR: refcount logging only)
// NOT PORTED: refcount logging. The lines are not owned by the list (DeleteLineList frees them).

//----------------------------------------------------------------------

// Gecko: layout/generic/nsLineBox.h nsLineIterator (final : public nsILineIterator)
pub struct nsLineIterator<'a> {
    mLines: &'a nsLineList,
    mIter: nsLineList_const_iterator,
    mIndex: i32,
    mNumLines: std::cell::Cell<i32>, // mutable
    mRightToLeft: bool,
}

impl<'a> nsLineIterator<'a> {
    // Gecko: layout/generic/nsLineBox.h nsLineIterator::nsLineIterator
    pub fn new(aLines: &'a nsLineList, aRightToLeft: bool) -> nsLineIterator<'a> {
        let mut this = nsLineIterator { mLines: aLines, mIter: aLines.begin(), mIndex: -1, mNumLines: std::cell::Cell::new(-1), mRightToLeft: aRightToLeft };
        this.mIter = this.mLines.begin();
        if this.mIter != this.mLines.end() {
            this.mIndex = 0;
        }
        this
    }

    // nsLineIterator() = delete; nsLineIterator(const nsLineIterator& aOther) = delete;
    // (no Default, no Clone)

    // Gecko: layout/generic/nsLineBox.h nsLineIterator::GetNextLine
    fn GetNextLine(&mut self) -> Option<*const nsLineBox> {
        MOZ_ASSERT!(self.mIter != self.mLines.end(), "Already at end!");
        self.mIndex += 1;
        self.mIter.operator_inc();
        if self.mIter == self.mLines.end() {
            MOZ_ASSERT!(self.mNumLines.get() < 0 || self.mNumLines.get() == self.mIndex);
            self.mNumLines.set(self.mIndex);
            return None;
        }
        Some(self.mIter.get())
    }

    // Note that this updates the iterator's current position to the given line.
    // Gecko: layout/generic/nsLineBox.h nsLineIterator::GetLineAt
    fn GetLineAt(&mut self, aIndex: i32) -> Option<*const nsLineBox> {
        MOZ_ASSERT!(self.mIndex >= 0);
        if aIndex < 0 || (self.mNumLines.get() >= 0 && aIndex >= self.mNumLines.get()) {
            return None;
        }
        // Check if we should start counting lines from mIndex, or reset to the
        // start or end of the list and count from there (if the requested index is
        // closer to an end than to the current position).
        if aIndex < self.mIndex / 2 {
            // Reset to the beginning and search from there.
            self.mIter = self.mLines.begin();
            self.mIndex = 0;
        } else if self.mNumLines.get() > 0 && aIndex > (self.mNumLines.get() + self.mIndex) / 2 {
            // Jump to the end and search back from there.
            self.mIter = self.mLines.end();
            self.mIter.operator_dec();
            self.mIndex = self.mNumLines.get() - 1;
        }
        while self.mIndex > aIndex {
            // This cannot run past the start of the list, because we checked that
            // aIndex is non-negative.
            self.mIter.operator_dec();
            self.mIndex -= 1;
        }
        while self.mIndex < aIndex {
            // Here we have to check for reaching the end, as aIndex could be out of
            // range (if mNumLines was not initialized, so we couldn't range-check
            // aIndex on entry).
            if self.mIter == self.mLines.end() {
                MOZ_ASSERT!(self.mNumLines.get() < 0 || self.mNumLines.get() == self.mIndex);
                self.mNumLines.set(self.mIndex);
                return None;
            }
            self.mIter.operator_inc();
            self.mIndex += 1;
        }
        Some(self.mIter.get())
    }
}

impl<'a> nsILineIterator for nsLineIterator<'a> {
    // Gecko: layout/generic/nsLineBox.h nsLineIterator::GetNumLines
    fn GetNumLines(&self) -> i32 {
        if self.mNumLines.get() < 0 {
            self.mNumLines.set(self.mLines.size() as i32); // This is O(N) in number of lines!
        }
        self.mNumLines.get()
    }

    // Gecko: layout/generic/nsLineBox.h nsLineIterator::IsLineIteratorFlowRTL
    fn IsLineIteratorFlowRTL(&mut self) -> bool {
        self.mRightToLeft
    }

    // Note that this updates the iterator's current position!
    // Gecko: layout/generic/nsLineBox.cpp nsLineIterator::GetLine
    fn GetLine(&mut self, aLineNumber: i32) -> Result<nsILineIterator_LineInfo, nsresult> {
        let Some(line) = self.GetLineAt(aLineNumber) else {
            return Err(NS_ERROR_FAILURE);
        };
        let line = unsafe { &*line };
        let mut structure = nsILineIterator_LineInfo::default();
        structure.mFirstFrameOnLine = line.mFirstChild;
        structure.mNumFramesOnLine = line.GetChildCount();
        structure.mLineBounds = line.GetPhysicalBounds();
        structure.mIsWrapped = line.IsLineWrapped();
        Ok(structure)
    }

    // Gecko: layout/generic/nsLineBox.cpp nsLineIterator::FindLineContaining
    fn FindLineContaining(&mut self, aFrame: nsIFrame, aStartLine: i32) -> i32 {
        let mut line = self.GetLineAt(aStartLine);
        MOZ_ASSERT!(line.is_some(), "aStartLine out of range");
        while let Some(l) = line {
            if unsafe { &*l }.Contains(aFrame) {
                return self.mIndex;
            }
            line = self.GetNextLine();
        }
        -1
    }

    // Gecko: layout/generic/nsLineBox.cpp nsLineIterator::FindFrameAt
    fn FindFrameAt(&mut self, aLineNumber: i32, aPos: nsPoint, aFrameFound: &mut Option<nsIFrame>, aPosIsBeforeFirstFrame: &mut bool, aPosIsAfterLastFrame: &mut bool) -> nsresult {
        // MOZ_ASSERT(aFrameFound && aPosIsBeforeFirstFrame && aPosIsAfterLastFrame, "null OUT ptr");
        // if (!aFrameFound || !aPosIsBeforeFirstFrame || !aPosIsAfterLastFrame) return NS_ERROR_NULL_POINTER;
        // NOT PORTED: the null checks (the out-parameters are references).

        let Some(line) = self.GetLineAt(aLineNumber) else {
            *aFrameFound = None;
            *aPosIsBeforeFirstFrame = true;
            *aPosIsAfterLastFrame = false;
            return NS_OK;
        };
        let line = unsafe { &*line };

        if line.ISize() == 0 && line.BSize() == 0 {
            return NS_ERROR_FAILURE;
        }

        let mut finder = LineFrameFinder::new(aPos, line.mContainerSize, line.mWritingMode, self.mRightToLeft);
        for frame in line.ChildFrames() {
            finder.Scan(frame);
            if finder.IsDone() {
                break;
            }
        }
        finder.Finish(aFrameFound, aPosIsBeforeFirstFrame, aPosIsAfterLastFrame);
        NS_OK
    }

    // Gecko: layout/generic/nsLineBox.cpp nsLineIterator::CheckLineOrder
    fn CheckLineOrder(&mut self, aLine: i32, aIsReordered: &mut bool, aFirstVisual: &mut Option<nsIFrame>, aLastVisual: &mut Option<nsIFrame>) -> nsresult {
        let line = self.GetLineAt(aLine);
        MOZ_ASSERT!(line.is_some(), "aLine out of range!");

        let line = line.map(|l| unsafe { &*l });
        if line.is_none() || line.unwrap().mFirstChild.is_none() {
            // empty line
            *aIsReordered = false;
            *aFirstVisual = None;
            *aLastVisual = None;
            return NS_OK;
        }
        let line = line.unwrap();

        // nsIFrame* leftmostFrame;
        // nsIFrame* rightmostFrame;
        // *aIsReordered = nsBidiPresUtils::CheckLineOrder(line->mFirstChild, line->GetChildCount(), &leftmostFrame, &rightmostFrame);
        //
        // // map leftmost/rightmost to first/last according to paragraph direction
        // *aFirstVisual = mRightToLeft ? rightmostFrame : leftmostFrame;
        // *aLastVisual = mRightToLeft ? leftmostFrame : rightmostFrame;
        //
        // return NS_OK;
        // NOT PORTED: nsBidiPresUtils::CheckLineOrder (bidi reordering is outside the stage 2b scope).
        unimplemented!("NOT PORTED: nsBidiPresUtils::CheckLineOrder")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests written from the documented behaviour of nsLineBox and nsLineList (Gecko has no gtest for
    // them; its layout tests exercise them through nsBlockFrame).

    use mrt_style::mrt_dom::{Dom, Kind, NodeId};
    use mrt_style::style::properties::ComputedValues;
    use mrt_style::style::servo_arc::Arc as ServoArc;
    use mrt_style::StyleDom;

    const WM: fn() -> WritingMode = WritingMode::default;

    // Styles of the elements with the given ids, in order.
    fn styles_of(html: &str, ids: &[&str]) -> Vec<ServoArc<ComputedValues>> {
        let mut sd = StyleDom::new(Dom::parse(html), (1489.0, 844.0));
        sd.resolve();
        ids.iter()
            .map(|want| {
                let dom = &sd.tree.dom;
                let node = (0..dom.nodes.len() as NodeId)
                    .find(|&n| dom.node(n).kind == Kind::Element && dom.attrs_of(n).iter().any(|a| dom.names.get(a.name) == "id" && dom.attr_value(a) == *want))
                    .unwrap_or_else(|| panic!("no element with id {want}"));
                sd.computed(node).expect("no computed style")
            })
            .collect()
    }

    // `n` sibling frames: inline-level ones (display: inline) or block-level ones (display: block).
    fn kids(n: usize, block: bool) -> Vec<nsIFrame> {
        let html = format!(
            "<div id=p>{}</div>",
            (0..n).map(|i| format!("<span id=c{i} style=\"display:{}\"></span>", if block { "block" } else { "inline" })).collect::<String>()
        );
        let ids: Vec<String> = std::iter::once("p".to_string()).chain((0..n).map(|i| format!("c{i}"))).collect();
        let ids: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
        let mut s = styles_of(&html, &ids);
        let ps = s.remove(0);
        WithFrameTree(|t| {
            let p = t.AddFrame(None, nsRect::new(0, 0, 400, 400), WM(), Some(ps));
            s.into_iter().map(|st| t.AddFrame(Some(p), nsRect::new(0, 0, 10, 10), WM(), Some(st))).collect()
        })
    }

    fn inline_line(k: &[nsIFrame]) -> *mut nsLineBox {
        let line = NS_NewLineBox__nsIFrame_bool(Some(k[0]), false);
        unsafe {
            (*line).mChildCount = k.len() as u32;
        }
        line
    }

    #[test]
    fn new_line_is_dirty_and_sized_like_its_frames() {
        let k = kids(3, false);
        let line = unsafe { &mut *inline_line(&k) };
        assert!(line.IsDirty() && line.IsInline() && !line.IsBlock());
        assert_eq!(line.GetChildCount(), 3);
        assert_eq!(line.mContainerSize, nsSize::new(-1, -1));
        assert_eq!(line.ChildFrames().into_iter().collect::<Vec<_>>(), k);
        assert_eq!(line.LastChild(), Some(k[2]));
        line.Destroy();
    }

    #[test]
    fn block_line_has_exactly_one_child() {
        let k = kids(1, true);
        let line = unsafe { &mut *NS_NewLineBox__nsIFrame_bool(Some(k[0]), true) };
        assert!(line.IsBlock());
        assert_eq!(line.GetChildCount(), 1);
        line.Destroy();
    }

    #[test]
    fn flag_accessors() {
        let k = kids(1, false);
        let line = unsafe { &mut *NS_NewLineBox__nsIFrame_bool(Some(k[0]), false) };
        line.ClearDirty();
        assert!(!line.IsDirty());
        line.MarkDirty();
        assert!(line.IsDirty());
        line.SetLineWrapped(true);
        assert!(line.IsLineWrapped());
        line.SetHasClearance();
        assert!(line.HasClearance());
        line.ClearHasClearance();
        assert!(!line.HasClearance());
        line.MarkPreviousMarginDirty();
        assert!(line.IsPreviousMarginDirty());
        line.DisableResizeReflowOptimization();
        assert!(line.ResizeReflowOptimizationDisabled());
        line.SetHadFloatPushed();
        line.SetMovedFragments();
        line.SetHasLineClampEllipsis();
        assert!(line.HadFloatPushed() && line.MovedFragments() && line.HasLineClampEllipsis());
        line.Destroy();
    }

    #[test]
    fn clear_types_before_and_after() {
        let k = kids(1, true);
        let block = unsafe { &mut *NS_NewLineBox__nsIFrame_bool(Some(k[0]), true) };
        assert!(!block.HasFloatClearTypeBefore());
        block.SetFloatClearTypeBefore(UsedClear::Left);
        assert_eq!(block.FloatClearTypeBefore(), UsedClear::Left);
        assert_eq!(block.FloatClearTypeAfter(), UsedClear::None);
        assert!(!block.HasForcedLineBreakAfter());
        block.Destroy();

        let k = kids(1, false);
        let inl = unsafe { &mut *NS_NewLineBox__nsIFrame_bool(Some(k[0]), false) };
        inl.SetForcedLineBreakAfter(UsedClear::Both);
        assert!(inl.HasForcedLineBreakAfter() && inl.HasFloatClearTypeAfter());
        assert_eq!(inl.FloatClearTypeAfter(), UsedClear::Both);
        assert_eq!(inl.FloatClearTypeBefore(), UsedClear::None);
        inl.ClearForcedLineBreak();
        assert!(!inl.HasForcedLineBreakAfter() && !inl.HasFloatClearTypeAfter());
        inl.Destroy();
    }

    #[test]
    fn bounds_and_container_size() {
        let k = kids(1, false);
        let line = unsafe { &mut *NS_NewLineBox__nsIFrame_bool(Some(k[0]), false) };
        // Zero bounds are the empty rect whatever the container size.
        assert_eq!(line.GetPhysicalBounds(), nsRect::new(0, 0, 0, 0));
        line.SetBounds(WM(), 5, 10, 100, 20, nsSize::new(400, 400));
        assert_eq!((line.IStart(), line.BStart(), line.ISize(), line.BSize(), line.IEnd(), line.BEnd()), (5, 10, 100, 20, 105, 30));
        assert_eq!(line.GetPhysicalBounds(), nsRect::new(5, 10, 100, 20));
        line.SlideBy(7, nsSize::new(400, 400));
        assert_eq!(line.BStart(), 17);
        line.IndentBy(3, nsSize::new(400, 400));
        assert_eq!(line.IStart(), 8);
        line.ExpandBy(4, nsSize::new(400, 400));
        assert_eq!(line.ISize(), 104);
        line.SetBoundsEmpty();
        assert_eq!(line.GetPhysicalBounds(), nsRect::new(0, 0, 0, 0));
        line.SetLogicalAscent(12);
        assert_eq!(line.GetLogicalAscent(), 12);
        line.Destroy();
    }

    #[test]
    fn overflow_areas_are_stored_only_when_they_differ_from_the_bounds() {
        let k = kids(1, false);
        let line = unsafe { &mut *NS_NewLineBox__nsIFrame_bool(Some(k[0]), false) };
        line.SetBounds(WM(), 0, 0, 100, 20, nsSize::new(400, 400));
        let bounds = line.GetPhysicalBounds();
        assert_eq!(line.GetOverflowAreas(), OverflowAreas::new(bounds, bounds));
        assert!(line.mData.is_none());

        let big = OverflowAreas::new(nsRect::new(-5, 0, 120, 25), nsRect::new(0, 0, 130, 20));
        line.SetOverflowAreas(&big);
        assert!(line.mData.is_some());
        assert_eq!(line.InkOverflowRect(), nsRect::new(-5, 0, 120, 25));
        assert_eq!(line.ScrollableOverflowRect(), nsRect::new(0, 0, 130, 20));
        assert_eq!(line.GetOverflowArea__OverflowType(OverflowType::Ink), nsRect::new(-5, 0, 120, 25));

        // Sliding moves the stored overflow areas by the same amount.
        line.SlideBy(10, nsSize::new(400, 400));
        assert_eq!(line.InkOverflowRect(), nsRect::new(-5, 10, 120, 25));

        // Setting them back to the bounds frees the extra data.
        let bounds = line.GetPhysicalBounds();
        line.SetOverflowAreas(&OverflowAreas::new(bounds, bounds));
        assert!(line.mData.is_none());
        line.Destroy();
    }

    #[test]
    fn in_flow_child_bounds() {
        let k = kids(1, false);
        let line = unsafe { &mut *NS_NewLineBox__nsIFrame_bool(Some(k[0]), false) };
        line.SetBounds(WM(), 0, 0, 100, 20, nsSize::new(400, 400));
        assert_eq!(line.GetInFlowChildBounds(), None);
        line.SetInFlowChildBounds(Some(nsRect::new(1, 2, 3, 4)));
        assert_eq!(line.GetInFlowChildBounds(), Some(nsRect::new(1, 2, 3, 4)));
        line.SlideBy(5, nsSize::new(400, 400));
        assert_eq!(line.GetInFlowChildBounds(), Some(nsRect::new(1, 7, 3, 4)));
        line.SetInFlowChildBounds(None);
        assert!(line.mData.is_none());
        line.Destroy();
    }

    #[test]
    fn carried_out_bend_margin() {
        let k = kids(1, true);
        let line = unsafe { &mut *NS_NewLineBox__nsIFrame_bool(Some(k[0]), true) };
        assert!(line.GetCarriedOutBEndMargin().IsZero());
        // Setting zero when nothing is stored changes nothing and allocates nothing.
        assert!(!line.SetCarriedOutBEndMargin(CollapsingMargin::default()));
        assert!(line.mData.is_none());

        let mut m = CollapsingMargin::default();
        m.Include__nscoord(30);
        assert!(line.SetCarriedOutBEndMargin(m)); // changed
        assert_eq!(line.GetCarriedOutBEndMargin().Get(), 30);
        assert!(!line.SetCarriedOutBEndMargin(m)); // same value: not changed

        // Back to zero: changed, and the extra data is freed.
        assert!(line.SetCarriedOutBEndMargin(CollapsingMargin::default()));
        assert!(line.mData.is_none());
        line.Destroy();
    }

    #[test]
    fn floats_and_float_edges_of_inline_lines() {
        let k = kids(3, false);
        let line = unsafe { &mut *inline_line(&k) };
        assert!(!line.HasFloats());
        line.AppendFloats(vec![k[0]]);
        line.AppendFloats(vec![k[1]]);
        line.AppendFloats(Vec::new()); // nothing to append
        assert!(line.HasFloats());
        assert_eq!(line.Floats(), &vec![k[0], k[1]]);
        assert!(!line.RemoveFloat(k[2]));
        assert!(line.RemoveFloat(k[0]));
        assert_eq!(line.Floats(), &vec![k[1]]);
        line.ClearFloats();
        assert!(!line.HasFloats());
        assert!(line.mData.is_none());

        let (mut s, mut e) = (0, 0);
        assert!(!line.GetFloatEdges(&mut s, &mut e));
        line.SetFloatEdges(5, 95);
        assert!(line.GetFloatEdges(&mut s, &mut e));
        assert_eq!((s, e), (5, 95));
        line.ClearFloatEdges();
        assert!(!line.GetFloatEdges(&mut s, &mut e));
        line.Destroy();
    }

    #[test]
    fn index_of_contains_and_rl_index_of() {
        let k = kids(5, false);
        let line = unsafe { &mut *inline_line(&k) };
        for (i, f) in k.iter().enumerate() {
            assert_eq!(line.IndexOf(*f), i as i32);
            assert_eq!(line.RLIndexOf(*f, Some(k[4])), i as i32);
            assert!(line.Contains(*f));
        }
        let other = kids(1, false);
        assert_eq!(line.IndexOf(other[0]), -1);
        assert_eq!(line.RLIndexOf(other[0], Some(k[4])), -1);
        assert!(!line.Contains(other[0]));
        line.Destroy();
    }

    #[test]
    fn note_frame_added_and_removed_switch_to_a_hash_table_at_200_children() {
        let k = kids(1, false);
        let line = unsafe { &mut *NS_NewLineBox__nsIFrame_bool(Some(k[0]), false) };
        let many = kids(250, false);
        line.mFirstChild = Some(many[0]);
        line.mChildCount = 0;
        for f in &many[..199] {
            line.NoteFrameAdded(*f);
        }
        assert!(!line.mFlags.mHasHashedFrames);
        assert_eq!(line.GetChildCount(), 199);
        line.NoteFrameAdded(many[199]);
        assert!(line.mFlags.mHasHashedFrames);
        assert_eq!(line.GetChildCount(), 200);
        assert!(line.Contains(many[7]));
        assert!(!line.Contains(many[240]));
        line.NoteFrameRemoved(many[199]);
        assert!(!line.mFlags.mHasHashedFrames);
        assert_eq!(line.GetChildCount(), 199);
        line.Destroy();
    }

    #[test]
    fn new_line_from_another_line_moves_frame_counts() {
        let k = kids(6, false);
        let from = inline_line(&k);
        // Move the last 2 frames to a new line.
        let to = NS_NewLineBox__nsLineBox_nsIFrame_int32_t(from, Some(k[4]), 2);
        unsafe {
            assert_eq!((*from).GetChildCount(), 4);
            assert_eq!((*to).GetChildCount(), 2);
            assert_eq!((*to).mContainerSize, (*from).mContainerSize);
            (*from).Destroy();
            (*to).Destroy();
        }
    }

    #[test]
    fn is_empty_and_cached_is_empty() {
        let k = kids(2, false);
        let line = unsafe { &mut *inline_line(&k) };
        // Frames are not empty unless they say so. (The test frames are blocks, and an empty block is
        // empty, so they say so explicitly through SetIsEmpty.)
        k[0].SetIsEmpty(false);
        k[1].SetIsEmpty(false);
        assert!(!line.IsEmpty());
        k[0].SetIsEmpty(true);
        assert!(!line.IsEmpty());
        k[1].SetIsEmpty(true);
        assert!(line.IsEmpty());
        // A marker makes an inline line non-empty.
        line.SetHasMarker();
        assert!(!line.IsEmpty());
        line.ClearHasMarker();
        assert!(line.IsEmpty());

        // CachedIsEmpty reads through while the line is dirty, then caches.
        assert!(line.IsDirty());
        assert!(line.CachedIsEmpty());
        assert!(!line.IsValidCachedIsEmpty());
        line.ClearDirty();
        assert!(line.CachedIsEmpty());
        assert!(line.IsValidCachedIsEmpty());
        line.SetHasMarker(); // invalidates the cache
        assert!(!line.IsValidCachedIsEmpty());
        assert!(!line.CachedIsEmpty());
        line.Destroy();

        // A block line is empty when its frame is.
        let b = kids(1, true);
        let bline = unsafe { &mut *NS_NewLineBox__nsIFrame_bool(Some(b[0]), true) };
        b[0].SetIsEmpty(false);
        assert!(!bline.IsEmpty());
        b[0].SetIsEmpty(true);
        assert!(bline.IsEmpty());
        bline.Destroy();
    }

    fn list_of(lines: &[*mut nsLineBox]) -> nsLineList {
        let mut l = nsLineList::new();
        for &line in lines {
            l.push_back(line);
        }
        l
    }

    fn lines(n: usize) -> Vec<*mut nsLineBox> {
        let k = kids(n, false);
        k.iter().map(|f| NS_NewLineBox__nsIFrame_bool(Some(*f), false)).collect()
    }

    fn order(l: &nsLineList) -> Vec<*const nsLineBox> {
        let mut v = Vec::new();
        let mut it = l.begin();
        while it != l.end() {
            v.push(it.get());
            it.operator_inc();
        }
        v
    }

    fn free(ls: &[*mut nsLineBox]) {
        for &l in ls {
            unsafe { (*l).Destroy() };
        }
    }

    #[test]
    fn line_list_push_pop_front_back() {
        let ls = lines(3);
        let mut l = nsLineList::new();
        assert!(l.empty());
        assert_eq!(l.size(), 0);
        l.push_back(ls[1]);
        l.push_front(ls[0]);
        l.push_back(ls[2]);
        assert_eq!(l.size(), 3);
        assert_eq!(l.front(), ls[0] as *const nsLineBox);
        assert_eq!(l.back(), ls[2] as *const nsLineBox);
        assert_eq!(order(&l), vec![ls[0] as *const nsLineBox, ls[1], ls[2]]);
        l.pop_front();
        assert_eq!(l.front(), ls[1] as *const nsLineBox);
        l.pop_back();
        assert_eq!(l.back(), ls[1] as *const nsLineBox);
        l.pop_back();
        assert!(l.empty());
        free(&ls);
    }

    #[test]
    fn line_list_iterators_forward_and_reverse() {
        let ls = lines(3);
        let mut l = list_of(&ls);
        let mut it = l.begin_mut();
        assert!(it.IsInline());
        it.operator_inc();
        assert_eq!(it.get_mut(), ls[1]);
        assert_eq!(it.prev().get_mut(), ls[0]);
        assert_eq!(it.next_().get_mut(), ls[2]);
        // Mutating through a non-const iterator.
        it.ClearDirty();
        assert!(!unsafe { &*ls[1] }.IsDirty());
        let mut rv = Vec::new();
        let mut r = l.rbegin_mut();
        while r != l.rend_mut() {
            rv.push(r.get_mut());
            r.operator_inc();
        }
        assert_eq!(rv, vec![ls[2], ls[1], ls[0]]);
        // begin(aLine) starts at a given line.
        let at = l.begin_mut__nsLineBox(ls[2]);
        assert_eq!(at.next_(), l.end_mut());
        free(&ls);
    }

    #[test]
    fn line_list_insert_and_erase() {
        let ls = lines(4);
        let mut l = list_of(&[ls[0], ls[3]]);
        let pos = l.begin_mut().next_(); // ls[3]
        let it = l.before_insert(pos, ls[2]);
        assert_eq!(it.get(), ls[2] as *const nsLineBox);
        let p0 = l.begin_mut();
        let it = l.after_insert(p0, ls[1]);
        assert_eq!(it.get(), ls[1] as *const nsLineBox);
        assert_eq!(order(&l), vec![ls[0] as *const nsLineBox, ls[1], ls[2], ls[3]]);
        let second = l.begin_mut().next_();
        let after = l.erase(second);
        assert_eq!(after.get(), ls[2] as *const nsLineBox);
        assert_eq!(order(&l), vec![ls[0] as *const nsLineBox, ls[2], ls[3]]);
        free(&ls);
    }

    #[test]
    fn line_list_swap_clear_and_splice() {
        let ls = lines(5);
        let mut a = list_of(&[ls[0], ls[1]]);
        let mut b = list_of(&[ls[2], ls[3], ls[4]]);
        a.swap(&mut b);
        assert_eq!(order(&a), vec![ls[2] as *const nsLineBox, ls[3], ls[4]]);
        assert_eq!(order(&b), vec![ls[0] as *const nsLineBox, ls[1]]);
        // The swapped lists still link back to their own sentinels.
        assert_eq!(a.rbegin().get(), ls[4] as *const nsLineBox);
        assert_eq!(b.begin().next_().next_(), b.end());

        // Splice the whole of b before the end of a.
        let end = a.end_mut();
        a.splice__iterator_nsLineList(end, &mut b);
        assert!(b.empty());
        assert_eq!(a.size(), 5);

        free(&ls);
    }

    #[test]
    fn line_list_splice_range() {
        // (splice of one element between two lists is not tested: its assertion `position != i`
        // compares iterators of different lists, which the iterator asserts against, in C++ too.)
        let ls = lines(5);
        let mut src = list_of(&ls);
        let mut dst = nsLineList::new();
        dst.push_back(NS_NewLineBox__nsIFrame_bool(Some(kids(1, false)[0]), false));
        src.pop_front(); // dst gets ls[1..4)
        // Move [ls[2], ls[4]) to the end of dst.
        let d = dst.end_mut();
        let first = src.begin_mut(); // ls[1]
        let last = src.end_mut().prev(); // ls[4]
        dst.splice__iterator_nsLineList_iterator_iterator(d, &mut src, first, last);
        assert_eq!(order(&src), vec![ls[4] as *const nsLineBox]);
        assert_eq!(dst.size(), 4);
        assert_eq!(dst.back(), ls[3] as *const nsLineBox);
        // An empty range moves nothing.
        let s = src.begin_mut();
        let d = dst.end_mut();
        dst.splice__iterator_nsLineList_iterator_iterator(d, &mut src, s, s);
        assert_eq!(src.size(), 1);
        free(&ls);
    }

    #[test]
    fn delete_line_list_destroys_frames_last_to_first() {
        let k = kids(4, false);
        let l1 = NS_NewLineBox__nsIFrame_bool(Some(k[0]), false);
        unsafe { (*l1).mChildCount = 2 };
        let l2 = NS_NewLineBox__nsIFrame_bool(Some(k[2]), false);
        unsafe { (*l2).mChildCount = 2 };
        let mut lines = list_of(&[l1, l2]);
        let mut frames = nsFrameList::new(Some(k[0]), Some(k[3]));
        let mut ctx = FrameDestroyContext::default();
        nsLineBox::DeleteLineList(nsPresContext::Current(), &mut lines, &mut frames, &mut ctx);
        assert!(lines.empty() && frames.IsEmpty());
        assert_eq!(ctx.mDestroyedFrames, vec![k[3], k[2], k[1], k[0]]);
    }

    #[test]
    fn r_find_line_containing() {
        let k = kids(5, false);
        let l1 = NS_NewLineBox__nsIFrame_bool(Some(k[0]), false);
        unsafe { (*l1).mChildCount = 2 };
        let l2 = NS_NewLineBox__nsIFrame_bool(Some(k[2]), false);
        unsafe { (*l2).mChildCount = 3 };
        let mut l = list_of(&[l1, l2]);
        let begin = l.begin_mut();
        for (frame, line, index) in [(k[4], l2, 2), (k[2], l2, 0), (k[1], l1, 1), (k[0], l1, 0)] {
            let mut end = l.end_mut();
            let mut idx = 0;
            assert!(nsLineBox::RFindLineContaining(frame, &begin, &mut end, Some(k[4]), &mut idx));
            assert_eq!(end.get(), line as *const nsLineBox);
            assert_eq!(idx, index);
        }
        let other = kids(1, false)[0];
        let mut end = l.end_mut();
        let mut idx = 0;
        assert!(!nsLineBox::RFindLineContaining(other, &begin, &mut end, Some(k[4]), &mut idx));
        assert_eq!(idx, -1);
        free(&[l1, l2]);
    }

    #[test]
    fn line_iterator_numbering_and_lookup() {
        let k = kids(6, false);
        let mut ls = Vec::new();
        for i in 0..3 {
            let line = NS_NewLineBox__nsIFrame_bool(Some(k[i * 2]), false);
            unsafe {
                (*line).mChildCount = 2;
                (*line).SetBounds(WM(), 0, (i as nscoord) * 20, 100, 20, nsSize::new(400, 400));
            }
            ls.push(line);
        }
        let l = list_of(&ls);
        let mut it = nsLineIterator::new(&l, false);
        assert_eq!(it.GetNumLines(), 3);
        assert!(!it.IsLineIteratorFlowRTL());
        let info = it.GetLine(1).unwrap();
        assert_eq!(info.mFirstFrameOnLine, Some(k[2]));
        assert_eq!(info.mNumFramesOnLine, 2);
        assert_eq!(info.mLineBounds, nsRect::new(0, 20, 100, 20));
        assert_eq!(info.GetLastFrameOnLine(), Some(k[3]));
        assert!(it.GetLine(3).is_err());
        assert!(it.GetLine(-1).is_err());
        assert_eq!(it.FindLineContaining(k[5], 0), 2);
        assert_eq!(it.FindLineContaining(k[0], 0), 0);
        assert_eq!(it.FindLineContaining(k[3], 2), -1);

        // FindFrameAt: frames at x 0..10, 0..10 (same rect in this test), a position inside finds one.
        let mut found = None;
        let (mut before, mut after) = (false, false);
        assert_eq!(it.FindFrameAt(1, nsPoint::new(5, 0), &mut found, &mut before, &mut after), NS_OK);
        assert!(found.is_some() && !before && !after);
        assert_eq!(it.FindFrameAt(7, nsPoint::new(5, 0), &mut found, &mut before, &mut after), NS_OK);
        assert!(found.is_none() && before && !after);
        free(&ls);
    }
}
