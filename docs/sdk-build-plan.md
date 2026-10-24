# Build without a Firefox source checkout: plan

**Status: planned, not implemented.** The current build still needs the engine outputs and compression sources described in [build setup](build.md). No SDK release, SDK download command or prebuilt runtime package is available yet. This document defines the intended build and validation contract before implementation.

## Goal and scope

A new reader should be able to clone this repository, obtain a pinned dependency SDK, build `realsite` and `mrt-api`, and try the standalone HTTP viewer without downloading a Firefox source checkout. The SDK contains build inputs derived from Firefox; it does not remove SpiderMonkey or replace the JavaScript engine.

Maintainers may use a compatible Firefox checkout to produce the SDK. Consumer builds must depend only on this repository, the SDK, locked Rust dependencies and declared system development libraries. The initial target is Linux x86_64 with a declared GNU/Linux ABI and minimum system-library versions. Android, other operating systems and arbitrary system `libmozjs` versions are outside the first delivery.

A later prebuilt application package can remove the compiler requirement for readers who only want to try the viewer. That package is a separate deliverable from the SDK used to compile the application.

## Current dependencies to replace

| Current dependency | Planned treatment |
|---|---|
| `MRT_JS_OBJDIR/dist/include` and `js/src/js-confdefs.h` | Package the complete compatible public, Mozilla and generated header closure in the SDK. Both JS glue and the vendored parser use it. |
| `MRT_JS_OBJDIR/dist/bin/libmozjs-*.so` | Package the matching shared engine library, its SONAME links and declared runtime dependencies. Continue using shared linkage; do not substitute SpiderMonkey's static Rust archive. |
| Firefox `modules/zlib/src` headers | Supply compatible `zlib.h`, `zconf.h` and `mozzconf.h` through the SDK or a reviewed source-local dependency. Preserve the required symbol naming. |
| `../../firefox-source/third_party/rust/libz-rs-sys` and `zlib-rs` Cargo paths | Replace with exact-version registry dependencies or a reviewed vendored copy, with lockfile/provenance and license notices. Confirm required features and the existing `MOZ_Z_` prefix before selecting the source. No Firefox-tree path remains in the consumer manifest. |
| Compiler fallback to a local Mozilla toolchain | Support an explicitly configured C++ compiler and an ordinary system-toolchain fallback. Document compiler/ABI requirements. |
| Recorded environment setup through `mach python` | Validate and document ordinary Python with pinned automation dependencies and an installed Firefox executable. No source-tree Python environment is required by the consumer path. |

The HTML tokenizer/tree builder, Stylo and HarfBuzz sources are already vendored. They remain in this repository. Their build still needs the correct header/library inputs; vendoring those source files alone does not make today's build independent of Firefox.

## Versioned SDK contract

The proposed selector is `MRT_SDK_DIR`. It is a future interface, not a setting accepted by the current build. A reviewed SDK identity and expected checksum must be pinned by the application, rather than selecting the newest release implicitly.

An SDK should contain:

```text
sdk.json
include/
  js/
  mozilla/
  js-confdefs.h
  ... complete generated/header dependencies ...
lib/
  libmozjs-<engine-version>.so
  ... required SONAME links and declared bundled dependencies ...
share/licenses/
```

The exact layout becomes a versioned build interface. `sdk.json` must identify its schema version, SDK identity, public engine source revision and applied patches, engine version, target architecture/OS/libc requirements, C++ ABI requirements, build configuration, library SONAMEs, content checksums and license/provenance inventory. Record compiler versions and flags without private build-directory paths.

Packaging must dereference exported-header symlinks that point back into a Firefox checkout and validate that every required file is inside the SDK. Internal library links may remain relative and contained. Reject absolute links, traversal outside the extraction root, missing files and incompatible metadata. Copying `dist/include` with unresolved links is insufficient.

Build scripts must select and validate a single SDK before invoking the compiler. Missing SDKs, mismatched headers/libraries, unsupported targets and integrity failures must produce actionable errors. An SDK consumer build must not silently fall back to `FIREFOX_SRC`, an adjacent `firefox-source` directory, a local `.mozbuild` installation or an unrelated system engine. The existing maintainer source-build path can remain as an explicitly selected alternative.

## Linking and runtime packaging

The built runtime must load its matching shared engine from a documented SDK location or an application-relative `lib/` directory. Runtime search paths must be relocatable, for example an appropriate `$ORIGIN`-relative path on Linux. They must not embed the SDK producer's checkout/object directory. Document which native libraries are bundled and which remain system requirements, including Brotli, EGL/OpenGL ES, the C++ runtime, Fontconfig and fonts as applicable.

Installing the SDK does not provide a full browser. The viewer runs in the reader's ordinary browser; native page services run in `mrt-api`. Video/audio playback continues to depend on the attached browser's codecs and policies.

## First-run environment recording

A source-independent quick start also needs a way to produce `API_ENV` without `mach`. Validate a recording workflow in a fresh Python virtual environment, using a pinned `marionette_driver` dependency set and `FIREFOX_BIN` pointing at an installed compatible Firefox executable. Record against the included local demo, using a fresh empty profile by default.

Document how to obtain each required secure/isolated-context entry, where the JSON is saved, and how to select it with `API_ENV`. The initial recording may launch Firefox; subsequent standalone service execution must not launch Firefox or import Marionette. An ordinary browser is still needed to display the viewer.

Do not ship a developer's browser profile, cookies, permissions, page content, benchmark results or environment dump inside the SDK. Any future generic environment preset needs its own documented provenance and compatibility checks; it must not be presented as equivalent to every browser/profile. Recorded-data comparisons remain opt-in and require locally recorded inputs.

## Implementation sequence

1. Define the SDK schema and pinned engine/source/compression dependency identities. Inventory header symlinks, shared-library dependencies and required licenses.
2. Produce a relocatable SDK in a controlled maintainer build, with deterministic file inventory, checksums and public provenance. Resolve applicable redistribution obligations before publishing binary artifacts.
3. Add SDK validation/selection to the JS and parser builds. Remove consumer Firefox paths from Cargo and compression header lookup; preserve tested compression behavior.
4. Validate source-independent Python recording and runtime startup; update the README quick start only after those paths work.
5. Add the clean-environment acceptance job and release documentation. A prebuilt application bundle may follow, with its own runtime dependency and relocation checks.

These steps are a plan, not a claim that release infrastructure or implementation already exists. This documentation change does not select a new project-wide license.

## Acceptance criteria

- Build a fresh source checkout in a clean Linux environment with no Firefox source/object tree, sibling engine alias, private Cargo configuration, private `bench/` data or pre-existing target cache. Only the pinned SDK, locked Rust dependencies, toolchain and declared system libraries are available.
- Build both release binaries without editing manifests. Verify that SDK-only builds do not read outside their declared inputs. Producer-side builds may use Firefox source; consumer-side builds may not.
- Relocate the SDK/application and remove access to the producer's build directories. Inspect dynamic dependencies and run the resulting binaries successfully.
- Reject a missing SDK, corrupt archive/checksum, unsupported target, mismatched engine/header metadata and header links escaping the SDK, with clear diagnostics and no fallback to a source checkout.
- Verify compression bytes, stream errors and symbol linkage against the existing fixtures; run default unit tests plus standalone/API regressions. Default tests must not acquire personal recordings.
- In a fresh Python environment, record the local demo's browser environment using an installed Firefox executable and no Firefox source tree. Then run the service without browser automation, attach a viewer, type/click/scroll, traverse local history and reconnect.
- Run the complete real-browser viewer suite against SDK-built binaries. Check API-origin storage/network behavior, media and WebGL replay using the existing local fixtures.
- Inspect SDK/application archives and producer logs intended for publication for credentials, personal paths, profiles and captured data; preserve all required third-party notices and source provenance.

See [testing](testing.md) for today's checks and [publication hygiene](publication.md) for the source/data boundary.
