# mini-runtime

mini-runtime runs a web page outside a full browser and lets you interact with it through a **standalone HTTP viewer**. The page's JavaScript and DOM live in a small runtime process. Open the viewer URL in an ordinary browser to see the page, click, type, scroll and navigate. You can close the viewer and reconnect to the same running page later.

The project explores how much state and memory a page needs when its execution is separated from the browser that displays it. The runtime uses Firefox's SpiderMonkey JavaScript engine and HTML parser, Stylo for CSS, and its own DOM and layout integration. A native API process supplies network access, cookies, storage, cryptography and WebGL. A Python driver connects these components and serves the viewer.

The browser displays a sandboxed copy of the page; the page's scripts execute in the runtime. The standalone service starts no Firefox process and needs no browser automation connection. Firefox is used once to record browser environment values during initial setup, and optionally to automate tests.

This is an experimental implementation, with partial web compatibility. It supports local demos and selected live-site flows, rather than every website or browser feature.

## What you can try

- Click, type and scroll, then close and reopen the viewer without restarting the current document.
- Follow links and forms, and use the viewer toolbar's **Back** and **Forward** buttons.
- Play video/audio through the attached browser, including supported MediaSource video streams.
- Display WebGL drawing, with native graphics execution continuing while the viewer is detached.
- Disconnect the viewer so the runtime can discard rebuildable style/layout data while retaining the page's DOM and JavaScript state.

Loading another document creates a new runtime for that document. Closing the viewer preserves the current page; stopping the service ends the session.

## Quick start

The SDK consumer build targets Linux x86_64 with glibc and the GNU C++ ABI. You need Python 3, Rust 1.93.0, a C++20 compiler, Fontconfig/fonts, Brotli development libraries, EGL/OpenGL ES development libraries, and the pinned engine SDK. **No public SDK download is published yet**; obtain the matching archive from a maintainer. The consumer build requires no Firefox source checkout. An installed Firefox executable is used for one-time environment recording; an ordinary browser displays the viewer.

[Build setup](docs/build.md) covers SDK installation/integrity checks, native prerequisites, and the explicit maintainer source-build alternative.

### 1. Build the two binaries

After [installing the SDK](docs/build.md#install-and-select-the-sdk), run from this repository's root:

```sh
export MINI_RUNTIME="$PWD"
export MRT_SDK_DIR=/absolute/path/to/mrt-sdk
export CXX=/usr/bin/c++
(cd mrt-js && cargo build --release --locked --bin realsite)
(cd mrt-api && cargo build --release --locked)
```

Run Cargo inside mrt-js so it reads that crate's checked-in compression prefix configuration. No manifest editing or private Cargo settings are needed. The runtime automatically stages its matching engine in `mrt-js/target/release/lib/`; keep that directory with realsite when relocating it. The API binary is `mrt-api/target/release/mrt-api`.

### 2. Serve the included demo

In a second terminal, from this repository's root:

```sh
python3 -m http.server 8000 --bind 127.0.0.1 --directory bench/http-viewer
```

Leave this server running. The source page is `http://localhost:8000/demo.html`.

### 3. Record the browser environment once

The runtime needs a recording of browser identity, exposed interfaces and viewport values. It cannot generate one in standalone mode. If this checkout already has a suitable recording in `bench/realsites-env/`, skip this step.

In the first terminal, create a normal Python environment with the pinned browser-automation dependencies and record against fresh local context pages:

```sh
python3 -m venv .venv
.venv/bin/pip install -r tools/sdk/requirements-browser.txt
export FIREFOX_BIN=/absolute/path/to/firefox
xvfb-run -a .venv/bin/python bench/record_env.py \
  --output "$MINI_RUNTIME/bench/realsites-env/local.json"
export API_ENV="$MINI_RUNTIME/bench/realsites-env/local.json"
mkdir -p "$MINI_RUNTIME/bench/realsites-mini-demo-profile"
```

Omit xvfb-run on a desktop display. This starts an installed Firefox in a fresh temporary profile and writes all three supported secure/isolated context entries. It does not require mach or Firefox source. The output path must be new. Subsequent standalone runs use only ordinary Python and the saved recording. See [environment setup](docs/build.md#recording-the-browser-environment).

### 4. Start the standalone viewer

From the repository root in the first terminal:

```sh
VIEWER_HTTP=127.0.0.1:8765 ONLY=demo \
  EXTRA_SITES='{"demo":"http://localhost:8000/demo.html"}' \
  REALSITES_PROFILE="$MINI_RUNTIME/bench/realsites-mini-demo-profile" \
  MINI_DIR=realsites-mini-viewer MINI_OUT=mini_realsites_viewer.json \
  python3 bench/mini_realsites.py
```

Open the complete printed **`VIEWER_URL`**, including its access token, in your browser. Click **Count a click**, type a draft and scroll. Close the viewer tab, then open the same URL again: the count, draft and scroll position should remain. The demo's **Runtime starts** value should still be `1`.

The driver selects the latest matching recording in `bench/realsites-env/`. Set `API_ENV=/absolute/path/to/recording.json` to choose one explicitly. If port 8765 is busy, use another port or `VIEWER_HTTP=127.0.0.1:0` for an automatically selected port. Each service hosts one runtime and one active viewer; opening another viewer replaces the previous attachment.

Press Ctrl+C in the service terminal to stop the runtime and write the report. Logs and captured page files are under `bench/realsites-mini-viewer/`; the report is `bench/mini_realsites_viewer.json`. Stop the demo HTTP server separately with Ctrl+C.

## Open another page

Choose a name and URL with `ONLY` and `EXTRA_SITES`:

```sh
VIEWER_HTTP=127.0.0.1:8765 API_ENV=/absolute/path/to/recording.json \
  ONLY=page EXTRA_SITES='{"page":"https://example.com/"}' \
  MINI_DIR=realsites-mini-page MINI_OUT=mini_realsites_page.json \
  python3 bench/mini_realsites.py
```

The recording must include the destination's secure/isolated context. For a site requiring login, set `REALSITES_PROFILE=/path/to/firefox-profile` to seed cookies and supported storage from a profile where you have logged in. The API stores are separate copies; they do not update that browser profile. Stores survive navigation within the run, but a fresh service run recreates them. Choose different output names to preserve earlier run artifacts.

Use **Back** and **Forward** in the viewer toolbar to navigate runtime history. The browser's own history buttons operate the viewer service page. Controls stay disabled while synchronization or navigation is pending, and when no corresponding history entry exists.

## Media replay quota

The viewer retains media commands so it can rebuild supported video streams after reconnect. Retention defaults to 32 MiB and 10,000 commands. Add `VIEWER_MEDIA_REPLAY_UNLIMITED=1` to the service launch command to disable both retention quotas for that new session. Unset or `0` keeps the defaults.

Unlimited retention allows memory to grow without bound. Browser decoder, transport and reconstruction limits still apply, so it does not guarantee every video can reconnect. See [media behavior and limits](docs/viewer.md#media-and-replay-quota).

## Current limits

The viewer listens on IPv4 loopback only. Keep the printed token private: it authorizes viewing, input and stopping the runtime. The displayed copy has the viewer service's origin; images and styles may load with the viewer browser's cookies rather than the runtime's cookie store.

New windows, downloads, nested frame rendering, 2D canvas pixels, camera/microphone capture, WebRTC calls and the origin-private file system are not implemented. Cross-document Back/Forward reloads a URL rather than restoring a cached document. Media support depends on the attached browser. Full rich editing, input methods and live-site compatibility remain incomplete. [Viewer operation](docs/viewer.md) explains these limits in more detail.

## Tests and further reading

Recorded-data benchmark checks are disabled by default. To use them, record your own inputs and opt in as described in [measurement tools](docs/measurements.md). The standard Rust tests do not read recorded benchmark data.

After building, run the self-contained local checks without a browser:

```sh
python3 bench/test_recorded_tests.py
python3 bench/test_no_firefox.py
python3 bench/test_http_viewer.py
```

The viewer command skips its optional real-browser tests by default. [Testing](docs/testing.md) explains how to run the full suite.

- [Documentation index](docs/README.md): guides for users and contributors.
- [Build setup](docs/build.md): SDK, dependencies, environment recording and troubleshooting.
- [Architecture](docs/architecture.md): processes, mirroring, navigation and hibernation.
- [Source layout](docs/source-layout.md): crates, adapters and where to change code.
- [Viewer operation](docs/viewer.md): configuration, services, media and known limits.
- [Measurement tools](docs/measurements.md): memory comparisons, layout experiments and parser maintenance.

For handling recordings and preparing future releases, see [publication hygiene](docs/publication.md).
