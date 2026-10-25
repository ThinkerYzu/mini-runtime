# Testing

Default Rust tests and the self-contained Python regressions do not require personal benchmark recordings. Checks that consume recorded Firefox measurements or live-site captures are opt-in: set `MRT_RECORDED_TESTS=1` after recording your own inputs. Without that setting, recorded-data comparison scripts print a skip message and exit successfully before reading data or starting binaries. With it set, missing inputs fail with recording instructions. See [recording measurements](measurements.md#record-your-own-inputs).

The Rust image-metadata case that reads binary files from `bench/bridge-cases/` is ignored by default. Generate its synthetic files with `python3 bench/make_images_formats.py` (requires ImageMagick with AVIF support), then run it explicitly:

```sh
(cd mrt-api && cargo test --release image::tests::sizes_from_headers_as_gecko_metadata_decode -- --ignored)
```

The viewer/API integration commands below are explicit fixture-based checks. Keep their checked-in synthetic HTML, JS, action JSON, image and media assets; they use the built-in desktop environment and generate temporary profiles and result files; override tests supply synthetic comparison recordings. Historical benchmark JSON, screenshots and traffic recordings are not inputs to those suites.

Build both release binaries first, then run from this repository's root:

```sh
python3 tools/test_view.py
python3 bench/test_recorded_tests.py
python3 bench/test_no_firefox.py
python3 bench/test_http_viewer.py
```

The launcher suite covers URL validation, launch from another working directory, inherited benchmark-setting isolation, native profile persistence/isolation, profile locking and clean termination. It uses local HTTP and Python `-S` without a browser or recording.

The standalone suite has 17 checks, covering built-in desktop/isolated environments, consistent network identity and strict explicit recording overrides. The viewer suite has 37 cases: 25 run without a browser and 12 require `VIEWER_BROWSER=1`. The local fixtures use the built-in environment and their own HTTP servers. To run all viewer cases, use the pinned ordinary-Python virtual environment from [build setup](build.md#recording-the-browser-environment):

```sh
VIEWER_BROWSER=1 FIREFOX_BIN=/path/to/firefox xvfb-run -a .venv/bin/python \
  bench/test_http_viewer.py
```

Append a test name to select a focused case:

| Test | Coverage |
|---|---|
| `Runtime.test_browser` | Input, scroll, tab replacement/reconnect, resize, CSSOM and script isolation. `VIEWER_SHOT=FILE` saves a screenshot. |
| `Runtime.test_browser_editable` | Ordered typing, caret/selection replacement, deletion, Unicode and editable reconnect. |
| `Runtime.test_browser_svg_search` | Lazy replacement of a search field, SVG namespace detection, typing and value preservation after reconnect. |
| `Runtime.test_browser_document_input` | Controlled textarea input and consecutive submissions. |
| `Runtime.test_browser_slow_fetch` | Slow binary fetch/XHR, responsive input, cookies, errors, abort/reuse and reconnect. |
| `Runtime.test_navigation_startup_redirects` | No browser required: immediate redirect chains, replace history, session storage, referrers, activation headers, failed successors and process cleanup. |
| `Runtime.test_browser_navigation` | Links, forms, cross-origin navigation, refusal/failure and reconnect. |
| `Runtime.test_browser_navigation_controls` | Back/Forward boundaries, branching, same/full-document traversal, cancellation and readiness. |
| `Runtime.test_browser_media` | Native media playback and MSE reconstruction on reconnect. |
| `Runtime.test_browser_webgl` | Shader drawing and replay on reconnect. |

`bench/http_viewer.py` owns transport; `bench/http-viewer/viewer.js` adapts the browser; `mrt-js/glue/bridge.js` and `mrt-js/src/bin/realsite.rs` own runtime mirroring and input. See [viewer operation](viewer.md) for supported behavior and [architecture](architecture.md) for process responsibilities.

## Additional checks

```sh
# From the repository root; each subshell restores the original directory.
(cd mrt-js && cargo test --release --offline)
(cd mrt-layout && cargo test --release --offline --lib)
(cd mrt-api && cargo test --release --offline)
```

The counter reconstruction regression covers the layout failure previously triggered by closing Wikipedia's enlarged image. With the engine/SDK settings used for the build, run:

```sh
(cd mrt-layout && cargo test --release --offline --lib counter_reconstruction_tests)
```

It changes an ordered list's start value when both its marker and a nested link use that counter, checking that both keep live frames with updated text and no pending rebuilds. For a live check, follow [Wikipedia interactions](viewer.md#wikipedia-interactions), wait several seconds before closing the image, and repeat three times. Verify the article is usable after every close. The completed validation covered all 869 layout library tests and this live sequence. Broader recovery from other layout panics remains incomplete: the engine's process-wide Rust panic hook can terminate the runtime before its layout recovery handler runs.

`bench/compare_bridge.py` compares individual API fixtures with Firefox. For example, from this source repository:

```sh
API_PROCESS=all FIREFOX_BIN=/path/to/firefox xvfb-run -a .venv/bin/python \
  bench/compare_bridge.py history fetch-bytes compression
```

Runtime tests require compatible binaries; they do not rebuild them automatically. Browser tests use Firefox only as the test driver and viewer. The standalone service itself still starts no Firefox or privileged bridge. A successful process exit is not sufficient for acceptance of a site flow; check its visible outcome as well.

## Automatic source setup acceptance

```sh
python3 tools/engine/test_build.py
python3 tools/build.py --fetch-only
python3 tools/build.py --fetch-only  # verify cache reuse, no new fetch
python3 tools/build.py --jobs 8
```

The lifecycle suite uses disposable local Git repositories. The live acquisition/build check uses the official repository and exact revision in tools/engine/source.json. Verify that the checkout is clean at that revision, only the JS project is built, and source/object/optional SDK reuse works without switching user checkouts. Setup may fetch the pinned cbindgen helper into its own cache if absent. It installs no OS packages. Run the standard standalone and viewer suites against the resulting binaries. The optional SDK-only namespace acceptance below checks a different path that has no source tree at all.

## SDK build acceptance

```sh
python3 tools/sdk/test_sdk.py
python3 tools/sdk/accept.py --sdk /absolute/path/to/sdk \
  --output /tmp/new-consumer-build --toolchain "$(rustc --print sysroot)"
```

The output directory must be absent. The acceptance runner copies only tracked source into a fresh directory, fetches locked public Rust inputs into a separate Cargo home, then uses bubblewrap to hide the producer filesystem, private Cargo configuration, recordings and all previous target caches. The namespace has only declared source/SDK/toolchain/system inputs and no network. It builds both release binaries, runs Rust, standalone and viewer protocol tests, then relocates the binaries and checks a standalone network flow with the SDK unmounted. The output remains for inspection. Linux user namespaces must be enabled.

Run the complete real-browser suite against those built binaries too:

```sh
REALSITE_BIN=/tmp/new-consumer-build/relocated/realsite \
  API_BIN=/tmp/new-consumer-build/relocated/mrt-api \
  VIEWER_BROWSER=1 FIREFOX_BIN=/path/to/firefox \
  xvfb-run -a .venv/bin/python bench/test_http_viewer.py
```

The manual [SDK acceptance workflow](../.github/workflows/sdk.yml) takes an explicit reviewed archive URL and digest; it does not select a newest release. The local full acceptance and ordinary-Python recording are separate checks from executing that remote workflow. SDK binary publication remains gated by [release checks](sdk-build-plan.md#release-checks).
