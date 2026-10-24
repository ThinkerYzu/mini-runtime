// Translated from layout/tables/nsITableLayoutStrategy.h (GECKO_REV).
/*
 * interface for the set of algorithms that determine column and table
 * isizes
 */
//
// Rust shape: the abstract class is a trait; the `Type` it keeps is the small base struct
// `nsITableLayoutStrategy_Base` that the two strategies hold in a field (and Deref to), as the
// inheritance rule of CONVENTIONS.md says. A table frame holds `Option<Box<dyn nsITableLayoutStrategy>>`.
// gfxContext* is `Option<gfxContext>`.
use crate::*;

// Gecko: layout/tables/nsITableLayoutStrategy.h nsITableLayoutStrategy::Type
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum nsITableLayoutStrategy_Type {
    Auto,
    Fixed,
}

// Gecko: layout/tables/nsITableLayoutStrategy.h nsITableLayoutStrategy
pub trait nsITableLayoutStrategy {
    /** Implement nsIFrame::GetMinISize for the table */
    fn GetMinISize(&mut self, aRenderingContext: Option<gfxContext>) -> nscoord;

    /** Implement nsIFrame::GetPrefISize for the table */
    fn GetPrefISize(&mut self, aRenderingContext: Option<gfxContext>, aComputingSize: bool) -> nscoord;

    /** Implement nsIFrame::MarkIntrinsicISizesDirty for the table */
    fn MarkIntrinsicISizesDirty(&mut self);

    /**
     * Compute final column isizes based on the intrinsic isize data and
     * the available isize.
     */
    fn ComputeColumnISizes(&mut self, aReflowInput: &ReflowInput);

    /**
     * Return the type of table layout strategy, without the cost of
     * a virtual function call
     */
    fn GetType(&self) -> nsITableLayoutStrategy_Type;
}

// Gecko: layout/tables/nsITableLayoutStrategy.h nsITableLayoutStrategy::mType (and GetType, the
// protected constructor)
#[derive(Clone, Copy, Debug)]
pub struct nsITableLayoutStrategy_Base {
    mType: nsITableLayoutStrategy_Type,
}

impl nsITableLayoutStrategy_Base {
    // Gecko: layout/tables/nsITableLayoutStrategy.h nsITableLayoutStrategy::nsITableLayoutStrategy
    pub fn new(aType: nsITableLayoutStrategy_Type) -> nsITableLayoutStrategy_Base {
        nsITableLayoutStrategy_Base { mType: aType }
    }

    // Gecko: layout/tables/nsITableLayoutStrategy.h nsITableLayoutStrategy::GetType
    pub fn GetType(&self) -> nsITableLayoutStrategy_Type {
        self.mType
    }
}
