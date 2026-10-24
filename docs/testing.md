# Testing

Default Rust tests and the self-contained Python regressions do not require personal benchmark recordings. Checks that consume recorded Firefox measurements or live-site captures are opt-in: set `MRT_RECORDED_TESTS=1` after recording your own inputs. Without that setting, recorded-data comparison scripts print a skip message and exit successfully before reading data or starting binaries. With it set, missing inputs fail with recording instructions. See [recording measurements](measurements.md#record-your-own-inputs).

The Rust image-metadata case that reads binary files from `bench/bridge-cases/` is ignored by default. Generate its synthetic files with `python3 bench/make_images_formats.py` (requires ImageMagick with AVIF support), then run it explicitly:

```sh
(cd mrt-api && cargo test --release image::tests::sizes_from_headers_as_gecko_metadata_decode -- --ignored)
```

The viewer/API integration commands below are explicit fixture-based checks. Keep their checked-in synthetic HTML, JS, action JSON, image and media assets; they generate temporary environment recordings, profiles and result files themselves. Historical benchmark JSON, screenshots and traffic recordings are not inputs to those suites.

Build both release binaries first, then run from this repository's root:

```sh
python3 bench/test_recorded_tests.py
python3 bench/test_no_firefox.py
python3 bench/test_http_viewer.py
```

The standalone suite has 14 checks. The viewer suite has 35 cases: 24 run without a browser and 11 require `VIEWER_BROWSER=1`. The local fixtures create their own environment recordings and HTTP servers. To run all viewer cases, use a Firefox source tree's `mach python` with Marionette available:

```sh
# Run from the Firefox source tree, using the absolute path to this checkout.
VIEWER_BROWSER=1 FIREFOX_BIN=/path/to/firefox xvfb-run -a ./mach python \
  /path/to/mini-runtime/bench/test_http_viewer.py
```

Append a test name to select a focused case:

| Test | Coverage |
|---|---|
| `Runtime.test_browser` | Input, scroll, tab replacement/reconnect, resize, CSSOM and script isolation. `VIEWER_SHOT=FILE` saves a screenshot. |
| `Runtime.test_browser_editable` | Ordered typing, caret/selection replacement, deletion, Unicode and editable reconnect. |
| `Runtime.test_browser_document_input` | Controlled textarea input and consecutive submissions. |
| `Runtime.test_browser_slow_fetch` | Slow binary fetch/XHR, responsive input, cookies, errors, abort/reuse and reconnect. |
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

`bench/compare_bridge.py` compares individual API fixtures with Firefox. For example, from a Firefox source tree:

```sh
API_PROCESS=all FIREFOX_BIN=/path/to/firefox xvfb-run -a ./mach python \
  /path/to/mini-runtime/bench/compare_bridge.py history fetch-bytes
```

Runtime tests require compatible binaries; they do not rebuild them automatically. Browser tests use Firefox only as the test driver and viewer. The standalone service itself still starts no Firefox or privileged bridge. A successful process exit is not sufficient for acceptance of a site flow; check its visible outcome as well.
