# Compression build inputs

Compression uses the public `libz-rs-sys` and `zlib-rs` crates, both pinned to `=0.6.8` in mrt-js/Cargo.toml, with registry archive checksums in Cargo.lock. No Rust compression implementation is vendored or supplied by the engine SDK. Upstream: <https://github.com/trifectatechfoundation/zlib-rs>, Zlib license.

Features remain `custom-prefix` and `avx512`. `mrt-js/.cargo/config.toml` sets `LIBZ_RS_SYS_PREFIX=MOZ_Z_`; the C++ glue calls that existing ABI. The public 0.6.8 source already includes the single unaligned 2-byte distance store that avoids the Intel Raptor Lake erratum, so Firefox's older vendored patch is not needed here. All 88 compression fixture lines, including format bytes, multi-write streams and errors, match Firefox 157.0.1 after this change.

The three source-local `zlib-headers/{zlib.h,zconf.h,mozzconf.h}` files are the C interface used by the existing glue, copied from Firefox `modules/zlib/src` at public Git revision `a8d334e3bde6e6572e3d162db5202def666e6206`. The zlib copyright/license notice is in zlib.h; Mozilla's symbol-prefix header has an MPL-2.0 notice. Retain these notices. These headers are separate from the SpiderMonkey SDK. No system zlib implementation is substituted for CompressionStream.
