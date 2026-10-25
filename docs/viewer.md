# Viewer operation and limitations

Start with the [README quick start](../README.md#quick-start). This guide describes configuration, service behavior and current limits. Commands below run from the repository root.

## Environment and stores

For a finite run without a viewer:

```sh
NO_FIREFOX=1 API_ENV=/path/to/recorded-env.json \
  ONLY=example EXTRA_SITES='{"example":"http://localhost:8000/page.html"}' \
  MINI_OUT=mini_realsites_standalone.json python3 bench/mini_realsites.py
```

Serve the selected source URL first. `API_ENV` is a browser environment recording made by the Firefox bridge harness, containing `entries` and `viewport`. If omitted, the driver selects the latest matching recording in `bench/realsites-env/`. Every secure/isolated context the page uses needs a matching entry. Standalone mode never starts a browser to supply missing entries or refresh an expired login.

`REALSITES_PROFILE=/path/to/profile` selects the Firefox profile used to seed native stores. Cookies, localStorage, saved permissions and supported IndexedDB databases are copied into partitioned API stores. The profile is read without starting Firefox; subsequent API writes do not update it. A fresh harness run recreates its stores, while navigation and API-process restarts within that run retain persisted data. Use distinct `MINI_DIR` and `MINI_OUT` values for runs whose artifacts you want to keep.

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
VIEWER_HTTP=127.0.0.1:8765 API_ENV=/path/to/recorded-env.json \
  ONLY=demo EXTRA_SITES='{"demo":"http://localhost:8000/demo.html"}' \
  MINI_DIR=realsites-mini-viewer MINI_OUT=mini_realsites_viewer.json \
  python3 bench/mini_realsites.py
```

Open the complete printed `VIEWER_URL`, including its token, in an ordinary browser. The service starts no browser itself. `VIEWER_HTTP` implies standalone mode; `ONLY` must select one runtime. Use `127.0.0.1:0` to choose a free port. Ctrl+C in the service terminal stops the runtime and writes the report.

Click, type and scroll in the viewer. Close its tab or use Disconnect, then open the same URL again: the current document's runtime, JavaScript state, form state and scroll position survive. Each attachment replaces the previous active viewer. A disconnected viewer's lease expires within five seconds; the runtime hibernates under its existing timer rules and wakes on attachment.

Snapshots include DOM/shadow trees, adopted stylesheets, CSSOM edits, form state and scroll position, followed by incremental updates. Input waits for snapshot, viewport and scroll synchronization. Accepted input runs before a replacement snapshot, including its handlers and microtasks. Slow viewers resynchronize after the bounded update journal overflows (128 batches or 8 MiB).

Page scripts execute in the runtime. The viewer renders a sandboxed copy with page scripts disabled. Its document keeps the service's origin; it never acquires the source site's privileges. Linked images and styles load using the viewer browser's resource policies and cookies, separately from the API process. Nested frame documents and 2D canvas pixels are not mirrored.

### Navigation and history

Links, forms, `location` assignments, reloads and history traversal can load another document. The driver loads it into a new runtime and switches the viewer after it is ready. Same-document history changes keep the runtime. The service URL and sandbox frame stay on the viewer service throughout.

Use the toolbar's **Back** and **Forward** buttons to traverse runtime history. They show actual history availability and stay disabled while synchronizing, loading, disconnected or waiting for a command. Reconnect restores their state. The browser's own Back/Forward buttons and Alt+Left do not control runtime history. Older running runtimes leave the toolbar controls disabled until their service/runtime is restarted with the new executable.

Failures or refused destinations keep the current document and show a reason on the status line. New windows, downloads, non-HTML destinations, nested-frame navigation and a browser-style leave-page prompt remain unsupported. Cross-document history traversal reloads the URL as GET; it does not restore a back-forward cache, history state or a submitted form body. Cookies, localStorage, IndexedDB, permissions and per-origin sessionStorage persist across document navigation within the run.

### Media and replay quota

HTTP media commands drive native browser video/audio and MediaSource/SourceBuffer objects, returning playback state, events and promise results. Media resource bytes come through the API process with the page's origin and cookies. Normal browser autoplay and codec restrictions apply.

Reconnect reconstructs logical MSE sources/buffers from retained commands and restores saved position and play intent. URL media restores its saved position paused. Replay retention defaults to **32 MiB** of command payload accounting and **10,000 commands**. To disable both retention quotas, add this setting when starting a new service:

```sh
VIEWER_MEDIA_REPLAY_UNLIMITED=1 VIEWER_HTTP=127.0.0.1:8765 \
  API_ENV=/path/to/recorded-env.json ONLY=x \
  MINI_DIR=realsites-mini-x-unlimited MINI_OUT=mini_realsites_x_unlimited.json \
  python3 bench/mini_realsites.py
```

Unset or `0` retains the defaults. This setting does not change an existing session and permits unbounded replay retention. Live transport limits (4 MiB per append, 8 MiB pending segment bytes, 1,024 queued events/commands), browser resource limits and reconstruction limits (16 MiB, 10,000 commands, ten-second deadline) still apply. Unlimited retention can exceed what reconnect can reconstruct.

For the media fixture, serve `bench` instead of `bench/http-viewer` and use `http://localhost:8000/http-viewer/media-demo.html` as the source URL. Attach, play/pause/seek, reconnect, then try Rebuild MSE. Use `webgl-demo.html` from the original demo server to check drawing and WebGL replay. The WebGL journal has its own 32 MiB/10,000-batch bound; overflow loses the contexts.

### Try X

With a recorded environment and a profile whose X login is current:

```sh
VIEWER_HTTP=127.0.0.1:8765 API_ENV=/path/to/recorded-env.json \
  REALSITES_PROFILE=/path/to/firefox-profile ONLY=x \
  MINI_DIR=realsites-mini-x-viewer MINI_OUT=mini_realsites_x_viewer.json \
  REALSITE_ARGS=--console python3 bench/mini_realsites.py
```

Live trials cover Home/Explore navigation, Back/Forward, scrolling, reconnect, video playback, unsent composer typing and consecutive Grok send-button prompts. Editable selection is preserved through mirror updates, and bounded native editable DOM snapshots reach runtime input handlers. Full rich editing, IME and posting remain unverified. Grok's agreement must be accepted through its visible controls; use the send button because Enter can restore a submitted draft.

Chat/Calls styles are mirrored, including shadow-root adopted sheets. Chat still needs OPFS support. Start an instant call can navigate to `call.x.com`, but capture/WebRTC is unavailable. Clean logs alone do not establish that a site feature works.

### Listener and access

The listener accepts IPv4 loopback addresses only. Its random URL token authorizes attachment, input and explicit stop (`POST /api/stop`); requests validate Host and Origin. Transport uses HTTP long polling and JSON POSTs, with one active viewer per service and no external Python server package.

