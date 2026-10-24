# Source layout

The repository contains several Rust crates and a Python driver, rather than one Cargo workspace. The browser-facing adapter is JavaScript served by that driver. Paths below are relative to the repository root.

## Runtime crates

| Path | Responsibility | Useful entry points |
|---|---|---|
| `mrt-dom/` | Arena DOM and parser callbacks. Nodes are stored in flat arrays and referenced by ids. | `src/`, `build.rs`, `domcount` binary. |
| `mrt-style/` | Stylo traits over DOM nodes, stylesheet parsing, style computation and Firefox user-agent styles. | `src/`, `assets/firefox-ua/`, `vendor-ua.sh`, `stylecount` binary. |
| `mrt-layout/` | Frame tree, geometry, layout and display-list data translated from Firefox. | `src/layout/`, `src/driver.rs`, `layoutcount` and `layoutprof` binaries. |
| `mrt-text/` | Font selection and text shaping, including vendored HarfBuzz. | `src/gfx/thebes/`, `shim/`, `vendor/harfbuzz/`. |
| `mrt-js/` | SpiderMonkey embedding, DOM bindings, script loading, events, timers and runtime mirroring. | `src/bin/realsite.rs`, `src/host.rs`, `glue/`. |
| `mrt-api/` | Native network, storage, crypto, recorded environment, permissions, image and graphics services. | `src/main.rs`, `fetch.rs`, `fetch_jobs.rs`, `idb*.rs`, `webgl.rs`. |

`mrt-js/glue/mrt_js.cpp` and its header form the C++ boundary to SpiderMonkey. `prelude.js` supplies JavaScript-side interfaces and event machinery. `bridge.js` builds snapshots and updates, dispatches input, and transports calls through the driver. JavaScript DOM reflectors keep node ids; their wrapper cache is weak unless script attaches state that requires retention.

Read the [layout translation conventions](../mrt-layout/CONVENTIONS.md) before editing translated layout code. `mrt-layout/GECKO_REV` records its source revision; `tools/gecko-drift.py` identifies translated functions whose Firefox source changed. [Stylo's local changes](../vendor/stylo/README-mrt.md) explain the vendored style engine adjustments.

## Driver and viewer

| Path | Responsibility |
|---|---|
| `bench/mini_realsites.py` | Top-level driver: document loading, process/API routing, environment recordings, navigation and reports. |
| `bench/http_viewer.py` | HTTP listener, authenticated sessions, snapshot/update queues, input and lifecycle transport. |
| `bench/http-viewer/index.html` | Viewer toolbar and sandbox frame. |
| `bench/http-viewer/viewer.js` | DOM/CSS mirroring, editable input, readiness, reconnect, navigation controls and browser backend coordination. |
| `bench/http-viewer/media.js` | Browser media elements, MediaSource commands and replay. |
| `bench/http-viewer/webgl.js` | Viewer-side graphics command replay. |

The viewer runs inside an ordinary browser. The Python process is the service; `realsite` is the page execution process; `mrt-api` implements supported external services. See [architecture](architecture.md) for the data flow and ownership of state.

## Tests and tools

| Path | Responsibility |
|---|---|
| `bench/recorded_tests.py`, `bench/test_recorded_tests.py` | Recorded-data opt-in policy and synthetic policy regressions. |
| `bench/test_http_viewer.py` | Protocol/runtime tests and optional real-browser viewer tests. |
| `bench/test_no_firefox.py` | Standalone checks with no display, Marionette or browser fallback. |
| `bench/http-viewer/*.html` | Local demo and browser regression pages. |
| `bench/bridge-cases/` | API behavior fixtures compared with Firefox by `bench/compare_bridge.py`. |
| `mrt-js/tests/bindings.rs` | JavaScript binding cases in one process with explicit engine shutdown. |
| `bench/test_api_*.py` | Focused API-process integration checks. |
| `pages/` | Static snapshots and synthetic pages for memory/layout comparisons. |
| `bench/firefox_*.py`, `bench/compare_*.py` | Firefox reference runs and comparison tools. |
| `parser/vendor/`, `parser/shim/` | Firefox's HTML tokenizer/tree builder and its standalone C++ adapter. |
| `docs/` | Source-local build, operation, design, testing and measurement guides. |

Generated runtime output is kept under each crate's `target/`; parser objects go under `parser/obj/`. Environment recordings go under `bench/realsites-env/`. The driver writes captured documents, request logs and API logs below `bench/<MINI_DIR>/`, and reports to `bench/<MINI_OUT>`. Generated state is not the source of a test fixture.

## Common changes

- For toolbar or rendering behavior, begin in `viewer.js` and the corresponding browser fixture/test.
- For snapshot ordering or session replacement, begin in `http_viewer.py` and the protocol tests.
- For DOM interfaces and input dispatch, inspect `prelude.js`, `bridge.js`, `mrt_js.cpp` and `src/host.rs` together.
- For navigation across documents, inspect the driver and runtime history code; changing only the toolbar cannot implement document loading.
- For native API behavior, change the service under `mrt-api/src/` and compare a focused fixture with Firefox.

[Testing](testing.md) describes the commands for each level. [Measurement tools](measurements.md) cover parser maintenance, snapshots and memory experiments.
