# mini-runtime

mini-runtime runs a web page outside a full browser and lets you interact with it through a **standalone HTTP viewer**. The page's JavaScript and DOM live in a small runtime process. Open the viewer URL in an ordinary browser to see the page, click, type, scroll and navigate. You can close the viewer and reconnect to the same running page later.

The project explores how much state and memory a page needs when its execution is separated from the browser that displays it. The runtime uses Firefox's SpiderMonkey JavaScript engine and HTML parser, Stylo for CSS, and its own DOM and layout integration. A native API process supplies network access, cookies, storage, cryptography and WebGL. A Python driver connects these components and serves the viewer.

The browser displays a sandboxed copy of the page; the page's scripts execute in the runtime. The standalone service starts no Firefox process and needs no browser automation connection. An installed Firefox is optional for compatibility comparisons and automated browser tests.

This is an experimental implementation, with partial web compatibility. It supports selected live-site flows and local demos, rather than every website or browser feature.

## What you can try

- Click, type and scroll, then close and reopen the viewer without restarting the current document.
- Follow links and forms, and use the viewer toolbar's **Back** and **Forward** buttons.
- Play video/audio through the attached browser, including supported MediaSource video streams.
- Display WebGL drawing, with native graphics execution continuing while the viewer is detached.
- Disconnect the viewer so the runtime can discard rebuildable style/layout data while retaining the page's DOM and JavaScript state.

Loading another document creates a new runtime for that document. Closing the viewer preserves the current page; stopping the service ends the session.

## Quick start

The default build targets Linux x86_64. Install Rust, a current Clang/C++20 toolchain, Python 3, Git, make/ar/pkg-config, Brotli development libraries, EGL/OpenGL ES development libraries, Fontconfig and fonts. The build script downloads pinned Mozilla source and builds SpiderMonkey and the two runtime binaries. The first run needs internet access, several gigabytes of disk space and time to compile the engine.

An ordinary browser displays the viewer. No Firefox installation or browser environment recording is required.

### 1. Build the two binaries

From this repository's root, run the build script. No environment variables need to be set:

```sh
python3 tools/build.py
```

Outputs are `mrt-js/target/release/realsite` and `mrt-api/target/release/mrt-api`. Keep the cached engine object directory while using these binaries.

See [build setup](docs/build.md) for prerequisites and other build options.

### 2. Open Wikipedia in the standalone HTTP viewer

From the repository root:

```sh
python3 tools/view.py https://en.wikipedia.org/wiki/Main_Page
```

Open the complete printed **`VIEWER_URL`**, including its access token, in your browser. Wikipedia runs in the standalone runtime; the browser displays its HTTP viewer. Follow an article link, scroll, and use the viewer toolbar's **Back** and **Forward** buttons. Close the viewer tab and open the same URL again to reconnect to the current page.

Try searching for **Quicksort**, opening its animated sorting image, and closing it with the top-right **X** to return to the article. See [Wikipedia interactions](docs/viewer.md#wikipedia-interactions) for the remaining focus limitation.

The launcher uses the built-in desktop environment and a dedicated default mini-runtime profile. Native cookies and persistent storage survive service restarts. It prints the profile and session-file locations, followed by a **`VIEWER_URL`** with an automatically selected loopback port. Press Ctrl+C in the service terminal to stop it.

## Open another page

Pass any HTTP(S) URL to the launcher:

```sh
python3 tools/view.py https://example.com/
```

Use the viewer toolbar's **Back** and **Forward** buttons for runtime history. The browser's own history buttons operate the viewer service page. See [viewer options](docs/viewer.md#launcher-and-profiles) for profiles, ports and comparison environments, or the [local demo](docs/viewer.md#local-demo-service) for a synthetic page.

## Media replay quota

The viewer retains media commands so it can rebuild supported video streams after reconnect. Retention defaults to 32 MiB and 10,000 commands. Add `VIEWER_MEDIA_REPLAY_UNLIMITED=1` to the service launch command to disable both retention quotas for that new session. Unset or `0` keeps the defaults.

Unlimited retention allows memory to grow without bound. Browser decoder, transport and reconstruction limits still apply, so it does not guarantee every video can reconnect. See [media behavior and limits](docs/viewer.md#media-and-replay-quota).

## Current limits

The viewer listens on IPv4 loopback only. Keep the printed token private: it authorizes viewing, input and stopping the runtime. The displayed copy has the viewer service's origin; images and styles may load with the viewer browser's cookies rather than the runtime's cookie store.

New windows, downloads, nested frame rendering, 2D canvas pixels, camera/microphone capture, WebRTC calls and the origin-private file system are not implemented. Cross-document Back/Forward reloads a URL rather than restoring a cached document. Media support depends on the attached browser. Full rich editing, input methods and live-site compatibility remain incomplete. [Viewer operation](docs/viewer.md) explains these limits in more detail.

Google search follows startup redirects, but results remain unverified behind its unusual-traffic check. The viewer cannot complete that CAPTCHA yet. See [Google Search](docs/viewer.md#google-search) for the tested behavior and remaining limitations.

## Tests and further reading

Recorded-data benchmark checks are disabled by default. To use them, record your own inputs and opt in as described in [measurement tools](docs/measurements.md). The standard Rust tests do not read recorded benchmark data.

After building, run the self-contained local checks without a browser:

```sh
python3 tools/test_view.py
python3 bench/test_recorded_tests.py
python3 bench/test_no_firefox.py
python3 bench/test_http_viewer.py
```

The viewer command skips its optional real-browser tests by default. [Testing](docs/testing.md) explains how to run the full suite.

- [Documentation index](docs/README.md): guides for users and contributors.
- [Build setup](docs/build.md): dependencies, optional build inputs and comparison recordings, and troubleshooting.
- [Architecture](docs/architecture.md): processes, mirroring, navigation and hibernation.
- [Source layout](docs/source-layout.md): crates, adapters and where to change code.
- [Viewer operation](docs/viewer.md): configuration, services, media and known limits.
- [Measurement tools](docs/measurements.md): memory comparisons, layout experiments and parser maintenance.

For handling recordings and preparing future releases, see [publication hygiene](docs/publication.md).
