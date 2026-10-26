"use strict";
// Only this service's code runs in the outer page. The mirrored document lives
// in a frame with scripts, forms, popups and navigation privileges disabled.
const token = new URL(location.href).searchParams.get("token") || "";
const frame = document.getElementById("page"), status = document.getElementById("status");
const select = document.getElementById("runtime"), connect = document.getElementById("attach"), disconnect = document.getElementById("detach");
const back = document.getElementById("back"), forward = document.getElementById("forward");
let historyState = null, historyPending = null;
function connectedStatus() {
  if (session && inputPending && !restoring && !navigating)
    status.title = status.textContent = "Processing input…";
  else if (session && ready && !navigating)
    status.title = status.textContent = "Connected · " + pageURL + (failures ? " · some CSS rules unavailable" : "") + (notice ? " · " + notice : "");
}
function historyControls() {
  const enabled = !!session && ready && !navigating && historyPending === null && historyState !== null;
  back.disabled = !enabled || historyState.index <= 0;
  forward.disabled = !enabled || historyState.index + 1 >= historyState.count;
}
function discardInput() {
  if (events.some(e => e.type === "viewer-history" && e.task === historyPending)) historyPending = null;
  events = [];
}
function historyCommand(delta) {
  if ((delta === -1 ? back : forward).disabled) return;
  historyPending = ++task;
  events.push({type: "viewer-history", delta, task: historyPending});
  historyControls();
  send();
}
back.onclick = () => historyCommand(-1);
forward.onclick = () => historyCommand(1);
let session = null, after = 0, ready = false, events = [], posting = false, task = 0;
const INPUT_BATCH = 256;
let syncedViewport = null, inputPending = false;
let nodes = new Map(), ids = new WeakMap(), rules = new Map(), watched = new WeakSet();
let scrollSeq = 0, scrollEcho = null, pageURL = "", failures = 0;
let restoring = null, restoreFrame = false;
// a navigation of the runtime's page: the URL loading (no input meanwhile), the last notice (a failed
// or refused navigation) and its number
let navigating = null, notice = "", noticeSeq = 0;
const NAVIGATED = /^Document replaced by a navigation/;
const resourceDone = new WeakSet(), resourceWatched = new WeakSet();
const viewport = () => [Math.max(1, frame.clientWidth), Math.max(1, frame.clientHeight)];
// The service's API (DESIGN "WebSocket transport"): every call but the runtime list goes over one
// WebSocket. ?transport=auto (the default) falls back to HTTP long polling, until the page is reloaded,
// when the socket cannot open; ws never polls; poll never opens a socket.
const transport = new URL(location.href).searchParams.get("transport") || "auto";
const MAX_CALLS = 16, MAX_UNSENT = 16 * 1024 * 1024;
let polling = transport === "poll", socketOpen = null, calls = new Map(), callId = 0;
function openSocket() {
  if (socketOpen) return socketOpen;
  return socketOpen = new Promise((resolve, reject) => {
    const ws = new WebSocket("ws://" + location.host + "/api/ws?token=" + encodeURIComponent(token));
    let opened = false;
    ws.onopen = () => { opened = true; resolve(ws); };
    ws.onmessage = e => {
      const answer = JSON.parse(e.data), call = calls.get(answer.id);
      if (!call) return;
      calls.delete(answer.id);
      if ("error" in answer) call.reject(new Error(answer.error)); else call.resolve(answer.ok);
    };
    // (an error is always followed by close)
    ws.onclose = () => {
      socketOpen = null;
      const lost = calls; calls = new Map();
      for (const call of lost.values()) call.reject(new Error("Viewer connection lost"));
      if (!opened) reject(new Error("WebSocket unavailable"));
    };
  });
}
async function http(path, body) {
  if (path === "poll") { path = "poll?session=" + encodeURIComponent(body.session) + "&after=" + body.after; body = undefined; }
  const response = await fetch("/api/" + path, { headers: {"X-Viewer-Token": token, "Content-Type": "application/json"},
    ...(body === undefined ? {} : {method: "POST", body: JSON.stringify(body)}) });
  const result = await response.json();
  if (!response.ok) throw new Error(result.error || response.status);
  return result;
}
const api = async (path, body) => {
  if (polling || path === "runtimes") return http(path, body);
  let ws;
  try { ws = await openSocket(); }
  catch (e) {
    if (transport === "ws") throw e;
    polling = true;
    return http(path, body);
  }
  if (calls.size >= MAX_CALLS || ws.bufferedAmount > MAX_UNSENT) throw new Error("Viewer connection congested");
  return new Promise((resolve, reject) => {
    const id = ++callId;
    calls.set(id, {resolve, reject});
    ws.send(JSON.stringify({id, op: path, body: body === undefined ? {} : body}));
  });
};
let media, mediaEpoch = null, mediaEvents = [], mediaPosting = false;
function newMedia(epoch) {
  media?.dispose(); mediaEvents = []; mediaEpoch = epoch;
  media = createMrtMedia(frame.contentWindow, () => nodes, () => ids, event => {
    if (event.k === "el" && ["timeupdate", "progress"].includes(event.ev)) {
      const at = mediaEvents.findIndex(e => e.k === "el" && e.el === event.el && e.ev === event.ev);
      if (at >= 0) { mediaEvents[at] = event; return; }
    }
    if (mediaEvents.length >= 1024) { disconnected("Playback event queue full. Click Connect to reattach."); return; }
    mediaEvents.push(event);
  }, true);
}
async function sendMedia() {
  if (!session || mediaPosting || !mediaEvents.length) return;
  const current = session, epoch = mediaEpoch;
  mediaPosting = true;
  try { await api("media-events", {session: current, epoch, events: mediaEvents.splice(0, 128)}); }
  catch (e) { failed(e, current); }
  finally { mediaPosting = false; }
}
let gpu;
function newGPU() {
  gpu?.dispose();
  gpu = createMrtWebGL(frame.contentWindow, (id, canvas) => {
    const old = nodes.get(id);
    if (!old) return;
    for (const name of old.getAttributeNames()) canvas.setAttribute(name, old.getAttribute(name));
    old.replaceWith(canvas); nodes.set(id, canvas); ids.set(canvas, id);
  });
}
// A request failed: after a navigation the session is the old document's, so attach again at once
// (the runtime has the new one); any other failure disconnects
function failed(e, current) {
  if (session !== current) return;
  if (NAVIGATED.test(e.message)) { session = null; ready = false; events = []; attach(); return; }
  disconnected(e.message + ". Click Connect to reattach.");
}
function disconnected(message) {
  gpu?.dispose(); media?.dispose(); mediaEvents = [];
  session = null; ready = false; inputPending = false; events = []; restoring = null; scrollEcho = null;
  disconnect.disabled = true; connect.disabled = false; frame.style.pointerEvents = "none";
  historyState = null; historyPending = null; historyControls();
  status.textContent = message;
}
// Discrete input (keys, buttons, clicks, focus, form changes) is sent as soon as the task that
// captured it ends, and again as soon as the call in flight is answered (task 61692): waiting for the
// 30 ms timer or the next poll answer was most of a keydown's latency. Pointer movement, crossing,
// wheel and scroll records wait for those, so a stream of moves does not become a call per event.
const LAZY = new Set("pointermove pointerover pointerenter pointerout pointerleave gotpointercapture lostpointercapture mousemove mouseover mouseenter mouseout mouseleave wheel scroll scrollend".split(" "));
let urgent = false, soon = false;
function sendSoon() {
  if (soon) return;
  soon = true;
  setTimeout(() => { soon = false; send(); }, 0);
}
async function send() {
  if (!session || posting) return;
  posting = true;
  const current = session, size = viewport();
  const changed = !syncedViewport || size[0] !== syncedViewport[0] || size[1] !== syncedViewport[1];
  // Input captured in the old geometry must not be sent with a new viewport.
  const batch = changed || navigating ? [] : events.splice(0, INPUT_BATCH);
  if (!events.length) urgent = false;
  if (changed) { discardInput(); ready = false; historyControls(); frame.style.pointerEvents = "none"; }
  try {
    const result = await api("input", {session: current, ack: restoring ? restoring.before : after, events: batch,
      viewport: size, visible: !document.hidden});
    if (session === current) {
      const now = viewport();
      inputPending = !!result.inputPending || events.length >= INPUT_BATCH;
      ready = result.ready && !inputPending && !restoring && now[0] === size[0] && now[1] === size[1];
      if (ready) syncedViewport = size;
      frame.style.pointerEvents = ready ? "auto" : "none";
      connectedStatus();
      historyControls();
    }
  } catch (e) { failed(e, current); }
  finally {
    posting = false;
    // (while the service's queue is full, the timers retry instead)
    if (urgent && events.length && session && !inputPending) sendSoon();
  }
}
const XHTML = "http://www.w3.org/1999/xhtml";
const childrenOf = n => n.localName === "template" ? n.content : n;
// (a frame document's own base URL, from its spec; the page's is pageURL)
const docBases = new WeakMap();
function resourceBase(head) {
  let base = Array.from(head.children).find(n => n.localName === "base" && n.id === "__mrt_base");
  if (!base) { base = head.ownerDocument.createElement("base"); base.id = "__mrt_base"; head.prepend(base); }
  base.href = docBases.get(head.ownerDocument) ?? pageURL;
  return base;
}
function adoptedStyle(doc, text) {
  let el = doc.getElementById("__mrt_adopted");
  if (!el && doc.head) { el = doc.createElement("style"); el.id = "__mrt_adopted"; doc.head.append(el); }
  if (el) el.textContent = text;
}
// Frames (DESIGN "Frame documents in the HTTP viewer"): a frame element of the copy is a real
// <iframe> or <frame> with no src. Its document inherits the copy's sandbox and CSP (no scripts, no
// navigation) and the service's origin, so this page builds it. A frame document's spec is built in
// a document of its own until its element is in the copy (attachFrames), then its nodes move into the
// frame's document; if the browser replaces that document, they move again.
let frameStates = new WeakMap(), docStates = new Map(), pendingFrames = new Set();
function frameDocument(el, spec) {
  const old = frameStates.get(el);
  if (old) { pendingFrames.delete(old); frameStates.delete(el); }
  if (spec === null) { el.contentDocument?.replaceChildren(); return; }
  let state;
  if (typeof spec === "number") {
    state = docStates.get(spec);
    if (!state) throw new Error("Missing mirror document " + spec);
    if (frameStates.get(state.el) === state) frameStates.delete(state.el);
    state.el = el;
  } else {
    const holder = frame.contentDocument.implementation.createHTMLDocument("");
    holder.replaceChildren();
    docBases.set(holder, spec.u);
    nodes.set(spec.i, holder); ids.set(holder, spec.i);
    state = {el, id: spec.i, doc: holder, url: spec.u, adopted: spec.adopted || ""};
    docStates.set(spec.i, state);
    holder.append(...spec.c.map(c => build(c, holder)));
  }
  frameStates.set(el, state);
  pendingFrames.add(state);
}
function moveFrameDocument(state, doc) {
  // (a document's replaceChildren() refuses a second root element before it removes the first)
  const moved = Array.from(state.doc.childNodes);
  doc.replaceChildren();
  doc.append(...moved);
  docBases.set(doc, state.url);
  nodes.set(state.id, doc); ids.set(doc, state.id);
  state.doc = doc;
  if (doc.head) resourceBase(doc.head);
  adoptedStyle(doc, state.adopted);
  if (doc.defaultView) listen(doc.defaultView);
}
// (again after a move: a frame inside a moved document has its own document only once that one is
// in its frame)
function attachFrames() {
  for (let moved = true; moved; ) {
    moved = false;
    for (const state of pendingFrames) {
      const doc = state.el.isConnected ? state.el.contentDocument : null;
      if (!doc) continue;
      if (doc !== state.doc) moveFrameDocument(state, doc);
      pendingFrames.delete(state);
      moved = true;
      watchFrameLoad(state);
    }
  }
}
const frameLoads = new WeakSet();
function watchFrameLoad(state) {
  const el = state.el;
  if (frameLoads.has(el)) return;
  frameLoads.add(el);
  el.addEventListener("load", () => {
    const current = frameStates.get(el), doc = el.contentDocument;
    if (current && doc && doc !== current.doc) moveFrameDocument(current, doc);
  });
}
// Stylesheets are inserted by the mirror after the frame's own load event.
// Track their load/error explicitly; sheet presence alone misses @import loads.
function watchResource(node) {
  if (node.localName !== "link" || resourceWatched.has(node)) return;
  resourceWatched.add(node);
  for (const type of ["load", "error"]) node.addEventListener(type, () => resourceDone.add(node));
}
function pendingStyles(doc, win) {
  for (const link of doc.querySelectorAll('link[rel~="stylesheet"]')) {
    if (!link.disabled && (!link.media || win.matchMedia(link.media).matches) &&
        link.hasAttribute("href") && !resourceDone.has(link)) return true;
  }
  return false;
}
// The next restoration step: at the next animation frame, or after 100 ms when no frame comes (a
// requested frame was seen not to run in Firefox after the frame's document was replaced)
function nextRestoreStep() {
  restoreFrame = true;
  let done = false;
  const step = () => { if (!done) { done = true; restoreScroll(); } };
  requestAnimationFrame(step);
  setTimeout(step, 100);
}
function holdScroll(x, y) {
  const before = restoring ? restoring.before : after;
  restoring = {x, y, before, stable: 0, last: null};
  ready = false; discardInput(); historyControls(); frame.style.pointerEvents = "none";
  status.textContent = "Synchronizing…";
  if (!restoreFrame) nextRestoreStep();
}
function restoreScroll() {
  restoreFrame = false;
  const state = restoring;
  if (!state || !session) return;
  const doc = frame.contentDocument, win = frame.contentWindow;
  // Layout must include every DOM/CSSOM operation, before applying the offset.
  const root = doc.scrollingElement || doc.documentElement;
  const maxX = Math.max(0, (root?.scrollWidth || 0) - win.innerWidth);
  const maxY = Math.max(0, (root?.scrollHeight || 0) - win.innerHeight);
  const x = Math.min(state.x, maxX), y = Math.min(state.y, maxY);
  scrollEcho = [x, y];
  win.scrollTo({left: x, top: y, behavior: "instant"});
  const current = [win.scrollX, win.scrollY];
  const reached = current[0] === state.x && current[1] === state.y;
  const stylesPending = pendingStyles(doc, win);
  const settled = !stylesPending && !Array.from(doc.images).some(img => !img.complete) && doc.fonts?.status !== "loading";
  // Images/fonts can make an initially unreachable offset reachable. Two
  // stable frames drain native scroll events before accepting user input.
  state.stable = !stylesPending && (reached || settled) && state.last &&
    current[0] === state.last[0] && current[1] === state.last[1] ? state.stable + 1 : 0;
  state.last = current;
  if (state.stable >= 2) {
    restoring = null;
    send();
  } else nextRestoreStep();
}
function attrs(node, list) {
  watchResource(node);
  if (node.localName === "link" && list.some(([name, value]) =>
      (name.toLowerCase() === "href" || (name.toLowerCase() === "rel" && !node.sheet)) && node.getAttribute(name) !== value)) resourceDone.delete(node);
  // (a frame's src would load the page's URL in the copy: its document comes from the runtime)
  const frameEl = ["iframe", "frame"].includes(node.localName);
  const allowed = list.filter(([name]) => !name.toLowerCase().startsWith("on") && !["srcdoc", "sandbox", "http-equiv"].includes(name.toLowerCase()) &&
    !(frameEl && name.toLowerCase() === "src"));
  const want = new Set(allowed.map(([name]) => name));
  for (const name of node.getAttributeNames()) if (!want.has(name)) node.removeAttribute(name);
  for (const [name, value] of allowed) {
    if (name === "src" && value.startsWith("blob:") && ["video", "audio"].includes(node.localName)) continue;
    if (["href", "src", "action", "formaction", "xlink:href"].includes(name.toLowerCase()) && /^\s*(javascript|vbscript):/i.test(value)) continue;
    try { if (node.getAttribute(name) !== value) node.setAttribute(name, value); } catch (_) {}
  }
  if (node.localName === "script") node.setAttribute("type", "application/x-mrt-inert");
}
function build(spec, doc = frame.contentDocument) {
  if (typeof spec === "number") {
    if (!nodes.has(spec)) throw new Error("Missing mirror node " + spec);
    return nodes.get(spec);
  }
  let node;
  if (spec.t !== undefined) node = doc.createTextNode(spec.t);
  else if (spec.m !== undefined) node = doc.createComment(spec.m);
  else {
    // Plugins cannot become another bridge here; frames show their runtime documents (frameDocument).
    const name = ["object", "embed", "base"].includes(spec.n) ? "mrt-placeholder" : spec.n;
    node = (name === "canvas" && gpu?.canvases.get(spec.i)) ||
      (["video", "audio"].includes(name) && media?.elements.get(spec.i)) || doc.createElementNS(spec.s || XHTML, name);
    attrs(node, spec.a);
    if (name === "head" && node.namespaceURI === XHTML) resourceBase(node);
    childrenOf(node).append(...spec.c.map(c => build(c, doc)));
    if (spec.r) shadow(node, spec.r);
    if (spec.f !== undefined && ["iframe", "frame"].includes(name) && node.namespaceURI === XHTML) frameDocument(node, spec.f);
  }
  nodes.set(spec.i, node); ids.set(node, spec.i);
  if (node.localName === "video" || node.localName === "audio") media?.track(spec.i, node);
  return node;
}
function shadow(host, spec) {
  const root = host.shadowRoot || host.attachShadow({mode: "open"});
  nodes.set(spec.i, root); ids.set(root, spec.i);
  root.replaceChildren(...spec.c.map(c => build(c, host.ownerDocument)));
  mirrorAdoptedSheets(root, spec.adopted);
  watch(root); media?.watchRoot(root);
}
function setRules(node, list) {
  if (!node.sheet) return;
  try {
    while (node.sheet.cssRules.length) node.sheet.deleteRule(node.sheet.cssRules.length - 1);
    for (const rule of list) node.sheet.insertRule(rule, node.sheet.cssRules.length);
  } catch (_) { failures++; }
}
function syncChildren(node, wanted) {
  if (node.localName === "head" && node.namespaceURI === XHTML) wanted = [resourceBase(node), ...wanted];
  const parent = childrenOf(node), keep = new Set(wanted);
  for (const child of Array.from(parent.childNodes)) if (!keep.has(child)) child.remove();
  let cursor = parent.firstChild;
  for (const child of wanted) {
    if (child === cursor) cursor = cursor.nextSibling;
    else parent.insertBefore(child, cursor);
  }
}
// Mirrored rich-text renders can replace the native editor's text nodes. Preserve
// logical anchor/focus positions within its editing host, including direction.
function editableSelection(doc) {
  const host = doc.activeElement;
  const sel = doc.defaultView.getSelection();
  if (!host?.isContentEditable || !sel?.anchorNode || !host.contains(sel.anchorNode) || !host.contains(sel.focusNode)) return null;
  const offset = (node, at) => {
    const range = doc.createRange();
    range.setStart(host, 0); range.setEnd(node, at);
    return range.toString().length;
  };
  const endpoint = (node, at) => {
    const path = [];
    for (let n = node; n !== host; n = n.parentNode) path.unshift(Array.prototype.indexOf.call(n.parentNode.childNodes, n));
    return {path, offset: at};
  };
  return {host, anchor: offset(sel.anchorNode, sel.anchorOffset), focus: offset(sel.focusNode, sel.focusOffset),
    a: endpoint(sel.anchorNode, sel.anchorOffset), f: endpoint(sel.focusNode, sel.focusOffset)};
}
function restoreEditableSelection(doc, saved) {
  if (!saved || !saved.host.isConnected || doc.activeElement !== saved.host) return;
  const point = (offset, endpoint) => {
    let candidate = saved.host;
    for (const index of endpoint.path) candidate = candidate?.childNodes[index];
    if (candidate && endpoint.offset <= (candidate.nodeType === 3 ? candidate.length : candidate.childNodes.length)) {
      const range = doc.createRange();range.setStart(saved.host, 0);range.setEnd(candidate, endpoint.offset);
      if (range.toString().length === offset) return [candidate, endpoint.offset];
    }
    const walker = doc.createTreeWalker(saved.host, 4);
    let node, last;
    while ((node = walker.nextNode())) {
      if (offset <= node.length) return [node, offset];
      offset -= node.length; last = node;
    }
    return last ? [last, last.length] : [saved.host, 0];
  };
  const a = point(saved.anchor, saved.a), f = point(saved.focus, saved.f), sel = doc.defaultView.getSelection();
  if (sel.anchorNode !== a[0] || sel.anchorOffset !== a[1] || sel.focusNode !== f[0] || sel.focusOffset !== f[1])
    sel.setBaseAndExtent(...a, ...f);
}
function apply(batch) {
  const doc = frame.contentDocument, win = frame.contentWindow;
  const caret = batch.snapshot ? null : editableSelection(doc);
  for (const op of batch.ops) if (op[0] === "u") pageURL = op[1];
  if (doc.head) resourceBase(doc.head);
  if (batch.snapshot) {
    historyState = null;
    holdScroll(0, 0);
    nodes = new Map([[0, doc]]); ids = new WeakMap([[doc, 0]]); rules = new Map();
    frameStates = new WeakMap(); docStates = new Map(); pendingFrames = new Set();
  }
  for (const [kind, id, arg, fourth] of batch.ops) {
    if (kind === "u") { pageURL = id; continue; }
    if (kind === "history") {
      historyState = {index: id, count: arg};
      if (fourth === historyPending) historyPending = null;
      historyControls();
      continue;
    }
    if (kind === "adopted") { adoptedStyle(doc, id); continue; }
    if (kind === "scroll") { scrollSeq = id; holdScroll(arg, fourth); continue; }
    if (kind === "m") { media?.apply(id); continue; }
    const node = nodes.get(id);
    if (!node) throw new Error("Unknown mirror node " + id);
    if (kind === "c") {
      const owner = node.nodeType === 9 ? node : node.ownerDocument;
      const wanted = arg.map(c => build(c, owner));
      if (node.nodeType === 9) {
        for (const child of Array.from(node.childNodes)) if (child.nodeType !== 10) child.remove();
        node.append(...wanted);
      } else syncChildren(node, wanted);
    } else if (kind === "f") frameDocument(node, arg);
    else if (kind === "fu" || kind === "fadopted") {
      const state = docStates.get(id);
      if (!state) throw new Error("Unknown mirror document " + id);
      if (kind === "fu") { state.url = arg; docBases.set(state.doc, arg); if (state.doc.head) resourceBase(state.doc.head); }
      else { state.adopted = arg; adoptedStyle(state.doc, arg); }
    } else if (kind === "a") attrs(node, arg);
    else if (kind === "d") { if (node.data !== arg) node.data = arg; }
    else if (kind === "r") shadow(node, arg);
    else if (kind === "adopted-root") mirrorAdoptedSheets(node, arg);
    else if (kind === "v") { if (node.value !== arg) node.value = arg; if (node.localName === "input") node.checked = fourth; }
    else if (kind === "s") { rules.set(id, arg); setRules(node, arg); }
  }
  if (doc.head) resourceBase(doc.head);
  attachFrames();
  // Moving a stylesheet reparses it; restore script-inserted rules after moves.
  for (const [id, list] of rules) if (nodes.get(id)?.isConnected) setRules(nodes.get(id), list);
  restoreEditableSelection(doc, caret);
  after = batch.seq;
}
const OWN = {
  UIEvent: [null, ["detail", "which"]],
  MouseEvent: ["UIEvent", ["screenX", "screenY", "clientX", "clientY", "pageX", "pageY", "button", "buttons", "ctrlKey", "shiftKey", "altKey", "metaKey"]],
  PointerEvent: ["MouseEvent", ["pointerId", "width", "height", "pressure", "pointerType", "isPrimary"]],
  WheelEvent: ["MouseEvent", ["deltaX", "deltaY", "deltaZ", "deltaMode"]],
  KeyboardEvent: ["UIEvent", ["key", "code", "keyCode", "charCode", "location", "repeat", "isComposing", "ctrlKey", "shiftKey", "altKey", "metaKey"]],
  FocusEvent: ["UIEvent", []], InputEvent: ["UIEvent", ["data", "inputType", "isComposing"]], SubmitEvent: [null, []]
};
const TYPES = "pointerover pointerenter pointerdown pointermove pointerup pointercancel pointerout pointerleave gotpointercapture lostpointercapture mouseover mouseenter mousemove mousedown mouseup mouseout mouseleave click auxclick dblclick contextmenu wheel keydown keypress keyup focus blur focusin focusout beforeinput input change submit reset select scroll scrollend".split(" ");
const idOf = n => n && ids.has(n) ? ids.get(n) : null;
function capture(e) {
  const path = e.composedPath(), target = idOf(path[0]);
  if (!e.isTrusted || target === null) return;
  // Keep the viewer on the service URL. Page scripts make navigation decisions in the runtime.
  if (e.type === "submit" || (e.type === "click" && path.some(n => ["a", "area"].includes(n.localName) || (n.localName === "button" && n.form)))) e.preventDefault();
  if (inputPending && ["keydown", "keypress", "beforeinput"].includes(e.type)) e.preventDefault();
  if (!ready || !session) return;
  if ((e.type === "scroll" || e.type === "scrollend") && target !== 0) return;
  // (the page's document, or a frame's: its own focus and selection; the page's scroll)
  const top = frame.contentDocument, win = frame.contentWindow;
  const doc = path[0].nodeType === 9 ? path[0] : path[0].ownerDocument, view = doc.defaultView || win;
  const iface = Object.prototype.toString.call(e).slice(8, -1), fields = {};
  for (let i = iface; OWN[i]; i = OWN[i][0]) for (const f of OWN[i][1]) fields[f] = e[f];
  const ev = {type: e.type, iface, target, active: idOf(doc.activeElement), task: ++task, bubbles: e.bubbles,
    cancelable: e.cancelable, composed: e.composed, fields, scroll: [win.scrollX, win.scrollY], scrollSeq, at: Date.now()};
  if (doc !== top) ev.outerActive = idOf(top.activeElement);
  if ((e.type === "scroll" || e.type === "scrollend") && scrollEcho) {
    if (scrollEcho[0] === win.scrollX && scrollEcho[1] === win.scrollY) return;
    scrollEcho = null;
  }
  if ("relatedTarget" in e) ev.related = idOf(e.relatedTarget);
  if (iface === "SubmitEvent") ev.submitter = idOf(e.submitter);
  const sel = view.getSelection();
  if (sel) ev.selection = [sel.type, sel.toString(), idOf(sel.anchorNode), sel.anchorOffset, idOf(sel.focusNode), sel.focusOffset];
  const caret = editableSelection(doc);
  if (caret) {
    ev.editSelection = {host: idOf(caret.host), anchor: caret.anchor, focus: caret.focus, a: caret.a, f: caret.f};
    if (e.type === "input") {
      let count = 0, size = 0;
      const snapshot = (node, depth = 0) => {
        if (depth > 64) throw new Error("Editable DOM too deep");
        if (++count > 2048) throw new Error("Editable DOM too large");
        if (node.nodeType === 3) { size += node.data.length; if (size > 65536) throw new Error("Editable text too large"); return {t: node.data}; }
        if (node.nodeType !== 1 || !/^(div|p|span|br|b|i|u|s|strong|em|a|code|pre|blockquote|ul|ol|li)$/.test(node.localName)) { const text = node.textContent || ""; size += text.length; if (size > 65536) throw new Error("Editable text too large"); return {t: text}; }
        return {n: node.localName, a: [...node.attributes].filter(a => !/^on/i.test(a.name)).map(a => [a.name, a.value]), c: [...node.childNodes].map(n => snapshot(n, depth + 1))};
      };
      try { ev.editable = [...caret.host.childNodes].map(n => snapshot(n)); }
      catch (err) { disconnected(err.message + ". Click Connect to resynchronize."); return; }
    }
  }
  if (["input", "change"].includes(e.type) && ["input", "textarea"].includes(path[0].localName)) ev.value = [path[0].value, !!path[0].checked];
  events.push(ev);
  if (!LAZY.has(e.type)) { urgent = true; sendSoon(); }
  // Pointer crossing can synchronously enter hundreds of nested nodes before
  // a send timer or HTTP response runs. Pause as soon as this batch is full.
  if (events.length >= INPUT_BATCH) {
    ready = false; inputPending = true;
    if (["keydown", "keypress", "beforeinput"].includes(e.type)) e.preventDefault();
    frame.style.pointerEvents = "none";
    connectedStatus(); historyControls();
  }
}
// a frame document's window: its events are captured as the page's
function listen(win) {
  if (watched.has(win)) return;
  watched.add(win);
  for (const type of TYPES) win.addEventListener(type, capture, true);
}
function watch(root) {
  if (watched.has(root)) return;
  watched.add(root);
  for (const type of TYPES) root.addEventListener(type, e => { if (!e.composed) capture(e); }, true);
}
async function attach() {
  if (session) await api("detach", {session}).catch(() => {});
  ready = false; inputPending = false; events = []; after = 0; syncedViewport = null; restoring = null; scrollEcho = null;
  navigating = null; notice = ""; noticeSeq = 0;
  historyState = null; historyPending = null; historyControls();
  let current = null;
  try {
    const result = await api("attach", {runtime: select.value, viewport: viewport()});
    newGPU(); newMedia(result.mediaEpoch);
    session = result.session; pageURL = result.url;
    connect.disabled = true; disconnect.disabled = false; frame.style.pointerEvents = "none";
    status.textContent = "Synchronizing…";
    current = session;
    while (session === current) {
      const reply = await api("poll", {session: current, after});
      if (session !== current) break;
      if (reply.stopped) { disconnected("Runtime stopped"); break; }
      if (reply.mediaEpoch !== mediaEpoch) newMedia(reply.mediaEpoch);
      navigating = reply.navigation ? reply.navigation.url : null;
      if (reply.notice && reply.notice.seq !== noticeSeq) { noticeSeq = reply.notice.seq; notice = reply.notice.text; }
      if (navigating) { ready = false; discardInput(); historyControls(); frame.style.pointerEvents = "none"; status.textContent = "Loading " + navigating + "…"; }
      if (reply.mediaType) await api("media-type", {session: current, id: reply.mediaType.id, reply: media.capabilities(reply.mediaType.type)});
      for (const batch of reply.batches) apply(batch);
      if (reply.webgl) {
        const executor = gpu;
        const result = await executor.execute(reply.webgl.request);
        if (session !== current) break;
        await api("webgl", {session: current, id: reply.webgl.id, reply: result});
      }
      const lost = gpu.takeLost();
      if (lost.length) await api("webgl-lost", {session: current, ids: lost});
      await send();
      if (session === current) { connectedStatus(); historyControls(); }
    }
  } catch (e) { failed(e, current); }
}
connect.onclick = attach;
disconnect.onclick = () => { const current = session; disconnected("Disconnected; the runtime is still running"); if (current) api("detach", {session: current}).catch(() => {}); };
window.addEventListener("pagehide", () => { if (session) fetch("/api/detach", {method: "POST", keepalive: true,
  headers: {"X-Viewer-Token": token, "Content-Type": "application/json"}, body: JSON.stringify({session})}).catch(() => {}); });
document.addEventListener("visibilitychange", send);
window.addEventListener("resize", () => { ready = false; discardInput(); historyControls(); frame.style.pointerEvents = "none"; send(); });
setInterval(() => { if (events.length) send(); }, 30);
setInterval(sendMedia, 100);
setInterval(send, 1000); // Keep the lease alive during asynchronous texture loads.
(async () => {
  if (!frame.contentDocument?.body) await new Promise(resolve => frame.addEventListener("load", resolve, {once: true}));
  const doc = frame.contentDocument, win = frame.contentWindow;
  for (const type of TYPES) win.addEventListener(type, capture, true);
  try {
    for (const rt of await api("runtimes")) { const option = document.createElement("option"); option.value = rt.id; option.textContent = rt.id; select.append(option); }
    const chosen = new URL(location.href).searchParams.get("runtime");
    if (chosen) select.value = chosen;
    await attach();
  } catch (e) { disconnected(e.message); }
})();
