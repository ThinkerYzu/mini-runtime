"""Loopback HTTP transport for a runtime's DOM mirror and input. No browser dependency.

The runtime owns the document: detach discards transport state; attach requests a
new snapshot from the runtime. A bounded journal covers changes while the browser
applies it. Overrun requests another snapshot rather than retaining history.
"""
import base64
from collections import deque
import hashlib
from webgl_dummy import DummyWebGL
import hmac
import http.server
import ipaddress
import json
import os
from pathlib import Path
import re
import secrets
import signal
import socket
import struct
import threading
import time
from urllib.parse import parse_qs, urlsplit

ASSETS = Path(__file__).with_name("http-viewer")
MAX_BODY = 1024 * 1024
MAX_GL = 8 * 1024 * 1024
# Media element resources through the API process (GAPS B172): one API request's range, the handles
# a runtime may hold, the bytes kept in memory (runtime Blobs, and the whole answers of servers that
# ignore Range), the largest one kept, and the responses streaming at once (VIEWER_MEDIA_CHUNK: another
# range size, for the tests)
MEDIA_CHUNK = int(os.environ.get("VIEWER_MEDIA_CHUNK") or 1024 * 1024)
MAX_RESOURCES = 4096
MAX_RESOURCE_BYTES = 64 * 1024 * 1024
MAX_RESOURCE_ONE = 32 * 1024 * 1024
MAX_STREAMS = 8
# The WebSocket transport (DESIGN "WebSocket transport"): RFC 6455's handshake GUID, and the seconds
# a socket may send nothing, or an answer may wait to be written, before the socket is closed
WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"
WS_TIMEOUT = 15


def after_notice(viewer):
    """Whether a poll has news beyond batches: a notice, or a navigation's start or end, it has not
    given yet"""
    return viewer.polled_notice != viewer.notice_seq or viewer.polled_navigating != viewer.navigating


# the capability answers a profile keeps (Viewer media_cache)
MAX_MEDIA_CACHE = 256


def load_media_cache(path):
    """The capability answers kept at `path`, by media type: {type: [isTypeSupported, canPlayType]};
    empty when there is no file or it is not such a map."""
    try:
        with open(path) as f:
            data = json.load(f)
    except (OSError, ValueError, TypeError):
        return {}
    if not isinstance(data, dict):
        return {}
    return {k: v for k, v in list(data.items())[:MAX_MEDIA_CACHE]
            if isinstance(k, str) and len(k) <= 4096 and isinstance(v, list) and len(v) == 2
            and type(v[0]) is bool and v[1] in ("", "maybe", "probably")}


class Viewer:
    def __init__(self, name, page_url, viewport, *, lease=5, first_attach=60, media_cache=None, max_batches=128,
                 max_bytes=8 * 1024 * 1024, gl_max_bytes=32 * 1024 * 1024, gl_max_batches=10000):
        self.name, self.page_url = name, page_url
        self.viewport = list(viewport)
        self.lease, self.max_batches, self.max_bytes = lease, max_batches, max_bytes
        # The started page is a tab opened to be seen (task 9a0d7, GAPS B216): it stays visible while
        # its first viewer comes, up to `first_attach` seconds. Hidden from the start, a page put off
        # its work for the visible document (Instagram's post-load scrollTo(0, 0)) until the viewer
        # had shown the feed and taken the owner's first scroll, which it then undid.
        self.started_at, self.first_attach = time.monotonic(), first_attach
        self.lock = threading.Condition()
        self.session = None
        self.last_seen = 0
        self.visible = True
        self.ready = False
        # the session's viewer has applied a snapshot at the current viewport: the node ids of the
        # input it captures stay valid through a later resynchronization (task d71f3)
        self.viewport_applied = False
        self.snapshot_needed = False
        self.awaiting_snapshot = False
        self.snapshot_seq = None
        self.seq = 0
        self.journal = deque()
        self.journal_bytes = 0
        self.events = []
        self.stop_requested = False
        self.runtime_pid = None
        self.media_epoch = 0
        self.media_query = None
        # whether a viewer has attached since this document began: until one has, a capability
        # query waits for it instead of answering negative (task 6416e)
        self.document_attached = False
        # the viewer browsers' capability answers, by media type, kept in the native profile's store
        # (task 9a0d7, GAPS B216): a query before the first viewer gets the answer the profile's
        # browser gave before, at once, as Firefox answers from its own codecs; the wait froze
        # Instagram's whole startup until the viewer came
        self.media_cache_path = media_cache
        self.media_cache = load_media_cache(media_cache)
        self.media_next = 0
        self.media_events = []
        self.media_bytes = 0
        self.gl_dummy = DummyWebGL()
        # the runtime's live context ids (made and not dropped), whichever backend answers
        self.gl_live = set()
        self.gl_history = []
        self.gl_bytes = 0
        self.gl_max_bytes, self.gl_max_batches = gl_max_bytes, gl_max_batches
        self.gl_cursor = 0
        self.gl_epoch = 0
        self.gl_job = None
        self.gl_next = 0
        self.gl_lost = set()
        # handle -> a media element's resource (B172): {"url", "crossOrigin", "kind", "site", "top",
        # "origin", "referrer", "doc"} for a network one, with "data" and "type" once its bytes are
        # held (a runtime Blob, or a server's whole answer)
        self.resources = {}
        self.resource_bytes = 0
        # the service origin the handles' URLs are on (Service), and the driver's API-process call
        # for a network resource's byte range: fetch(entry, range) -> the `media` op's reply
        self.resource_base = ""
        self.media_fetch = None
        self.streams = threading.BoundedSemaphore(MAX_STREAMS)
        # a navigation (DESIGN "Top-level navigation in the HTTP viewer"): the URL loading, the last
        # message for the status line (a failure or refusal) with its number, and the sessions the
        # last commit ended, which are told so, and when it was
        self.navigating = None
        self.notice = None
        self.notice_seq = 0
        self.navigated_sessions = set()
        self.committed_at = None
        self.polled_notice = 0
        self.polled_navigating = None
        self.stats = {"media_resources": 0, "media_resource_requests": 0, "media_resource_bytes": 0, "media_resource_chunks": 0, "media_resource_cancels": 0, "media_ops": 0, "media_replays": 0, "media_bytes": 0, "media_events": 0, "media_queries": 0, "media_attach_waits": 0, "media_cache_answers": 0, "media_resets": 0, "media_stale_events": 0, "webgl_jobs": 0, "webgl_timeouts": 0, "webgl_dummy": 0, "webgl_native": 0, "webgl_replays": 0, "webgl_overflows": 0, "attachments": 0, "snapshots": 0, "resyncs": 0, "events": 0,
                      "navigations": 0, "navigation_failures": 0, "navigation_refusals": 0, "navigation_input_dropped": 0,
                      "resync_input": 0, "update_peak_bytes": 0, "journal_peak_bytes": 0, "journal_peak_batches": 0, "last_overflow": None,
                      "http_api_calls": 0, "http_api_bytes_in": 0, "http_api_bytes_out": 0,
                      "ws_connections": 0, "ws_calls": 0, "ws_bytes_in": 0, "ws_bytes_out": 0}

    def _detach(self):
        self._media_reset()
        self.gl_epoch += 1
        self.gl_job = None
        self.gl_cursor = 0
        self.session = None
        self.ready = self.viewport_applied = False
        self.snapshot_needed = self.awaiting_snapshot = False
        self.snapshot_seq = None
        self.journal.clear()
        self.journal_bytes = 0
        # Accepted input remains owned by the runtime across viewer changes.
        self.lock.notify_all()

    def _expire(self):
        if self.session and time.monotonic() - self.last_seen > self.lease:
            self._detach()

    def _check(self, session):
        self._expire()
        if session and session in self.navigated_sessions:
            raise ValueError("Document replaced by a navigation; attach again")
        if not self.session or session != self.session:
            raise ValueError("Viewer replaced, disconnected or expired; attach again")
        self.last_seen = time.monotonic()

    def _media_reset(self):
        self.media_epoch += 1
        self.media_query = None
        self.media_events.clear()
        self.media_bytes = 0
        self.stats["media_resets"] += 1

    def _resync(self):
        self.ready = False
        self.snapshot_needed = self.awaiting_snapshot = True
        self.snapshot_seq = None
        # Accepted input remains owned by the runtime across viewer changes.
        self.journal.clear()
        self.journal_bytes = 0

    @staticmethod
    def size(viewport):
        if not isinstance(viewport, list) or len(viewport) != 2 or any(type(n) is not int or not 1 <= n <= 8192 for n in viewport):
            raise ValueError("viewport must be [width, height], 1..8192 CSS pixels")
        return viewport

    def attach(self, viewport):
        viewport = self.size(viewport)
        with self.lock:
            self._detach()
            self.session = secrets.token_urlsafe(24)
            self.last_seen = time.monotonic()
            self.viewport = viewport
            self.visible = True
            self._resync()
            self.document_attached = True
            self.stats["attachments"] += 1
            self.committed_at = None
            return {"session": self.session, "runtime": self.name, "url": self.page_url, "pid": self.runtime_pid, "mediaEpoch": self.media_epoch}

    def detach(self, session):
        with self.lock:
            self._check(session)
            self._detach()

    def publish(self, ops, snapshot=None):
        with self.lock:
            # the document's URL after pushState or replaceState (bridge.js `u`), whether or not a viewer
            # takes these ops: /api/runtimes and the next attachment report it (B203)
            for o in ops:
                if o[0] == "u":
                    self.page_url = o[1]
            self._expire()
            if not self.session or (snapshot and snapshot != self.session):
                return
            if self.awaiting_snapshot and not snapshot:
                return
            self.stats["media_ops"] += sum(o[0] == "m" for o in ops)
            self.stats["media_replays"] += sum(o[0] == "m" and o[1].get("o") == "attach" and o[1].get("replay", False) for o in ops)
            self.stats["media_bytes"] += sum(len(o[1].get("b64", "")) * 3 // 4 for o in ops if o[0] == "m")
            size = len(json.dumps(ops))
            if not snapshot:
                self.stats["update_peak_bytes"] = max(self.stats["update_peak_bytes"], size)
            if not snapshot and (len(self.journal) >= self.max_batches or self.journal_bytes + size > self.max_bytes):
                self.stats["resyncs"] += 1
                # (what overflowed: the unacknowledged batches and bytes, and the update that did not fit)
                self.stats["last_overflow"] = {"batches": len(self.journal), "bytes": self.journal_bytes, "update_bytes": size,
                                               "media_b64": sum(len(o[1].get("b64", "")) for o in ops if o[0] == "m")}
                self._media_reset()
                self._resync()
                self.lock.notify_all()
                return
            if snapshot:
                self.journal.clear()
                self.journal_bytes = 0
                self.awaiting_snapshot = False
                self.stats["snapshots"] += 1
            self.seq += 1
            # The budget bounds the updates after a snapshot, not the snapshot itself: one that carries
            # a large media replay may exceed it, and counting it made the next update resynchronize
            # again before the viewer could acknowledge it, without end (task 3450e).
            self.journal.append({"seq": self.seq, "snapshot": bool(snapshot), "ops": ops})
            self.journal_bytes += 0 if snapshot else size
            self.stats["journal_peak_bytes"] = max(self.stats["journal_peak_bytes"], self.journal_bytes)
            self.stats["journal_peak_batches"] = max(self.stats["journal_peak_batches"], len(self.journal))
            if snapshot:
                self.snapshot_seq = self.seq
            self.lock.notify_all()

    def poll(self, session, after, timeout=1):
        with self.lock:
            self._check(session)
            end = time.monotonic() + timeout
            while not self.stop_requested:
                self._check(session)
                batches = [b for b in self.journal if b["seq"] > after]
                self._gl_prepare_replay()
                job = self.gl_job
                query = self.media_query
                if batches or (job and not job["sent"]) or (query and not query["sent"]) or after_notice(self):
                    result = {"batches": batches, "ready": self.ready, "mediaEpoch": self.media_epoch}
                    self._state(result)
                    if query and not query["sent"]:
                        query["sent"] = True
                        result["mediaType"] = {"id": query["id"], "type": query["type"]}
                    if job and not job["sent"]:
                        job["sent"] = True
                        result["webgl"] = {"id": job["id"], "request": job["request"]}
                    return result
                left = end - time.monotonic()
                if left <= 0:
                    break
                self.lock.wait(left)
            return self._state({"batches": [], "stopped": self.stop_requested, "mediaEpoch": self.media_epoch})

    def _state(self, result):
        """A poll's navigation fields: the load in progress, and the latest notice"""
        if self.navigating:
            result["navigation"] = {"state": "loading", "url": self.navigating}
        if self.notice:
            result["notice"] = {"seq": self.notice_seq, "text": self.notice}
        self.polled_notice, self.polled_navigating = self.notice_seq, self.navigating
        return result

    def navigation_started(self, url):
        with self.lock:
            self.navigating = url
            self.lock.notify_all()

    def navigation_failed(self, url, why):
        with self.lock:
            self.navigating = None
            self.stats["navigation_failures"] += 1
            self._notice(f"Navigation to {url} failed: {why}")

    def navigation_notice(self, text):
        with self.lock:
            self.stats["navigation_refusals"] += 1
            self._notice(text)

    def _notice(self, text):
        self.notice, self.notice_seq = text[:2000], self.notice_seq + 1
        self.lock.notify_all()

    def navigation_committed(self, url, pid):
        """The page is a new runtime's document: the session ends (its viewer attaches again and gets
        a snapshot of the new document), and everything kept for the old document goes: accepted
        input, media handles and bytes, the WebGL history and contexts"""
        with self.lock:
            if self.session:
                self.navigated_sessions.add(self.session)
            self._detach()
            self.events.clear()
            self.resources.clear()
            self.resource_bytes = 0
            self.gl_dummy = DummyWebGL()
            self.gl_live.clear()
            self.gl_history.clear()
            self.gl_bytes = 0
            self.gl_lost.clear()
            self.page_url, self.runtime_pid = url, pid
            self.document_attached = False
            self.navigating = None
            self.notice = None
            self.committed_at = time.monotonic()
            self.stats["navigations"] += 1
            self.lock.notify_all()

    def input(self, session, ack, events, viewport=None, visible=None):
        if not isinstance(events, list) or len(events) > 256 or any(not isinstance(e, dict) for e in events):
            raise ValueError("events must be an array of at most 256 records")
        if viewport is not None:
            self.size(viewport)
        with self.lock:
            self._check(session)
            if type(ack) is not int or ack < 0 or ack > self.seq:
                raise ValueError("invalid sequence acknowledgement")
            if viewport is not None and viewport != self.viewport:
                self.viewport = viewport
                self.viewport_applied = False
                self._resync()
            if visible is not None:
                self.visible = bool(visible)
            if self.snapshot_seq is not None and ack >= self.snapshot_seq:
                self.ready = self.viewport_applied = True
            # The browser acknowledged applying these batches. Keep no delivered history.
            while self.journal and self.journal[0]["seq"] <= ack:
                batch = self.journal.popleft()
                if not batch["snapshot"]:
                    self.journal_bytes -= len(json.dumps(batch["ops"]))
            if events and not self.viewport_applied:
                raise ValueError("snapshot and viewport must be applied before input")
            if events and not self.ready:
                # Input the viewer captured before it heard of a resynchronization (a journal overflow,
                # a media queue reset) refers to node ids the new snapshot keeps, so it is taken as any
                # input that lags the copy is; refusing it disconnected the viewer (task d71f3). The
                # reply's ready false pauses capture until the new snapshot is applied.
                self.stats["resync_input"] += len(events)
            if events and self.navigating:
                # (the old document is frozen and about to go: its input is dropped, not an error
                # that would disconnect the viewer)
                self.stats["navigation_input_dropped"] += len(events)
                return {"ready": False}
            if len(self.events) + len(events) > 1024:
                raise ValueError("input queue full")
            self.events.extend(events)
            self.stats["events"] += len(events)
            self.lock.notify_all()
            # Allow for this batch crossing the pause threshold plus the one captured
            # while its reply was in flight (both at most 256 records).
            # Snapshot readiness stays true so that already-captured input is accepted.
            pending = len(self.events) >= 512
            return {"ready": self.ready and not pending, "inputPending": pending}

    def media_type(self, req, timeout=5):
        """A media capability query of the runtime ([isTypeSupported, canPlayType, epoch, attached]),
        answered by the attached viewer's browser. A document that no viewer has attached to yet waits
        for the first one: a page opened at its URL checks its codecs while it loads, before anyone
        opens the viewer, and a player that hears "nothing plays" then never asks again (YouTube,
        task 6416e), unless the profile kept that type's answer from an earlier viewer (media_cache).
        Once a viewer has attached, a detached query is answered negative at once."""
        kind = req.get("type", "")
        if not isinstance(kind, str) or len(kind) > 4096:
            return {"err": "Invalid media type"}
        with self.lock:
            self._expire()
            if not self.document_attached and kind in self.media_cache:
                self.stats["media_cache_answers"] += 1
                return {"ok": [*self.media_cache[kind], self.media_epoch, True]}
            if not self.document_attached and not self.stop_requested:
                self.stats["media_attach_waits"] += 1
                while not self.document_attached and not self.stop_requested:
                    self.lock.wait(1)
            epoch, session = self.media_epoch, self.session
            if not session or self.stop_requested:
                return {"ok": [False, "", epoch, False]}
            if self.media_query:
                return {"err": "Media capability request already pending"}
            self.media_next += 1
            query = {"id": self.media_next, "type": kind, "sent": False, "reply": None}
            self.media_query = query
            self.stats["media_queries"] += 1
            self.lock.notify_all()
            end = time.monotonic() + timeout
            while self.session == session and self.media_epoch == epoch and not self.stop_requested and query["reply"] is None:
                self._expire()
                left = end - time.monotonic()
                if left <= 0:
                    self._detach()
                    break
                self.lock.wait(min(left, 0.1))
            if self.media_query is query:
                self.media_query = None
            if self.session != session or self.media_epoch != epoch or query["reply"] is None:
                return {"ok": [False, "", self.media_epoch, False]}
            self._remember_media_type(kind, query["reply"])
            return {"ok": [*query["reply"], epoch, True]}

    def _remember_media_type(self, kind, reply):
        """Keeps a viewer browser's answer in the profile's capability file (media_cache)."""
        if not self.media_cache_path or self.media_cache.get(kind) == reply:
            return
        if kind not in self.media_cache and len(self.media_cache) >= MAX_MEDIA_CACHE:
            return
        self.media_cache[kind] = list(reply)
        try:
            os.makedirs(os.path.dirname(self.media_cache_path), mode=0o700, exist_ok=True)
            tmp = self.media_cache_path + ".tmp"
            with open(tmp, "w") as f:
                json.dump(self.media_cache, f)
            os.replace(tmp, self.media_cache_path)
        except OSError:
            pass

    def media_type_reply(self, session, ident, reply):
        with self.lock:
            self._check(session)
            query = self.media_query
            if not query or query["id"] != ident or not query["sent"] or query["reply"] is not None:
                raise ValueError("Stale media capability reply")
            if not isinstance(reply, list) or len(reply) != 2 or type(reply[0]) is not bool or reply[1] not in ("", "maybe", "probably"):
                raise ValueError("Invalid media capability reply")
            query["reply"] = reply
            self.lock.notify_all()
            return {}

    def media_input(self, session, epoch, events):
        if not isinstance(events, list) or len(events) > 256 or any(not isinstance(e, dict) or e.get("k") not in ("el", "ms", "sb", "play", "loss") for e in events):
            raise ValueError("Invalid media events")
        size = len(json.dumps(events))
        with self.lock:
            self._check(session)
            if type(epoch) is not int or epoch > self.media_epoch:
                raise ValueError("Stale media backend")
            if epoch < self.media_epoch:
                # The events of a backend reset (resynchronization, a full queue) while this session
                # stays attached, sent before its poll brought the new epoch: they are dropped, and the
                # viewer goes on with the new backend (task 3450e)
                self.stats["media_stale_events"] += len(events)
                return {"stale": True}
            if len(self.media_events) + len(events) > 1024 or self.media_bytes + size > MAX_BODY:
                self._media_reset()
                self._resync()
                self.lock.notify_all()
                raise ValueError("Media event queue full; backend reset")
            self.media_events.extend(events)
            self.media_bytes += size
            self.stats["media_events"] += len(events)
            self.lock.notify_all()
            return {}

    def media_resource(self, req, context):
        """A runtime's `mediaResource` op: register a media element's resource and get the URL the
        viewer loads it from, or release handles. `context` is the driver's {site, top, doc} of the
        runtime; the request carries the URL, its crossorigin, the element kind, origin and
        referrer, or a runtime Blob's bytes (`b64`, `type`)."""
        with self.lock:
            for handle in req.get("release") or []:
                entry = self.resources.pop(handle, None)
                if entry and entry.get("data") is not None:
                    self.resource_bytes -= len(entry["data"])
            if not req.get("url"):
                return {"ok": None}
            if len(self.resources) >= MAX_RESOURCES:
                return {"err": "Too many media resources"}
            entry = {k: req.get(k) for k in ("url", "crossOrigin", "origin", "referrer", "metaReferrer")}
            entry["kind"] = "audio" if req.get("kind") == "audio" else "video"
            entry.update(context)
            if req.get("b64") is not None:
                data = base64.b64decode(req["b64"])
                if len(data) > MAX_RESOURCE_ONE or self.resource_bytes + len(data) > MAX_RESOURCE_BYTES:
                    return {"err": "Media Blob too large for the viewer"}
                entry.update(data=data, type=req.get("type") or "", blob=True)
                self.resource_bytes += len(data)
            elif not re.match(r"(?i)https?:", entry["url"] or ""):
                return {"err": "Unsupported media URL"}
            handle = secrets.token_urlsafe(18)
            self.resources[handle] = entry
            self.stats["media_resources"] += 1
            return {"ok": {"url": f"{self.resource_base}/media/{handle}", "handle": handle}}

    def resource(self, handle):
        """The resource and the viewer session a /media/ request is served for; only an attached
        viewer loads them."""
        with self.lock:
            self._expire()
            if not self.session or self.stop_requested:
                raise PermissionError("No viewer attached")
            self.stats["media_resource_requests"] += 1
            return self.resources.get(handle), self.session

    def resource_live(self, handle, session, entry):
        with self.lock:
            return self.session == session and not self.stop_requested and self.resources.get(handle) is entry

    def resource_keep(self, entry, data, kind):
        """Keep a server's whole answer (one that ignored Range) to serve its ranges, if it fits."""
        with self.lock:
            if entry.get("data") is None and len(data) <= MAX_RESOURCE_ONE and self.resource_bytes + len(data) <= MAX_RESOURCE_BYTES:
                entry.update(data=data, type=kind)
                self.resource_bytes += len(data)

    def gl_state(self):
        return {"epoch": self.gl_epoch, "attached": bool(self.session), "lost": list(self.gl_lost)}

    def _gl_prepare_replay(self):
        if not self.gl_job and self.gl_cursor < len(self.gl_history):
            self.gl_next += 1
            self.gl_job = {"id": self.gl_next, "request": self.gl_history[self.gl_cursor],
                           "replay": True, "sent": False, "reply": None}
            self.stats["webgl_replays"] += 1

    def webgl(self, req, timeout=10, native=None):
        """A runtime's WebGL batch. `native` (the API process's real contexts, GAPS B169) answers
        when given, attached or not; an attached viewer still runs the batch, for its pixels only.
        Without it, an attached viewer answers and the detached dummy stands in."""
        cmds = req.get("cmds")
        if (not isinstance(cmds, list) or len(cmds) > 100000 or
                any(not isinstance(c, list) or len(c) != 5 or type(c[0]) is not int or
                    not isinstance(c[2], str) or not isinstance(c[3], list) for c in cmds) or
                len(json.dumps(req)) > MAX_GL):
            return {"err": "Invalid or oversized WebGL batch"}
        with self.lock:
            self._expire()
            end = time.monotonic() + timeout
            # Attachment can replay even while the runtime is idle. A new batch
            # waits for the retained prefix before it is appended or dispatched.
            while self.session and (self.gl_job or self.gl_cursor < len(self.gl_history)) and not self.stop_requested:
                self.lock.notify_all()
                self.lock.wait(0.05)
                self._expire()
                if time.monotonic() >= end:
                    self.stats["webgl_timeouts"] += 1
                    self._detach()
            if native:
                fallback = self._gl_native(native, req)
            else:
                try:
                    fallback = self.gl_dummy.execute(req)
                except (ValueError, KeyError, IndexError, TypeError) as e:
                    return {"err": str(e)}
            for c in cmds:
                if c[2] == "$create":
                    self.gl_live.add(c[0])
                elif c[2] == "$drop":
                    self.gl_live.discard(c[0])
            # Keep cross-canvas texture dependencies in order, including drops:
            # an earlier upload may have copied a subsequently dropped canvas.
            dropped = {c[0] for c in cmds if c[2] == "$drop"}
            record = {"cmds": cmds,
                      "bin": req.get("bin"), "sync": bool(req.get("sync"))}
            size = len(json.dumps(record))
            overflow = (self.gl_bytes + size > self.gl_max_bytes or len(self.gl_history) >= self.gl_max_batches)
            if overflow:
                ids = list(self.gl_live | {c[0] for c in cmds})
                self.gl_lost.update(ids)
                self.gl_live.clear()
                self.gl_dummy = DummyWebGL()
                self.gl_history.clear()
                self.gl_bytes = self.gl_cursor = 0
                self.stats["webgl_overflows"] += 1
                fallback.update(result=None, lost=ids, outs=[])
            else:
                self.gl_history.append(record)
                self.gl_bytes += size
            job = None
            session = self.session
            if session and not self.stop_requested and not overflow:
                self.gl_next += 1
                job = {"id": self.gl_next, "request": req, "replay": False, "sent": False, "reply": None}
                self.gl_job = job
                self.stats["webgl_jobs"] += 1
                self.lock.notify_all()
                while self.session == session and not self.stop_requested and job["reply"] is None:
                    self._expire()
                    left = end - time.monotonic()
                    if left <= 0:
                        self.stats["webgl_timeouts"] += 1
                        self._detach()
                        break
                    self.lock.wait(min(left, 0.1))
                if self.gl_job is job:
                    self.gl_job = None
            viewed = job and self.session == session and job["reply"] is not None
            reply = job["reply"] if viewed and not native else fallback
            if viewed:
                self.gl_cursor = len(self.gl_history)
            if native:
                self.stats["webgl_native"] += 1
            elif reply is fallback:
                self.stats["webgl_dummy"] += 1
            if dropped and not self.gl_live:
                self.gl_history.clear()
                self.gl_bytes = self.gl_cursor = 0
                self.gl_dummy.objects.clear()
            reply["viewer"] = self.gl_state()
            return {"ok": reply}

    @staticmethod
    def _gl_native(native, req):
        """The API process's answer; a process that is gone has lost every context of the batch."""
        r = native(req)
        if "ok" in r:
            return r["ok"]
        return {"result": None, "outs": [], "throws": [], "lost": sorted({c[0] for c in req["cmds"]}), "lastThrew": None}

    def webgl_reply(self, session, ident, reply):
        with self.lock:
            self._check(session)
            job = self.gl_job
            if not job or job["id"] != ident or not job["sent"] or job["reply"] is not None:
                raise ValueError("Stale WebGL reply")
            if (not isinstance(reply, dict) or not all(k in reply for k in ("result", "outs", "throws", "lost", "lastThrew")) or
                    not all(isinstance(reply[k], list) for k in ("outs", "throws", "lost")) or len(json.dumps(reply)) > MAX_GL):
                raise ValueError("Invalid WebGL reply")
            job["reply"] = reply
            if job["replay"]:
                self.gl_lost.update(reply["lost"])
                # A real context creation failure cannot be a successful replay.
                req = job["request"]
                if req["cmds"] and req["cmds"][-1][2] == "$create" and reply["result"] is None:
                    self.gl_lost.add(req["cmds"][-1][0])
                self.gl_cursor += 1
                self.gl_job = None
            self.lock.notify_all()
            return {}

    def webgl_lost(self, session, ids):
        if not isinstance(ids, list) or len(ids) > 1024 or any(type(i) is not int for i in ids):
            raise ValueError("Invalid context ids")
        with self.lock:
            self._check(session)
            self.gl_lost.update(ids)
            self.lock.notify_all()
            return {}

    def wait(self, req):
        with self.lock:
            self._expire()
            due = 0.1 if req.get("until") is None else max(0, (req["until"] - req["now"]) / 1000)
            if not req.get("busy") and not self.events and not self.snapshot_needed and not self.stop_requested:
                self.lock.wait(min(due, 0.1))
                self._expire()
            events, self.events = self.events, []
            # (a just navigated page waits a lease for its viewer to attach again before hibernating)
            reattaching = self.committed_at is not None and time.monotonic() - self.committed_at < self.lease
            # (the started page, before any viewer has attached: see first_attach)
            awaited = (not self.document_attached and self.committed_at is None and
                       time.monotonic() - self.started_at < self.first_attach)
            state = {"stop": self.stop_requested, "events": events,
                     "hidden": (not self.session and not reattaching and not awaited) or not self.visible, "mirror": bool(self.session),
                     "viewport": self.viewport, "webgl": self.gl_state(),
                     "mediaBackend": {"epoch": self.media_epoch, "attached": bool(self.session)}, "media": self.media_events}
            self.media_events = []
            self.media_bytes = 0
            self.gl_lost.clear()
            if self.session and self.snapshot_needed:
                state["snapshot"] = self.session
                self.snapshot_needed = False
            return {"ok": state}

    def stop(self):
        with self.lock:
            self.stop_requested = True
            self.lock.notify_all()


class NotFound(Exception):
    """An unknown API op or runtime (HTTP 404)"""


class SocketClosed(Exception):
    """The WebSocket ended: a close frame, a protocol error or a lost connection. `code` is the
    close status to send back, or None when nothing can be sent."""
    def __init__(self, code=None):
        super().__init__(code)
        self.code = code


def media_range(header):
    """A request's single byte range: (start, end or None), (None, suffix length), or None for the
    whole resource (no header, or one this does not serve: several ranges, another unit)."""
    got = re.fullmatch(r"\s*bytes\s*=\s*(\d*)\s*-\s*(\d*)\s*", header or "")
    if not got or (not got[1] and not got[2]):
        return None
    if not got[1]:
        return (None, int(got[2]))
    start, end = int(got[1]), (int(got[2]) if got[2] else None)
    return None if end is not None and end < start else (start, end)


class Service:
    def __init__(self, address, viewer):
        host, port = address.rsplit(":", 1)
        if host == "localhost":
            host = "127.0.0.1"
        if not ipaddress.ip_address(host).is_loopback or ":" in host:
            raise ValueError("VIEWER_HTTP must bind to an IPv4 loopback address, e.g. 127.0.0.1:8765")
        self.viewer = viewer
        self.token = secrets.token_urlsafe(32)
        self.server = http.server.ThreadingHTTPServer((host, int(port)), self.handler())
        self.server.daemon_threads = True
        self.origin = f"http://{host}:{self.server.server_port}"
        viewer.resource_base = self.origin
        self.url = f"{self.origin}/?token={self.token}"
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        # the open WebSockets' connections, ended by drop_sockets and close
        self.sockets = set()
        self.sockets_lock = threading.Lock()
        self.old_sigint = self.old_sigterm = None

    def call(self, op, req):
        """One API op for either transport: its result. Raises NotFound for an unknown op or
        runtime, and ValueError, KeyError or TypeError for a refused call (HTTP 409)."""
        viewer = self.viewer
        if op == "attach":
            if req.get("runtime") != viewer.name:
                raise NotFound("Unknown runtime")
            return viewer.attach(req["viewport"])
        if op == "poll":
            after = req.get("after", 0)
            if type(after) is not int:
                raise ValueError("after must be an integer")
            return viewer.poll(req.get("session", ""), after)
        if op == "media-type":
            return viewer.media_type_reply(req["session"], req["id"], req["reply"])
        if op == "media-events":
            return viewer.media_input(req["session"], req["epoch"], req["events"])
        if op == "webgl":
            return viewer.webgl_reply(req["session"], req["id"], req["reply"])
        if op == "webgl-lost":
            return viewer.webgl_lost(req["session"], req["ids"])
        if op == "input":
            return viewer.input(req["session"], req["ack"], req.get("events", []), req.get("viewport"), req.get("visible"))
        if op == "detach":
            viewer.detach(req["session"])
            return {}
        if op == "stop":
            viewer.stop()
            return {}
        raise NotFound("Not found")

    def handler(self):
        service = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass  # Do not log bearer tokens or viewer input.

            def flush_headers(self):
                if getattr(self, "counted", False):
                    service.viewer.stats["http_api_bytes_out"] += sum(map(len, getattr(self, "_headers_buffer", [])))
                super().flush_headers()

            def count_request(self, body=0):
                """Count an HTTP API call: its request line, headers and body (DESIGN "WebSocket
                transport", counters); its answer is counted as it is written."""
                self.counted = True
                stats = service.viewer.stats
                stats["http_api_calls"] += 1
                stats["http_api_bytes_in"] += len(self.raw_requestline) + body + 2 + sum(len(k) + len(v) + 4 for k, v in self.headers.items())

            def reply(self, code, data, content_type="application/json"):
                body = json.dumps(data).encode() if content_type == "application/json" else data
                self.send_response(code)
                self.send_header("Content-Type", content_type)
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Cache-Control", "no-store")
                self.send_header("Referrer-Policy", "no-referrer")
                self.send_header("X-Content-Type-Options", "nosniff")
                if urlsplit(self.path).path == "/frame":
                    policy = "default-src http: https: data: blob:; script-src 'none'; style-src http: https: 'unsafe-inline'; object-src 'none'; frame-src 'none'; form-action 'none'; frame-ancestors 'self'"
                else:
                    policy = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; frame-src 'self'; frame-ancestors 'self'; object-src 'none'"
                self.send_header("Content-Security-Policy", policy)
                self.end_headers()
                self.wfile.write(body)
                if getattr(self, "counted", False):
                    service.viewer.stats["http_api_bytes_out"] += len(body)

            def permitted(self, api=False):
                if self.headers.get("Host") != urlsplit(service.origin).netloc:
                    self.reply(403, {"error": "Unexpected Host"})
                    return False
                if self.headers.get("Origin", service.origin) != service.origin:
                    self.reply(403, {"error": "Unexpected Origin"})
                    return False
                if api and not hmac.compare_digest(self.headers.get("X-Viewer-Token", ""), service.token):
                    self.reply(403, {"error": "Invalid viewer token"})
                    return False
                return True

            def media_reply(self, code, headers=(), body=b""):
                self.send_response(code)
                for k, v in headers:
                    self.send_header(k, v)
                self.send_header("Cache-Control", "no-store")
                self.send_header("Referrer-Policy", "no-referrer")
                if not any(k == "Content-Length" for k, _ in headers):
                    self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def media(self, handle):
                """GET /media/<handle> (GAPS B172): a media element's resource, as the page's
                origin and cookie jar get it through the API process. The browser's Range picks
                the bytes; a network resource streams in MEDIA_CHUNK ranges, each one API request,
                until the range ends, the browser closes the connection or the viewer detaches."""
                viewer = service.viewer
                try:
                    entry, session = viewer.resource(handle)
                except PermissionError as e:
                    self.media_reply(409, body=str(e).encode())
                    return
                if not entry:
                    self.media_reply(404)
                    return
                if not viewer.streams.acquire(blocking=False):
                    self.media_reply(503, [("Retry-After", "1")])
                    return
                try:
                    self.media_stream(viewer, handle, session, entry, media_range(self.headers.get("Range")))
                except (BrokenPipeError, ConnectionResetError):
                    viewer.stats["media_resource_cancels"] += 1
                finally:
                    viewer.streams.release()

            def media_held(self, viewer, data, kind, wanted):
                total = len(data)
                if wanted is None:
                    self.media_reply(200, [("Content-Type", kind), ("Accept-Ranges", "bytes")], data)
                    viewer.stats["media_resource_bytes"] += total
                    return
                start, end = wanted
                if start is None:
                    start, end = max(0, total - end), total - 1
                if start >= total:
                    self.media_reply(416, [("Content-Range", f"bytes */{total}")])
                    return
                end = total - 1 if end is None else min(end, total - 1)
                self.media_reply(206, [("Content-Type", kind), ("Accept-Ranges", "bytes"),
                                       ("Content-Range", f"bytes {start}-{end}/{total}")], data[start:end + 1])
                viewer.stats["media_resource_bytes"] += end + 1 - start

            def media_stream(self, viewer, handle, session, entry, wanted):
                if entry.get("data") is not None:
                    self.media_held(viewer, entry["data"], entry.get("type") or "", wanted)
                    return
                if viewer.media_fetch is None:
                    self.media_reply(502, body=b"No API process")
                    return
                if wanted and wanted[0] is None:
                    first = f"bytes=-{min(wanted[1], MEDIA_CHUNK)}" if wanted[1] else None
                    start, end = None, None
                else:
                    start, end = wanted or (0, None)
                    first = f"bytes={start}-{start + MEDIA_CHUNK - 1 if end is None else min(end, start + MEDIA_CHUNK - 1)}"
                if first is None:
                    self.media_reply(416, [("Content-Range", "bytes */*")])
                    return
                reply = viewer.media_fetch(entry, first)
                viewer.stats["media_resource_chunks"] += 1
                ok = reply.get("ok")
                if not isinstance(ok, dict) or ok.get("pending"):
                    self.media_reply(504 if isinstance(ok, dict) else 502, body=str(reply.get("err", "pending"))[:500].encode())
                    return
                headers = {k.lower(): v for k, v in ok.get("headers") or []}
                kind = headers.get("content-type", "")
                data = base64.b64decode(ok.get("bodyBase64") or "")
                status = ok.get("status")
                if status == 200:
                    # the server ignored Range: its whole answer, kept when it fits
                    viewer.resource_keep(entry, data, kind)
                    self.media_held(viewer, data, kind, wanted)
                    return
                if status == 416:
                    self.media_reply(416, [("Content-Range", headers.get("content-range", "bytes */*"))])
                    return
                got = re.match(r"bytes (\d+)-(\d+)/(\d+|\*)", headers.get("content-range", ""))
                if status != 206 or not got:
                    self.media_reply(status if isinstance(status, int) and 400 <= status < 600 else 502)
                    return
                at, last, total = int(got[1]), int(got[2]), got[3]
                total = None if total == "*" else int(total)
                if start is None:
                    start, end = at, last
                if at != start or last < at or len(data) != last + 1 - at:
                    self.media_reply(502, body=b"Mismatched range answer")
                    return
                if end is None or (total is not None and end >= total):
                    end = total - 1 if total is not None else None
                # one response for the whole range the browser asked for (all of it without a
                # Range), its bytes fetched a chunk at a time
                out = [("Content-Type", kind), ("Accept-Ranges", "bytes")]
                if wanted is not None:
                    out.append(("Content-Range", f"bytes {start}-{'' if end is None else end}/{total if total is not None else '*'}"))
                if end is not None:
                    out.append(("Content-Length", str(end + 1 - start)))
                self.media_reply(206 if wanted is not None else 200, out, data)
                viewer.stats["media_resource_bytes"] += len(data)
                at = last + 1
                while end is None or at <= end:
                    if not viewer.resource_live(handle, session, entry):
                        viewer.stats["media_resource_cancels"] += 1
                        return
                    stop = at + MEDIA_CHUNK - 1 if end is None else min(end, at + MEDIA_CHUNK - 1)
                    reply = viewer.media_fetch(entry, f"bytes={at}-{stop}")
                    viewer.stats["media_resource_chunks"] += 1
                    ok = reply.get("ok") or {}
                    got = re.match(r"bytes (\d+)-(\d+)/", dict((k.lower(), v) for k, v in ok.get("headers") or []).get("content-range", ""))
                    data = base64.b64decode(ok.get("bodyBase64") or "")
                    if ok.get("status") != 206 or not got or int(got[1]) != at or len(data) != int(got[2]) + 1 - at or not data:
                        return  # the connection closes short; the browser asks again from where it got to
                    self.wfile.write(data)
                    viewer.stats["media_resource_bytes"] += len(data)
                    at += len(data)

            def websocket(self):
                """GET /api/ws (DESIGN "WebSocket transport"): check the handshake, then answer
                the browser's calls until the socket ends. Polls wait in their own thread, one at a
                time; every other call is answered here, so an answer that cannot be written stops
                this thread reading."""
                stats = service.viewer.stats
                query = parse_qs(urlsplit(self.path).query)
                key = self.headers.get("Sec-WebSocket-Key", "")
                if self.headers.get("Host") != urlsplit(service.origin).netloc:
                    self.reply(403, {"error": "Unexpected Host"})
                    return
                if self.headers.get("Origin") != service.origin:
                    self.reply(403, {"error": "Unexpected Origin"})
                    return
                if not hmac.compare_digest(query.get("token", [""])[0], service.token):
                    self.reply(403, {"error": "Invalid viewer token"})
                    return
                if (self.headers.get("Upgrade", "").lower() != "websocket" or
                        "upgrade" not in self.headers.get("Connection", "").lower().replace(" ", "").split(",") or
                        self.headers.get("Sec-WebSocket-Version") != "13" or not key):
                    self.reply(400, {"error": "Expected a WebSocket handshake"})
                    return
                accept = base64.b64encode(hashlib.sha1((key + WS_GUID).encode()).digest()).decode()
                handshake = (f"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
                             f"Sec-WebSocket-Accept: {accept}\r\n\r\n").encode()
                self.close_connection = True
                self.connection.settimeout(WS_TIMEOUT)
                # (answers are small frames written back to back by two threads: without this,
                # Nagle's algorithm holds the second one for the browser's delayed ACK, ~40 ms)
                self.connection.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
                sending = threading.Lock()
                # set once an answer could not be written: nothing more is read or answered
                ended = threading.Event()
                stats["ws_connections"] += 1
                stats["ws_bytes_in"] += len(self.raw_requestline) + 2 + sum(len(k) + len(v) + 4 for k, v in self.headers.items())

                def send(opcode, payload):
                    n = len(payload)
                    head = bytes([0x80 | opcode]) + (bytes([n]) if n < 126 else struct.pack("!BH", 126, n) if n < 65536 else struct.pack("!BQ", 127, n))
                    with sending:
                        try:
                            self.connection.sendall(head + payload)
                        except OSError:
                            # (a timed-out write may have sent part of the frame: end the socket)
                            ended.set()
                            try:
                                self.connection.shutdown(socket.SHUT_RDWR)
                            except OSError:
                                pass
                            raise
                        stats["ws_bytes_out"] += len(head) + n

                def answer(ident, op, body):
                    try:
                        if not isinstance(body, dict):
                            raise TypeError("request must be an object")
                        out = {"id": ident, "ok": service.call(op, body)}
                    except (NotFound, ValueError, KeyError, TypeError) as e:
                        out = {"id": ident, "error": str(e)}
                    try:
                        send(1, json.dumps(out).encode())
                    except OSError:
                        pass  # the reader sees the socket end

                polling = threading.Event()

                def poll(ident, body):
                    try:
                        answer(ident, "poll", body)
                    finally:
                        polling.clear()

                with service.sockets_lock:
                    service.sockets.add(self.connection)
                try:
                    with sending:
                        self.connection.sendall(handshake)
                        stats["ws_bytes_out"] += len(handshake)
                    while True:
                        opcode, payload = self.ws_read(send)
                        if ended.is_set():
                            break
                        stats["ws_calls"] += 1
                        try:
                            call = json.loads(payload)
                            ident, op, body = call["id"], call["op"], call.get("body", {})
                            if type(ident) is not int or not isinstance(op, str):
                                raise TypeError
                        except (ValueError, KeyError, TypeError):
                            raise SocketClosed(1007)
                        limit = MAX_GL if op == "webgl" else MAX_BODY
                        if len(payload) > limit:
                            raise SocketClosed(1009)
                        if op == "poll":
                            if polling.is_set():
                                send(1, json.dumps({"id": ident, "error": "A poll is already waiting"}).encode())
                                continue
                            polling.set()
                            threading.Thread(target=poll, args=(ident, body), daemon=True).start()
                        else:
                            answer(ident, op, body)
                except SocketClosed as e:
                    if e.code is not None:
                        try:
                            send(8, struct.pack("!H", e.code))
                        except OSError:
                            pass
                except (OSError, ValueError):
                    pass  # a lost or timed-out connection, or a file closed under the reader
                finally:
                    with service.sockets_lock:
                        service.sockets.discard(self.connection)

            def ws_read(self, send):
                """The browser's next text message, as bytes; answers pings, and raises SocketClosed
                for a close frame, a binary message, an unmasked or oversized frame, or a lost
                connection. Fragmented messages are joined, up to MAX_GL."""
                stats = service.viewer.stats
                message = None
                while True:
                    head = self.rfile.read(2)
                    if len(head) < 2:
                        raise SocketClosed()
                    fin, opcode, masked, n = head[0] & 0x80, head[0] & 0x0F, head[1] & 0x80, head[1] & 0x7F
                    if head[0] & 0x70 or not masked:
                        raise SocketClosed(1002)
                    extra = 0
                    if n == 126:
                        extra, n = 2, struct.unpack("!H", self.ws_exact(2))[0]
                    elif n == 127:
                        extra, n = 8, struct.unpack("!Q", self.ws_exact(8))[0]
                    if opcode >= 8 and (not fin or n > 125):
                        raise SocketClosed(1002)
                    if n + len(message or b"") > MAX_GL:
                        raise SocketClosed(1009)
                    mask = self.ws_exact(4)
                    data = self.ws_exact(n)
                    stats["ws_bytes_in"] += 2 + extra + 4 + n
                    if n:
                        data = (int.from_bytes(data, "big") ^ int.from_bytes((mask * (n // 4 + 1))[:n], "big")).to_bytes(n, "big")
                    if opcode == 8:
                        raise SocketClosed(1000)
                    if opcode == 9:
                        send(10, data)
                        continue
                    if opcode == 10:
                        continue
                    if opcode == 2 or (opcode == 1) == (message is not None) or opcode not in (0, 1):
                        raise SocketClosed(1003 if opcode == 2 else 1002)
                    message = (message or b"") + data
                    if fin:
                        return 1, message

            def ws_exact(self, n):
                data = self.rfile.read(n)
                if len(data) < n:
                    raise SocketClosed()
                return data

            def do_GET(self):
                path = urlsplit(self.path).path
                if path.startswith("/media/"):
                    # a media element's own load: no token header, the handle is the capability
                    if self.headers.get("Host") != urlsplit(service.origin).netloc:
                        self.reply(403, {"error": "Unexpected Host"})
                        return
                    try:
                        self.media(path[len("/media/"):])
                    except (BrokenPipeError, ConnectionResetError):
                        pass
                    return
                if path == "/api/ws":
                    self.websocket()
                    return
                if path.startswith("/api/"):
                    self.count_request()
                if not self.permitted(api=path.startswith("/api/")):
                    return
                try:
                    if path == "/":
                        self.reply(200, (ASSETS / "index.html").read_bytes(), "text/html; charset=utf-8")
                    elif path == "/media.js":
                        self.reply(200, (ASSETS / "media.js").read_bytes(), "text/javascript; charset=utf-8")
                    elif path == "/webgl.js":
                        self.reply(200, (ASSETS / "webgl.js").read_bytes(), "text/javascript; charset=utf-8")
                    elif path == "/adopted.js":
                        self.reply(200, (ASSETS / "adopted.js").read_bytes(), "text/javascript; charset=utf-8")
                    elif path == "/viewer.js":
                        self.reply(200, (ASSETS / "viewer.js").read_bytes(), "text/javascript; charset=utf-8")
                    elif path == "/frame":
                        self.reply(200, b"<!doctype html><html><head></head><body></body></html>", "text/html; charset=utf-8")
                    elif path == "/api/runtimes":
                        self.reply(200, [{"id": service.viewer.name, "url": service.viewer.page_url, "pid": service.viewer.runtime_pid}])
                    elif path == "/api/poll":
                        q = parse_qs(urlsplit(self.path).query)
                        self.reply(200, service.call("poll", {"session": q.get("session", [""])[0], "after": int(q.get("after", ["0"])[0])}))
                    else:
                        self.reply(404, {"error": "Not found"})
                except (ValueError, KeyError) as e:
                    self.reply(409, {"error": str(e)})
                except (BrokenPipeError, ConnectionResetError):
                    pass

            def do_POST(self):
                try:
                    length = int(self.headers.get("Content-Length", "0"))
                except ValueError:
                    length = 0
                self.count_request(max(length, 0))
                if not self.permitted(api=True):
                    return
                try:
                    limit = MAX_GL if urlsplit(self.path).path == "/api/webgl" else MAX_BODY
                    if not 0 < length <= limit:
                        self.reply(413, {"error": "Invalid request size"})
                        return
                    req = json.loads(self.rfile.read(length))
                    if not isinstance(req, dict):
                        raise ValueError("request must be an object")
                    path = urlsplit(self.path).path
                    if not path.startswith("/api/") or path == "/api/poll":
                        raise NotFound("Not found")
                    self.reply(200, service.call(path[len("/api/"):], req))
                except NotFound as e:
                    self.reply(404, {"error": str(e)})
                except (ValueError, KeyError, TypeError) as e:
                    self.reply(409, {"error": str(e)})
                except (BrokenPipeError, ConnectionResetError):
                    pass

        return Handler

    def start(self):
        self.thread.start()
        if threading.current_thread() is threading.main_thread():
            self.old_sigint = signal.signal(signal.SIGINT, lambda *_: self.viewer.stop())
            self.old_sigterm = signal.signal(signal.SIGTERM, lambda *_: self.viewer.stop())
        return self

    def drop_sockets(self):
        """End every open WebSocket's connection without a close frame, as a lost network does."""
        with self.sockets_lock:
            for conn in list(self.sockets):
                try:
                    conn.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass

    def close(self):
        self.viewer.stop()
        self.drop_sockets()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()
        if self.old_sigint is not None:
            signal.signal(signal.SIGINT, self.old_sigint)
            signal.signal(signal.SIGTERM, self.old_sigterm)
