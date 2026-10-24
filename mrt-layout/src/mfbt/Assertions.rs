// Gecko: mfbt/Assertions.h (MOZ_ASSERT and friends), xpcom/base/nsDebug.h (NS_ASSERTION, NS_WARNING)
// Only the macros the translated layout code uses. Same argument order as the C++ macros.

#[macro_export]
macro_rules! MOZ_ASSERT {
    ($cond:expr $(,)?) => { debug_assert!($cond) };
    ($cond:expr, $msg:expr $(,)?) => { debug_assert!($cond, "{}", $msg) };
}

#[macro_export]
macro_rules! MOZ_ASSERT_UNREACHABLE {
    ($msg:expr $(,)?) => { debug_assert!(false, "{}", $msg) };
}

#[macro_export]
macro_rules! NS_ASSERTION {
    ($cond:expr, $msg:expr $(,)?) => { debug_assert!($cond, "{}", $msg) };
}

#[macro_export]
macro_rules! NS_WARNING {
    ($msg:expr $(,)?) => { if cfg!(debug_assertions) { eprintln!("WARNING: {}", $msg) } };
}

#[macro_export]
macro_rules! NS_WARNING_ASSERTION {
    ($cond:expr, $msg:expr $(,)?) => { if cfg!(debug_assertions) && !($cond) { eprintln!("WARNING: {}", $msg) } };
}

#[macro_export]
macro_rules! MOZ_CRASH {
    ($msg:expr $(,)?) => { panic!("MOZ_CRASH: {}", $msg) };
}

#[macro_export]
macro_rules! NS_ERROR {
    ($msg:expr $(,)?) => { debug_assert!(false, "{}", $msg) };
}
