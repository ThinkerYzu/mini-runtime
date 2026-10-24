# Build and initial setup

The default command downloads the pinned Mozilla source checkout when absent, builds SpiderMonkey locally, then builds the standalone viewer binaries. A separate SDK download is optional. The initial target is native Linux x86_64 with glibc and the GNU C++ ABI. This builds the JS engine, not Firefox's browser application.

## Automatic source checkout and build

From the repository root:

```sh
python3 tools/build.py
```

[tools/engine/source.json](../tools/engine/source.json) pins Mozilla's repository and exact public commit `2839629039f3c910952215d88e1a3565d9dcda29`, compatible with the current SpiderMonkey 154 glue. Setup uses a shallow Git fetch and checks out that exact commit. It includes Mozilla's shared build dependencies outside js/, so the source download still contains more than SpiderMonkey's engine directory. No repository history or browser binary is built.

Source lives at `firefox-source/` in the mini-runtime repository root, regardless of the shell working directory. This Git-ignored path can be an existing checkout or a symlink to one. If absent, setup fetches the pinned Mozilla commit there. A broken symlink is refused and preserved. Engine outputs are stored separately under `${XDG_CACHE_HOME:-$HOME/.cache}/mini-runtime/spidermonkey/<revision>/`; build tools also use this cache. Expect several gigabytes of source/build storage and a substantial initial engine compile. The command rechecks existing source and runs an incremental build on later invocations. Partial downloads are staged and never installed as a usable checkout. A modified or wrong-revision checkout is refused rather than switched or cleaned.

To reuse a local clean checkout at the pinned revision, run from the repository root:

```sh
ln -s /path/to/clean/pinned/mozilla-checkout firefox-source
python3 tools/build.py
```

The symlink resolves to its physical checkout path for engine builds. Setup never switches an existing checkout to the pin. `--cache` changes object/tool storage, not the default source location. Explicit `--source` requires an existing checkout.

```sh
python3 tools/build.py --fetch-only             # acquire source, no compilation
python3 tools/build.py --jobs 4                 # limit engine compilation concurrency
python3 tools/build.py --cache /path/to/cache   # choose object/tool storage
python3 tools/build.py --source /path/to/source # existing clean checkout at the pinned revision
python3 tools/build.py --objdir /path/to/obj-js # compatible engine already built
python3 tools/build.py --sdk /path/to/sdk       # optional SDK, no source download or engine compile
```

The source build uses a generated mozconfig outside the source tree, with an explicit object directory, an optimized non-debug shared engine and Mozilla's automatic toolchain bootstrap disabled. It uses declared Clang/Rust/system tools, never installs OS packages, and reports missing prerequisites. If no compatible cbindgen is supplied through CBINDGEN or PATH, it installs pinned cbindgen0.29.4 with Cargo into the cache, then reuses it. No global Cargo tool installation is modified. `CC`/`CXX` can select explicit compilers. Run setup under the Python interpreter you want mach to use. Existing running viewer services are not restarted.

After the engine build, the command selects the existing `MRT_ENGINE_SOURCE=1` integration and invokes Cargo with its lockfiles from the correct crate directories. Source-mode binaries load the shared engine from that cached object directory; retain it while using them. The optional SDK mode retains its relocatable staged lib/ behavior described below. Neither mode needs manifest editing or private Cargo configuration.

## Native prerequisites

- Rust 1.93.0 (tested), Cargo, a current Clang C++20 compiler with GNU libstdc++ ABI for the engine build, Git, make, ar, pkg-config and Python 3 with venv support. SDK-only consumer compilation also works with GCC 15.2. Mozilla may require additional compiler/linker development tools; missing prerequisites fail explicitly.
- Brotli development headers/libraries (`brotlienc`, `brotlidec`), EGL/OpenGL ES development libraries (`libEGL`, `libGLESv2`), Fontconfig and fonts. Mesa llvmpipe is the tested software renderer.
- Engine runtime requirements are recorded in `sdk.json`: glibc >= 2.18, GLIBCXX >= 3.4.21, CXXABI >= 1.3, and the listed system SONAMEs. The application and other system libraries may require newer versions. Clean consumer acceptance was run on Ubuntu 26.04; the manual CI job targets Ubuntu 24.04.
- An installed Firefox executable for one-time environment recording and optional viewer tests. Xvfb is useful without a display. Neither recording nor viewing requires browser source.

Typical Debian/Ubuntu development packages: `build-essential clang lld llvm python3 python3-venv git pkg-config libbrotli-dev libegl1-mesa-dev libgles2-mesa-dev fontconfig fonts-dejavu-core xvfb`. Rust is installed separately. Clean isolation tests additionally use `bubblewrap` and `binutils`.

## Install and select the SDK

**Optional:** no public binary SDK download is published yet. Use automatic source setup above unless you already have a compatible archive. The SDK identity, producer revision and manifest SHA-256 are pinned in [sdk.lock.json](../tools/sdk/sdk.lock.json). A manifest binds every header, shared library and notice to a content checksum. Do not substitute arbitrary system libmozjs packages.

From the repository root, install a producer-supplied archive using its reviewed SHA-256:

```sh
python3 tools/sdk/sdk.py install /path/to/sdk.tar \
  --sha256 REVIEWED_ARCHIVE_SHA256 --output /absolute/path/to/mrt-sdk
export MRT_SDK_DIR=/absolute/path/to/mrt-sdk
python3 tools/sdk/sdk.py validate "$MRT_SDK_DIR"
```

The installer verifies the archive digest before extraction, rejects traversal, links, devices and duplicate entries, then validates the pinned manifest and full inventory. SDK v1 contains only regular files and directories; exported headers are dereferenced during production. Missing or corrupt SDKs and incompatible metadata/targets fail before JS/parser compilation. SDK mode ignores old `JS`, `MRT_JS_OBJDIR` and `FIREFOX_SRC` settings and never consults `.mozbuild`. `MRT_ENGINE_SOURCE=1` and `MRT_SDK_DIR` are mutually exclusive.

## Build mini-runtime and the API process

From the repository root:

```sh
export MINI_RUNTIME="$PWD"
export CXX=/usr/bin/c++
(cd mrt-js && cargo build --release --locked --bin realsite)
(cd mrt-api && cargo build --release --locked)
```

Run the first command inside `mrt-js` so Cargo reads its checked-in `.cargo/config.toml`, which sets `LIBZ_RS_SYS_PREFIX=MOZ_Z_`. No manifests need editing. Compression uses exact public libz-rs-sys/zlib-rs 0.6.8 crates with the existing prefix/features and three small source-local C interface headers; see [provenance](../vendor/zlib-PROVENANCE.md). Other Rust sources are locked registry dependencies and mrt-api's pinned public mp4parse Git dependency. The first fetch needs network access; `--offline` is available once those inputs are cached. There is no root Cargo workspace.

Outputs are `mrt-js/target/release/realsite`, `mrt-js/target/release/lib/libmozjs-154a1.so` and `mrt-api/target/release/mrt-api`. JS binaries and tests load the staged engine via `$ORIGIN/lib:$ORIGIN/../lib`, without an SDK/producer absolute loader path. Keep `realsite` and its `lib/` directory together when moving them; `mrt-api` does not link libmozjs. The runtime does not need the SDK headers after compilation. An ordinary browser displays the viewer, with its own codecs and playback policies.

Parser and HarfBuzz objects are built automatically. Parser compiler failures now stop the build; its objects live in Cargo's output directory. `CXX` selects the compiler, otherwise `c++` is used. A rebuilt executable does not replace a running service; restart a service when you want it to use new code.

## Recording the browser environment

Create a normal Python virtual environment for recording and browser tests:

```sh
python3 -m venv .venv
.venv/bin/pip install -r tools/sdk/requirements-browser.txt
export FIREFOX_BIN=/absolute/path/to/firefox
xvfb-run -a .venv/bin/python bench/record_env.py \
  --output "$PWD/bench/realsites-env/local.json"
export API_ENV="$PWD/bench/realsites-env/local.json"
```

Omit `xvfb-run -a` on a desktop display. The complete Python dependency set is version pinned; it was tested in a fresh Python 3.14 environment. The recorder launches an installed Firefox in a fresh temporary profile, serves synthetic local context pages, and records insecure, secure and secure-plus-isolated contexts. It checks each actual context before saving; insecure plus isolated is not a browser-supported combination. It does not start the runtime, read personal profiles or require the demo server. The file contains `firefox`, `viewport` and `entries` keyed by `secure=0|1,isolated=0|1`, using the same capture code as the browser bridge.

The output path is explicit and must not already exist. `--port` selects another Marionette port if an automation session is already using 2828. These are browser/profile-specific values, not a universal compatibility preset. Record your own environment and keep it private. Use an ordinary empty profile for the demo; the service's `REALSITES_PROFILE` can point to an empty directory. Login-site profile seeding remains optional and local.

Subsequent standalone execution uses only Python's standard library, the binaries, and this recording. It starts no Firefox and imports no Marionette. Missing context entries fail explicitly. The historical harness can still record environment values from a site's actual profile, using this virtual environment rather than `mach python`.

Continue with [the demo](../README.md#2-serve-the-included-demo).

## Maintainer source-build alternative

A producer needs the compatible Firefox revision and a standalone shared SpiderMonkey build. The downloadable engine source is pinned to public Mozilla revision `2839629039f3c910952215d88e1a3565d9dcda29`. The existing local SDK records producer checkout `a8d334e3bde6e6572e3d162db5202def666e6206`, which includes two local Gecko-only commits on that base; that commit must not be used for automatic downloads. Layout/HarfBuzz provenance files describe other translated components, not engine compatibility. The configuration is:

```sh
ac_add_options --enable-project=js
ac_add_options --enable-optimize
ac_add_options --disable-debug
ac_add_options --disable-jemalloc
mk_add_options MOZ_OBJDIR=@TOPSRCDIR@/obj-js
```

Use Mozilla's [source setup](https://firefox-source-docs.mozilla.org/setup/linux_build.html) and `MOZCONFIG=... ./mach build` from that source tree. Shared libmozjs is required; its static archive contains a conflicting Rust standard library.

To build against the object directory explicitly instead of an SDK:

```sh
unset MRT_SDK_DIR
export MRT_ENGINE_SOURCE=1
export MRT_JS_OBJDIR=/absolute/path/to/firefox/obj-js
export CXX=/usr/bin/c++
(cd mrt-js && cargo build --release --locked --bin realsite)
(cd mrt-api && cargo build --release --locked)
```

This mode retains the producer library's absolute runtime path. It is a development alternative, not the consumer distribution. Compression sources no longer come from Firefox in either mode. The Android helper selects source mode explicitly; SDK v1 does not support Android.

See [SDK production and release checks](sdk-build-plan.md) and [testing](testing.md).

## Troubleshooting

| Symptom | Check |
|---|---|
| Missing SDK or manifest checksum mismatch | Obtain the exact SDK matching the checked-in lock. Do not edit the pin to bypass a failure. |
| SDK integrity failure or symlinks | Reinstall the intact archive with its reviewed digest; never copy unresolved dist/include links. |
| libmozjs cannot be loaded | Keep realsite next to its staged lib/ directory; inspect `ldd` and `readelf -d`. |
| Undefined MOZ_Z_ symbols | Run Cargo inside mrt-js so its prefix configuration is loaded. |
| C++ compilation failure | Use the declared C++20 GNU-ABI toolchain and complete pinned SDK; the first compiler error is now fatal. |
| Missing EGL/GLES, Brotli or fonts | Install their development/runtime packages and check `fc-match serif`. |
| No environment recording or context entry | Run record_env.py, then set API_ENV to its absolute output path. |
| Marionette import or port failure | Use the pinned virtual environment; select an unused `--port`. |
| Port busy or stale viewer code | Choose another VIEWER_HTTP port, or restart the intended service with the new executable. |
