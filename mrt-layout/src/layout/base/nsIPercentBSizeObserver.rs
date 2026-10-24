// Translated from layout/base/nsIPercentBSizeObserver.h (GECKO_REV).
use crate::*;

/**
 * This interface is supported by frames that need to provide computed bsize
 * values to children during reflow which would otherwise not happen. Currently
 * only table cells support this.
 */
// Gecko: layout/base/nsIPercentBSizeObserver.h nsIPercentBSizeObserver
// (NOT PORTED: NS_DECL_QUERYFRAME_TARGET, the do_QueryFrame machinery)
pub trait nsIPercentBSizeObserver {
    // Notify the observer that aReflowInput has no computed bsize,
    // but it has a percent bsize
    // Gecko: layout/base/nsIPercentBSizeObserver.h nsIPercentBSizeObserver::NotifyPercentBSize
    fn NotifyPercentBSize(&self, aReflowInput: &ReflowInput);

    // Ask the observer if it should observe aReflowInput.frame
    // Gecko: layout/base/nsIPercentBSizeObserver.h nsIPercentBSizeObserver::NeedsToObserve
    fn NeedsToObserve(&self, aReflowInput: &ReflowInput) -> bool;
}
