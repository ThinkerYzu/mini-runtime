// Gecko: xpcom/base/ErrorList.h (nsresult); only the success code is used so far.
use crate::*;

pub type nsresult = u32;
pub const NS_OK: nsresult = 0;
// Gecko: xpcom/base/ErrorList.h NS_ERROR_FAILURE (NS_ERROR_GENERATE_FAILURE(NS_ERROR_MODULE_XPCOM, 0x4005))
pub const NS_ERROR_FAILURE: nsresult = 0x80004005;
// Gecko: xpcom/base/nsError.h NS_FAILED (the high bit of an nsresult is the error bit)
pub fn NS_FAILED(_rv: nsresult) -> bool {
    (_rv & 0x80000000) != 0
}
// (xpcom/base/ErrorList.h NS_ERROR_OUT_OF_MEMORY; a constant, so no drift marker)
pub const NS_ERROR_OUT_OF_MEMORY: nsresult = 0x8007000e;
