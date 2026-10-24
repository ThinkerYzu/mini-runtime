# Build and initial setup

This guide prepares the binaries and environment recording used by the [README quick start](../README.md#quick-start). The supported development path described here is Linux with a compatible Firefox source checkout. The repository does not currently provide a portable installer or prebuilt runtime distribution.

## Native prerequisites

Install these through your development environment or distribution:

- A Rust toolchain supporting edition 2024, Cargo, a C++20 compiler (`clang++`), `make` and `ar`.
- Python 3 for the driver and HTTP service. Standalone runs need only its standard library.
- Fontconfig's `fc-match` and installed fonts for font selection.
- Brotli headers and libraries (`brotlienc`, `brotlidec`), plus the C++ standard library.
- EGL and OpenGL ES development libraries (`libEGL`, `libGLESv2`) and an OpenGL ES 3 implementation. Mesa llvmpipe is the tested software-rendering configuration.
- For initial recording and browser tests: a Firefox executable, a compatible Firefox source tree with `mach python` and Marionette available, and Xvfb if running headless.

Useful checks are `rustc --version`, `cargo --version`, `clang++ --version`, `python3 --version` and `fc-match serif`. These commands verify tools, not complete source compatibility. Firefox's own source-build prerequisites must also be installed.

## Build SpiderMonkey

The runtime links Firefox's JavaScript engine as a shared library and uses that build's generated headers. An installed browser executable alone does not supply them. Obtain the source and prepare build dependencies using Mozilla's [Linux source setup guide](https://firefox-source-docs.mozilla.org/setup/linux_build.html). For an existing checkout, its `./mach bootstrap` workflow prepares the development environment. Choose full source-build prerequisites rather than an artifact-only browser setup: this project compiles against engine headers and libraries.

From the Firefox source root, create a dedicated build configuration:

```sh
cat > mozconfig-mini-runtime <<'CONFIG'
ac_add_options --enable-project=js
ac_add_options --enable-optimize
ac_add_options --disable-debug
ac_add_options --disable-jemalloc
mk_add_options MOZ_OBJDIR=@TOPSRCDIR@/obj-js
CONFIG
MOZCONFIG="$PWD/mozconfig-mini-runtime" ./mach build
```

The required outputs are:

- `obj-js/dist/include/`: generated SpiderMonkey and Mozilla headers.
- `obj-js/js/src/js-confdefs.h`: engine build definitions.
- `obj-js/dist/bin/libmozjs-*.so`: the shared JavaScript library.

Check these before building the runtime. A static-only JavaScript library is insufficient: its bundled Rust code conflicts with this application's Rust linkage.

The tested engine in this development setup is `libmozjs-154a1.so`. SpiderMonkey embedding interfaces and Firefox's vendored Rust crates are version-sensitive; arbitrary newer source revisions are not guaranteed to compile unchanged. `mrt-layout/GECKO_REV` records the source revision used for translated layout code, and `mrt-text/vendor/harfbuzz/FIREFOX_REV` records the vendored text source. Those files describe component provenance, not a universal engine compatibility promise.

## Point the runtime at your Firefox checkout

From the mini-runtime repository root:

```sh
export MINI_RUNTIME="$PWD"
export FIREFOX_SRC=/absolute/path/to/firefox/source
export MRT_JS_OBJDIR="$FIREFOX_SRC/obj-js"
export JS="$MRT_JS_OBJDIR"
export CXX="$(command -v clang++)"
```

`MRT_JS_OBJDIR` configures the JavaScript glue build. `JS` configures the parser build's header path. Both variables must be set explicitly for your engine build.

Three dependency entries in `mrt-js/Cargo.toml` default to `../../firefox-source`: `libz-rs-sys`, `zlib-rs` and its patch entry. Change them to your checkout if it is elsewhere. On a fresh checkout, this command makes the replacement:

```sh
python3 - <<'PY'
import os
from pathlib import Path
p = Path('mrt-js/Cargo.toml')
p.write_text(p.read_text().replace(
    '../../firefox-source', os.environ['FIREFOX_SRC']))
PY
```

This intentionally edits your local manifest. If you previously changed it to another checkout, update those three entries directly. The Firefox tree must contain the referenced `third_party/rust/libz-rs-sys` and `third_party/rust/zlib-rs` crates with compatible APIs.

## Build mini-runtime and the API process

```sh
(cd mrt-js && cargo build --release --bin realsite)
(cd mrt-api && cargo build --release)
```

Run the first command inside `mrt-js`: Cargo must read `mrt-js/.cargo/config.toml`, which sets Firefox's zlib symbol prefix. Using `--manifest-path mrt-js/Cargo.toml` from the root does not load that directory's Cargo configuration. Each crate has its own manifest and target directory; there is no root workspace.

The DOM crate's build script compiles the vendored parser automatically. The text crate compiles vendored HarfBuzz. Cargo fetches Rust dependencies on the first build; `--offline` works only after those dependencies are cached.

The JavaScript build embeds a runtime search path for its shared library. Keep the SpiderMonkey build available after compilation. Rebuilding the runtime does not update an existing service: stop and start a new service to use the new binary.

## Recording the browser environment

`API_ENV` supplies values that would normally come from a browser: navigator identity, interface names, screen information and viewport dimensions. It is a JSON file with an `entries` object keyed by `secure=0|1,isolated=0|1` and a two-element `viewport` array. These values are recorded from Firefox; a made-up minimal fixture is suitable for tests, not a general live-site environment.

Start the demo HTTP server as shown in the README, then use its one-time recording command. The harness runs Firefox under Marionette with `API_PROCESS=all`, records the relevant environment and completes a finite runtime run. It saves `bench/realsites-env/<firefox-build-id>.json` in this source checkout. It does not need an HTTP viewer during recording.

For a different context, repeat the recording command with that source page in `EXTRA_SITES`. For example, an HTTPS page generally needs a secure, non-isolated entry. A page using cross-origin isolation needs its own matching entry. Standalone mode fails explicitly if an entry is missing; it never starts Firefox to fill it.

Use the source tree's `./mach python` for this step. Check that its Python environment can import `marionette_driver`; install the Firefox checkout's automation dependencies if it cannot. `FIREFOX_BIN` names an executable, not the source directory or a launcher command with arguments. A normal compatible Firefox installation can be used; building the full browser is not required just to display the standalone viewer.

The demo uses a fresh empty profile. For a login-based site, prepare a separate Firefox profile and log in normally, then select it with `REALSITES_PROFILE`. The harness seeds supported profile data into its own stores. Standalone execution does not refresh expired authentication.

Environment recordings and browser profiles are local generated artifacts. They are not supplied with a fresh clone. By default the driver selects the newest matching recording from `bench/realsites-env/`; `API_ENV=/absolute/path/to/file.json` selects one explicitly. Live runs and replay recordings use different filename suffixes.

## Troubleshooting

| Symptom | Check |
|---|---|
| No `libmozjs-*.so` or missing headers | Build the shared engine and set both `MRT_JS_OBJDIR` and `JS` to its object directory. |
| Cargo cannot find `libz-rs-sys` or `zlib-rs` | Update all three Firefox dependency paths in `mrt-js/Cargo.toml`. |
| Undefined `MOZ_Z_` compression symbols | Build from inside `mrt-js` so its `.cargo/config.toml` is read. |
| Parser compilation errors or unresolved `mrt_parser_*` | Check compiler output and generated Firefox headers. The parser shell build can continue after compiler errors, so its shell exit code alone is insufficient. |
| Missing `EGL`, `GLESv2`, Brotli or C++ libraries | Install the corresponding development libraries; verify the compiler/linker search paths. |
| No matching fonts | Install Fontconfig and fonts; check `fc-match serif`. |
| `NO_FIREFOX=1 needs an existing browser environment recording` | Run the one-time recording step or set `API_ENV` to an existing recording. |
| Missing secure/isolated context | Record a page with the required context using the Firefox harness. |
| `marionette_driver` import fails | Use the Firefox source tree's configured `mach python` environment with automation dependencies available. |
| Demo does not load | Keep the source server on port 8000 running and check the source URL. Open the printed viewer URL, not the source page, to interact with the runtime. |
| Port already in use | Choose another loopback port or use `VIEWER_HTTP=127.0.0.1:0`. |
| Updated toolbar stays disabled on an old service | Restart that service with the updated runtime; refreshing viewer assets alone does not replace its executable. |

Continue with [the local demo](../README.md#2-serve-the-included-demo), then [testing](testing.md) or [viewer operation](viewer.md).
