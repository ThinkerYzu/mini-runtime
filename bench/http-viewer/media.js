// Shared browser media executor: native decoding, MSE and playback events.
globalThis.createMrtMedia = function(W, getNodes, getIds, emit, bounded = false) {
let active = true, pendingBytes = 0, pendingOps = 0;
const queue = {push: m => { if (active) emit(m); }};
const elements = new Map(), restored = new Set(), roots = new Set(), urls = new Set();
const num = v => (typeof v === "number" && !Number.isFinite(v) ? String(v) : v);
const dec = v => (typeof v === "string" ? Number(v) : v);
const ranges = r => { const out = []; for (let i = 0; i < r.length; i++) out.push([num(r.start(i)), num(r.end(i))]); return out; };
const lastSeq = new WeakMap();
const state = el => ({ readyState: el.readyState, networkState: el.networkState, currentTime: num(el.currentTime), duration: num(el.duration),
  paused: el.paused, ended: el.ended, seeking: el.seeking, volume: el.volume, muted: el.muted, playbackRate: el.playbackRate,
  defaultPlaybackRate: el.defaultPlaybackRate, buffered: ranges(el.buffered), seekable: ranges(el.seekable), played: ranges(el.played),
  videoWidth: el.videoWidth || 0, videoHeight: el.videoHeight || 0, error: el.error ? [el.error.code, el.error.message] : null });
const TYPES = ["loadstart", "progress", "suspend", "abort", "error", "emptied", "stalled", "loadedmetadata", "loadeddata", "canplay",
  "canplaythrough", "playing", "waiting", "seeking", "seeked", "ended", "durationchange", "timeupdate", "play", "pause", "ratechange",
  "resize", "volumechange"];
// (an event is taken once: it reaches the window's listener, and the element's own when it has one)
const taken = new WeakSet();
const listener = e => {
  const el = e.target;
  if (!active || !e.isTrusted || !el || el.nodeType !== 1 || (el.localName !== "video" && el.localName !== "audio") || taken.has(e)) return;
  taken.add(e);
  if ([...sources.values()].some(rec => rec.el === el && rec.replaying)) return;
  const ids = getIds();
  if (!ids || !ids.has(el)) return;
  queue.push({ type: "media", k: "el", el: ids.get(el), ev: e.type, s: state(el), seq: lastSeq.get(el) || 0 });
};
// at the window, and at each open shadow root of the copy (media events do not leave a shadow tree)
const watchRoot = root => { roots.add(root); for (const t of TYPES) root.addEventListener(t, listener, true); };
// and at each element given a source: one out of the document (the page removed it) still tells the
// runtime that Firefox paused it
const watched = new WeakSet();
const watchElement = el => { if (!watched.has(el)) { watched.add(el); watchRoot(el); } };
watchRoot(W);
for (const node of getNodes().values()) if (node.nodeType === 11) watchRoot(node);
const sources = new Map();
const ownURL = new WeakMap();
// the bytes as the page's own Uint8Array, decoded in its realm (a fetch of a data: URL would be
// refused by a page's CSP, as X's connect-src refuses it)
const bytesOf = b64 => {
  const U8 = (W.wrappedJSObject || W).Uint8Array;
  if (U8.fromBase64) return Promise.resolve(U8.fromBase64(b64));
  const bin = W.atob(b64), bytes = new U8(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return Promise.resolve(bytes);
};
const report = (rec, op, error) => {
  pendingBytes -= op._transportBytes || 0; op._transportBytes = 0;
  const sb = rec.sbs.get(op.sb);
  let buffered = [];
  try { buffered = sb ? ranges(sb.buffered) : []; } catch (e) {}
  if (error && rec.replaying) { lose(rec, error); return; }
  queue.push({ type: "media", k: "sb", sb: op.sb, n: op.n, buffered, dur: num(rec.ms.duration), error: error || null });
};
const fail = (rec, op, e) => { rec.busy = false; rec.current = null; report(rec, op, e.name + ": " + e.message); };
function release(rec) {
  rec.lost = true; W.clearTimeout(rec.timer);
  for (const op of rec.queue) pendingBytes -= op._transportBytes || 0;
  pendingOps -= rec.queue.length;
  if (rec.current) pendingBytes -= rec.current._transportBytes || 0;
  rec.queue = []; rec.current = null; sources.delete(rec.id);
  const url = rec.el && ownURL.get(rec.el);
  if (url) { W.URL.revokeObjectURL(url); urls.delete(url); ownURL.delete(rec.el); }
}
function lose(rec, message) {
  if (rec.lost || !active) return;
  rec.lost = true; rec.replaying = false;
  W.clearTimeout(rec.timer);
  for (const op of rec.queue) pendingBytes -= op._transportBytes || 0;
  pendingOps -= rec.queue.length; rec.queue = [];
  if (rec.current) pendingBytes -= rec.current._transportBytes || 0;
  rec.current = null; rec.busy = false;
  queue.push({type:"media", k:"loss", ms:rec.id, message});
}
function pump(rec) {
  while (active && !rec.lost && rec.open && !rec.busy && rec.queue.length) {
    const op = rec.queue.shift(); pendingOps--;
    try { step(rec, op); } catch (e) {
      if (rec.replaying) lose(rec, e.name + ": " + e.message);
      else if (op.o === "append" || op.o === "remove") fail(rec, op, e);
    }
  }
}
function step(rec, op) {
  const sb = rec.sbs.get(op.sb);
  switch (op.o) {
    case "resume": {
      const el = rec.el;
      rec.busy = true;
      const finish = () => {
        if (!active || rec.lost) return;
        try {
          for (const name of ["volume", "muted", "playbackRate", "defaultPlaybackRate"]) el[name] = op[name];
          if (op.currentTime > 0) el.currentTime = op.currentTime;
          rec.replaying = false; rec.busy = false; W.clearTimeout(rec.timer);
          queue.push({type:"media", k:"el", el:op.el, ev:"timeupdate", s:state(el), seq:op.seq, resumed:true});
          if (!op.paused) el.play().catch(() => {
            if (active) queue.push({type:"media", k:"el", el:op.el, ev:"pause", s:state(el), seq:op.seq, resumed:true});
          });
          pump(rec);
        } catch (e) { lose(rec, e.name + ": " + e.message); }
      };
      if (el.readyState >= 1 || !rec.sbs.size) finish();
      else el.addEventListener("loadedmetadata", finish, {once:true});
      return;
    }
    case "addsb": {
      const b = rec.ms.addSourceBuffer(op.type);
      rec.sbs.set(op.sb, b);
      b.addEventListener("error", () => { if (rec.current && rec.current.sb === op.sb) rec.current.error = "error"; });
      b.addEventListener("updateend", () => {
        const cur = rec.current;
        if (!cur || cur.sb !== op.sb) return;
        rec.current = null;
        rec.busy = false;
        report(rec, cur, cur.error);
        pump(rec);
      });
      return;
    }
    case "rmsb": if (sb) { rec.ms.removeSourceBuffer(sb); rec.sbs.delete(op.sb); } return;
    case "sbset":
      if (!["mode", "timestampOffset", "appendWindowStart", "appendWindowEnd"].includes(op.name)) throw new TypeError("Unknown SourceBuffer attribute");
      if (sb) sb[op.name] = op.name === "mode" ? op.value : dec(op.value); return;
    case "changetype": if (sb) sb.changeType(op.type); return;
    case "abort": if (sb && rec.ms.readyState === "open") sb.abort(); return;
    case "duration": rec.ms.duration = dec(op.value); return;
    case "eos": if (op.error) rec.ms.endOfStream(op.error); else rec.ms.endOfStream(); return;
    case "append":
      rec.busy = true;
      rec.current = op;
      bytesOf(op.b64).then(buf => { if (rec.current === op) sb.appendBuffer(buf); })
        .catch(e => { if (rec.current === op) { fail(rec, op, e); pump(rec); } });
      return;
    case "remove":
      rec.busy = true;
      rec.current = op;
      sb.remove(dec(op.start), dec(op.end));
      return;
  }
}
const apply = op => {
  if (!active) return;
  const el = getNodes().get(op.el) || null;
  if (el) lastSeq.set(el, op.seq);
  switch (op.o) {
    case "restore": {
      if (!el || restored.has(op.el)) return;
      restored.add(op.el); watchElement(el);
      for (const name of ["volume", "muted", "playbackRate", "defaultPlaybackRate"]) el[name] = op[name];
      el.pause();
      const seek = () => { if (active && op.currentTime > 0) try { el.currentTime = op.currentTime; } catch (_) {} };
      if (el.readyState >= 1) seek(); else el.addEventListener("loadedmetadata", seek, {once: true});
      return;
    }
    case "attach": {
      for (const rec of sources.values()) if (rec.el === el) release(rec);
      const ms = new W.MediaSource();
      const rec = { id:op.ms, ms, el, open: false, queue: [], busy: false, current: null, sbs: new Map(), replaying:!!op.replay };
      if (rec.replaying) rec.timer = W.setTimeout(() => lose(rec, "Media reconstruction timed out"), 10000);
      sources.set(op.ms, rec);
      ms.addEventListener("sourceopen", () => {
        if (!active || rec.lost) return;
        rec.open = true;
        queue.push({ type: "media", k: "ms", ms: op.ms, ev: "sourceopen" });
        pump(rec);
      });
      if (el) { watchElement(el); const url = W.URL.createObjectURL(ms); urls.add(url); ownURL.set(el, url); el.src = url; }
      return;
    }
    case "detach": {
      const rec = sources.get(op.ms);
      // Remove the owned URL only if the page has not already changed it.
      if (el && el.getAttribute("src") === ownURL.get(el)) { el.removeAttribute("src"); el.load(); }
      if (rec) release(rec);
      return;
    }
    case "play":
      if (!el) { queue.push({type:"media", k:"play", p:op.p, ok:false, name:"AbortError", message:"Media element unavailable"}); return; }
      if (el) watchElement(el);
      if (el) el.play().then(() => queue.push({ type: "media", k: "play", p: op.p, ok: true }),
                             e => queue.push({ type: "media", k: "play", p: op.p, ok: false, name: e.name, message: e.message }));
      return;
    case "pause": if (el) el.pause(); return;
    case "load": if (el) el.load(); return;
    case "set":
      if (!["currentTime", "volume", "muted", "playbackRate", "defaultPlaybackRate"].includes(op.name)) throw new TypeError("Unknown media attribute");
      if (el) el[op.name] = op.name === "muted" ? op.value : dec(op.value); return;
    case "abort": {
      const rec = sources.get(op.ms);
      if (!rec) return;
      if (op.replay) { rec.queue.push(op); pendingOps++; pump(rec); return; }
      rec.queue = rec.queue.filter(o => {
        if (o.sb !== op.sb || (o.o !== "append" && o.o !== "remove")) return true;
        pendingBytes -= o._transportBytes || 0; pendingOps--; return false;
      });
      if (rec.current && rec.current.sb === op.sb) { pendingBytes -= rec.current._transportBytes || 0; rec.current._transportBytes = 0; rec.current = null; rec.busy = false; }
      const sb = rec.sbs.get(op.sb);
      try { if (sb && rec.ms.readyState === "open") sb.abort(); } catch (e) {}
      pump(rec);
      return;
    }
  }
  const rec = sources.get(op.ms);
  if (!rec || rec.lost) {
    if (op.o === "append" || op.o === "remove") queue.push({type:"media", k:"sb", sb:op.sb, n:op.n, buffered:[], error:"InvalidStateError: MediaSource unavailable"});
    return;
  }
  const bytes = op.b64 ? op.b64.length * 3 / 4 : 0;
  if (bounded && (bytes > 4 * 1024 * 1024 ||
      pendingBytes + bytes > (op.replay ? 16 : 8) * 1024 * 1024 || pendingOps >= (op.replay ? 10000 : 1024))) {
    if (rec.replaying) lose(rec, "QuotaExceededError: media replay queue full");
    else if (op.o === "append" || op.o === "remove") report(rec, op, "QuotaExceededError: media transport queue full");
    return;
  }
  op._transportBytes = bytes; pendingBytes += bytes; pendingOps++;
  rec.queue.push(op);
  pump(rec);
};

return {
  elements, watchRoot,
  track(id, el) { elements.set(id, el); watchElement(el); },
  capabilities(type) { return [!!W.MediaSource?.isTypeSupported(type), W.document.createElement("video").canPlayType(type)]; },
  apply(op) {
    try { apply(op); }
    catch (e) {
      if (op.o === "play") queue.push({type:"media", k:"play", p:op.p, ok:false, name:e.name, message:e.message});
      else if (op.o === "append" || op.o === "remove") queue.push({type:"media", k:"sb", sb:op.sb, n:op.n, buffered:[], error:e.name+": "+e.message});
    }
  },
  dispose() {
    active = false;
    for (const root of roots) for (const t of TYPES) root.removeEventListener(t, listener, true);
    for (const el of elements.values()) { el.pause(); if (ownURL.has(el)) { el.removeAttribute("src"); el.load(); } }
    for (const rec of sources.values()) { W.clearTimeout(rec.timer); rec.queue = []; rec.current = null; }
    for (const url of urls) W.URL.revokeObjectURL(url);
    pendingBytes = pendingOps = 0;
    sources.clear(); elements.clear(); roots.clear(); urls.clear();
  }
};
};
