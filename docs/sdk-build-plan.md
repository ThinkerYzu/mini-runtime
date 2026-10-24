# Build without a Firefox source checkout: SDK contract

**Status:** SDK tooling and the consumer build are implemented. A matching local SDK can build the standalone HTTP viewer with no Firefox tree. No SDK download or prebuilt application release is published yet. The initial local engine artifact retains producer diagnostic paths and is not publication-ready.

## Goal and scope

Clone this repository, obtain its pinned SDK, build realsite and mrt-api, record a browser environment with an installed Firefox, then run the standalone HTTP viewer. Consumer inputs are this source, the SDK, locked public Rust dependencies and declared system libraries. Maintainers use compatible Firefox source to produce the SDK. SpiderMonkey remains the JavaScript engine.

Version 1 supports native Linux x86_64, glibc and GNU libstdc++ ABI. Android, cross builds and arbitrary system libmozjs versions are outside this SDK. A prebuilt application bundle is a separate deliverable.

## Dependency identities

| Input | Implementation |
|---|---|
| Engine | Shared libmozjs-154a1.so, public Firefox revision a8d334e3bde6e6572e3d162db5202def666e6206, standalone optimized non-debug/non-jemalloc configuration. |
| Engine headers | Complete dereferenced dist/include closure plus js-confdefs.h, tied to the library by the pinned manifest. Both JS glue and the vendored HTML parser consume them. |
| Compression | Exact public libz-rs-sys/zlib-rs 0.6.8 dependencies, retaining the upstream Raptor Lake workaround. Vendored zlib/zconf/mozzconf headers preserve MOZ_Z_ symbol names. See [provenance and upstream checksums](../vendor/zlib-PROVENANCE.md). |
| Other Rust inputs | Locked registry crates and mrt-api's exact public mp4parse Git revision, including its submodules. No Firefox-tree Cargo paths remain. |
| C++ compiler | Explicit CXX or system c++; no .mozbuild fallback. C++20 GNU C++ ABI, GCC 15.2 tested. |
| Browser recording | Ordinary Python, complete pinned [automation dependencies](../tools/sdk/requirements-browser.txt), an installed Firefox, and synthetic local context pages. No mach required. |

The tokenizer/tree builder, Stylo and HarfBuzz remain vendored as before.

## Versioned SDK contract

`MRT_SDK_DIR` selects one SDK; [sdk.lock.json](../tools/sdk/sdk.lock.json) pins its identity and manifest SHA-256. The SDK layout is:

```text
sdk.json
include/
  js/
  mozilla/
  js-confdefs.h
  ... complete exported headers ...
lib/
  libmozjs-154a1.so
share/licenses/
  Firefox-LICENSE
  license.html
  provenance.json
```

The schema-1 manifest binds public engine revision, version, patches, configure options, compiler version, target, ABI symbol-version requirements, system-library SONAMEs and every file's checksum. It contains no local source/object paths. The producer dereferences exported header links, rejects an unexpected engine SONAME or embedded loader path, and checks source/configuration compatibility. v1 forbids all SDK symlinks, including internal links, by storing the SONAME library as a regular file.

Validation checks the pinned manifest before accepting its inventory. Missing, additional or modified contents fail. The installer checks a separately reviewed archive digest before extracting; absolute/traversing paths, links, special files and duplicates fail. Extraction happens in a temporary staging directory and is installed only after validation.

JS/parser build scripts validate before invoking C++, watch all SDK inputs for Cargo rebuilds, and reject unsupported target/host combinations or insufficient glibc. No SDK failure falls back to a source checkout, private Cargo configuration, .mozbuild or a system engine. Explicit maintainer source builds use `MRT_ENGINE_SOURCE=1` and `MRT_JS_OBJDIR`, mutually exclusive with SDK selection.

## Linking and runtime packaging

The JS build atomically stages its matching shared library into the Cargo profile's lib/ directory and links `$ORIGIN/lib:$ORIGIN/../lib`. This supports both release binaries and test binaries in release/deps. Relocating realsite with its lib/ directory needs no SDK mount or producer files. mrt-api has no libmozjs dependency. System Brotli, EGL/GLES, C++ runtime, Fontconfig and fonts remain declared requirements, not bundled SDK files.

The viewer is displayed by an ordinary browser, whose media codecs and policies still apply. Installing this SDK does not install a browser. SDK mode does not change runtime semantics or recorded-data opt-in.

## Maintainer production

See [source build setup](build.md#maintainer-source-build-alternative). For the already pinned object build:

```sh
python3 tools/sdk/sdk.py produce \
  --source /path/to/compatible/firefox --obj /path/to/compatible/firefox/obj-js \
  --output /path/to/new-sdk
python3 tools/sdk/sdk.py pack /path/to/new-sdk /path/to/new-sdk.tar
```

Produce verifies the existing pin by default. When deliberately introducing another reviewed producer build, use `--lock-output /path/to/candidate.lock.json`. It refuses to overwrite existing SDK/lock outputs. Review the candidate metadata, provenance, license inventory and content changes before updating the application pin. Two runs against the current engine outputs produced the identical pinned manifest. The tar packer normalizes ownership, timestamps, ordering and modes for deterministic archive bytes.

The current pin describes an existing development engine build. Its metadata is public, but libmozjs contains diagnostic source paths and debug information from that producer. Keep this candidate local. A future controlled producer should build in a neutral directory, remap C++ and Rust source/debug paths, retain exact corresponding source/build inputs, and strip only appropriate debug sections before generating and reviewing a new pin. Rebuilding the engine can change bytes and therefore requires an explicit pin update.

## First-run environment recording

[record_env.py](../bench/record_env.py) serves synthetic insecure, secure and isolated-context pages on loopback, using Firefox's local DNS preference only in a fresh temporary profile. It saves all three feasible context entries and the viewport to an explicit new JSON path. It uses the same [capture code](../bench/browser_env.js) as the bridge. No profile, cookie jar, permission data, page capture or environment dump is shipped with the SDK.

[Build setup](build.md#recording-the-browser-environment) gives the ordinary-Python commands. Recording may launch Firefox once. Later standalone service execution uses ordinary Python with no Firefox or Marionette import. An ordinary browser displays the viewer. Missing environment/context data fails explicitly; it does not launch automation to fill it in.

## Acceptance criteria

- Build both release binaries from a fresh tracked source copy with no Firefox source/object tree, sibling alias, private Cargo configuration, recordings or target cache accessible.
- Use only the pinned SDK, locked public Rust inputs, declared toolchain and system libraries. Disable network during compilation.
- Relocate the runnable binaries and engine library, remove SDK/producer access, inspect loader dependencies and exercise a standalone network flow.
- Reject missing/corrupt SDKs, bad manifest/engine/target identities, escaping links and unsafe archive entries without source fallback.
- Run JS/API unit tests, standalone and HTTP-viewer regressions; run the entire optional browser suite against SDK-built binaries. Verify compression bytes/stream errors against Firefox fixtures.
- Record a real environment in a fresh ordinary-Python environment with an installed Firefox and no mach, then attach a viewer, interact, traverse history and reconnect.
- Keep personal data out of source and artifacts; retain third-party notices and inspect publication artifacts before release.

The reproducible clean-build runner is [accept.py](../tools/sdk/accept.py); malformed-input coverage is [test_sdk.py](../tools/sdk/test_sdk.py). Commands and the manual CI job are in [testing](testing.md#sdk-build-acceptance).

## Release checks

Before publishing SDK binaries, produce the controlled path-remapped engine, review/update the SDK pin, inspect all artifact contents and logs, and resolve redistribution/source-availability and attribution obligations. The local development SDK is not a release. The project-wide license for original code is still an owner decision; this work does not select one.

Publish an immutable reviewed archive and its SHA-256, then provide its URL in the source build instructions. Run the manual SDK acceptance workflow against that exact artifact. Its registry/Git fetch preparation is separate from its filesystem/network-isolated build. Run browser acceptance separately with an installed compatible Firefox. A later prebuilt application archive needs its own system dependency, relocation, license and privacy review.

See [publication hygiene](publication.md) for the source/data boundary.
