# Viewer operation and limitations

Start with the [README quick start](../README.md#quick-start). This guide describes configuration, service behavior and current limits. Commands below run from the repository root.

## Launcher and profiles

Use `python3 tools/view.py URL` to open a single HTTP(S) site. It uses the built-in desktop environment and prints a tokenized viewer URL on a free IPv4 loopback port. The launcher starts no browser. Open that URL in an ordinary browser; Ctrl+C stops the service.

```sh
python3 tools/view.py https://en.wikipedia.org/wiki/Main_Page
python3 tools/view.py --port 8765 https://example.com/
python3 tools/view.py --profile /path/to/another-native-profile https://example.com/
```

The default native profile is `${XDG_DATA_HOME:-$HOME/.local/share}/mini-runtime/profiles/default`. It starts empty and does not import a personal Firefox profile. Native cookies, localStorage, IndexedDB and saved permissions persist in its `store/` directory across launches. Live page/JavaScript state, history and sessionStorage last only for the running service. One launcher can use a profile at a time; choose another `--profile` for a concurrent independent session. This directory is a mini-runtime profile, not a Firefox profile.

Each run writes captures, console/request logs and `report.json` under a new `${XDG_CACHE_HOME:-$HOME/.cache}/mini-runtime/sessions/viewer-*/` directory. The launcher prints the location; no generated state needs to be written into the repository. It ignores inherited benchmark mode/site/evaluator settings. `VIEWER_MEDIA_REPLAY_UNLIMITED` remains available as described below.

For an explicit comparison environment, use `--environment /path/to/recording.json`. Otherwise the built-in environment is used regardless of an inherited `API_ENV`. See [optional environment recording](build.md#recording-the-browser-environment). The sections below also show direct driver commands for benchmark and developer workflows.

## Environment and stores

For a finite run without a viewer:

```sh
NO_FIREFOX=1 \
  ONLY=example EXTRA_SITES='{"example":"http://localhost:8000/page.html"}' \
  MINI_OUT=mini_realsites_standalone.json python3 bench/mini_realsites.py
```

Serve a local source URL first; live sites need no local source server. By default, standalone mode uses a built-in mini-runtime desktop environment with a 1280x720 initial viewport, fixed navigator/screen values, English language preferences and no touch input. The HTTP viewer updates the runtime viewport on attachment. No installed Firefox, automation packages or recording is required. The profile covers insecure, secure and secure-plus-isolated contexts, while the page URL and response headers determine the actual context.

For Firefox compatibility comparisons, explicitly set `API_ENV=/path/to/recorded-env.json` to use a recording containing `entries` and `viewport`. Invalid recordings or missing contexts fail without fallback. Unset `API_ENV` to use defaults; standalone mode never selects recordings automatically. See [optional environment recording](build.md#recording-the-browser-environment).

`REALSITES_PROFILE=/path/to/profile` selects the Firefox profile used to seed native stores. Cookies, localStorage, saved permissions and supported IndexedDB databases are copied into partitioned API stores. The profile is read without starting Firefox; subsequent API writes do not update it. A fresh direct harness run recreates its stores, while navigation and API-process restarts within that run retain persisted data. The user-facing launcher instead keeps native stores in its profile across launches. Use distinct `MINI_DIR` and `MINI_OUT` values for runs whose artifacts you want to keep.

Standalone and HTTP viewer modes require `API_PROCESS=all`, which is their default. They cannot be combined with `INTERACTIVE`, `RENDER`, phone mode (`MRT_ADB`) or `API_ENV_CHECK`. Firefox-only frame bridge operations and other unavailable services fail explicitly. Reports include `unsupported_ops`, `firefox_started: false` and `bridge_tab: {"made": false}`; unsupported requests cause a nonzero driver exit status.

### Native services and limits

| Service | Current behavior |
|---|---|
| Network | Fetch, XHR, script loading and WebSockets use the API process with the original page's cookies and policy checks. Fetch/XHR preserve binary response bytes even under text MIME types. In HTTP viewer mode, asynchronous fetch/XHR use completion jobs so input stays responsive during slow responses. Replies arrive as complete bodies; progressive streaming is not implemented. Jobs have a 120-second deadline, a 16 MiB body limit and a 16-job bound. |
| Storage | Partitioned SQLite stores provide cookies, localStorage and native IndexedDB. Supported profile IndexedDB data is imported once per origin and partition; the store is not shared with the browser afterwards. Quota, durability, locking, worker lifecycle and full API parity remain incomplete. |
| Images | PNG, JPEG, GIF, WebP, BMP, ICO, AVIF metadata and SVG intrinsic sizes are supported. Raster sizing uses metadata rather than decoding all pixels. Displayed images still load separately in the viewer browser. Some intrinsic-size and painted-image behavior remains incomplete. |
| Permissions | Saved profile decisions, inherited host decisions and relevant default preferences seed API answers. Queries do not create grants or display original-site prompts in the viewer. Storage estimates cover localStorage payloads, not total browser quota usage. Frame policies, permission-change events and quota accounting remain incomplete; the origin-private file system (OPFS) is unavailable. |
| WebGL | Real OpenGL ES contexts execute in the API process, including while detached. The viewer replays a bounded command journal to display the result. Native calls do not depend on viewer attachment; detached execution is not a black-pixel dummy in this mode. |
| Media | Plain standalone mode has no playback decoder and reports negative media capabilities. HTTP viewer mode uses the attached browser's codecs, media elements and MediaSource backend. Capabilities are negative while detached; backend changes invalidate runtime caches. |

## Local demo service

The [README quick start](../README.md#quick-start) opens live Wikipedia. For a synthetic local page, first serve the demonstration page in another terminal:

```sh
python3 -m http.server 8000 --bind 127.0.0.1 --directory bench/http-viewer
```

Then start the viewer service from the repository root:

```sh
python3 tools/view.py http://localhost:8000/demo.html
```

Open the complete printed `VIEWER_URL`, including its token, in an ordinary browser. The launcher chooses a free loopback port; use `--port 8765` for a fixed port. Ctrl+C stops the runtime and writes the report into the printed session directory. Stop the local page server separately with Ctrl+C.

Click, type and scroll in the viewer. Close its tab or use Disconnect, then open the same URL again: the current document's runtime, JavaScript state, form state and scroll position survive. Each attachment replaces the previous active viewer. A disconnected viewer's lease expires within five seconds; the runtime hibernates under its existing timer rules and wakes on attachment.

Snapshots include DOM/shadow trees, adopted stylesheets, CSSOM edits, form state and scroll position, followed by incremental updates. Input waits for snapshot, viewport and scroll synchronization. Accepted input runs before a replacement snapshot, including its handlers and microtasks. Slow viewers resynchronize after the bounded update journal overflows (128 batches or 8 MiB). If page execution falls behind, the viewer shows **Processing input** and pauses new input before its queue fills. It resumes automatically after queued input reaches the runtime, preserving the connection and accepted events. The browser also pauses immediately when a synchronous event burst fills its local batch, including pointer entry into deeply nested content; resuming does not send that entry again while the pointer rests there. Reload the viewer tab to load updated viewer JavaScript.

Page scripts execute in the runtime. The viewer renders a sandboxed copy with page scripts disabled. Its document keeps the service's origin; it never acquires the source site's privileges. Linked images and styles load using the viewer browser's resource policies and cookies, separately from the API process. 2D canvas pixels are not mirrored.

Frames whose documents run in the page's runtime are shown with those documents. This covers same-origin frames, `srcdoc` frames, `about:blank` frames and frames nested inside them. Each one is a frame of the sandboxed copy, so page scripts stay disabled there too. Clicks, typing and other input inside a frame reach the frame's own window in the runtime. A frame whose document runs elsewhere (another site, in a separate runtime) shows an empty box. Frames of another origin of the same site cannot open in this mode. Scrolling inside a frame, media and WebGL in frames, and frame navigation from the viewer are not supported.

### Wikipedia interactions

Watch the [Wikipedia session demo](assets/README.md#wikipedia-session-demo) for browsing and search, closing the viewer tab, and reopening the same viewer URL with the unfinished search preserved. Keep the service running while the tab is closed; stopping it ends the live session.

From the [quick start](../README.md#quick-start), search for **Quicksort**, open its animated sorting image, and click the top-right **X**. Closing the enlarged image returns to the article while keeping the same runtime. Repeated image open/close cycles are validated.

Wikipedia search stays visible when its search widget initializes and supports typing and suggestions. Initial replacement of the static search field can require a second click before typing. More generally, page-requested focus changes do not yet reach the viewer.

After rebuilding the runtime, stop the old service with Ctrl+C, rerun `python3 tools/view.py URL` and open the newly printed viewer URL. Disconnecting and reconnecting retains the running executable. A service restart keeps native profile stores but starts fresh live page state.

### YouTube interactions

Searching **work music** and opening the first video has been checked with advancing playback and a subsequent **focus music** search. Rebuild and restart the service to use the selector performance fix; reconnecting alone keeps the old runtime executable.

Longer playback stops once the default media replay retention quota (32 MiB, 10,000 commands; see below) is used up: appends are then refused and the player shows an error, typically after about a minute of video. Launch with `VIEWER_MEDIA_REPLAY_UNLIMITED=1` to lift it. Separately, some runs stall at the end of their initial 60-second media buffer even with the quota lifted; that cause is still unknown. The short playback check above does not establish uninterrupted long-video playback.

Reattaching a playing YouTube page can still disconnect with **Stale media backend**. This is tracked separately from the search-to-video stall. Restarting the service starts a fresh live page.

### Navigation and history

Links, forms, `location` assignments, reloads and history traversal can load another document. The driver loads it into a new runtime and switches the viewer after it is ready. Same-document history changes keep the runtime. The service URL and sandbox frame stay on the viewer service throughout.

Fragment links such as `#Algorithm` scroll to the matching element ID or legacy named anchor. URLs opened with a fragment start at that target; encoded IDs and repeated clicks on the same fragment work too. An empty `#` or unmatched `#top` returns to the page start. Same-document Back/Forward restores saved scroll positions unless the page requests manual restoration. Nested scroll containers, CSS scroll offsets and text-fragment directives remain unsupported.

Use the toolbar's **Back** and **Forward** buttons to traverse runtime history. They show actual history availability and stay disabled while synchronizing, loading, disconnected or waiting for a command. Reconnect restores their state. The browser's own Back/Forward buttons and Alt+Left do not control runtime history. Older running runtimes leave the toolbar controls disabled until their service/runtime is restarted with the new executable.

Failures or refused destinations keep the current document and show a reason on the status line. New windows, downloads, non-HTML destinations, nested-frame navigation and a browser-style leave-page prompt remain unsupported. Cross-document history traversal reloads the URL as GET; it does not restore a back-forward cache, history state or a submitted form body. Cookies, localStorage, IndexedDB, permissions and per-origin sessionStorage persist across document navigation within the run.

A document can temporarily have no root element, for example after `document.open()`. The viewer stays connected in this state, supports reconnect and toolbar Back, and displays content if the document is recreated later. Pages that rely on parser-time `document.write`, including enable-javascript.com, can still render blank because that API is incomplete; they should no longer leave the viewer stuck at **Synchronizing**. Restart the launcher after rebuilding to use the fix.

Google Images previews also require stylesheets in temporary HTML documents. This is supported locally, along with queue backpressure for busy page handlers. Live CSS rule declaration edits used to switch an already-open preview are also supported. Restart an existing service to load these changes.

### Google Search

To open Google from the repository root:

```sh
python3 tools/view.py https://www.google.com/
```

Search form submission and immediate startup redirects are supported. Isolated live searches have opened results on javascript.com, Wikipedia, MDN, W3Schools and web.dev and returned with toolbar Back. Google can instead redirect a search to its unusual-traffic verification page; reference Firefox on the same network has received the same check.

reCAPTCHA's frames are of `www.google.com`, the page's own origin, so the viewer displays them and passes your input to them. Their names and named lookup (`parent.frames[name]`) work, as do the cross-window MessagePort and ArrayBuffer transfers they use. Solving the check is left to you; the runtime does not automate it. Whether Google then accepts the result is not established. Rebuild and restart the service to load runtime fixes; reconnecting alone keeps the running executable. These compatibility fixes do not remove Google's traffic verification.

### Media and replay quota

HTTP media commands drive native browser video/audio and MediaSource/SourceBuffer objects, returning playback state, events and promise results. Media resource bytes come through the API process with the page's origin and cookies. Normal browser autoplay and codec restrictions apply.

Reconnect reconstructs logical MSE sources/buffers from retained commands and restores saved position and play intent. URL media restores its saved position paused. Replay retention defaults to **32 MiB** of command payload accounting and **10,000 commands**. To disable both retention quotas, add this setting when starting a new service:

```sh
VIEWER_MEDIA_REPLAY_UNLIMITED=1 python3 tools/view.py https://x.com/home
```

Unset or `0` retains the defaults. This setting does not change an existing session and permits unbounded replay retention. Live transport limits (4 MiB per append, 8 MiB pending segment bytes, 1,024 queued events/commands), browser resource limits and reconstruction limits (16 MiB, 10,000 commands, ten-second deadline) still apply. Unlimited retention can exceed what reconnect can reconstruct.

For the media fixture, serve `bench` instead of `bench/http-viewer` and use `http://localhost:8000/http-viewer/media-demo.html` as the source URL. Attach, play/pause/seek, reconnect, then try Rebuild MSE. Use `webgl-demo.html` from the original demo server to check drawing and WebGL replay. The WebGL journal has its own 32 MiB/10,000-batch bound; overflow loses the contexts.

### Developer harness: X with an imported login

For compatibility work using an existing Firefox login, the direct harness can seed native stores from that Firefox profile. This differs from the launcher's persistent native `--profile`. With a Firefox profile whose X login is current:

```sh
VIEWER_HTTP=127.0.0.1:8765 \
  REALSITES_PROFILE=/path/to/firefox-profile ONLY=x \
  MINI_DIR=realsites-mini-x-viewer MINI_OUT=mini_realsites_x_viewer.json \
  REALSITE_ARGS=--console python3 bench/mini_realsites.py
```

Live trials cover Home/Explore navigation, Back/Forward, scrolling, reconnect, video playback, unsent composer typing and consecutive Grok send-button prompts. Editable selection is preserved through mirror updates, and bounded native editable DOM snapshots reach runtime input handlers. Full rich editing, IME and posting remain unverified. Grok's agreement must be accepted through its visible controls; use the send button because Enter can restore a submitted draft.

Chat/Calls styles are mirrored, including shadow-root adopted sheets. Chat still needs OPFS support. Start an instant call can navigate to `call.x.com`, but capture/WebRTC is unavailable. Clean logs alone do not establish that a site feature works.

### Listener and access

The listener accepts IPv4 loopback addresses only. Its random URL token authorizes attachment, input and explicit stop (`POST /api/stop`); requests validate Host and Origin. One active viewer is served per service, with no external Python server package.

The viewer page sends its calls (attach, updates, input, media and WebGL replies, detach) over one WebSocket, `/api/ws`. The token is in that URL's query because a browser cannot set headers on a WebSocket; the Origin header must be the service's own. Page files, `/media/` resources, the runtime list and the page-close detach stay plain HTTP. Add `transport=` to the viewer URL to choose:

| `transport=` | Behavior |
|---|---|
| `auto` (default) | WebSocket; if it cannot open, HTTP long polling and JSON POSTs until the page is reloaded |
| `ws` | WebSocket only; a socket that cannot open shows "WebSocket unavailable" |
| `poll` | HTTP long polling and JSON POSTs only |

Both transports carry the same calls with the same rules (snapshot gating, ordered input and its backpressure, session replacement, navigation, media and WebGL). A WebSocket lost after opening shows "Viewer connection lost. Click Connect to reattach."; the runtime keeps running and Connect opens a new socket. The report's `viewer` section counts each transport's calls and bytes (`http_api_*`, `ws_*`). The measured difference is in [testing](testing.md#transport-comparison).

