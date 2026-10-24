# mrt-text

Fonts and text shaping for the mini runtime (text phase 1a). Firefox's HarfBuzz, vendored, plus Rust types named after the Gecko classes they stand in for.

## Layout

| Path | What |
|---|---|
| `vendor/harfbuzz/` | Firefox's `gfx/harfbuzz/src`, copied unchanged by `vendor.sh` (headers, `OT/`, and the 47 `.cc` files in `sources.txt`; revision in `FIREFOX_REV`). |
| `shim/mrt_text.{h,cc}` | 100 lines of C++: `mrt_create_hb_font` (gfxHarfBuzzShaper::CreateHBFont) and `mrt_shape_utf16` (the hb_shape part of ShapeText). |
| `build.rs` | Compiles the vendored files and the shim with the C++ compiler directly (`$CXX`, else `~/.mozbuild/clang/bin/clang++`, else `c++`), in parallel, into a static library. Same defines as Firefox's `moz.build`, except HarfBuzz's own Unicode tables are used. No crates, like `parser/build.sh`. |
| `src/harfbuzz.rs` | Hand-written declarations of the HarfBuzz and shim functions used. |
| `src/gfx/thebes/gfxFcPlatformFontList.rs` | `fc-match` lookup by family list, weight, style; cached. |
| `src/gfx/thebes/gfxFont.rs` | `gfxFontEntry`, `gfxFont`, `gfxFont_Metrics`, `gfxFontStyle`, `ShapedRun` (with `mClumps`, the character-to-glyph groups mrt-layout fills its glyph records from). |
| `src/gfx/src/nsFontMetrics.rs` | Metrics in app units (60 per CSS px) with Gecko's rounding. |
| `tools/crosscheck.py` | Compares widths, line height, baseline and `ex` with a real Firefox (`tools/firefox_text.py` drives `obj-hib` over Marionette). |

## Use

```
CARGO_TARGET_DIR=~/.cache/mrt-tgt-H cargo test --offline
CARGO_TARGET_DIR=~/.cache/mrt-tgt-H cargo run --offline --example textdump "Noto Sans" 16 400
CARGO_TARGET_DIR=~/.cache/mrt-tgt-H cargo build --offline --example textdump && tools/crosscheck.py
./vendor.sh [path/to/firefox/tree]     # refresh vendor/
```

```rust
let font = gfxFont::Lookup(&["Helvetica", "sans-serif"], gfxFontStyle { size: 16.0, weight: 400.0, italic: false }).unwrap();
let fm = nsFontMetrics::new(&font);
fm.MaxAscent(); fm.MaxDescent(); fm.ExternalLeading(); fm.XHeight(); fm.SpaceWidth();   // nscoord
fm.GetWidthUtf8("Hello world");                                                          // nscoord
let run = font.ShapeText(&utf16, /*rtl*/ false, Some("en"));                             // run.mCharAdvance, run.mWidth
```
