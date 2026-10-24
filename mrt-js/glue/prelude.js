// The part of the DOM bindings written in JS, run once before the page's scripts: events, timers,
// `element.style`, `classList` and a few helpers. Firefox implements these in C++ (EventTarget and
// EventListenerManager, nsGlobalWindowInner's timeouts, nsDOMCSSAttributeDeclaration); here they hold
// JS values (listeners, callbacks), so keeping them in JS needs no native roots. What they allocate
// is part of the page's JS heap. See DESIGN "Stage 3: JS".
"use strict";
(function (global) {
  const define = (obj, name, value) =>
    Object.defineProperty(obj, name, { value, writable: true, configurable: true, enumerable: false });

  // What every window of the runtime shares (DESIGN, "Same-origin frames: the build plan", S3): the
  // state Gecko keeps on a node rather than in a window (listeners, an element's style declaration
  // and token lists, a script's started flag) and the event loop (the clock and the timers). The
  // page's window makes it; a frame's window gets it as __mrtShared (the C++ layer's CreateFrameWindow).
  const shared = global.__mrtShared || {};
  // A dedicated worker's global (DESIGN, "Workers: the build plan"): the C++ layer's worker scope
  // defines __mrtWorkerScope. What is [Exposed=Window] only is not defined there; the statements that
  // define it are guarded by `if (!isWorker)`.
  const isWorker = typeof __mrtWorkerScope === "object";
  const share = (name, make) => (Object.prototype.hasOwnProperty.call(shared, name) ? shared[name] : (shared[name] = make()));

  // Event handler IDL attributes (`onload`, ...) defined as accessors on an interface's prototype or
  // on the window: Gecko keeps the handler in the target's EventListenerManager; here in a table by
  // target, which dispatch reads after the listeners, as it reads a handler set as an own property.
  // EventHandler is [LegacyTreatNonObjectAsNull]: a value that is not an object is stored as null.
  const eventHandlers = share("eventHandlers", () => new WeakMap());
  const eventHandlerOf = (target, name) => { const m = eventHandlers.get(target); return m ? m[name] : undefined; };
  // The refresh tick for the frames (see refreshWanted below): `at` when the next one runs (-1: none
  // queued), the DOM's generation at the last one's layout phase, how many times a frame window was
  // given a `resize` listener or handler, and the cross-origin frames waiting for the layout to give
  // them their size (bridge.js), and the paints waiting for the tick (`paints`: webgl.js's flushes of
  // its command queue, Gecko's OnBeforePaintTransaction).
  const refresh = share("refresh", () => ({ at: -1, generation: -1, watchers: 0, opens: [], paints: new Set(), scrolls: [] }));
  // The windows whose document has ResizeObservers (bridge.js: each entry's observers and its notify(),
  // Gecko's DetermineProximityToViewportAndNotifyResizeObservers), whether an observe() or the loop
  // limit asked for a tick (ScheduleResizeObserversNotification), and the DOM's generation when the
  // last tick looked at them.
  const resizeObservers = share("resizeObservers", () => ({ docs: [], scheduled: false, generation: -1 }));
  // The same for IntersectionObservers (bridge.js: each entry's observers and its update(), Gecko's
  // Document::UpdateIntersections): whether an observe() asked for a tick
  // (EnsureIntersectionObservationsUpdateHappens), and the DOM's generation at the last update.
  const intersectionObservers = share("intersectionObservers", () => ({ docs: [], scheduled: false, generation: -1 }));
  // a frame window's own `resize` listener or handler: its size changes are looked for at each tick
  const watchResize = (target, type) => {
    if (type === "resize" && target === global && global.__mrtShared) refresh.watchers++;
  };
  function defineEventHandlers(obj, names) {
    for (const name of names) {
      Object.defineProperty(obj, name, {
        get() { return eventHandlerOf(this, name) ?? null; },
        set(value) {
          if (value !== null && value !== undefined) watchResize(this, name.slice(2));
          let m = eventHandlers.get(this);
          if (!m) eventHandlers.set(this, (m = Object.create(null)));
          m[name] = (typeof value === "object" || typeof value === "function") && value !== null ? value : null;
        },
        enumerable: true,
        configurable: true,
      });
    }
  }

  // ---- events ----

  class Event {
    constructor(type, init = {}) {
      this.type = String(type);
      this.bubbles = !!init.bubbles;
      this.cancelable = !!init.cancelable;
      this.composed = !!init.composed;
      this.target = null;
      this.currentTarget = null;
      this.eventPhase = 0;
      this.defaultPrevented = false;
      this.timeStamp = clock.now;
      this._stop = false;
      this._stopNow = false;
      this._path = [];
    }
    // The path as the current target may see it: without the nodes of closed shadow trees it is not
    // in (Event::ComposedPath).
    composedPath() {
      const current = this.currentTarget;
      if (!current) return [];
      return this._path.filter(n => !isClosedShadowHidden(n, current));
    }
    stopPropagation() { this._stop = true; }
    stopImmediatePropagation() { this._stop = this._stopNow = true; }
    preventDefault() { if (this.cancelable) this.defaultPrevented = true; }
    // Event::InitEvent: nothing while the event is being dispatched (its path is set); otherwise the
    // type and flags anew, the stop and canceled flags cleared and the target forgotten
    initEvent(type, bubbles = false, cancelable = false) {
      if (arguments.length < 1) throw new TypeError("Event.initEvent: At least 1 argument required, but only 0 passed");
      if (this._path.length) return;
      this.type = String(type);
      this.bubbles = !!bubbles;
      this.cancelable = !!cancelable;
      this.defaultPrevented = false;
      this._stop = this._stopNow = false;
      this.target = null;
    }
  }
  class CustomEvent extends Event {
    constructor(type, init = {}) {
      super(type, init);
      this.detail = init.detail === undefined ? null : init.detail;
    }
    // CustomEvent::InitCustomEvent (a Gecko-specific deprecated method)
    initCustomEvent(type, bubbles = false, cancelable = false, detail = null) {
      if (arguments.length < 1) throw new TypeError("CustomEvent.initCustomEvent: At least 1 argument required, but only 0 passed");
      if (this._path.length) return;
      this.initEvent(type, bubbles, cancelable);
      this.detail = detail;
    }
  }
  // a WebIDL interface's prototype names it for Object.prototype.toString ("[object CustomEvent]")
  for (const C of [Event, CustomEvent]) Object.defineProperty(C.prototype, Symbol.toStringTag, { value: C.name, configurable: true });
  define(global, "Event", Event);
  define(global, "CustomEvent", CustomEvent);

  // The listeners of a target, in a property keyed by a symbol: adding it to a node's reflector
  // preserves the reflector (its addProperty hook), as a listener keeps a Gecko wrapper alive.
  // One flat array per target, four entries per listener: type, callback, flags (CAPTURE, ONCE) and
  // the window whose script added it (its incumbent global, for window.event; undefined in a worker).
  // A Map of lists with a record per listener cost about 400 bytes a listener (1,600 listeners:
  // 0.6 MB); Gecko keeps its listeners in C++ (EventListenerManager).
  const LISTENERS = share("LISTENERS", () => Symbol("listeners"));
  const CAPTURE = 1, ONCE = 2, STRIDE = 4;
  // window.event (nsGlobalWindowInner::GetEvent): the event a listener of this window is handling,
  // set around each call (nsPIDOMWindowInner::SetEvent). Another window of the runtime sets it
  // through the shared symbol, as a listener's window need not be the target's.
  const SWAP_EVENT = share("SWAP_EVENT", () => Symbol("swapEvent"));
  let currentEvent;
  if (!isWorker) {
    define(global, SWAP_EVENT, event => { const old = currentEvent; currentEvent = event; return old; });
    // [Replaceable]: an assignment makes an own data property
    Object.defineProperty(global, "event", {
      get() { return currentEvent; },
      set(value) { define(global, "event", value); Object.defineProperty(global, "event", { enumerable: true }); },
      enumerable: true,
      configurable: true,
    });
  }
  function listenersOf(target, create) {
    if (!Object.prototype.hasOwnProperty.call(target, LISTENERS)) {
      if (!create) return null;
      define(target, LISTENERS, []);
    }
    return target[LISTENERS];
  }
  const captureOf = options => (typeof options === "boolean" ? options : !!(options && options.capture));
  function find(list, type, callback, capture) {
    for (let i = 0; i < list.length; i += STRIDE) {
      if (list[i] === type && list[i + 1] === callback && !!(list[i + 2] & CAPTURE) === capture) return i;
    }
    return -1;
  }
  // addEventListener. `this` is undefined for a bare call (`addEventListener(...)` in a script): the
  // window, as in a browser, where the global's own methods do not need a receiver. In a window it is
  // called through the C++ layer's __mrtWithIncumbent, which passes first the window whose script
  // called it (CallbackObject's incumbent global, which a JS function cannot know).
  function addListener(incumbent, type, callback, options) {
    if (callback == null) return;
    type = String(type);
    const capture = captureOf(options);
    const once = !!(options && typeof options === "object" && options.once);
    const list = listenersOf(this ?? global, true);
    if (find(list, type, callback, capture) < 0) list.push(type, callback, (capture ? CAPTURE : 0) | (once ? ONCE : 0), incumbent);
    watchResize(this ?? global, type);
  }
  function removeEventListener(type, callback, options) {
    const list = listenersOf(this ?? global, false);
    if (!list) return;
    const i = find(list, String(type), callback, captureOf(options));
    if (i >= 0) list.splice(i, STRIDE);
  }
  // An exception a callback threw and nobody caught (a listener's, a timer's). In a window it goes to
  // the console (no error event at the window yet, GAPS B31); in a worker, bridge.js's worker section
  // reports it as Gecko does (setWorkerErrorReport: an ErrorEvent at the worker's global, then at its
  // Worker in the parent, WorkerErrorReport).
  let workerErrorReport = null;
  function reportException(e, what) {
    if (workerErrorReport) workerErrorReport(e);
    else console.error(what, e, e && e.stack);
  }
  // HTML's "clean up after running script" (Gecko's nsAutoMicroTask and CallbackObject::CallSetup,
  // EnterMicroTask and LeaveMicroTask): each call of page code by the runtime (a listener, a timer, a
  // script element, an observer's callback) counts in the C++ layer, and when the outermost one
  // returns there is a microtask checkpoint. So a promise callback queued in one listener of an event
  // the runtime fires runs before the next listener; in an event a script dispatches, it waits for
  // that script. The windows share the count (a script dispatching at a node in a frame is still on
  // the stack).
  const enterCallback = __mrtEnterCallback, leaveCallback = __mrtLeaveCallback;
  // A page callback whose exception goes to the caller, after the checkpoint (CallSetup's
  // eRethrowExceptions)
  function callPage(f, thisArg, args) {
    enterCallback();
    try {
      return Reflect.apply(f, thisArg, args);
    } finally {
      leaveCallback();
    }
  }
  // A node in a shadow tree, not the shadow root itself (nsIContent::GetEventTargetParent's
  // mItemInShadowTree): its listeners set no window.event (WindowFromListener).
  const inShadowTree = target => isNode(target) && target.nodeType !== 11 && !!shadowHost(rootOf(target));
  function invoke(target, event, capturePhase) {
    const list = listenersOf(target, false) || [];
    // the listeners of this type when dispatch reaches the target (a copy: listeners may change it)
    const now = [];
    for (let i = 0; i < list.length; i += STRIDE) {
      if (list[i] === event.type) now.push(list[i + 1], list[i + 2], list[i + 3]);
    }
    event.currentTarget = target;
    let shadow;
    for (let i = 0; i < now.length; i += 3) {
      if (event._stopNow) break;
      const callback = now[i], flags = now[i + 1];
      if (!!(flags & CAPTURE) !== capturePhase) continue;
      if (flags & ONCE) removeEventListener.call(target, event.type, callback, !!(flags & CAPTURE));
      // EventListenerManager::HandleEventSingleListener: the listener's window's event while it runs
      const win = now[i + 2] && !(shadow ??= inShadowTree(target)) ? now[i + 2] : undefined;
      const swap = win && win[SWAP_EVENT];
      const old = swap && swap(event);
      enterCallback();
      try {
        if (typeof callback === "function") callback.call(target, event);
        else callback.handleEvent(event);
      } catch (e) {
        // WidgetEvent::mFlags.mExceptionWasRaised: IndexedDB aborts a transaction on it
        event._exceptionWasRaised = true;
        reportException(e, "uncaught exception in a listener:");
      }
      // the checkpoint with window.event still this event (nsAutoMicroTask's scope ends before
      // HandleEventSingleListener puts the old one back)
      leaveCallback();
      if (swap) swap(old);
    }
    // The event handler property (`el.onload = f`), at the target and while bubbling. Gecko keeps it
    // in the listener list at the place it was first set; here it runs after the listeners. Only an own
    // property is read, so that the missing-API log does not count the lookup.
    if (capturePhase === true || event._stopNow) return;
    const d = Object.getOwnPropertyDescriptor(target, "on" + event.type);
    const handler = d ? ("value" in d ? d.value : d.get && d.get.call(target)) : eventHandlerOf(target, "on" + event.type);
    if (typeof handler !== "function") return;
    event.currentTarget = target;
    // a handler's window is the target's (WindowFromListener without a WebIDL callback)
    const swap = !isWorker && !inShadowTree(target) && global[SWAP_EVENT];
    const old = swap && swap(event);
    enterCallback();
    try {
      // an ErrorEvent at a global calls its onerror with the error's parts, and true cancels it
      // (OnErrorEventHandlerNonNull, JSEventHandler::HandleEvent); every other handler gets the event,
      // and false cancels it
      const parts = event._onErrorArgs;
      if (parts && event.type === "error") {
        if (handler.call(target, ...parts) === true) event.preventDefault();
      } else if (handler.call(target, event) === false) event.preventDefault();
    } catch (e) {
      event._exceptionWasRaised = true;
      reportException(e, "uncaught exception in a listener:");
    }
    leaveCallback();
    if (swap) swap(old);
  }
  // ---- the event path across shadow trees (the DOM's dispatch; Gecko's GetEventTargetParent) ----
  const LINK_HOST = 5, LINK_ASSIGNED_SLOT = 8;
  // a worker has no nodes
  const isNode = isWorker ? () => false : n => n instanceof Node;
  const shadowHost = n => __mrtLink(n, LINK_HOST);
  const rootOf = n => n.getRootNode();
  // A is a shadow-including inclusive ancestor of B
  function shadowIncludingContains(a, b) {
    for (let n = b; n; n = n.parentNode || shadowHost(n)) if (n === a) return true;
    return false;
  }
  // retarget(A, B): A, or the host of the shadow tree that hides it from B
  function retarget(a, b) {
    for (;;) {
      if (!isNode(a)) return a;
      const root = rootOf(a);
      const host = shadowHost(root);
      if (!host || (isNode(b) && shadowIncludingContains(root, b))) return a;
      a = host;
    }
  }
  // A is closed-shadow-hidden from B
  function isClosedShadowHidden(a, b) {
    if (!isNode(a)) return false;
    const root = rootOf(a);
    const host = shadowHost(root);
    if (!host || (isNode(b) && shadowIncludingContains(root, b))) return false;
    return root.mode === "closed" || isClosedShadowHidden(host, b);
  }
  // "get the parent": a slotted node's slot, a shadow root's host (not for an event that is not
  // composed and started in that tree), the window after the document, except for a load event
  // (Document::GetEventTargetParent: load events must not propagate to the window, bug 335251)
  function parentInPath(n, event, firstRoot) {
    if (n === global) return null;
    const host = shadowHost(n);
    if (host) return !event.composed && n === firstRoot ? null : host;
    const slot = __mrtLink(n, LINK_ASSIGNED_SLOT);
    if (slot) return slot;
    const p = n.parentNode;
    if (p) return p;
    return n === document && event.type !== "load" ? global : null;
  }
  // The propagation path: the target and its parents as above; `target` is retargeted against each
  // entry, and cleared after dispatch when the event never left the target's shadow tree.
  function dispatchEvent(event) {
    if (this == null) return dispatchEvent.call(global, event);
    const target = this;
    const path = [target];
    if (isNode(target)) {
      const firstRoot = rootOf(target);
      for (let n = parentInPath(target, event, firstRoot); n; n = parentInPath(n, event, firstRoot)) path.push(n);
    } else {
      // targets that are not nodes name their parent as Gecko's GetEventTargetParent does (an
      // IndexedDB request's events go on to its transaction, then its database)
      for (let n = target; n && typeof n._eventTargetParent === "function" && (n = n._eventTargetParent()); ) path.push(n);
    }
    event._path = path;
    for (let i = path.length - 1; i > 0 && !event._stop; i--) {
      event.eventPhase = 1;
      event.target = retarget(target, path[i]);
      invoke(path[i], event, true);
    }
    // At the target, its capture listeners first, then the others (Gecko's
    // HandleEventTargetChain calls the target item twice, in the capture and the bubbling phase);
    // a capture listener that stops propagation there keeps the others from running.
    if (!event._stop) {
      event.eventPhase = 2;
      event.target = event._legacyTarget || target;
      invoke(target, event, true);
      if (!event._stop) invoke(target, event, false);
    }
    for (let i = 1; i < path.length && event.bubbles && !event._stop; i++) {
      event.eventPhase = 3;
      event.target = retarget(target, path[i]);
      invoke(path[i], event, false);
    }
    const last = event._legacyTarget || retarget(target, path[path.length - 1]);
    event.target = isNode(last) && shadowHost(rootOf(last)) ? null : last;
    event.eventPhase = 0;
    event.currentTarget = null;
    event._path = [];
    return !event.defaultPrevented;
  }
  // The window's load event: dispatched at the window, its target the document (the legacy target
  // override; nsDocumentViewer::LoadComplete sets the event's mTarget to the document)
  function fireWindowLoad() {
    const event = new Event("load");
    event._legacyTarget = global.document;
    global.dispatchEvent(event);
  }
  // the window gets them through its prototype chain (Window.prototype, then EventTarget.prototype)
  // in a window the C++ layer's wrapper gives addEventListener its caller's window; a worker's
  // listeners have none
  define(EventTarget.prototype, "addEventListener", isWorker
    ? function addEventListener(type, callback, options) { return addListener.call(this, undefined, type, callback, options); }
    : __mrtWithIncumbent(addListener, "addEventListener", 2));
  define(EventTarget.prototype, "removeEventListener", removeEventListener);
  define(EventTarget.prototype, "dispatchEvent", dispatchEvent);

  // ---- timers, on a virtual clock (the driver runs them after the page's scripts) ----

  // One clock and one queue of timers for all the runtime's windows (one event loop); each timer
  // knows its window, and a closed window's are dropped (TimeoutManager is per window in Gecko). The
  // clock: the virtual time, the next timer id, the nesting level, whether the last timer run was
  // an animation frame callback, and, on the real clock, the wall-clock time it counts from.
  const clock = share("clock", () => ({ now: 0, next: 1, nesting: 0, ranFrame: false, realFrom: null }));
  // The time now. On the virtual clock it moves only when a task starts; on the real clock
  // (realsite --interactive, SPEC decision 25) it follows the wall clock and never goes back.
  // (the wall clock as the runtime found it: a page may replace Date, as the replay's deterministic
  // script does)
  const wallNow = share("wallNow", () => Date.now);
  const clockNow = () => {
    if (clock.realFrom !== null) clock.now = Math.max(clock.now, wallNow() - clock.realFrom);
    return clock.now;
  };
  const timers = share("timers", () => new Map());
  // Gecko's TimeoutManager: a timeout's nesting level is its creator's plus one, up to
  // dom.clamp.timeout.nesting.level (5); from that level its delay is at least dom.min_timeout_value
  // (4 ms) (CalculateDelay); an interval's level grows with each repetition. A chain of zero-delay
  // timeouts thus moves the clock 4 ms a link from the fifth on. Not for the runtime's own tasks
  // (queueTask) or animation frames.
  const CLAMP_NESTING_LEVEL = 5, MIN_TIMEOUT_MS = 4;
  const clampedDelay = (delay, nesting) => (nesting >= CLAMP_NESTING_LEVEL ? Math.max(delay, MIN_TIMEOUT_MS) : delay);
  // Each task has a priority, Gecko's EventQueuePriority as the scheduler's tasks use it
  // (WebTaskScheduler::GetEventQueuePriority): 2 MediumHigh, 1 Normal (every other task), 0 Low.
  // Among the tasks due at one time the higher priority runs first, then the earlier `order` (the
  // id, unless the task was put back, below).
  const PRIORITY_NORMAL = 1;
  // `nested`: a timeout whose nesting level counts (setTimeout's); AbortSignal.timeout's has none
  function addTimer(callback, ms, args, repeat, timeout = false, priority = PRIORITY_NORMAL, nested = timeout) {
    const id = clock.next++;
    const nesting = nested ? (clock.nesting < CLAMP_NESTING_LEVEL ? clock.nesting + 1 : clock.nesting) : 0;
    const delay = timeout ? clampedDelay(Math.max(0, Number(ms) || 0), nesting) : Math.max(0, Number(ms) || 0);
    const now = clockNow();
    timers.set(id, { callback, args, at: now + delay, delay, every: repeat ? Math.max(delay, 1) : 0, win: global, nesting, timeout, priority, order: id });
    if (timeout && visibility.background) backgroundTimeoutAdded(id, global, now + delay);
    return id;
  }
  // A hibernated page is a background tab (SPEC decision 22, GAPS B79): `background` from
  // __mrt.hide() on, for every window of the runtime (nsDocShell::ActivenessMaybeChanged's
  // SetIsBackground). A background window's TimeoutManager has a minimum scheduling delay of
  // dom.min_background_timeout_value (1000 ms): its executor fires no sooner than 1 s after it was
  // last scheduled, and then runs, as one batch, every timeout of the window due by then that was
  // there when it started (RunTimeout's firing id). The executor is scheduled again at the hide
  // (UpdateBackgroundState), at each batch's start, and when a timeout comes due before every one
  // waiting. `gates`: by window, the earliest time its next batch may start; `batches`: by window,
  // its last batch (the time and the first timer id not in it). Budget throttling starts only 30 s
  // after load (dom.timeout.throttling_delay) and is not modelled (GAPS B120); workers are not
  // throttled on release (dom.workers.throttling.enabled is nightly only).
  const visibility = share("visibility", () => ({ background: false, gates: new WeakMap(), batches: new WeakMap() }));
  const BACKGROUND_TIMEOUT_MS = 1000;
  // this window's document's visibility state, "hidden" once the task of __mrt.windowHidden ran
  let documentHidden = false;
  if (!isWorker) Object.defineProperty(Document.prototype, "hidden", { get() { return documentHidden; }, configurable: true, enumerable: true });
  if (!isWorker) Object.defineProperty(Document.prototype, "visibilityState", { get() { return documentHidden ? "hidden" : "visible"; }, configurable: true, enumerable: true });
  const backgroundTimeout = t => visibility.background && t.timeout;
  // when timer `id` runs: its own time, or for a background timeout its batch's
  function dueAt(id, t) {
    if (!backgroundTimeout(t)) return t.at;
    const batch = visibility.batches.get(t.win);
    if (batch && t.at <= batch.at && id < batch.next) return batch.at;
    return Math.max(t.at, visibility.gates.get(t.win) ?? 0);
  }
  // a timeout of window `win` due at `at` was added: the executor is scheduled again, 1 s on, when
  // the timeout is due before every other one of the window (TimeoutExecutor::MaybeReschedule)
  function backgroundTimeoutAdded(id, win, at) {
    for (const [other, t] of timers) {
      if (other !== id && t.win === win && t.timeout && t.at <= at) return;
    }
    visibility.gates.set(win, clock.now + BACKGROUND_TIMEOUT_MS);
  }
  // (`aAt`, `bAt`: when each runs, dueAt)
  const runsBefore = (aAt, a, bAt, b) => aAt !== bAt ? aAt < bAt : a.priority !== b.priority ? a.priority > b.priority : a.order < b.order;
  // Timeouts wait for the scheduler's user-visible and user-blocking tasks (Gecko's
  // HasScheduledNormalOrHighPriorityWebTasks, kept by bridge.js's scheduler): in a window,
  // TimeoutManager::RunTimeout stops after a timeout while one is queued and queues itself again
  // behind them; in a worker it runs none while one is queued. Here such a timeout goes back behind
  // the tasks queued so far. A run of the executor: the timeouts due when its first ran, queued
  // before it (`run`: when and the queue order then; null once another kind of task ran).
  const webTasks = share("webTasks", () => ({ scheduled: 0, run: null }));
  const inExecutorRun = t => webTasks.run !== null && t.at <= webTasks.run.at && t.order < webTasks.run.order;
  const timeoutWaits = t => t.timeout && webTasks.scheduled > 0 && (isWorker || inExecutorRun(t));
  // after a task ran: a timeout starts a run unless it is part of the one going on
  const ranTask = t => { webTasks.run = !t.timeout ? null : inExecutorRun(t) ? webTasks.run : { at: t.at, order: clock.next }; };
  const windowClosed = win => win !== undefined && typeof __mrtWindows === "function" && __mrtWindows(5, win);

  // The refresh tick for the frames (DESIGN, "The frames' layout (S5)"). Gecko's refresh driver, on
  // each tick, runs the resize steps (a `resize` at each window whose document's size changed since
  // the last tick, PresShell::RunResizeSteps) and then flushes layout, which finds the next changes of
  // the frames' sizes. A layout read finds them too, as the parent's flush sizes the frames. Here a
  // tick is a task at the next animation frame (nextDueTimer) once it is wanted: when a resize is
  // waiting, when a cross-origin frame waits for its size, when the DOM changed since the last tick
  // and a frame window listens for `resize`, when an observe() asked for one, or when the DOM changed
  // and a document has ResizeObservers or IntersectionObservers (the layout phase is only for them: a
  // flush per tick otherwise changes nothing a page sees). The ResizeObserver loop runs after the
  // resize steps, then the intersection observations are updated. A hibernated page has no tick
  // (its documents have no PresShell, so no refresh driver) but for the cross-origin frames inserted
  // meanwhile, whose runtimes still start.
  function refreshWanted() {
    // a worker has no document to lay out
    if (isWorker || refresh.at >= 0) return false;
    if (visibility.background) return refresh.opens.length > 0;
    if (refresh.opens.length > 0 || refresh.paints.size > 0 || refresh.scrolls.length > 0 || __mrtRefresh(0)) return true;
    if (resizeObservers.scheduled) return true;
    if (resizeObservers.docs.some(d => d.observers.length > 0) && __mrtRefresh(1) !== resizeObservers.generation) return true;
    if (intersectionObservers.scheduled) return true;
    if (intersectionObservers.docs.some(d => d.observers.length > 0) && __mrtRefresh(1) !== intersectionObservers.generation) return true;
    return refresh.watchers > 0 && __mrtRefresh(1) !== refresh.generation;
  }
  function refreshTick() {
    refresh.at = -1;
    if (visibility.background) {
      for (const open of refresh.opens.splice(0)) open();
      return;
    }
    __mrtRefresh(3);
    if (refresh.watchers > 0) {
      __mrtRefresh(2);
      refresh.generation = __mrtRefresh(1);
    }
    // the cross-origin frames inserted since the last tick get their size from this layout, and their
    // runtimes
    for (const open of refresh.opens.splice(0)) open();
    // the scroll steps: the scroll events posted since the last tick (PresShell::PostScrollEvent)
    for (const scroll of refresh.scrolls.splice(0)) {
      try { scroll(); } catch (e) { reportException(e, "uncaught exception in a scroll event:"); }
    }
    // the layout phase's ResizeObserver loop, document by document (a closed window's dropped)
    resizeObservers.scheduled = false;
    resizeObservers.docs = resizeObservers.docs.filter(d => !windowClosed(d.win));
    for (const d of resizeObservers.docs.slice()) {
      if (d.observers.length > 0) d.notify();
    }
    resizeObservers.generation = __mrtRefresh(1);
    // the update intersection observations steps (step 19, after the layout phase), each document
    // queueing the task that delivers its entries
    intersectionObservers.scheduled = false;
    intersectionObservers.docs = intersectionObservers.docs.filter(d => !windowClosed(d.win));
    for (const d of intersectionObservers.docs.slice()) {
      if (d.observers.length > 0) d.update();
    }
    intersectionObservers.generation = __mrtRefresh(1);
    // the paint, after the layout phase (WebGL's queued commands sent)
    const paints = [...refresh.paints];
    refresh.paints.clear();
    for (const paint of paints) {
      try { paint(); } catch (e) { reportException(e, "uncaught exception in a paint:"); }
    }
  }
  // The task due first of the runtime's dedicated workers (bridge.js's Worker, worker.rs), as one of
  // this loop's: running it runs the worker's task on the worker's thread, then hands what the worker
  // posted to the Worker object that owns it (shared.workers, by worker id). Not a timer: its id 0 is
  // never in `timers`. A worker's own global has no workers of its own (no nested workers yet).
  const workers = share("workers", () => new Map());
  function runWorkerTask(id) {
    const out = JSON.parse(__mrtWorker(3, id));
    const deliver = workers.get(id);
    if (deliver) deliver(out);
  }
  function workerTask() {
    const w = !isWorker && typeof __mrtWorker === "function" ? __mrtWorker(2) : null;
    return w && { callback: runWorkerTask, args: [w[0]], at: w[1], every: 0, nesting: 0, timeout: false };
  }
  // The earliest timer due, as [id, timer, when it runs], a closed window's dropped on the way, and
  // in a hibernated page the animation frame callbacks left waiting (a document without a PresShell
  // runs none; Gecko runs them once the page is shown); a refresh tick queued first if one is
  // wanted; a worker's task when it is due before every timer. The tick is the next animation
  // frame's, after its callbacks, as Gecko's refresh driver runs them before the layout phase: now
  // when the task that just ran was one of them, else with the earliest frame callback waiting, else
  // 16 ms on.
  function nextDueTimer() {
    if (refreshWanted()) {
      let at = clock.now + 16;
      if (clock.ranFrame) at = clock.now;
      else if (!visibility.background) for (const t of timers.values()) if (t.frame && t.at < at) at = Math.max(t.at, clock.now);
      refresh.at = at;
      addTimer(refreshTick, at - clock.now, [], false);
    }
    // a timeout that waits goes behind the others once (twice, the count is wrong: it runs)
    let next = null;
    const waited = new Set();
    for (;;) {
      next = null;
      for (const [id, t] of timers) {
        if (windowClosed(t.win)) { timers.delete(id); continue; }
        if (t.frame && visibility.background) continue;
        const at = dueAt(id, t);
        if (!next || runsBefore(at, t, next[2], next[1])) next = [id, t, at];
      }
      if (!next || !timeoutWaits(next[1]) || waited.has(next[0])) break;
      waited.add(next[0]);
      next[1].order = clock.next++;
    }
    const w = workerTask();
    if (w && (!next || w.at < next[2])) return [0, w, w.at];
    return next;
  }
  // A handler that is not a function is a string (or a TrustedScript), compiled when the timer runs
  // (nsGlobalWindowInner::SetTimeoutOrInterval's string handler): checked against the document's
  // Trusted Types when it is set, evaluated in the global scope, with no second check. In a window it
  // is named after the place that set the timer, the first frame outside the glue
  // (WindowScriptTimeoutHandler::Call: its mCaller's file and line); a worker's still runs in an
  // eval.
  const timerHandler = (callback, sink) => {
    if (typeof callback === "function") return callback;
    const scope = isWorker ? "WorkerGlobalScope" : "Window";
    const code = global.__mrt.trustedTypes.compliant(callback, "TrustedScript", `${scope} ${sink}`, `${scope}.${sink}`);
    if (!isWorker) {
      const caller = String(new Error().stack).split("\n").map(f => /@(.*):(\d+):\d+$/.exec(f))
        .find(at => at && at[1] !== "bridge.js" && at[1] !== "prelude.js");
      return () => __mrtClassicScript(code, caller ? caller[1] : "", caller ? +caller[2] : 1, 0);
    }
    return () => { global.__mrt.trustedTypes.exemptNextEval(); (0, eval)(code); };
  };
  // a timer that calls page code (runTimers and runNextTimer count it as a callback)
  const pageTimer = id => {
    timers.get(id).page = true;
    return id;
  };
  define(global, "setTimeout", (callback, ms, ...args) => pageTimer(addTimer(timerHandler(callback, "setTimeout"), ms, args, false, true)));
  define(global, "setInterval", (callback, ms, ...args) => pageTimer(addTimer(timerHandler(callback, "setInterval"), ms, args, true, true)));
  define(global, "clearTimeout", id => { timers.delete(id); });
  define(global, "clearInterval", id => { timers.delete(id); });
  // an animation frame callback: a timer 16 ms on, marked as a frame's (the refresh tick follows it)
  define(global, "requestAnimationFrame", callback => {
    const id = pageTimer(addTimer(() => callback(clock.now), 16, [], false));
    timers.get(id).frame = true;
    return id;
  });
  define(global, "cancelAnimationFrame", id => { timers.delete(id); });
  define(global, "queueMicrotask", callback => { Promise.resolve().then(callback); });
  define(global, "performance", { now: clockNow });

  // An interval's next run, counted from now, the time its callback runs (its nesting level up by
  // one, to the clamp level, and its delay clamped as a timeout's: TimeoutManager::RescheduleTimeout),
  // or the timer gone.
  function rescheduleOrDelete(id, t) {
    if (!t.every) {
      timers.delete(id);
      return;
    }
    if (t.timeout && t.nesting < CLAMP_NESTING_LEVEL) t.nesting += 1;
    t.at = clock.now + (t.timeout ? clampedDelay(t.every, t.nesting) : t.every);
  }

  // Timer `id` is about to run at `at` (nextDueTimer's): the clock moves there, and a background
  // timeout outside its window's last batch starts a new one, which schedules the window's
  // executor again 1 s on (RunTimeout's MaybeSchedule at its start).
  function startTimer(id, t, at) {
    clock.now = clock.realFrom === null ? at : Math.max(at, clockNow());
    if (!backgroundTimeout(t)) return;
    const batch = visibility.batches.get(t.win);
    if (batch && t.at <= batch.at && id < batch.next) return;
    visibility.batches.set(t.win, { at, next: clock.next });
    visibility.gates.set(t.win, at + BACKGROUND_TIMEOUT_MS);
  }

  // Runs due timers in time order, then priority and queue order, until none is left before `limit` ms on the
  // virtual clock. Returns how many callbacks ran.
  function runTimers(limit) {
    let ran = 0;
    for (;;) {
      const next = nextDueTimer();
      if (!next || next[2] > limit) return ran;
      const [id, t, at] = next;
      startTimer(id, t, at);
      const nesting = t.nesting;
      rescheduleOrDelete(id, t);
      ranTask(t);
      clock.ranFrame = !!t.frame;
      ran++;
      const outer = clock.nesting;
      clock.nesting = t.timeout ? nesting : 0;
      if (t.page) enterCallback();
      try {
        if (typeof t.callback === "function") t.callback(...t.args);
      } catch (e) {
        reportException(e, "uncaught exception in a timer:");
      }
      if (t.page) leaveCallback();
      clock.nesting = outer;
    }
  }

  // One due timer (the earliest), for the driver's stepwise loop (Page::run_timers_stepwise): 1 if
  // one ran, 0 if none is left before `limit`.
  function runNextTimer(limit) {
    const next = nextDueTimer();
    if (!next || next[2] > limit) return 0;
    const [id, t, at] = next;
    if (clock.profile) clock.lastRun = t;
    startTimer(id, t, at);
    const nesting = t.nesting;
    rescheduleOrDelete(id, t);
    ranTask(t);
    clock.ranFrame = !!t.frame;
    const outer = clock.nesting;
    clock.nesting = t.timeout ? nesting : 0;
    // A page's timer is a callback of its own; the runtime's own tasks are not, the page code they
    // call counts itself. Its microtasks run at the timer's nesting level (the handler's Call ends
    // with the checkpoint, before nsGlobalWindowInner::RunTimeoutHandler puts the level back), so
    // a timeout set in one is nested one deeper.
    if (t.page) enterCallback();
    try {
      if (typeof t.callback === "function") t.callback(...t.args);
    } catch (e) {
      reportException(e, "uncaught exception in a timer:");
    }
    if (t.page) leaveCallback();
    clock.nesting = outer;
    return 1;
  }

  // ---- CSSStyleDeclaration, CSSStyleProperties: element.style and getComputedStyle ----

  // Gecko's CSSStyleDeclaration (dom/webidl/CSSStyleDeclaration.webidl) and its generated subclass
  // CSSStyleProperties, one accessor per property name (layout/style/GenerateCSSProperties.py,
  // gen_webidl; the names are release Firefox's, glue/css_properties.js). Both element.style and
  // getComputedStyle's result are CSSStyleProperties, as in Gecko, so a page that wraps
  // CSSStyleDeclaration.prototype.setProperty (Facebook's Hyperion) sees every call made through it.
  //
  // Each object stands for a backend, Gecko's nsDOMCSSDeclaration subclass: the style attribute
  // (nsDOMCSSAttributeDeclaration) or the element's current values (nsComputedDOMStyle). The object
  // is a Proxy only for the indexed getter (`style[0]`, a legacy platform object's); every other
  // property is looked up on its target, whose prototype is CSSStyleProperties.prototype.

  // @CSS_PROPERTIES@

  const toCSS = name => (name.startsWith("--") ? name : name.replace(/[A-Z]/g, c => "-" + c.toLowerCase()));
  // the CSS name of an attribute: dashed names as they are, camelCase (cssFloat, webkitX, WebkitX,
  // MozX) as nsCSSProps::PropertyIDLName spells it
  const cssName = name => name === "cssFloat" ? "float" : /^webkit[A-Z]/.test(name) ? "-" + toCSS(name) : toCSS(name);

  const STYLE = { get: 0, priority: 1, set: 2, remove: 3, text: 4, length: 5, item: 6 };
  const COMPUTED = { value: 0, length: 1, item: 2 };
  const noModification = what => { throw new DOMException(what, "NoModificationAllowedError"); };

  // nsDOMCSSAttributeDeclaration: the element's declaration block, which the C++ layer parses and
  // serializes with Stylo (__mrtStyle, mrt_style::cssom); a change is written back to the attribute
  function inlineDeclaration(el) {
    const css = () => el.getAttribute("style") || "";
    const write = next => { if (next !== null) el.setAttribute("style", next); };
    const backend = {
      get: name => __mrtStyle(STYLE.get, css(), name, "", false),
      priority: name => __mrtStyle(STYLE.priority, css(), name, "", false),
      // nsDOMCSSDeclaration::SetProperty: an empty value removes; a priority other than "" or
      // "important" is ignored
      set(name, value, priority) {
        if (value === "") return void backend.remove(name);
        if (priority !== "" && priority.toLowerCase() !== "important") return;
        write(__mrtStyle(STYLE.set, css(), name, value, priority !== ""));
      },
      remove(name) {
        const text = css(), old = __mrtStyle(STYLE.get, text, name, "", false);
        write(__mrtStyle(STYLE.remove, text, name, "", false));
        return old;
      },
      text: () => __mrtStyle(STYLE.text, css(), "", "", false),
      setText: text => el.setAttribute("style", __mrtStyle(STYLE.text, text, "", "", false)),
      length: () => Number(__mrtStyle(STYLE.length, css(), "", "", false)),
      item: i => __mrtStyle(STYLE.item, css(), "", String(i), false),
    };
    return backend;
  }

  // nsComputedDOMStyle: a read-only declaration whose every read asks for the element's current
  // value (__mrtComputed: style and layout flushed, the value from Stylo or the frames,
  // mrt_layout's nsComputedDOMStyle). The pseudo-element is kept as given and parsed on the Rust side.
  function computedDeclaration(elt, pseudo) {
    return {
      get: name => {
        // a property an animation is changing has the animation's value (Element.animate, below)
        const animated = animatedStyle(elt, pseudo, name);
        return animated !== null ? animated : __mrtComputed(COMPUTED.value, elt, pseudo, name);
      },
      priority: () => "",
      set(name) { noModification(`Can't set value for property '${name}' in computed style`); },
      remove(name) { noModification(`Can't remove property '${name}' from computed style`); },
      text: () => "",
      setText() { noModification("Can't set cssText on computed style"); },
      length: () => Number(__mrtComputed(COMPUTED.length, null, "", "")),
      item: i => __mrtComputed(COMPUTED.item, null, "", String(i)),
    };
  }

  // the backend of each declaration object, by the object and by its Proxy's target (shared by the
  // runtime's windows, like the elements' style objects)
  const declarations = share("declarations", () => new WeakMap());
  const declarationTargets = share("declarationTargets", () => new WeakMap());
  const backendOf = (obj, what) => {
    const backend = declarations.get(obj);
    if (!backend) throw new TypeError(`'${what}' called on an object that does not implement interface CSSStyleDeclaration.`);
    return backend;
  };
  const illegalConstructor = () => { throw new TypeError("Illegal constructor."); };
  const defineInterface = (name, parent, members) => {
    const C = { [name]: function () { illegalConstructor(); } }[name];
    if (parent) {
      Object.setPrototypeOf(C, parent);
      Object.setPrototypeOf(C.prototype, parent.prototype);
    }
    // WebIDL: operations and attributes are enumerable, the constructor is not
    for (const [key, desc] of Object.entries(Object.getOwnPropertyDescriptors(members)))
      Object.defineProperty(C.prototype, key, { ...desc, enumerable: true });
    Object.defineProperty(C.prototype, Symbol.toStringTag, { value: name, configurable: true });
    define(global, name, C);
    return C;
  };

  const CSSStyleDeclaration = defineInterface("CSSStyleDeclaration", null, {
    get cssText() { return backendOf(this, "cssText").text(); },
    set cssText(text) { backendOf(this, "cssText").setText(String(text)); },
    get length() { return backendOf(this, "length").length(); },
    item(index) { return backendOf(this, "item").item(Number(index) >>> 0); },
    getPropertyValue(property) { return backendOf(this, "getPropertyValue").get(String(property)); },
    getPropertyPriority(property) { return backendOf(this, "getPropertyPriority").priority(String(property)); },
    setProperty(property, value, priority = "") {
      const backend = backendOf(this, "setProperty");
      backend.set(String(property), value === null ? "" : String(value), priority === null ? "" : String(priority));
    },
    removeProperty(property) { return backendOf(this, "removeProperty").remove(String(property)); },
    get parentRule() { backendOf(this, "parentRule"); return null; },
  });
  const CSSStyleProperties = defineInterface("CSSStyleProperties", CSSStyleDeclaration, {});
  // The property accessors, GetPropertyValue and SetPropertyValue by property id as the generated
  // ones call them: not through the JS-visible methods, and with no priority. 1,159 accessor pairs
  // cost about 0.46 MB a window, so they are made when the interface is first used, as Gecko makes
  // an interface's prototype on first use: the first declaration object, or the first read of the
  // CSSStyleProperties global (which then becomes a plain value).
  let accessorsMade = false;
  const makePropertyAccessors = () => {
    if (accessorsMade) return;
    accessorsMade = true;
    const proto = CSSStyleProperties.prototype;
    for (const attr of CSS_PROPERTY_ATTRIBUTES) {
      const property = cssName(attr);
      Object.defineProperty(proto, attr, {
        get() { return backendOf(this, attr).get(property); },
        set(value) { backendOf(this, attr).set(property, value === null ? "" : String(value), ""); },
        enumerable: true,
        configurable: true,
      });
    }
  };
  Object.defineProperty(global, "CSSStyleProperties", {
    get() { makePropertyAccessors(); define(global, "CSSStyleProperties", CSSStyleProperties); return CSSStyleProperties; },
    set(v) { define(global, "CSSStyleProperties", v); },
    configurable: true,
    enumerable: false,
  });

  // the indexed getter: `obj[i]` is item(i) while i < length, read-only
  const arrayIndex = name => typeof name === "string" && /^(0|[1-9]\d*)$/.test(name) && Number(name) < 4294967295;
  const indexed = (target, name) => {
    if (!arrayIndex(name)) return undefined;
    const backend = declarationTargets.get(target), i = Number(name);
    return i < backend.length() ? backend.item(i) : undefined;
  };
  const declarationHandler = {
    get(target, name, receiver) { return arrayIndex(name) ? indexed(target, name) : Reflect.get(target, name, receiver); },
    has(target, name) { return arrayIndex(name) ? indexed(target, name) !== undefined : Reflect.has(target, name); },
    getOwnPropertyDescriptor(target, name) {
      if (!arrayIndex(name)) return Reflect.getOwnPropertyDescriptor(target, name);
      const value = indexed(target, name);
      return value === undefined ? undefined : { value, writable: false, enumerable: true, configurable: true };
    },
    ownKeys(target) {
      const n = declarationTargets.get(target).length(), keys = [];
      for (let i = 0; i < n; i++) keys.push(String(i));
      return keys.concat(Reflect.ownKeys(target));
    },
    set(target, name, value, receiver) { return arrayIndex(name) ? false : Reflect.set(target, name, value, receiver); },
    defineProperty(target, name, desc) { return arrayIndex(name) ? false : Reflect.defineProperty(target, name, desc); },
    deleteProperty(target, name) { return arrayIndex(name) ? indexed(target, name) === undefined : Reflect.deleteProperty(target, name); },
  };
  const makeDeclaration = backend => {
    makePropertyAccessors();
    const target = Object.create(CSSStyleProperties.prototype);
    const obj = new Proxy(target, declarationHandler);
    declarations.set(obj, backend);
    declarationTargets.set(target, backend);
    return obj;
  };

  // ElementCSSInlineStyle: [SameObject, PutForwards=cssText] style, one object per element
  const styles = share("styles", () => new WeakMap());
  if (!isWorker) Object.defineProperty(HTMLElement.prototype, "style", {
    configurable: true,
    enumerable: true,
    get() {
      let style = styles.get(this);
      if (!style) styles.set(this, (style = makeDeclaration(inlineDeclaration(this))));
      return style;
    },
    set(text) { this.style.cssText = text; },
  });

  define(global, "getComputedStyle", function getComputedStyle(elt, pseudoElt) {
    if (!(elt instanceof Element)) throw new TypeError("Window.getComputedStyle: Argument 1 does not implement interface Element.");
    const pseudo = pseudoElt === undefined || pseudoElt === null ? "" : String(pseudoElt);
    return makeDeclaration(computedDeclaration(elt, pseudo));
  });

  // ---- classList, and the ChildNode and ParentNode helpers ----

  // DOMException, with the legacy codes of its names (bridge.js until session 45)
  const DOM_EXCEPTION_CODES = { IndexSizeError: 1, HierarchyRequestError: 3, WrongDocumentError: 4, InvalidCharacterError: 5,
    NoModificationAllowedError: 7, NotFoundError: 8, NotSupportedError: 9, InvalidStateError: 11, SyntaxError: 12,
    InvalidModificationError: 13, NamespaceError: 14, InvalidAccessError: 15, TypeMismatchError: 17, SecurityError: 18,
    NetworkError: 19, AbortError: 20, URLMismatchError: 21, QuotaExceededError: 22, TimeoutError: 23,
    InvalidNodeTypeError: 24, DataCloneError: 25 };
  class DOMException extends Error {
    constructor(message = "", name = "Error") { super(String(message)); define(this, "name", String(name)); }
    get code() { return DOM_EXCEPTION_CODES[this.name] || 0; }
  }
  define(global, "DOMException", DOMException);

  // Gecko: dom/base/nsDOMTokenList.cpp. The list reads and writes the `class` attribute itself, not
  // `className`: a Polymer element that declares a `className` property shadows it with an accessor
  // that reads undefined, which threw in YouTube's ytd-watch-flexy.updatePageData_ and left the watch
  // page without its data. One list per element ([SameObject]).
  const tokenLists = share("tokenLists", () => new WeakMap());
  const isHTMLWhitespace = c => c === " " || c === "\t" || c === "\n" || c === "\f" || c === "\r";
  class DOMTokenList {
    constructor(element, attr) { this._element = element; this._attr = attr; }
    // GetParsedAttr and RemoveDuplicates: the attribute's tokens, each once, or null without one
    _tokens() {
      const value = this._element.getAttribute(this._attr);
      if (value === null) return null;
      const tokens = [];
      for (const t of value.split(/[ \t\n\f\r]+/)) if (t && !tokens.includes(t)) tokens.push(t);
      return tokens;
    }
    _set(tokens) { this._element.setAttribute(this._attr, tokens.join(" ")); }
    // CheckToken
    static _check(token, method) {
      if (token === "") throw new DOMException(`DOMTokenList.${method}: The empty string is not a valid token.`, "SyntaxError");
      for (const c of token) {
        if (isHTMLWhitespace(c)) throw new DOMException(`DOMTokenList.${method}: The token can not contain whitespace.`, "InvalidCharacterError");
      }
    }
    get length() { const t = this._tokens(); return t ? t.length : 0; }
    item(i) { const t = this._tokens(); i = Number(i) >>> 0; return t && i < t.length ? t[i] : null; }
    get value() { return this._element.getAttribute(this._attr) ?? ""; }
    set value(v) { this._element.setAttribute(this._attr, String(v)); }
    toString() { return this.value; }
    contains(token) { const t = this._tokens(); return !!t && t.includes(String(token)); }
    // AddInternal
    add(...tokens) {
      tokens = tokens.map(String);
      for (const t of tokens) DOMTokenList._check(t, "add");
      const result = this._tokens() || [];
      for (const t of tokens) if (!result.includes(t)) result.push(t);
      this._set(result);
    }
    // RemoveInternal: nothing is written without the attribute
    remove(...tokens) {
      tokens = tokens.map(String);
      for (const t of tokens) DOMTokenList._check(t, "remove");
      const current = this._tokens();
      if (!current) return;
      this._set(current.filter(t => !tokens.includes(t)));
    }
    toggle(token, force) {
      token = String(token);
      DOMTokenList._check(token, "toggle");
      const forceOn = force !== undefined && !!force, forceOff = force !== undefined && !force;
      const current = this._tokens();
      let present = !!current && current.includes(token);
      if (present) {
        if (!forceOn) { this._set(current.filter(t => t !== token)); present = false; }
      } else if (!forceOff) {
        this._set([...(current || []), token]);
        present = true;
      }
      return present;
    }
    // Replace and ReplaceInternal: the first of the old or new token becomes the new one, the
    // attribute is not touched when the old token is absent
    replace(token, newToken) {
      token = String(token); newToken = String(newToken);
      if (newToken === "") throw new DOMException("DOMTokenList.replace: The empty string is not a valid token.", "SyntaxError");
      DOMTokenList._check(token, "replace");
      DOMTokenList._check(newToken, "replace");
      const current = this._tokens();
      if (!current || !current.includes(token)) return false;
      const result = [];
      let sawIt = false;
      for (const t of current) {
        if (t === token || t === newToken) {
          if (!sawIt) { sawIt = true; result.push(newToken); }
          continue;
        }
        result.push(t);
      }
      this._set(result);
      return true;
    }
    supports() { throw new TypeError("DOMTokenList.supports: Element.classList has no supported tokens"); }
    forEach(callback, thisArg) { (this._tokens() || []).forEach((t, i) => callback.call(thisArg, t, i, this)); }
    keys() { return (this._tokens() || []).keys(); }
    values() { return (this._tokens() || []).values(); }
    entries() { return (this._tokens() || []).entries(); }
    [Symbol.iterator]() { return this.values(); }
  }
  Object.defineProperty(DOMTokenList.prototype, Symbol.toStringTag, { value: "DOMTokenList", configurable: true });
  define(global, "DOMTokenList", DOMTokenList);
  if (!isWorker) Object.defineProperty(Element.prototype, "classList", {
    configurable: true,
    enumerable: true,
    get() {
      let list = tokenLists.get(this);
      if (!list) {
        // indexed access (classList[0]), as the WebIDL indexed getter
        list = new Proxy(new DOMTokenList(this, "class"), {
          get(target, key, receiver) {
            if (typeof key === "string" && /^(0|[1-9][0-9]*)$/.test(key)) return target.item(Number(key)) ?? undefined;
            const v = Reflect.get(target, key, target);
            return typeof v === "function" ? v.bind(target) : v;
          },
          set(target, key, v) { return Reflect.set(target, key, v, target); },
        });
        tokenLists.set(this, list);
      }
      return list;
    },
    set(v) { this.classList.value = v; },
  });
  // document.implementation (DOMImplementation): new HTML documents in the same arena
  class DOMImplementation {
    createHTMLDocument(title) { return __mrtCreateHTMLDocument(title); }
    hasFeature() { return true; }
  }
  const implementation = new DOMImplementation();
  if (!isWorker) Object.defineProperty(Document.prototype, "implementation", { get() { return implementation; }, configurable: true, enumerable: true });
  define(global, "DOMImplementation", DOMImplementation);
  // DOMParser (dom/base/DOMParser.cpp): text/html into a new document, scripts not run and no
  // declarative shadow roots; the XML types through parseXML below.
  class DOMParser {
    parseFromString(str, type) {
      type = String(type);
      if (!["text/html", "text/xml", "application/xml", "application/xhtml+xml", "image/svg+xml"].includes(type)) {
        throw new TypeError(`DOMParser.parseFromString: '${type}' (value of argument 2) is not a valid value for enumeration SupportedType.`);
      }
      const doc = type === "text/html" ? __mrtParseDocument(String(str)) : parseXML(String(str));
      preventExecution(doc);
      return doc;
    }
  }
  define(global, "DOMParser", DOMParser);

  // The DOM methods parseXML builds with, taken before the page can replace them.
  const xmlDom = isWorker ? null : {
    createElementNS: Document.prototype.createElementNS, createTextNode: Document.prototype.createTextNode,
    createComment: Document.prototype.createComment, setAttribute: Element.prototype.setAttribute,
    appendChild: Node.prototype.appendChild, removeChild: Node.prototype.removeChild,
  };
  const XML_PREDEFINED = { lt: "<", gt: ">", amp: "&", quot: "\"", apos: "'" };
  const XML_NAME = /[A-Za-z_:À-￿][\w.\-:·À-￿]*/y;
  const XMLNS = "http://www.w3.org/2000/xmlns/", XML_NS = "http://www.w3.org/XML/1998/namespace";
  const XML_PARSERERROR_NS = "http://www.mozilla.org/newlayout/xml/parsererror.xml";
  // An XML document from `src`, as Gecko's expat sink builds it for DOMParser (nsExpatDriver,
  // nsXMLContentSink): elements, attributes, text, comments, CDATA, namespaces declared with xmlns.
  //
  // The document is a new one of this arena (as createHTMLDocument's, B23) with its children
  // replaced, so it is still an HTML document for the bindings: an XHTML element's tagName is
  // uppercased. A namespace other than XHTML, SVG and MathML is not kept, nor an element's prefix
  // (B18); a CDATA section is a text node, and processing instructions and the doctype are left out
  // (B27). A document that is not well-formed is Gecko's error document: a <parsererror> root
  // whose text says what is wrong and where, with the line in a <sourcetext> child.
  function parseXML(src) {
    src = src.replace(/\r\n?/g, "\n");
    const doc = __mrtParseDocument("");
    const d = xmlDom;
    xmlDom.removeChild.call(doc, doc.documentElement);
    let i = 0, root = null, parent = doc;
    // per open element: its node, its qualified name, and the namespaces in scope inside it
    const open = [];
    let scope = { xml: XML_NS, xmlns: XMLNS };
    class XMLError { constructor(msg, at) { this.msg = msg; this.at = at; } }
    const fail = (msg, at = i) => { throw new XMLError(msg, at); };
    const name = () => {
      XML_NAME.lastIndex = i;
      const m = XML_NAME.exec(src);
      if (!m) fail("not well-formed");
      i += m[0].length;
      return m[0];
    };
    const skipSpace = () => { while (i < src.length && " \t\n".includes(src[i])) i++; };
    const unescape = (s, at) => s.replace(/&([^;&<]*);?/g, (m, ref, off) => {
      if (!m.endsWith(";")) fail("not well-formed", at + off);
      if (ref[0] === "#") {
        const cp = ref[1] === "x" ? parseInt(ref.slice(2), 16) : parseInt(ref.slice(1), 10);
        if (!(cp > 0 && cp <= 0x10ffff) || !/^#(x[0-9A-Fa-f]+|[0-9]+)$/.test(ref)) fail("reference to invalid character number", at + off);
        return String.fromCodePoint(cp);
      }
      if (!Object.prototype.hasOwnProperty.call(XML_PREDEFINED, ref)) fail("undefined entity", at + off);
      return XML_PREDEFINED[ref];
    });
    const add = node => d.appendChild.call(parent, node);
    const text = (s, at) => {
      if (parent === doc) {
        // (before the root expat reports the text where it ends)
        if (/[^ \t\n]/.test(s)) root ? fail("junk after document element", at + s.search(/[^ \t\n]/)) : fail("not well-formed", at + s.length);
      } else if (s) add(d.createTextNode.call(doc, unescape(s, at)));
    };
    const resolve = (qname, isAttr, at) => {
      const colon = qname.indexOf(":");
      if (colon < 0) return isAttr ? null : (scope[""] || null);
      const prefix = qname.slice(0, colon);
      if (prefix === "xmlns" && isAttr) return XMLNS;
      if (!(prefix in scope)) fail("prefix not bound to a namespace", at);
      return scope[prefix];
    };
    try {
      while (i < src.length) {
        const lt = src.indexOf("<", i);
        if (lt < 0) { text(src.slice(i), i); i = src.length; break; }
        if (lt > i) text(src.slice(i, lt), i);
        i = lt;
        if (src.startsWith("<!--", i)) {
          const end = src.indexOf("-->", i + 4);
          if (end < 0) fail("unclosed token");
          add(d.createComment.call(doc, src.slice(i + 4, end)));
          i = end + 3;
        } else if (src.startsWith("<![CDATA[", i)) {
          if (parent === doc) fail("syntax error");
          const end = src.indexOf("]]>", i + 9);
          if (end < 0) fail("unclosed CDATA section");
          add(d.createTextNode.call(doc, src.slice(i + 9, end)));
          i = end + 3;
        } else if (src.startsWith("<?", i)) {
          if (src.startsWith("<?xml", i) && /[\s?]/.test(src[i + 5] || "") && i !== 0) fail("XML or text declaration not at start of entity");
          const end = src.indexOf("?>", i + 2);
          if (end < 0) fail("unclosed token");
          i = end + 2;
        } else if (src.startsWith("<!DOCTYPE", i)) {
          if (root) fail("syntax error");
          // to the end of the declaration, past an internal subset in brackets
          let depth = 0;
          for (i += 9; i < src.length && (src[i] !== ">" || depth); i++) {
            if (src[i] === "[") depth++;
            else if (src[i] === "]") depth--;
          }
          if (i >= src.length) fail("unclosed token");
          i++;
        } else if (src[i + 1] === "/") {
          const at = i;
          i += 2;
          const qname = name();
          skipSpace();
          if (src[i] !== ">") fail(i >= src.length ? "unclosed token" : "not well-formed");
          i++;
          const top = open.pop();
          if (!top) fail("syntax error", at);
          if (top.qname !== qname) fail(`mismatched tag. Expected: </${top.qname}>.`, at + 2);
          parent = open.length ? open[open.length - 1].node : doc;
          scope = top.outer;
        } else {
          const at = i;
          i++;
          if (parent === doc && root) fail("junk after document element", at);
          const qname = name();
          const attrs = [];
          for (;;) {
            const before = i;
            skipSpace();
            if (src[i] === ">" || src.startsWith("/>", i)) break;
            if (i >= src.length) fail("unclosed token", at);
            if (i === before) fail("not well-formed");
            const nameAt = i;
            const aname = name();
            skipSpace();
            if (src[i] !== "=") fail("not well-formed");
            i++;
            skipSpace();
            const q = src[i];
            if (q !== "\"" && q !== "'") fail("not well-formed");
            const end = src.indexOf(q, i + 1);
            if (end < 0) fail("unclosed token", at);
            const raw = src.slice(i + 1, end);
            if (raw.includes("<")) fail("not well-formed", i + 1 + raw.indexOf("<"));
            if (attrs.some(a => a[0] === aname)) fail("duplicate attribute", nameAt);
            attrs.push([aname, unescape(raw, i + 1).replace(/[\t\n]/g, " ")]);
            i = end + 1;
          }
          const outer = scope;
          for (const [aname, value] of attrs) {
            if (aname === "xmlns" || aname.startsWith("xmlns:")) {
              if (scope === outer) scope = Object.create(outer);
              scope[aname === "xmlns" ? "" : aname.slice(6)] = value || null;
            }
          }
          let node;
          try {
            node = d.createElementNS.call(doc, resolve(qname, false, at), qname);
          } catch (e) {
            if (e instanceof XMLError) throw e;
            fail("not well-formed", at + 1);
          }
          for (const [aname, value] of attrs) {
            resolve(aname, true, at);
            d.setAttribute.call(node, aname, value);
          }
          add(node);
          if (parent === doc) root = node;
          if (src.startsWith("/>", i)) {
            i += 2;
            scope = outer;
          } else {
            i++;
            open.push({ node, qname, outer });
            parent = node;
          }
        }
      }
      if (open.length || !root) fail("no root element found", src.length);
    } catch (e) {
      if (!(e instanceof XMLError)) throw e;
      return parseXMLError(src, e.msg, e.at);
    }
    return doc;
  }
  // Gecko's document for XML that is not well-formed (nsExpatDriver::HandleError): its message,
  // the document's location, the line and column (1-based, counted in UTF-16 units), then the
  // line itself and a caret under the column.
  function parseXMLError(src, msg, at) {
    const doc = __mrtParseDocument("");
    const d = xmlDom;
    d.removeChild.call(doc, doc.documentElement);
    const lineStart = src.lastIndexOf("\n", at - 1) + 1;
    const lineEnd = src.indexOf("\n", at) < 0 ? src.length : src.indexOf("\n", at);
    const line = src.slice(0, lineStart).split("\n").length;
    const column = at - lineStart + 1;
    const err = d.createElementNS.call(doc, XML_PARSERERROR_NS, "parsererror");
    d.appendChild.call(err, d.createTextNode.call(doc,
      `XML Parsing Error: ${msg}\nLocation: ${global.document.URL}\nLine Number ${line}, Column ${column}:`));
    const source = d.createElementNS.call(doc, XML_PARSERERROR_NS, "sourcetext");
    d.appendChild.call(source, d.createTextNode.call(doc, src.slice(lineStart, lineEnd) + "\n" + "-".repeat(column - 1) + "^"));
    d.appendChild.call(err, source);
    d.appendChild.call(doc, err);
    return doc;
  }

  // Interfaces made on first use, as a Firefox window makes its interface objects (its resolve hook,
  // WebIDLGlobalNameHash): a page that never names them pays nothing for them (GAPS D9). The names
  // are on the global from the start (`in` and `typeof` see them); reading one runs `make`, which
  // defines the whole group.
  const lazyGlobals = (names, make) => {
    let made = false;
    const ensure = () => { if (!made) { made = true; make(); } };
    for (const name of names) {
      Object.defineProperty(global, name, {
        configurable: true,
        enumerable: false,
        get() { ensure(); return global[name]; },
        set(v) { ensure(); define(global, name, v); },
      });
    }
    return ensure;
  };

  lazyGlobals(["ReadableStream", "ReadableStreamDefaultReader", "ReadableStreamDefaultController", "WritableStream",
    "WritableStreamDefaultWriter", "WritableStreamDefaultController", "TransformStream", "TransformStreamDefaultController",
    "CountQueuingStrategy", "ByteLengthQueuingStrategy", "TextEncoder", "TextDecoder", "TextDecoderStream", "TextEncoderStream",
    "CompressionStream", "DecompressionStream"], () => {
    // ---- streams (dom/streams, the WHATWG Streams standard) ----
    //
    // ReadableStream with its default controller and reader, WritableStream, TransformStream, pipes,
    // tee and async iteration, by the standard's algorithms (Gecko's dom/streams follows them with
    // the same names). Not built (GAPS B27): byte streams and BYOB readers, abort signals on pipes,
    // and backpressure between a transform's two sides (a write goes straight to the transform).
    const streamError = msg => new TypeError(msg);
    const settled = p => { p.catch(() => {}); return p; };
    const newPromise = () => { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };
    const sizeOf = strategy => (strategy && typeof strategy.size === "function" ? strategy.size : () => 1);
    const highWaterMark = (strategy, d) => {
      const h = strategy && strategy.highWaterMark !== undefined ? Number(strategy.highWaterMark) : d;
      if (Number.isNaN(h) || h < 0) throw new RangeError("Invalid highWaterMark");
      return h;
    };

    // ReadableStreamClose, ReadableStreamError
    function readableStreamClose(stream) {
      stream._state = "closed";
      const reader = stream._reader;
      if (!reader) return;
      reader._closed.resolve(undefined);
      for (const r of reader._readRequests.splice(0)) r.close();
    }
    function readableStreamError(stream, e) {
      stream._state = "errored";
      stream._storedError = e;
      const reader = stream._reader;
      if (!reader) return;
      reader._closed.reject(e);
      settled(reader._closed.promise);
      for (const r of reader._readRequests.splice(0)) r.error(e);
    }
    // ReadableStreamCancel
    function readableStreamCancel(stream, reason) {
      stream._disturbed = true;
      if (stream._state === "closed") return Promise.resolve(undefined);
      if (stream._state === "errored") return Promise.reject(stream._storedError);
      readableStreamClose(stream);
      return stream._controller._cancelSteps(reason).then(() => undefined);
    }

    class ReadableStreamDefaultController {
      constructor(stream, source, strategy) {
        this._stream = stream;
        this._source = source;
        this._queue = [];
        this._queueTotalSize = 0;
        this._strategySize = sizeOf(strategy);
        this._strategyHWM = highWaterMark(strategy, 1);
        this._started = this._pulling = this._pullAgain = this._closeRequested = false;
      }
      get desiredSize() {
        const state = this._stream._state;
        return state === "errored" ? null : state === "closed" ? 0 : this._strategyHWM - this._queueTotalSize;
      }
      _canCloseOrEnqueue() { return !this._closeRequested && this._stream._state === "readable"; }
      close() {
        if (!this._canCloseOrEnqueue()) throw streamError("ReadableStreamDefaultController.close: The stream is not in a state that permits close");
        this._closeRequested = true;
        if (this._queue.length === 0) { this._clearAlgorithms(); readableStreamClose(this._stream); }
      }
      enqueue(chunk) {
        if (!this._canCloseOrEnqueue()) throw streamError("ReadableStreamDefaultController.enqueue: Enqueue is not permitted");
        const reader = this._stream._reader;
        if (reader && reader._readRequests.length > 0) {
          reader._readRequests.shift().chunk(chunk);
        } else {
          let size;
          try { size = this._strategySize(chunk); } catch (e) { this._error(e); throw e; }
          this._queue.push({ value: chunk, size });
          this._queueTotalSize += size;
        }
        this._callPullIfNeeded();
      }
      error(e) { this._error(e); }
      _error(e) {
        if (this._stream._state !== "readable") return;
        this._queue = [];
        this._queueTotalSize = 0;
        this._clearAlgorithms();
        readableStreamError(this._stream, e);
      }
      _clearAlgorithms() { this._source = {}; }
      _shouldCallPull() {
        if (!this._canCloseOrEnqueue() || !this._started) return false;
        const reader = this._stream._reader;
        if (reader && reader._readRequests.length > 0) return true;
        return this.desiredSize > 0;
      }
      _callPullIfNeeded() {
        if (!this._shouldCallPull()) return;
        if (this._pulling) { this._pullAgain = true; return; }
        this._pulling = true;
        const source = this._source;
        let result;
        try { result = typeof source.pull === "function" ? source.pull.call(source, this) : undefined; } catch (e) { this._error(e); return; }
        Promise.resolve(result).then(() => {
          this._pulling = false;
          if (this._pullAgain) { this._pullAgain = false; this._callPullIfNeeded(); }
        }, e => this._error(e));
      }
      _start() {
        const source = this._source;
        let result;
        try { result = typeof source.start === "function" ? source.start.call(source, this) : undefined; } catch (e) { this._error(e); throw e; }
        Promise.resolve(result).then(() => { this._started = true; this._callPullIfNeeded(); }, e => this._error(e));
      }
      _cancelSteps(reason) {
        this._queue = [];
        this._queueTotalSize = 0;
        const source = this._source;
        let result;
        try { result = typeof source.cancel === "function" ? source.cancel.call(source, reason) : undefined; } catch (e) { return Promise.reject(e); }
        this._clearAlgorithms();
        return Promise.resolve(result);
      }
      _pullSteps(readRequest) {
        if (this._queue.length > 0) {
          const { value, size } = this._queue.shift();
          this._queueTotalSize = Math.max(0, this._queueTotalSize - size);
          if (this._closeRequested && this._queue.length === 0) { this._clearAlgorithms(); readableStreamClose(this._stream); }
          else this._callPullIfNeeded();
          readRequest.chunk(value);
        } else {
          this._stream._reader._readRequests.push(readRequest);
          this._callPullIfNeeded();
        }
      }
    }

    class ReadableStreamDefaultReader {
      constructor(stream) {
        if (!(stream instanceof ReadableStream)) throw streamError("ReadableStreamDefaultReader constructor: Argument 1 does not implement interface ReadableStream.");
        if (stream._reader) throw streamError("ReadableStreamDefaultReader constructor: Cannot create a new reader for a readable stream already locked by another reader.");
        this._stream = stream;
        this._readRequests = [];
        stream._reader = this;
        this._closed = newPromise();
        if (stream._state === "closed") this._closed.resolve(undefined);
        else if (stream._state === "errored") { this._closed.reject(stream._storedError); settled(this._closed.promise); }
      }
      get closed() { return this._closed.promise; }
      read() {
        const stream = this._stream;
        if (!stream) return Promise.reject(streamError("ReadableStreamDefaultReader.read: Reader is released"));
        stream._disturbed = true;
        if (stream._state === "closed") return Promise.resolve({ done: true, value: undefined });
        if (stream._state === "errored") return Promise.reject(stream._storedError);
        return new Promise((resolve, reject) => stream._controller._pullSteps({
          chunk: value => resolve({ done: false, value }),
          close: () => resolve({ done: true, value: undefined }),
          error: reject,
        }));
      }
      cancel(reason) {
        if (!this._stream) return Promise.reject(streamError("ReadableStreamDefaultReader.cancel: Reader is released"));
        return readableStreamCancel(this._stream, reason);
      }
      releaseLock() {
        const stream = this._stream;
        if (!stream) return;
        const e = streamError("Releasing lock");
        if (stream._state === "readable") this._closed.reject(e);
        else this._closed = { promise: Promise.reject(e) };
        settled(this._closed.promise);
        for (const r of this._readRequests.splice(0)) r.error(e);
        stream._reader = null;
        this._stream = null;
      }
    }

    class ReadableStream {
      constructor(source = undefined, strategy = {}) {
        if (source === null) throw streamError("ReadableStream constructor: Value can't be converted to a dictionary.");
        source = source === undefined ? {} : source;
        if (source.type !== undefined && String(source.type) === "bytes") global.__mrt.stub("ReadableStream(bytes)");
        this._state = "readable";
        this._reader = null;
        this._storedError = undefined;
        this._disturbed = false;
        this._controller = new ReadableStreamDefaultController(this, source, strategy);
        this._controller._start();
      }
      get locked() { return !!this._reader; }
      getReader(options = {}) {
        if (options && options.mode !== undefined) {
          if (String(options.mode) !== "byob") throw streamError("ReadableStream.getReader: 'mode' member of ReadableStreamGetReaderOptions is not a valid value.");
          throw streamError("ReadableStream.getReader: Cannot get a BYOB reader for a stream that is not a byte stream.");
        }
        return new ReadableStreamDefaultReader(this);
      }
      cancel(reason) {
        if (this._reader) return Promise.reject(streamError("ReadableStream.cancel: Cannot cancel a stream locked by a reader."));
        return readableStreamCancel(this, reason);
      }
      pipeThrough(transform, options = {}) {
        if (this._reader) throw streamError("ReadableStream.pipeThrough: Cannot pipe from a locked stream.");
        if (transform.writable._writer) throw streamError("ReadableStream.pipeThrough: Cannot pipe to a locked stream.");
        settled(readableStreamPipeTo(this, transform.writable, options));
        return transform.readable;
      }
      pipeTo(dest, options = {}) {
        if (this._reader) return Promise.reject(streamError("ReadableStream.pipeTo: Cannot pipe from a locked stream."));
        if (!(dest instanceof WritableStream)) return Promise.reject(streamError("ReadableStream.pipeTo: Argument 1 does not implement interface WritableStream."));
        if (dest._writer) return Promise.reject(streamError("ReadableStream.pipeTo: Cannot pipe to a locked stream."));
        return readableStreamPipeTo(this, dest, options);
      }
      // ReadableStreamDefaultTee: both branches get every chunk; a branch's pull reads from the source
      tee() {
        const reader = this.getReader();
        let reading = false, canceled1 = false, canceled2 = false, reason1, reason2, branch1, branch2;
        const cancelDone = newPromise();
        const pull = () => {
          if (reading) return Promise.resolve();
          reading = true;
          reader.read().then(({ value, done }) => {
            reading = false;
            if (done) {
              if (!canceled1) branch1._controller.close();
              if (!canceled2) branch2._controller.close();
              cancelDone.resolve(undefined);
              return;
            }
            if (!canceled1) branch1._controller.enqueue(value);
            if (!canceled2) branch2._controller.enqueue(value);
          }, () => { reading = false; });
          return Promise.resolve();
        };
        const cancel = which => reason => {
          if (which === 1) { canceled1 = true; reason1 = reason; } else { canceled2 = true; reason2 = reason; }
          if (canceled1 && canceled2) cancelDone.resolve(readableStreamCancel(this, [reason1, reason2]));
          return cancelDone.promise;
        };
        branch1 = new ReadableStream({ pull, cancel: cancel(1) });
        branch2 = new ReadableStream({ pull, cancel: cancel(2) });
        reader.closed.catch(e => { branch1._controller._error(e); branch2._controller._error(e); cancelDone.resolve(undefined); });
        return [branch1, branch2];
      }
      values(options = {}) {
        const reader = this.getReader();
        const preventCancel = !!(options && options.preventCancel);
        const iterator = {
          next() {
            if (!reader._stream) return Promise.resolve({ value: undefined, done: true });
            return reader.read().then(r => { if (r.done) reader.releaseLock(); return r; }, e => { reader.releaseLock(); throw e; });
          },
          return(value) {
            if (!reader._stream) return Promise.resolve({ value, done: true });
            if (!preventCancel) {
              const p = reader.cancel(value);
              reader.releaseLock();
              return p.then(() => ({ value, done: true }));
            }
            reader.releaseLock();
            return Promise.resolve({ value, done: true });
          },
          [Symbol.asyncIterator]() { return this; },
        };
        return iterator;
      }
      [Symbol.asyncIterator](options) { return this.values(options); }
      static from(iterable) {
        const it = iterable[Symbol.asyncIterator] ? iterable[Symbol.asyncIterator]() : iterable[Symbol.iterator]();
        return new ReadableStream({
          pull: c => Promise.resolve(it.next()).then(r => (r.done ? c.close() : c.enqueue(r.value))),
          cancel: reason => (typeof it.return === "function" ? Promise.resolve(it.return(reason)).then(() => undefined) : undefined),
        }, { highWaterMark: 0 });
      }
    }

    // ReadableStreamPipeTo, one chunk at a time
    function readableStreamPipeTo(source, dest, options) {
      const preventClose = !!options.preventClose, preventAbort = !!options.preventAbort, preventCancel = !!options.preventCancel;
      const reader = source.getReader();
      const writer = dest.getWriter();
      source._disturbed = true;
      const done = () => { writer.releaseLock(); reader.releaseLock(); };
      const step = () => reader.read().then(({ value, done: end }) => {
        if (end) return preventClose ? undefined : writer.close();
        return writer.write(value).then(step, e => { if (!preventCancel) settled(reader.cancel(e)); throw e; });
      }, e => { if (!preventAbort) settled(writer.abort(e)); throw e; });
      return step().then(() => { done(); }, e => { done(); throw e; });
    }

    class WritableStreamDefaultController {
      constructor(stream, sink, strategy) {
        this._stream = stream;
        this._sink = sink;
        this._queue = [];
        this._queueTotalSize = 0;
        this._strategySize = sizeOf(strategy);
        this._strategyHWM = highWaterMark(strategy, 1);
        this._started = false;
        this._abortController = global.AbortController ? new global.AbortController() : null;
      }
      get signal() { return this._abortController ? this._abortController.signal : undefined; }
      error(e) { if (this._stream._state === "writable") writableStreamError(this._stream, e); }
      get _desiredSize() { return this._strategyHWM - this._queueTotalSize; }
      // WritableStreamDefaultControllerAdvanceQueueIfNeeded: one write or the close at a time
      _advance() {
        const stream = this._stream;
        if (!this._started || stream._inFlight || stream._state === "errored" || this._queue.length === 0) return;
        const item = this._queue.shift();
        const sink = this._sink;
        stream._inFlight = item;
        let result;
        try {
          result = item.close ? (typeof sink.close === "function" ? sink.close.call(sink) : undefined)
            : (typeof sink.write === "function" ? sink.write.call(sink, item.chunk, this) : undefined);
        } catch (e) { result = Promise.reject(e); }
        Promise.resolve(result).then(() => {
          stream._inFlight = null;
          if (!item.close) this._queueTotalSize = Math.max(0, this._queueTotalSize - item.size);
          item.promise.resolve(undefined);
          if (item.close) {
            stream._state = "closed";
            if (stream._writer) stream._writer._closed.resolve(undefined);
            return;
          }
          this._advance();
        }, e => {
          stream._inFlight = null;
          item.promise.reject(e);
          writableStreamError(stream, e);
        });
      }
    }
    function writableStreamError(stream, e) {
      if (stream._state === "errored") return;
      stream._state = "errored";
      stream._storedError = e;
      const c = stream._controller;
      for (const item of c._queue.splice(0)) { item.promise.reject(e); settled(item.promise.promise); }
      if (stream._writer) { stream._writer._closed.reject(e); settled(stream._writer._closed.promise); }
    }

    class WritableStreamDefaultWriter {
      constructor(stream) {
        if (stream._writer) throw streamError("WritableStreamDefaultWriter constructor: Cannot create a new writer for a writable stream already locked by another writer.");
        this._stream = stream;
        stream._writer = this;
        this._closed = newPromise();
        if (stream._state === "closed") this._closed.resolve(undefined);
        else if (stream._state === "errored") { this._closed.reject(stream._storedError); settled(this._closed.promise); }
      }
      get closed() { return this._closed.promise; }
      get ready() { return this._stream && this._stream._state === "errored" ? Promise.reject(this._stream._storedError) : Promise.resolve(undefined); }
      get desiredSize() {
        if (!this._stream) throw streamError("WritableStreamDefaultWriter.desiredSize: Writer is released");
        const s = this._stream;
        return s._state === "errored" ? null : s._state === "closed" ? 0 : s._controller._desiredSize;
      }
      write(chunk) {
        const stream = this._stream;
        if (!stream) return Promise.reject(streamError("WritableStreamDefaultWriter.write: Writer is released"));
        if (stream._state === "errored") return Promise.reject(stream._storedError);
        if (stream._state !== "writable" || stream._closeRequested) return Promise.reject(streamError("WritableStreamDefaultWriter.write: Cannot write to a closing or closed stream"));
        const c = stream._controller;
        let size;
        try { size = c._strategySize(chunk); } catch (e) { writableStreamError(stream, e); return Promise.reject(e); }
        const promise = newPromise();
        c._queue.push({ chunk, size, promise });
        c._queueTotalSize += size;
        c._advance();
        return promise.promise;
      }
      close() {
        const stream = this._stream;
        if (!stream) return Promise.reject(streamError("WritableStreamDefaultWriter.close: Writer is released"));
        return writableStreamClose(stream);
      }
      abort(reason) {
        if (!this._stream) return Promise.reject(streamError("WritableStreamDefaultWriter.abort: Writer is released"));
        return writableStreamAbort(this._stream, reason);
      }
      releaseLock() {
        const stream = this._stream;
        if (!stream) return;
        const e = streamError("Releasing lock");
        this._closed = { promise: Promise.reject(e) };
        settled(this._closed.promise);
        stream._writer = null;
        this._stream = null;
      }
    }
    function writableStreamClose(stream) {
      if (stream._state !== "writable" || stream._closeRequested) return Promise.reject(streamError("Cannot close a stream that is closing, closed or errored"));
      stream._closeRequested = true;
      const promise = newPromise();
      stream._controller._queue.push({ close: true, size: 0, promise });
      stream._controller._advance();
      return promise.promise;
    }
    function writableStreamAbort(stream, reason) {
      if (stream._state === "closed" || stream._state === "errored") return Promise.resolve(undefined);
      const c = stream._controller;
      if (c._abortController) c._abortController.abort(reason);
      const sink = c._sink;
      writableStreamError(stream, reason);
      let result;
      try { result = typeof sink.abort === "function" ? sink.abort.call(sink, reason) : undefined; } catch (e) { return Promise.reject(e); }
      return Promise.resolve(result).then(() => undefined);
    }

    class WritableStream {
      constructor(sink = undefined, strategy = {}) {
        if (sink === null) throw streamError("WritableStream constructor: Value can't be converted to a dictionary.");
        sink = sink === undefined ? {} : sink;
        this._state = "writable";
        this._writer = null;
        this._inFlight = null;
        this._closeRequested = false;
        this._storedError = undefined;
        const c = this._controller = new WritableStreamDefaultController(this, sink, strategy);
        let result;
        try { result = typeof sink.start === "function" ? sink.start.call(sink, c) : undefined; } catch (e) { writableStreamError(this, e); throw e; }
        Promise.resolve(result).then(() => { c._started = true; c._advance(); }, e => writableStreamError(this, e));
      }
      get locked() { return !!this._writer; }
      getWriter() { return new WritableStreamDefaultWriter(this); }
      close() {
        if (this._writer) return Promise.reject(streamError("WritableStream.close: Cannot close a stream locked by a writer."));
        return writableStreamClose(this);
      }
      abort(reason) {
        if (this._writer) return Promise.reject(streamError("WritableStream.abort: Cannot abort a stream locked by a writer."));
        return writableStreamAbort(this, reason);
      }
    }

    class TransformStreamDefaultController {
      constructor(readableController, stream) { this._rc = readableController; this._stream = stream; }
      get desiredSize() { return this._rc.desiredSize; }
      enqueue(chunk) { this._rc.enqueue(chunk); }
      error(e) { this._rc._error(e); writableStreamError(this._stream.writable, e); }
      terminate() {
        if (this._rc._canCloseOrEnqueue()) this._rc.close();
        writableStreamError(this._stream.writable, streamError("The transform stream was terminated"));
      }
    }
    class TransformStream {
      constructor(transformer = undefined, writableStrategy = {}, readableStrategy = { highWaterMark: 0 }) {
        if (transformer === null) throw streamError("TransformStream constructor: Value can't be converted to a dictionary.");
        transformer = transformer === undefined ? {} : transformer;
        let rc;
        this.readable = new ReadableStream({
          start: c => { rc = c; },
          cancel: reason => (typeof transformer.cancel === "function" ? transformer.cancel.call(transformer, reason) : writableStreamError(this.writable, reason)),
        }, readableStrategy);
        const controller = new TransformStreamDefaultController(rc, this);
        let started;
        try { started = typeof transformer.start === "function" ? transformer.start.call(transformer, controller) : undefined; } catch (e) { started = Promise.reject(e); }
        this.writable = new WritableStream({
          start: () => started,
          // a transform that throws or rejects errors both sides (PerformTransform's TransformStreamError)
          write: chunk => {
            let result;
            try {
              result = Promise.resolve(typeof transformer.transform === "function" ? transformer.transform.call(transformer, chunk, controller) : controller.enqueue(chunk));
            } catch (e) {
              result = Promise.reject(e);
            }
            return result.catch(e => { rc._error(e); throw e; });
          },
          close: () => {
            let result;
            try { result = Promise.resolve(typeof transformer.flush === "function" ? transformer.flush.call(transformer, controller) : undefined); } catch (e) { result = Promise.reject(e); }
            return result.then(() => { if (rc._canCloseOrEnqueue()) rc.close(); }, e => { rc._error(e); throw e; });
          },
          abort: reason => { rc._error(reason); },
        }, writableStrategy);
      }
    }
    class CountQueuingStrategy {
      constructor({ highWaterMark: h }) { this.highWaterMark = Number(h); }
      get size() { return () => 1; }
    }
    class ByteLengthQueuingStrategy {
      constructor({ highWaterMark: h }) { this.highWaterMark = Number(h); }
      get size() { return chunk => chunk.byteLength; }
    }
    // for bridge.js's Response, which takes a stream as its body (no stream exists before this runs)
    global.__mrt.isReadableStream = v => v instanceof ReadableStream;
    for (const c of [ReadableStream, ReadableStreamDefaultReader, ReadableStreamDefaultController, WritableStream,
      WritableStreamDefaultWriter, WritableStreamDefaultController, TransformStream, TransformStreamDefaultController,
      CountQueuingStrategy, ByteLengthQueuingStrategy]) {
      define(global, c.name, c);
    }

    // ---- TextEncoder, TextDecoder and their streams (dom/encoding), UTF-8 only ----
    //
    // Firefox decodes every label encoding_rs knows; here every label decodes as UTF-8 (GAPS B27).
    // The decoder follows the Encoding standard's UTF-8 decoder: U+FFFD for each maximal invalid
    // subpart (or a TypeError when `fatal`), the BOM dropped unless `ignoreBOM`, and with `stream` an
    // incomplete sequence at the end kept for the next call.
    const utf8Encode = str => {
      const out = [];
      for (let i = 0; i < str.length; i++) {
        let c = str.charCodeAt(i);
        if (c >= 0xd800 && c <= 0xdbff && i + 1 < str.length) {
          const d = str.charCodeAt(i + 1);
          if (d >= 0xdc00 && d <= 0xdfff) { c = 0x10000 + ((c - 0xd800) << 10) + (d - 0xdc00); i++; }
        }
        if (c >= 0xd800 && c <= 0xdfff) c = 0xfffd;
        if (c < 0x80) out.push(c);
        else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
        else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
        else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
      }
      return new Uint8Array(out);
    };
    class TextEncoder {
      get encoding() { return "utf-8"; }
      encode(s = "") { return utf8Encode(String(s)); }
      encodeInto(s, dest) {
        const bytes = utf8Encode(String(s));
        const n = Math.min(bytes.length, dest.length);
        dest.set(bytes.subarray(0, n));
        return { read: String(s).length, written: n };
      }
    }
    const toBytes = input => {
      if (input === undefined) return new Uint8Array(0);
      if (input instanceof ArrayBuffer) return new Uint8Array(input);
      if (ArrayBuffer.isView(input)) return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
      throw new TypeError("TextDecoder.decode: Argument 1 could not be converted to any of: ArrayBufferView, ArrayBuffer.");
    };
    class TextDecoder {
      constructor(label = "utf-8", options = {}) {
        this._label = String(label).trim().toLowerCase();
        this.fatal = !!(options && options.fatal);
        this.ignoreBOM = !!(options && options.ignoreBOM);
        this._pending = [];
        this._bomSeen = false;
      }
      get encoding() { return "utf-8"; }
      decode(input, options = {}) {
        const stream = !!(options && options.stream);
        const fresh = toBytes(input);
        const bytes = this._pending.length ? Uint8Array.from([...this._pending, ...fresh]) : fresh;
        this._pending = [];
        let out = "", i = 0;
        const fail = () => { if (this.fatal) { this._pending = []; this._bomSeen = false; throw new TypeError("TextDecoder.decode: Decoding failed."); } out += "\ufffd"; };
        while (i < bytes.length) {
          const b = bytes[i];
          if (b < 0x80) { out += String.fromCharCode(b); i++; continue; }
          let need, lower = 0x80, upper = 0xbf, cp;
          if (b >= 0xc2 && b <= 0xdf) { need = 1; cp = b & 0x1f; }
          else if (b >= 0xe0 && b <= 0xef) { need = 2; cp = b & 0xf; if (b === 0xe0) lower = 0xa0; if (b === 0xed) upper = 0x9f; }
          else if (b >= 0xf0 && b <= 0xf4) { need = 3; cp = b & 7; if (b === 0xf0) lower = 0x90; if (b === 0xf4) upper = 0x8f; }
          else { fail(); i++; continue; }
          let j = 1;
          for (; j <= need; j++) {
            if (i + j >= bytes.length) break;
            const c = bytes[i + j];
            if (c < lower || c > upper) break;
            lower = 0x80; upper = 0xbf;
            cp = (cp << 6) | (c & 0x3f);
          }
          if (j > need) { out += String.fromCodePoint(cp); i += need + 1; continue; }
          if (i + j >= bytes.length && stream) { this._pending = [...bytes.subarray(i)]; break; }
          fail();
          i += j;
        }
        if (!this._bomSeen && out.length > 0) {
          this._bomSeen = true;
          if (!this.ignoreBOM && out.charCodeAt(0) === 0xfeff) out = out.slice(1);
        }
        if (!stream) { this._pending = []; this._bomSeen = false; }
        return out;
      }
    }
    define(global, "TextEncoder", TextEncoder);
    define(global, "TextDecoder", TextDecoder);
    class TextDecoderStream {
      constructor(label = "utf-8", options = {}) {
        const decoder = new TextDecoder(label, options);
        const t = new TransformStream({
          transform(chunk, c) { const s = decoder.decode(chunk, { stream: true }); if (s) c.enqueue(s); },
          flush(c) { const s = decoder.decode(); if (s) c.enqueue(s); },
        });
        Object.assign(this, { encoding: decoder.encoding, fatal: decoder.fatal, ignoreBOM: decoder.ignoreBOM, readable: t.readable, writable: t.writable });
      }
    }
    class TextEncoderStream {
      constructor() {
        let high = "";
        const t = new TransformStream({
          transform(chunk, c) {
            let s = high + String(chunk);
            high = "";
            const last = s.charCodeAt(s.length - 1);
            if (last >= 0xd800 && last <= 0xdbff) { high = s.slice(-1); s = s.slice(0, -1); }
            if (s) c.enqueue(utf8Encode(s));
          },
          flush(c) { if (high) c.enqueue(utf8Encode("\ufffd")); },
        });
        Object.assign(this, { encoding: "utf-8", readable: t.readable, writable: t.writable });
      }
    }
    define(global, "TextDecoderStream", TextDecoderStream);
    define(global, "TextEncoderStream", TextEncoderStream);

    // ---- CompressionStream and DecompressionStream (dom/compression) ----
    //
    // CompressionStream::Constructor and DecompressionStream::Constructor: a TransformStream over
    // the format's algorithms (__mrtCompression, the C++ layer's translation of FormatZlib.cpp and
    // FormatBrotli.cpp), with BaseAlgorithms.cpp's transform and flush around them. "zstd" is in
    // the WebIDL enumeration but refused, as release Firefox refuses it with its pref off.
    const COMPRESSION_FORMATS = ["deflate", "deflate-raw", "gzip", "brotli"];
    // a chunk as WebIDL's BufferSource (OwningBufferSource::Init): the bytes of a view or a buffer
    const bufferSourceBytes = chunk => {
      if (chunk instanceof ArrayBuffer) return new Uint8Array(chunk);
      if (ArrayBuffer.isView(chunk)) return new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
      throw new TypeError("Value could not be converted to any of: ArrayBufferView, ArrayBuffer.");
    };
    const compressionInterface = (name, decompress) => {
      const streams = new WeakMap();
      const C = { [name]: function (format) {
        if (!new.target) throw new TypeError(`${name} constructor: 'new' is required`);
        if (arguments.length < 1) throw new TypeError(`${name} constructor: At least 1 argument required, but only 0 passed`);
        const f = String(format);
        if (!COMPRESSION_FORMATS.includes(f)) throw new TypeError(`${name} constructor: '${f}' (value of argument 1) is not a valid value for enumeration CompressionFormat.`);
        const handle = __mrtCompression(0, COMPRESSION_FORMATS.indexOf(f), decompress);
        // the algorithm's Compress or Decompress: [chunks, fullyConsumed, observedStreamEnd]
        const step = (bytes, flush) => {
          const r = __mrtCompression(1, handle, bytes, flush);
          if (typeof r === "string") throw new TypeError(r);
          return r;
        };
        streams.set(this, new TransformStream({
          // TransformCallbackImpl
          transform(chunk, controller) {
            const [chunks, fullyConsumed, observedStreamEnd] = step(bufferSourceBytes(chunk), false);
            for (const c of chunks) controller.enqueue(c);
            if (decompress && observedStreamEnd && !fullyConsumed) throw new TypeError("Unexpected input after the end of stream");
          },
          // FlushCallbackImpl
          flush(controller) {
            const [chunks, , observedStreamEnd] = step(undefined, true);
            for (const c of chunks) controller.enqueue(c);
            if (decompress && !observedStreamEnd) throw new TypeError("The input is ended without reaching the stream end");
          },
        }));
      } }[name];
      // GenericTransformStream's attributes
      for (const member of ["readable", "writable"]) {
        const { get } = Object.getOwnPropertyDescriptor({
          get [member]() {
            const stream = this !== null && typeof this === "object" ? streams.get(this) : undefined;
            if (!stream) throw new TypeError(`'get ${member}' called on an object that does not implement interface ${name}.`);
            return stream[member];
          },
        }, member);
        Object.defineProperty(C.prototype, member, { get, configurable: true, enumerable: true });
      }
      Object.defineProperty(C.prototype, Symbol.toStringTag, { value: name, configurable: true });
      define(global, name, C);
    };
    compressionInterface("CompressionStream", false);
    compressionInterface("DecompressionStream", true);
  });

  const ensureAnimations = lazyGlobals(["KeyframeEffect", "Animation"], () => {
    // ---- Element.animate (dom/animation): the timing, and some properties' values ----
    //
    // What a page can see of an animation's timing is kept: playState, currentTime on the runtime's
    // clock, the finished promise and the finish and cancel events, for an animation of finite
    // length. Nothing is rendered; getComputedStyle reads the values of color, opacity and transform
    // as the animation has them at its current time (animatedStyle, below), other properties keep
    // their style (GAPS B27).
    const timingOf = options => {
      const o = typeof options === "number" ? { duration: options } : Object(options || {});
      const n = (v, d) => (v === undefined ? d : Number(v));
      return { delay: n(o.delay, 0), endDelay: n(o.endDelay, 0), duration: o.duration === undefined || o.duration === "auto" ? 0 : Number(o.duration),
        iterations: n(o.iterations, 1), fill: o.fill || "auto", easing: o.easing || "linear", direction: o.direction || "normal", iterationStart: n(o.iterationStart, 0) };
    };
    class KeyframeEffect {
      constructor(target, keyframes, options) {
        this.target = target;
        this._keyframes = keyframes;
        this._timing = timingOf(options);
        this.composite = (options && options.composite) || "replace";
        this.pseudoElement = (options && options.pseudoElement) || null;
      }
      getTiming() { return Object.assign({}, this._timing); }
      getComputedTiming() {
        const t = this._timing;
        const activeDuration = t.duration * t.iterations;
        return Object.assign({}, t, { activeDuration, endTime: Math.max(0, t.delay + activeDuration + t.endDelay) });
      }
      updateTiming(timing) { Object.assign(this._timing, timingOf(Object.assign({}, this._timing, timing))); }
      getKeyframes() { return Array.isArray(this._keyframes) ? this._keyframes.map(k => Object.assign({}, k)) : []; }
      setKeyframes(k) { this._keyframes = k; }
    }
    class Animation extends EventTarget {
      constructor(effect = null) {
        super();
        this.effect = effect;
        this.id = "";
        this.onfinish = null;
        this.oncancel = null;
        this.onremove = null;
        this.playbackRate = 1;
        this._replaceState = "active";
        this._state = "idle";
        this._start = null;
        this._held = null;
        this._timer = null;
        this._newFinished();
        this.ready = Promise.resolve(this);
      }
      _newFinished() {
        this.finished = new Promise((resolve, reject) => { this._resolve = resolve; this._reject = reject; });
        this.finished.catch(() => {});
      }
      get playState() { return this._state; }
      get replaceState() { return this._replaceState; }
      get pending() { return false; }
      get startTime() { return this._state === "running" ? this._start : null; }
      get currentTime() {
        if (this._state === "idle") return null;
        if (this._held !== null) return this._held;
        return (clock.now - this._start) * this.playbackRate;
      }
      set currentTime(t) { this._held = Number(t); if (this._state === "running") { this._start = clock.now - this._held; this._held = null; this._schedule(); } }
      _end() { return this.effect ? this.effect.getComputedTiming().endTime : 0; }
      _schedule() {
        if (this._timer !== null) timers.delete(this._timer);
        this._timer = null;
        const end = this._end();
        if (!isFinite(end) || this.playbackRate <= 0) return;
        const left = Math.max(0, (end - this.currentTime) / this.playbackRate);
        this._timer = addTimer(() => { this._timer = null; this.finish(); }, left, [], false);
      }
      play() {
        if (this._state === "finished") { this._held = 0; this._newFinished(); }
        const from = this._held !== null ? this._held : 0;
        this._state = "running";
        this._held = null;
        this._start = clock.now - from;
        animations.add(this);
        this._schedule();
      }
      pause() {
        if (this._state === "idle" || this._state === "running") {
          this._held = this._state === "idle" ? 0 : this.currentTime;
          this._state = "paused";
          if (this._timer !== null) { timers.delete(this._timer); this._timer = null; }
        }
      }
      finish() {
        if (this._timer !== null) { timers.delete(this._timer); this._timer = null; }
        this._held = this._end();
        this._state = "finished";
        // a finished animation is no longer in effect unless it fills forwards, so getAnimations()
        // leaves it out
        const fill = this.effect ? this.effect._timing.fill : "auto";
        if (fill !== "forwards" && fill !== "both") animations.delete(this);
        this._resolve(this);
        // the finish event is a task of its own (Animation::QueuePlaybackEvent), after the promise's
        // reactions
        const event = new Event("finish");
        event.currentTime = this._held;
        event.timelineTime = clock.now;
        addTimer(() => this.dispatchEvent(event), 0, [], false);
      }
      cancel() {
        if (this._state === "idle") return;
        if (this._timer !== null) { timers.delete(this._timer); this._timer = null; }
        if (this._state !== "finished") {
          const message = "The operation was aborted. ";
          this._reject(new DOMException(message, "AbortError"));
        }
        this._state = "idle";
        this._held = null;
        this._start = null;
        animations.delete(this);
        this._newFinished();
        addTimer(() => this.dispatchEvent(new Event("cancel")), 0, [], false);
      }
      reverse() { this.playbackRate = -this.playbackRate; }
      updatePlaybackRate(r) { this.playbackRate = Number(r); }
      persist() { this._replaceState = "persisted"; }
      commitStyles() {}
    }
    define(global, "KeyframeEffect", KeyframeEffect);
    define(global, "Animation", Animation);
  });
  const animations = share("animations", () => new Set());

  // An animation's value of a property at its current time, as Gecko's KeyframeEffect composes it
  // (dom/animation: the active phase and the fill, iterations, direction, the effect's easing, then
  // the keyframe segment and its easing), for the properties built here: colors, opacity and a
  // transform whose two ends have the same functions (rotate, translate, scale and their axes),
  // serialized as Gecko's getComputedStyle does (rgb()/rgba(), matrix() to 6 significant digits).
  // null when no animation of the element changes the property, or its values are not built here.
  // X's search needs it: its x-client-transaction-id hashes the color and transform of a paused
  // animation (task 0e92).
  const ANIMATED = new Set(["color", "background-color", "border-color", "opacity", "transform"]);
  const camelOf = name => name.replace(/-([a-z])/g, (m, c) => c.toUpperCase());
  const bezier = (x1, y1, x2, y2) => {
    // Servo's style::bezier: Newton's method on x, bisection when it does not converge
    const cx = 3 * x1, bx = 3 * (x2 - x1) - cx, ax = 1 - cx - bx;
    const cy = 3 * y1, by = 3 * (y2 - y1) - cy, ay = 1 - cy - by;
    const X = t => ((ax * t + bx) * t + cx) * t, Y = t => ((ay * t + by) * t + cy) * t;
    const dX = t => (3 * ax * t + 2 * bx) * t + cx;
    return x => {
      // (outside 0..1, where Servo extrapolates along the end tangents, the ends are kept)
      if (x <= 0 || x >= 1) return x <= 0 ? 0 : 1;
      let t = x;
      for (let i = 0; i < 8; i++) {
        const e = X(t) - x;
        if (Math.abs(e) < 1e-7) return Y(t);
        const d = dX(t);
        if (Math.abs(d) < 1e-6) break;
        t -= e / d;
      }
      let lo = 0, hi = 1;
      t = x;
      while (lo < hi) {
        const v = X(t);
        if (Math.abs(v - x) < 1e-7) break;
        if (x > v) lo = t; else hi = t;
        const mid = (hi - lo) / 2 + lo;
        if (mid === t) break;
        t = mid;
      }
      return Y(t);
    };
  };
  const EASINGS = { linear: x => x, ease: bezier(0.25, 0.1, 0.25, 1), "ease-in": bezier(0.42, 0, 1, 1),
    "ease-out": bezier(0, 0, 0.58, 1), "ease-in-out": bezier(0.42, 0, 0.58, 1) };
  const easingOf = text => {
    text = String(text || "linear").trim();
    if (EASINGS[text]) return EASINGS[text];
    let m = /^cubic-bezier\(([^)]*)\)$/.exec(text);
    if (m) { const [a, b, c, d] = m[1].split(",").map(Number); return bezier(a, b, c, d); }
    m = /^steps\(\s*(\d+)\s*(?:,\s*([a-z-]+))?\s*\)$/.exec(text);
    if (m) {
      const n = Number(m[1]), pos = m[2] || "end";
      return x => {
        let step = Math.floor(x * n);
        if (pos === "start" || pos === "jump-start" || pos === "jump-both") step++;
        const jumps = pos === "jump-both" ? n + 1 : pos === "jump-none" ? n - 1 : n;
        return Math.max(0, Math.min(1, step / jumps));
      };
    }
    return EASINGS.linear;
  };
  // the keyframes as a list of {offset, easing, values by CSS name}, offsets filled in evenly
  const keyframeList = keyframes => {
    let list = [];
    if (Array.isArray(keyframes)) {
      list = keyframes.map(k => ({ offset: k.offset == null ? null : Number(k.offset), easing: k.easing, values: k }));
    } else if (keyframes && typeof keyframes === "object") {
      let n = 0;
      for (const [k, v] of Object.entries(keyframes)) if (k !== "offset" && k !== "easing" && k !== "composite") n = Math.max(n, Array.isArray(v) ? v.length : 1);
      for (let i = 0; i < n; i++) list.push({ offset: null, easing: null, values: {} });
      for (const [k, v] of Object.entries(keyframes)) {
        const values = Array.isArray(v) ? v : [v];
        values.forEach((value, i) => {
          if (k === "offset") list[i] && (list[i].offset = value == null ? null : Number(value));
          else if (k === "easing") list[i] && (list[i].easing = value);
          else if (k !== "composite" && values.length > 1) list[Math.round(i * (n - 1) / (values.length - 1))].values[k] = value;
          else if (k !== "composite") list[n - 1].values[k] = value;
        });
      }
      // a property-indexed easing given once is every keyframe's
      if (typeof keyframes.easing === "string") for (const k of list) k.easing = keyframes.easing;
    }
    if (list.length && list[0].offset == null) list[0].offset = list.length > 1 ? 0 : 1;
    if (list.length > 1 && list[list.length - 1].offset == null) list[list.length - 1].offset = 1;
    for (let i = 1; i < list.length - 1; i++) {
      if (list[i].offset != null) continue;
      let j = i;
      while (list[j].offset == null) j++;
      const from = list[i - 1].offset, to = list[j].offset, span = j - i + 1;
      for (let k = i; k < j; k++) list[k].offset = from + (to - from) * (k - i + 1) / span;
    }
    return list;
  };
  // the iteration progress after the effect's easing (Gecko's GetComputedTimingAt), or null outside
  // the effect's active interval without a fill
  const progressOf = (effect, localTime) => {
    const t = effect._timing;
    const duration = Math.max(0, t.duration), iterations = Math.max(0, t.iterations);
    const active = duration * iterations;
    const end = Math.max(0, t.delay + active + t.endDelay);
    const beforeEnd = Math.max(Math.min(t.delay, end), 0), activeEnd = Math.max(Math.min(t.delay + active, end), 0);
    const phase = localTime < beforeEnd ? "before" : localTime >= activeEnd ? "after" : "active";
    let activeTime;
    if (phase === "before") {
      if (t.fill !== "backwards" && t.fill !== "both") return null;
      activeTime = Math.max(localTime - t.delay, 0);
    } else if (phase === "after") {
      if (t.fill !== "forwards" && t.fill !== "both") return null;
      activeTime = Math.max(Math.min(localTime - t.delay, active), 0);
    } else activeTime = localTime - t.delay;
    const overall = (duration === 0 ? (phase === "before" ? 0 : iterations) : activeTime / duration) + t.iterationStart;
    let simple = isFinite(overall) ? overall % 1 : t.iterationStart % 1;
    if (simple === 0 && phase !== "before" && activeTime === active && iterations !== 0) simple = 1;
    const current = simple === 1 ? Math.floor(overall) - 1 : Math.floor(overall);
    const odd = Math.abs(current % 2) === 1;
    const forwards = t.direction === "normal" || (t.direction === "alternate" && !odd) || (t.direction === "alternate-reverse" && odd);
    return easingOf(t.easing)(forwards ? simple : 1 - simple);
  };
  // colors: #hex, rgb()/rgba() and a few names, as [r, g, b, a] in 0..1
  const NAMED = { black: "#000", white: "#fff", red: "#f00", green: "#008000", blue: "#00f", transparent: "#0000" };
  const parseColor = text => {
    text = NAMED[String(text).trim().toLowerCase()] || String(text).trim().toLowerCase();
    let m = /^#([0-9a-f]{3,8})$/.exec(text);
    if (m && [3, 4, 6, 8].includes(m[1].length)) {
      const h = m[1].length < 5 ? m[1].replace(/./g, c => c + c) : m[1];
      return [0, 2, 4, 6].map(i => (i < h.length ? parseInt(h.slice(i, i + 2), 16) / 255 : 1));
    }
    m = /^rgba?\(([^)]*)\)$/.exec(text);
    if (m) {
      const parts = m[1].split(/\s*[,/]\s*|\s+/).filter(Boolean);
      if (parts.length < 3) return null;
      const c = parts.slice(0, 3).map(v => (v.endsWith("%") ? parseFloat(v) / 100 : parseFloat(v) / 255));
      const a = parts[3] === undefined ? 1 : parts[3].endsWith("%") ? parseFloat(parts[3]) / 100 : parseFloat(parts[3]);
      return [...c, a].some(isNaN) ? null : [...c, a];
    }
    return null;
  };
  const clamp01 = x => Math.max(0, Math.min(1, x));
  const showNumber = x => { const v = Number(x.toPrecision(6)); return Object.is(v, -0) ? "0" : String(v); };
  const showColor = ([r, g, b, a]) => {
    const [R, G, B] = [r, g, b].map(c => Math.round(clamp01(c) * 255));
    a = clamp01(a);
    return a === 1 ? `rgb(${R}, ${G}, ${B})` : `rgba(${R}, ${G}, ${B}, ${showNumber(Math.round(a * 1000) / 1000)})`;
  };
  // premultiplied, as CSS Color 4 interpolates
  const mixColor = (a, b, p) => {
    const alpha = a[3] + (b[3] - a[3]) * p;
    if (alpha <= 0) return [0, 0, 0, 0];
    return [0, 1, 2].map(i => (a[i] * a[3] + (b[i] * b[3] - a[i] * a[3]) * p) / alpha).concat(alpha);
  };
  // transform functions: [name, numbers in px, deg or plain]
  const parseTransform = text => {
    text = String(text).trim();
    if (text === "none") return [];
    const out = [];
    const re = /([a-zA-Z0-9]+)\(([^)]*)\)\s*/y;
    let m, at = 0;
    while ((m = re.exec(text))) {
      at = re.lastIndex;
      const name = m[1].toLowerCase();
      const args = m[2].split(/\s*,\s*|\s+/).filter(Boolean).map(v => {
        const n = parseFloat(v);
        if (/deg$/.test(v)) return n;
        if (/grad$/.test(v)) return n * 0.9;
        if (/rad$/.test(v)) return n * 180 / Math.PI;
        if (/turn$/.test(v)) return n * 360;
        return n;
      });
      if (!["rotate", "rotatez", "translate", "translatex", "translatey", "scale", "scalex", "scaley"].includes(name) || args.some(isNaN)) return null;
      out.push([name, args]);
    }
    return at === text.length ? out : null;
  };
  const matrixOf = list => {
    let [a, b, c, d, e, f] = [1, 0, 0, 1, 0, 0];
    for (const [name, args] of list) {
      let m;
      if (name === "rotate" || name === "rotatez") {
        // Gecko: the angle a float in degrees, its radians a double, the sine and cosine floats
        // (StyleAngle, Matrix4x4::RotateZ)
        const r = Math.fround(args[0]) * Math.PI / 180, cos = Math.fround(Math.cos(r)), sin = Math.fround(Math.sin(r));
        m = [cos, sin, -sin, cos, 0, 0];
      } else if (name === "translate") m = [1, 0, 0, 1, args[0], args[1] || 0];
      else if (name === "translatex") m = [1, 0, 0, 1, args[0], 0];
      else if (name === "translatey") m = [1, 0, 0, 1, 0, args[0]];
      else if (name === "scale") m = [args[0], 0, 0, args.length > 1 ? args[1] : args[0], 0, 0];
      else if (name === "scalex") m = [args[0], 0, 0, 1, 0, 0];
      else m = [1, 0, 0, args[0], 0, 0];
      [a, b, c, d, e, f] = [a * m[0] + c * m[1], b * m[0] + d * m[1], a * m[2] + c * m[3], b * m[2] + d * m[3],
        a * m[4] + c * m[5] + e, b * m[4] + d * m[5] + f];
    }
    return `matrix(${[a, b, c, d, e, f].map(showNumber).join(", ")})`;
  };
  const interpolate = (prop, from, to, p) => {
    if (prop === "opacity") {
      const a = parseFloat(from), b = parseFloat(to);
      return isNaN(a) || isNaN(b) ? null : showNumber(clamp01(a + (b - a) * p));
    }
    if (prop === "transform") {
      const a = parseTransform(from), b = parseTransform(to);
      if (!a || !b) return null;
      if (!a.length && !b.length) return "none";
      // none against a list: the list's identity functions
      const ends = [a.length ? a : b.map(([n, args]) => [n, args.map(() => (n.startsWith("scale") ? 1 : 0))]),
        b.length ? b : a.map(([n, args]) => [n, args.map(() => (n.startsWith("scale") ? 1 : 0))])];
      if (ends[0].length !== ends[1].length || ends[0].some(([n], i) => n !== ends[1][i][0])) return null;
      return matrixOf(ends[0].map(([n, args], i) => [n, args.map((v, j) => v + ((ends[1][i][1][j] ?? v) - v) * p)]));
    }
    const a = parseColor(from), b = parseColor(to);
    return a && b ? showColor(mixColor(a, b, p)) : null;
  };
  function animatedStyle(elt, pseudo, name) {
    if (!ANIMATED.has(name) || !animations.size) return null;
    let value = null;
    // the last animation of the element that changes it wins (composite: replace)
    for (const animation of animations) {
      const effect = animation.effect;
      if (!effect || effect.target !== elt || (effect.pseudoElement || "") !== pseudo || animation.playState === "idle") continue;
      const time = animation.currentTime;
      if (time === null) continue;
      const progress = progressOf(effect, time);
      if (progress === null) continue;
      const frames = keyframeList(effect._keyframes).filter(k => k.values[name] !== undefined || k.values[camelOf(name)] !== undefined);
      if (frames.length < 2) continue;
      const valueOf = k => String(k.values[name] !== undefined ? k.values[name] : k.values[camelOf(name)]);
      // the segment the progress falls in (before the first or after the last: the end one)
      let i = 0;
      while (i < frames.length - 2 && progress >= frames[i + 1].offset) i++;
      const from = frames[i], to = frames[i + 1];
      const span = to.offset - from.offset;
      const local = span === 0 ? 0 : (progress - from.offset) / span;
      const v = interpolate(name, valueOf(from), valueOf(to), easingOf(from.easing)(local));
      if (v !== null) value = v;
    }
    return value;
  }
  if (!isWorker) define(Element.prototype, "animate", function (keyframes, options) {
    ensureAnimations();
    const animation = new global.Animation(new global.KeyframeEffect(this, keyframes, options));
    if (options && typeof options === "object" && options.id !== undefined) animation.id = String(options.id);
    animation.play();
    return animation;
  });
  if (!isWorker) define(Element.prototype, "getAnimations", function () {
    return [...animations].filter(a => a.effect && a.effect.target === this);
  });
  if (!isWorker) define(Document.prototype, "getAnimations", function () { return [...animations]; });

  const toNode = n => (typeof n === "object" && n !== null ? n : document.createTextNode(String(n)));
  if (!isWorker) for (const proto of [Element.prototype, Document.prototype, DocumentFragment.prototype]) {
    // one [CEReactions] method each: custom element reactions run once, at the end
    define(proto, "append", function (...nodes) { return __mrtCEReactions(function () { for (const n of nodes) this.appendChild(toNode(n)); }, this); });
    define(proto, "prepend", function (...nodes) {
      return __mrtCEReactions(function () {
        const first = this.firstChild;
        for (const n of nodes) this.insertBefore(toNode(n), first);
      }, this);
    });
    define(proto, "getElementsByTagName", function (tag) { return this.querySelectorAll(tag); });
    define(proto, "getElementsByClassName", function (names) {
      return this.querySelectorAll(names.trim().split(/\s+/).map(c => "." + CSS.escape(c)).join(""));
    });
  }
  if (!isWorker) for (const proto of [Element.prototype, CharacterData.prototype]) {
    define(proto, "remove", function () { if (this.parentNode) this.parentNode.removeChild(this); });
  }
  define(global, "CSS", { escape: s => String(s).replace(/[^a-zA-Z0-9_ -￿-]/g, c => "\\" + c) });

  // ---- URLs: one resolved against a base (bridge.js's URL class is built on these) ----

  const URL_RE = /^([a-zA-Z][a-zA-Z0-9+.-]*:)(?:\/\/(?:([^@/?#]*)@)?([^/?#:]*|\[[^\]]*\])(?::(\d*))?)?([^?#]*)(\?[^#]*)?(#.*)?$/;
  function removeDots(path) {
    const out = [];
    for (const seg of path.split("/")) {
      if (seg === "..") { if (out.length > 1) out.pop(); }
      else if (seg !== ".") out.push(seg);
    }
    const last = path.split("/").pop();
    if (last === "." || last === "..") out.push("");
    return out.join("/") || "/";
  }
  function resolve(input, base) {
    input = String(input).trim();
    if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(input)) return input;
    const b = URL_RE.exec(base);
    if (!b) throw new TypeError("Invalid URL: " + input);
    const [, proto, , host, port] = b;
    const origin = proto + "//" + host + (port ? ":" + port : "");
    if (input.startsWith("//")) return proto + input;
    if (input.startsWith("/")) return origin + removeDots(input.replace(/[?#].*/, "")) + (input.match(/[?#].*/) || [""])[0];
    if (input.startsWith("?")) return origin + b[5] + input;
    if (input.startsWith("#")) return origin + b[5] + (b[6] || "") + input;
    if (input === "") return base.replace(/#.*/, "");
    const dir = b[5].replace(/[^/]*$/, "");
    const m = input.match(/^([^?#]*)(.*)$/);
    return origin + removeDots(dir + m[1]) + m[2];
  }

  // ---- module scripts: the script element's side (Gecko's ScriptLoader for type=module) ----
  //
  // The C++ layer fetches, links and evaluates a module graph (__mrtModuleScript, Gecko's
  // ModuleLoaderBase). It asks __mrt.fetchModule for each module's source and __mrt.resolveURL for
  // each specifier; here a fetch answers data: URLs only, bridge.js forwards the others to Firefox.

  // nsContentUtils::IsJavascriptMIMEType and IsJsonMimeType, on the type's essence
  const JS_MIME_TYPES = new Set(["text/javascript", "text/ecmascript", "application/javascript", "application/ecmascript",
    "application/x-javascript", "application/x-ecmascript", "text/javascript1.0", "text/javascript1.1",
    "text/javascript1.2", "text/javascript1.3", "text/javascript1.4", "text/javascript1.5", "text/jscript",
    "text/livescript", "text/x-ecmascript", "text/x-javascript"]);
  // MimeTypeMatchesExpectedModuleType: the response's type against the import's (javascript, json)
  function mimeTypeMatches(contentType, moduleType) {
    const essence = String(contentType || "").split(";")[0].trim().toLowerCase();
    if (moduleType === "json") return essence === "application/json" || essence === "text/json" || /\/[^/]*\+json$/.test(essence);
    return JS_MIME_TYPES.has(essence);
  }
  // A data: URL's MIME type and text (fetch's data: URL processor; the bytes read as UTF-8), or
  // null when it is not one. Base64 needs atob (bridge.js).
  function readDataURL(url) {
    const m = /^data:([^,]*),([\s\S]*)$/i.exec(url);
    if (!m) return null;
    let type = m[1].trim(), bytes = m[2].replace(/%([0-9a-fA-F]{2})/g, (_, h) => String.fromCharCode(parseInt(h, 16)));
    if (/;\s*base64$/i.test(type)) {
      if (!global.atob) return null;
      type = type.replace(/;\s*base64$/i, "");
      try { bytes = global.atob(bytes); } catch (e) { return null; }
    }
    let text = bytes;
    try { text = decodeURIComponent(escape(bytes)); } catch (e) {}
    return { type: type || "text/plain;charset=US-ASCII", text };
  }
  // The document's base URL for module scripts (bridge.js: the page's URL)
  let documentBaseURL = "http://localhost/";
  // A <script type=module>: its graph fetched, linked and evaluated, then load at an external one, or
  // error when its graph could not be fetched (ScriptLoader::ProcessRequest and FireScriptAvailable).
  // Returns __mrtModuleScript's result: 0 evaluated, 1 not fetched, 2 threw; -1 when the element
  // had already started (ScriptElement's mAlreadyStarted).
  const alreadyStarted = share("alreadyStarted", () => new WeakSet());
  // The scripts the fragment parser (innerHTML, outerHTML, insertAdjacentHTML) or DOMParser made
  // never run, wherever they are inserted later: they are marked already started
  // (nsHtml5TreeOperation's PreventExecution). bridge.js's classic scripts keep their own mark ("ran").
  const ranScripts = share("ran", () => new WeakSet());
  function preventExecution(root) {
    for (const el of root.querySelectorAll("script")) {
      alreadyStarted.add(el);
      ranScripts.add(el);
    }
  }
  function runModuleScript(el) {
    if (alreadyStarted.has(el)) return -1;
    alreadyStarted.add(el);
    const src = el.getAttribute("src");
    let result;
    // the module's evaluation is page code (ScriptLoader::EvaluateScript's nsAutoMicroTask)
    if (src === null) {
      result = callPage(__mrtModuleScript, undefined, [global.__mrt.baseURL(), el.textContent]);
    } else {
      // an empty or unparsable src: the error event, nothing fetched
      const url = src.trim() === "" ? null : global.__mrt.resolveURL(src, global.__mrt.baseURL());
      result = url === null ? 1 : callPage(__mrtModuleScript, undefined, [url, null]);
    }
    if (result === 1) el.dispatchEvent(new Event("error"));
    else if (src !== null) el.dispatchEvent(new Event("load"));
    return result;
  }
  // A script element inserted by script is prepared once it is in the document (ScriptElement's
  // MaybeProcessScript, from its BindToTree): the inserted node itself or any script in its subtree,
  // in tree order once the insertion is done; one already started does not run again.
  // __mrt.insertedScript decides how it runs. Here only module scripts, each in a task of its own
  // (its graph is ready later in Gecko; an inline one, AsyncExecuteInlineModule); bridge.js also runs
  // classic ones. A <link> inserted by script starts its sheet's load (LinkStyle::BindToTree);
  // __mrt.insertedLink does nothing here, bridge.js fetches the sheet. The links inside a subtree
  // that was already in the document (a move) are left alone.
  const isModuleScript = el => (el.getAttribute("type") || "").trim().toLowerCase() === "module";
  if (!isWorker) for (const m of ["appendChild", "insertBefore"]) {
    const orig = Node.prototype[m];
    define(Node.prototype, m, function (node, ...rest) {
      const inserted = node && node.nodeType === 11 ? Array.from(node.childNodes) : [node];
      const moved = inserted.map(n => !!n && n.isConnected);
      const r = orig.call(this, node, ...rest);
      for (let i = 0; i < inserted.length; i++) {
        const n = inserted[i];
        if (!n || n.nodeType !== 1 || !n.isConnected) continue;
        if (n.tagName === "SCRIPT") global.__mrt.insertedScript(n);
        else if (n.tagName === "LINK") global.__mrt.insertedLink(n);
        else if (n.firstElementChild) {
          for (const el of n.querySelectorAll(moved[i] ? "script" : "script, link")) {
            if (el.tagName === "SCRIPT") global.__mrt.insertedScript(el);
            else global.__mrt.insertedLink(el);
          }
        }
      }
      return r;
    });
  }

  // ---- windows and frames (DESIGN, "Same-origin frames: the build plan") ----
  //
  // The relations between the runtime's windows come from the C++ layer (__mrtWindows), which keeps
  // each frame element's window (Gecko: the BrowsingContext tree, WindowProxy's indexed getter is
  // the C++ resolve hook's). A frame's window has no WindowProxy here: contentWindow is its global.
  const WINDOWS = { contentWindow: 0, contentDocument: 1, parent: 2, frameElement: 3, frames: 4, closed: 5, navigate: 6, top: 7 };
  const winGetter = (name, get) => Object.defineProperty(global, name, { get, set(v) { define(global, name, v); }, configurable: true, enumerable: true });
  winGetter("parent", () => __mrtWindows(WINDOWS.parent));
  winGetter("top", () => __mrtWindows(WINDOWS.top));
  winGetter("frameElement", () => __mrtWindows(WINDOWS.frameElement));
  winGetter("frames", () => global);
  winGetter("length", () => __mrtWindows(WINDOWS.frames).length);
  winGetter("closed", () => __mrtWindows(WINDOWS.closed, global));
  define(global, "opener", null);
  const frameProto = !isWorker && global.HTMLIFrameElement && global.HTMLIFrameElement.prototype;
  if (frameProto) {
    Object.defineProperty(frameProto, "contentWindow", { get() { return __mrtWindows(WINDOWS.contentWindow, this); }, configurable: true, enumerable: true });
    Object.defineProperty(frameProto, "contentDocument", { get() { return __mrtWindows(WINDOWS.contentDocument, this); }, configurable: true, enumerable: true });
  }

  // document.open, write and close (Document::Open, Write, Close), reduced: what is written is
  // parsed when the document is closed (Gecko parses each write as it comes: a script written in
  // pieces sees the document as it was; GAPS B54), as a whole document with scripting on, and its
  // nodes inserted into the document, so the frames in it load and its scripts run. Then the
  // document's load events, in a task. A write with no open document opens it.
  const writes = new WeakMap();
  if (!isWorker) define(Document.prototype, "open", function () {
    if (writes.has(this)) return this;
    while (this.firstChild) this.removeChild(this.firstChild);
    writes.set(this, []);
    return this;
  });
  if (!isWorker) define(Document.prototype, "write", function (...text) {
    if (!writes.has(this)) this.open();
    writes.get(this).push(text.join(""));
  });
  if (!isWorker) define(Document.prototype, "writeln", function (...text) { this.write(text.join("") + "\n"); });
  if (!isWorker) define(Document.prototype, "close", function () {
    const parts = writes.get(this);
    if (!parts) return;
    writes.delete(this);
    const parsed = __mrtParseDocument(parts.join(""), true);
    // the compatibility mode of what was written (its doctype or none)
    __mrtWindows(8, this, parsed);
    for (const c of Array.from(parsed.childNodes)) this.appendChild(c);
    const doc = this;
    global.__mrt.queueTask(() => {
      if (doc !== global.document) return;
      global.__mrt.setReadyState("complete");
      fireWindowLoad();
      const frame = global.frameElement;
      if (frame) frame.dispatchEvent(new Event("load"));
    });
  });

  // A src that leaves a frame at its initial about:blank: about:blank (with a query or fragment) and
  // a javascript: URL (Gecko evaluates it in the frame; a result that is not a string leaves the
  // document as it is; the evaluation is not done here, GAPS B52)
  const isInitialBlank = src => /^\s*(about:blank([?#].*)?|javascript:.*)\s*$/is.test(src);

  // A frame element inserted into a document with a window got its window (the C++ layer's
  // FramesInserted): its initial about:blank's load event at once, as Gecko fires it; a srcdoc or a
  // src other than about:blank is loaded in a task (bridge.js fetches a src).
  // A frame that loads blocks its parent document's load event from now until its own load (the
  // frame's docshell blocks it when its load starts, nsDocShell); loadFrame is handed the unblocking.
  function frameInserted(el) {
    const srcdoc = el.getAttribute("srcdoc");
    const src = el.getAttribute("src");
    if (srcdoc !== null) {
      const unblock = global.__mrt.blockOnload("frame " + (srcdoc !== null ? "srcdoc" : src));
      global.__mrt.queueTask(() => global.__mrt.loadFrame(el, srcdoc, "about:srcdoc", unblock));
    } else if (src !== null && src.trim() !== "" && !isInitialBlank(src)) {
      const unblock = global.__mrt.blockOnload("frame " + (srcdoc !== null ? "srcdoc" : src));
      global.__mrt.queueTask(() => global.__mrt.loadFrame(el, null, src, unblock));
    } else {
      el.dispatchEvent(new Event("load"));
    }
  }
  // A frame in the document whose src or srcdoc changed navigates, in a task (LoadSrc from
  // AfterSetAttr): srcdoc first, else a src other than about:blank; nothing for about:blank (GAPS B52).
  function frameSrcChanged(el) {
    const srcdoc = el.getAttribute("srcdoc");
    const src = el.getAttribute("src");
    if (srcdoc !== null) {
      const unblock = global.__mrt.blockOnload("frame " + (srcdoc !== null ? "srcdoc" : src));
      global.__mrt.queueTask(() => global.__mrt.loadFrame(el, srcdoc, "about:srcdoc", unblock));
    } else if (src !== null && src.trim() !== "" && !isInitialBlank(src)) {
      const unblock = global.__mrt.blockOnload("frame " + (srcdoc !== null ? "srcdoc" : src));
      global.__mrt.queueTask(() => global.__mrt.loadFrame(el, null, src, unblock));
    }
  }
  // A frame's document from its HTML (srcdoc) or its URL (bridge.js fetches it): the frame's window
  // takes the new document, sets its URL and runs its scripts; once the frame's window has its load
  // event, the frame element gets its own, and the parent document's load is unblocked.
  function loadFrame(el, html, url, unblock = () => {}) {
    if (html === null) { unblock(); return; }
    const doc = __mrtWindows(WINDOWS.navigate, el, html);
    const win = el.contentWindow;
    if (!doc || !win) { unblock(); return; }
    win.__mrt.frameNavigated(url);
    win.__mrt.runDocument(() => {
      el.dispatchEvent(new Event("load"));
      unblock();
    });
  }

  // ---- Trusted Types (Gecko's dom/security/trusted-types) ----
  //
  // The three types, policies and the factory, in windows and workers ([Exposed=(Window,Worker)]).
  // Each Trusted object's type and string are in `trustedData` (Gecko's mData), so the sinks and the
  // eval hooks can read them before the interfaces are made, in any window of the runtime. The document's CSP (bridge.js gives it,
  // as Firefox parsed it: `setPolicies`) decides which policy names may be made (`trusted-types`) and
  // whether the sinks take strings (`require-trusted-types-for 'script'`); bridge.js wraps the sinks
  // in a window that requires Trusted objects. Not built (GAPS B91): a worker's CSP, the violation
  // reports sent to a report-uri.
  // (shared by the runtime's windows: a frame's sink takes the parent's Trusted objects, as Gecko
  // unwraps them across realms)
  const trustedData = share("trustedData", () => new WeakMap());
  const TRUSTED_TYPE_NAMES = ["TrustedHTML", "TrustedScript", "TrustedScriptURL"];
  // the document's policies with a Trusted Types directive: [{ reportOnly, require, names }] (`names`:
  // the `trusted-types` values, null without that directive)
  let ttPolicies = [];
  let ttFactory = null;
  const ttCreatedNames = [];
  // a script the runtime runs itself (a page's <script>, a string timer): the next eval is not checked
  let ttEvalExempt = false;
  const ttRequireState = () => {
    let state = 0; // 0 none, 1 report only, 2 enforce (RequireTrustedTypesForDirectiveState)
    for (const p of ttPolicies) if (p.require) state = Math.max(state, p.reportOnly ? 1 : 2);
    return state;
  };
  // the securitypolicyviolation event at the document, in a task (Gecko's CSP queues it)
  const ttViolation = (policy, directive, blockedURI, sample) => {
    if (isWorker || !global.document) return;
    const doc = global.document;
    const init = { bubbles: true, composed: true, cancelable: false };
    const fields = { documentURI: doc.URL, referrer: "", blockedURI, effectiveDirective: directive, violatedDirective: directive,
      originalPolicy: policy.text || "", sourceFile: "", sample, disposition: policy.reportOnly ? "report" : "enforce",
      statusCode: 200, lineNumber: 0, columnNumber: 0 };
    addTimer(() => {
      const e = new Event("securitypolicyviolation", init);
      // (the runtime's events have no isTrusted, GAPS B72; this one is the browser's)
      Object.assign(e, fields, { isTrusted: true });
      doc.dispatchEvent(e);
    }, 0, [], false);
    console.error(`Content-Security-Policy: The page's settings blocked the ${directive === "trusted-types" ? "creation of a Trusted Types policy" : "use of a string at a Trusted Types sink"} (${sample}).`);
  };
  // CSPViolationData::MaybeTruncateSample: 40 UTF-16 units, without splitting a surrogate pair
  const ttTruncateSample = text => {
    if (text.length <= 40) return text;
    const code = text.charCodeAt(39);
    return text.slice(0, code >= 0xd800 && code <= 0xdbff ? 41 : 40);
  };
  // ReportSinkTypeMismatchViolations' sample: the sink, "|", and the input (a Function's without the
  // source text's prefix), truncated
  const FUNCTION_PREFIXES = ["function anonymous", "async function anonymous", "function* anonymous", "async function* anonymous"];
  const ttSinkSample = (sink, input) => {
    const prefix = sink === "Function" ? FUNCTION_PREFIXES.find(p => input.startsWith(p)) : undefined;
    return sink + "|" + ttTruncateSample(prefix ? input.slice(prefix.length) : input);
  };
  // nsCSPPolicy::ShouldCreateViolationForNewTrustedTypesPolicy
  const ttPolicyNameViolates = (names, name) => {
    if (names.includes("'none'")) return true;
    if (ttCreatedNames.includes(name) && !names.includes("'allow-duplicates'")) return true;
    return !(names.includes("*") || names.includes(name));
  };
  // TrustedTypePolicy::DetermineTrustedPolicyValue: the callback's string, or null (void)
  const ttDetermineValue = (callback, input, args, throwIfMissing, context) => {
    if (typeof callback !== "function") {
      if (throwIfMissing) throw new TypeError(`${context}: Function missing.`);
      return null;
    }
    const r = callback.call(undefined, input, ...args);
    return r === null || r === undefined ? null : String(r);
  };
  const ttMake = (type, data) => {
    const o = Object.create(global[type].prototype);
    trustedData.set(o, [type, data]);
    return o;
  };
  const ttPolicyOptions = new WeakMap();
  // TrustedTypeUtils::GetTrustedTypesCompliantString: the string sink `sink` of `type` gets for
  // `input`: a Trusted object's of that type; any other value as a string, through the default
  // policy when the document requires Trusted objects, and refused (a violation, and a TypeError
  // unless report only) when the default policy gives none
  function trustedTypesCompliantString(input, type, sink, context) {
    const d = input !== null && typeof input === "object" ? trustedData.get(input) : undefined;
    if (d && d[0] === type) return d[1];
    const string = String(input);
    const state = ttRequireState();
    if (state === 0) return string;
    const def = ttFactory && ttFactory.defaultPolicy;
    if (def) {
      const options = ttPolicyOptions.get(def);
      const value = ttDetermineValue(options["create" + type.slice("Trusted".length)], string, [type, sink], false);
      if (value !== null) return value;
    }
    for (const p of ttPolicies) if (p.require) ttViolation(p, "require-trusted-types-for", "trusted-types-sink", ttSinkSample(sink, string));
    if (state === 1) return string;
    throw new TypeError(`${context}: Sink type mismatch violation blocked by CSP`);
  }

  const ensureTrustedTypes = lazyGlobals([...TRUSTED_TYPE_NAMES, "TrustedTypePolicy", "TrustedTypePolicyFactory"], () => {
    const brand = (o, name, member) => {
      const d = o !== null && typeof o === "object" ? trustedData.get(o) : undefined;
      if (!d || d[0] !== name) throw new TypeError(`'${member}' called on an object that does not implement interface ${name}.`);
      return d[1];
    };
    for (const name of TRUSTED_TYPE_NAMES) {
      const C = { [name]: function () { throw new TypeError("Illegal constructor."); } }[name];
      const methods = {
        toString() { return brand(this, name, "toString"); },
        toJSON() { return brand(this, name, "toJSON"); },
      };
      for (const m of ["toString", "toJSON"]) Object.defineProperty(C.prototype, m, { value: methods[m], writable: true, enumerable: true, configurable: true });
      Object.defineProperty(C.prototype, Symbol.toStringTag, { value: name, configurable: true });
      define(global, name, C);
    }

    const policyNames = new WeakMap();
    const TrustedTypePolicy = { TrustedTypePolicy: function () { throw new TypeError("Illegal constructor."); } }.TrustedTypePolicy;
    const policyOf = (o, member) => {
      if (o === null || typeof o !== "object" || !policyNames.has(o)) throw new TypeError(`'${member}' called on an object that does not implement interface TrustedTypePolicy.`);
      return o;
    };
    Object.defineProperty(TrustedTypePolicy.prototype, "name", {
      get: Object.getOwnPropertyDescriptor({ get name() { return policyNames.get(policyOf(this, "get name")); } }, "name").get,
      enumerable: true, configurable: true,
    });
    for (const type of TRUSTED_TYPE_NAMES) {
      const member = "create" + type.slice("Trusted".length);
      const f = { [member](input, ...args) {
        policyOf(this, member);
        if (arguments.length < 1) throw new TypeError(`TrustedTypePolicy.${member}: At least 1 argument required, but only 0 passed`);
        // TrustedTypePolicy::CreateTrustedType: void is the empty string
        const value = ttDetermineValue(ttPolicyOptions.get(this)[member], String(input), args, true, `TrustedTypePolicy.${member}`);
        return ttMake(type, value === null ? "" : value);
      } }[member];
      Object.defineProperty(TrustedTypePolicy.prototype, member, { value: f, writable: true, enumerable: true, configurable: true });
    }
    Object.defineProperty(TrustedTypePolicy.prototype, Symbol.toStringTag, { value: "TrustedTypePolicy", configurable: true });
    define(global, "TrustedTypePolicy", TrustedTypePolicy);

    // TrustedTypePolicyFactory, one per global (nsGlobalWindowInner::TrustedTypes)
    const TrustedTypePolicyFactory = { TrustedTypePolicyFactory: function () { throw new TypeError("Illegal constructor."); } }.TrustedTypePolicyFactory;
    const P = TrustedTypePolicyFactory.prototype;
    let defaultPolicy = null, emptyHTML = null, emptyScript = null;
    const factoryOf = (o, member) => {
      if (o !== ttFactory) throw new TypeError(`'${member}' called on an object that does not implement interface TrustedTypePolicyFactory.`);
    };
    const methods = {
      createPolicy(policyName, policyOptions) {
        factoryOf(this, "createPolicy");
        if (arguments.length < 1) throw new TypeError("TrustedTypePolicyFactory.createPolicy: At least 1 argument required, but only 0 passed");
        const name = String(policyName);
        // the TrustedTypePolicyOptions dictionary: each member a callback or undefined
        if (policyOptions !== undefined && policyOptions !== null && typeof policyOptions !== "object" && typeof policyOptions !== "function") {
          throw new TypeError("TrustedTypePolicyFactory.createPolicy: Argument 2 can't be converted to a dictionary.");
        }
        const options = {};
        for (const m of ["createHTML", "createScript", "createScriptURL"]) {
          const v = policyOptions == null ? undefined : policyOptions[m];
          if (v === undefined) continue;
          if (typeof v !== "function") throw new TypeError(`TrustedTypePolicyFactory.createPolicy: '${m}' member of TrustedTypePolicyOptions is not ${v !== null && typeof v === "object" ? "callable" : "an object"}.`);
          options[m] = v;
        }
        // ShouldTrustedTypePolicyCreationBeBlockedByCSP: a violation for each policy whose
        // trusted-types directive does not allow the name; blocked if one of them is enforced
        let blocked = false;
        for (const p of ttPolicies) {
          if (!p.names || !ttPolicyNameViolates(p.names, name)) continue;
          ttViolation(p, "trusted-types", "trusted-types-policy", ttTruncateSample(name));
          if (!p.reportOnly) blocked = true;
        }
        if (blocked) throw new TypeError(`TrustedTypePolicyFactory.createPolicy: Content-Security-Policy blocked creating policy named '${name}'`);
        if (name === "default" && defaultPolicy) throw new TypeError("TrustedTypePolicyFactory.createPolicy: Tried to create a second default policy");
        const policy = Object.create(TrustedTypePolicy.prototype);
        policyNames.set(policy, name);
        ttPolicyOptions.set(policy, options);
        if (name === "default") defaultPolicy = policy;
        ttCreatedNames.push(name);
        return policy;
      },
      isHTML(value) { factoryOf(this, "isHTML"); return !!(value && typeof value === "object" && trustedData.get(value)?.[0] === "TrustedHTML"); },
      isScript(value) { factoryOf(this, "isScript"); return !!(value && typeof value === "object" && trustedData.get(value)?.[0] === "TrustedScript"); },
      isScriptURL(value) { factoryOf(this, "isScriptURL"); return !!(value && typeof value === "object" && trustedData.get(value)?.[0] === "TrustedScriptURL"); },
      // TrustedTypePolicyFactory::GetAttributeType and GetTrustedTypeDataForAttribute
      getAttributeType(tagName, attribute, elementNs = "", attrNs = "") {
        factoryOf(this, "getAttributeType");
        const attrNsId = attrNs == null || attrNs === "" ? "none" : String(attrNs) === "http://www.w3.org/1999/xlink" ? "xlink" : null;
        if (!attrNsId) return null;
        const ns = elementNs == null || elementNs === "" || String(elementNs) === "http://www.w3.org/1999/xhtml" ? "html"
          : String(elementNs) === "http://www.w3.org/2000/svg" ? "svg" : String(elementNs) === "http://www.w3.org/1998/Math/MathML" ? "mathml" : null;
        if (!ns) return null;
        const data = trustedTypeDataForAttribute(String(tagName).toLowerCase(), ns, String(attribute).toLowerCase(), attrNsId);
        return data ? data[0] : null;
      },
      // TrustedTypePolicyFactory::GetPropertyType
      getPropertyType(tagName, property, elementNs = "") {
        factoryOf(this, "getPropertyType");
        const tag = String(tagName).toLowerCase(), prop = String(property);
        if (elementNs == null || elementNs === "" || String(elementNs) === "http://www.w3.org/1999/xhtml") {
          if (tag === "iframe" && prop === "srcdoc") return "TrustedHTML";
          if (tag === "script" && (prop === "innerText" || prop === "text" || prop === "textContent")) return "TrustedScript";
          if (tag === "script" && prop === "src") return "TrustedScriptURL";
        }
        return prop === "innerHTML" || prop === "outerHTML" ? "TrustedHTML" : null;
      },
    };
    for (const [m, f] of Object.entries(methods)) Object.defineProperty(P, m, { value: f, writable: true, enumerable: true, configurable: true });
    const accessor = (name, get) => Object.defineProperty(P, name, {
      get: Object.getOwnPropertyDescriptor({ get [name]() { factoryOf(this, "get " + name); return get(); } }, name).get,
      enumerable: true, configurable: true,
    });
    // [StoreInSlot]: the same object at each read
    accessor("emptyHTML", () => (emptyHTML ??= ttMake("TrustedHTML", "")));
    accessor("emptyScript", () => (emptyScript ??= ttMake("TrustedScript", "")));
    accessor("defaultPolicy", () => defaultPolicy);
    Object.defineProperty(P, Symbol.toStringTag, { value: "TrustedTypePolicyFactory", configurable: true });
    define(global, "TrustedTypePolicyFactory", TrustedTypePolicyFactory);
    ttFactory = Object.create(P);
  });
  // window.trustedTypes and self.trustedTypes (WindowOrWorkerGlobalScope): made on first read
  Object.defineProperty(global, "trustedTypes", {
    get: Object.getOwnPropertyDescriptor({ get trustedTypes() { ensureTrustedTypes(); return ttFactory; } }, "trustedTypes").get,
    enumerable: true, configurable: true,
  });
  // the event handler content attribute names (Gecko's EventNameList.inc without the XUL-only ones,
  // nsContentUtils::IsEventAttributeName): tools/gen_event_attribute_names.py
  // @EVENT_ATTRIBUTE_NAMES@
  // TrustedTypeUtils::GetTrustedTypeDataForAttribute: [type, sink] or null; ns "html", "svg", "mathml";
  // attrNs "none" or "xlink"
  function trustedTypeDataForAttribute(localName, ns, name, attrNs) {
    if (attrNs === "none" && name.startsWith("on") && EVENT_ATTRIBUTE_NAMES.has(name.slice(2))) return ["TrustedScript", "Element " + name];
    if (ns === "html" && localName === "iframe" && attrNs === "none" && name === "srcdoc") return ["TrustedHTML", "HTMLIFrameElement srcdoc"];
    if (ns === "html" && localName === "script" && attrNs === "none" && name === "src") return ["TrustedScriptURL", "HTMLScriptElement src"];
    if (ns === "svg" && localName === "script" && name === "href") return ["TrustedScriptURL", "SVGScriptElement href"];
    return null;
  }
  const trustedTypes = {
    compliant: trustedTypesCompliantString,
    dataForAttribute: trustedTypeDataForAttribute,
    requireState: ttRequireState,
    // the document's CSP as Firefox parsed it (Document.cspJSON's policies)
    setPolicies(policies) { ttPolicies = policies; },
    get policies() { return ttPolicies; },
    // the C++ layer's eval hooks (JSSecurityCallbacks): a TrustedScript's string, for eval
    // (TrustedTypeUtils::HostGetCodeForEval), or undefined
    scriptData(o) { const d = trustedData.get(o); return d && d[0] === "TrustedScript" ? d[1] : undefined; },
    // a Trusted object's string when it is of `type`, else undefined
    dataOf(o, type) { const d = o !== null && typeof o === "object" ? trustedData.get(o) : undefined; return d && d[0] === type ? d[1] : undefined; },
    // AreArgumentsTrustedForEnsureCSPDoesNotBlockStringCompilation: whether eval or Function may
    // compile `code` (`isFunction`: the Function constructor, its body and parameters with the
    // arguments they came from)
    evalAllowed(code, isFunction, bodyString, bodyArg, parameterStrings, parameterArgs) {
      if (ttEvalExempt) { ttEvalExempt = false; return true; }
      if (ttRequireState() === 0) return true;
      const trusted = (arg, string) => arg !== null && typeof arg === "object" && trustedData.get(arg)?.[0] === "TrustedScript" && trustedData.get(arg)[1] === string;
      let isTrusted;
      if (!isFunction) isTrusted = bodyArg !== null && typeof bodyArg === "object";
      else {
        isTrusted = bodyString === null || trusted(bodyArg, bodyString);
        for (let i = 0; isTrusted && i < parameterArgs.length; i++) isTrusted = trusted(parameterArgs[i], parameterStrings[i]);
      }
      if (isTrusted) return true;
      try {
        return trustedTypesCompliantString(code, "TrustedScript", isFunction ? "Function" : "eval", "eval") === code;
      } catch (e) {
        return false;
      }
    },
    exemptNextEval() { ttEvalExempt = true; },
  };

  // ---- what the driver calls ----

  define(global, "__mrt", {
    url: { URL_RE, removeDots, resolve },
    trustedTypes,
    // an approximated API was used (bridge.js counts them in its report)
    stub() {},
    // a paint wanted at the next refresh tick (webgl.js: its queue flushed)
    requestPaint(paint) { refresh.paints.add(paint); },
    // a scroll event posted for the next refresh tick's scroll steps (bridge.js's root scroll)
    requestScroll(scroll) { refresh.scrolls.push(scroll); },
    // whether a value is a ReadableStream; replaced when the streams are made (until then none exists)
    isReadableStream: () => false,
    // for the C++ layer's module loader: a specifier parsed against a base (base null: as an
    // absolute URL only), or null when it does not parse; a URL's base needs a host
    resolveURL(spec, base) {
      spec = String(spec).trim();
      // a hierarchical URL serialized as the URL parser does: scheme and host lower case, the dot
      // segments of the path removed, "/" for an empty path
      const normalize = url => {
        const m = URL_RE.exec(url);
        if (!m || m[3] === undefined) return url;
        return m[1].toLowerCase() + "//" + (m[2] !== undefined ? m[2] + "@" : "") + m[3].toLowerCase() +
          (m[4] ? ":" + m[4] : "") + removeDots(m[5] || "/") + (m[6] || "") + (m[7] || "");
      };
      if (base === null) return /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(spec) ? normalize(spec) : null;
      const b = URL_RE.exec(base);
      if (!b || b[3] === undefined) return null;
      try { return normalize(resolve(spec, base)); } catch (e) { return null; }
    },
    baseURL: () => documentBaseURL,
    // a module's source: {url, text} or null (a data: URL only; bridge.js fetches the others)
    fetchModule(url, moduleType) {
      const d = url.startsWith("data:") ? readDataURL(url) : null;
      return d && mimeTypeMatches(d.type, moduleType) ? { url, text: d.text } : null;
    },
    readDataURL,
    mimeTypeMatches,
    runModuleScript,
    isModuleScript,
    // page code the runtime calls (bridge.js): a callback with its microtask checkpoint
    enterCallback,
    leaveCallback,
    callPage,
    insertedScript(el) {
      if (isModuleScript(el) && !alreadyStarted.has(el)) addTimer(() => runModuleScript(el), 0, [], false);
    },
    insertedLink(el) {},
    runTimers,
    // for a driver that merges runtimes' event loops (DESIGN, "Cross-origin frames in runtimes of
    // their own"): the virtual time of the next task, -1 when none is left; the clock set (a frame
    // runtime starts at its parent's time); a task queued at a virtual time
    nextTimerTime() { const next = nextDueTimer(); return next ? next[2] : -1; },
    setClock(ms) { clock.now = Math.max(clock.now, Number(ms) || 0); },
    queueAt(fn, at) { addTimer(fn, Math.max(0, (Number(at) || 0) - clock.now), [], false); },
    runNextTimer,
    // once profileTimers was called, what the timer runNextTimer ran last was, for the interactive
    // loop's profile (realsite's MRT_TIMER_PROFILE, GAPS A22): its kind, delay and the start of its
    // callback's source
    profileTimers() { clock.profile = true; },
    lastTimerKey() {
      const t = clock.lastRun;
      if (!t) return "";
      const kind = t.frame ? "frame" : t.every ? "interval" : t.timeout ? "timeout" : t.page ? "page" : "runtime";
      const f = t.callback;
      const src = typeof f === "function" ? (f.name || "") + " " + Function.prototype.toString.call(f).replace(/\s+/g, " ").slice(0, 100) : "";
      return `${kind} ${t.delay} ${src}`;
    },
    // a task of the C++ layer's (the rejection events) as a 0 ms timer, behind those already due;
    // the prelude's own queue, so a page that replaces setTimeout does not see it
    queueTask(fn, arg) { addTimer(fn, 0, [arg], false); },
    // the scheduler's (bridge.js): a task at a priority (PRIORITY_NORMAL's scale), after `ms`; the
    // count of queued user-visible and user-blocking tasks the timeouts wait for
    queuePriorityTask(fn, priority, ms = 0) { addTimer(fn, ms, [], false, false, priority); },
    // a timeout of the browser's own (Timeout::Reason::eAbortSignalTimeout): in the window's
    // timeouts, so it waits for the scheduler's tasks as they do, but at nesting level 0, unclamped
    queueTimeout(fn, ms) { addTimer(fn, ms, [], false, true, PRIORITY_NORMAL, false); },
    webTasks,
    // a worker's global: how its uncaught exceptions are reported (bridge.js's worker section)
    setWorkerErrorReport(fn) { workerErrorReport = fn; },
    // unhandledrejection (cancelable) and rejectionhandled at the window, from the C++ layer's
    // rejection tracking (Gecko's NotifyUnhandledRejections and PromiseRejectionTrackerCallback)
    rejectionEvent(type, promise, reason) {
      const init = { cancelable: type === "unhandledrejection", promise, reason };
      const event = global.PromiseRejectionEvent ? new global.PromiseRejectionEvent(type, init) : Object.assign(new Event(type, init), { promise, reason });
      global.dispatchEvent(event);
    },
    shared,
    // the real clock from now on, counting on from the virtual time reached (realsite --interactive)
    useRealClock() { if (clock.realFrom === null) clock.realFrom = wallNow() - clock.now; },
    clockNow,
    fireWindowLoad,
    defineEventHandlers,
    preventExecution,
    frameInserted,
    frameSrcChanged,
    loadFrame,
    // a frame's window, after its document changed: its URL (bridge.js knows URLs)
    frameNavigated(url) {},
    // the resize steps reached this window (__mrtRefresh): Gecko's resize event, at the window
    fireResize() { dispatchEvent.call(global, new Event("resize")); },
    // The page hibernates (Page::hibernate) as its tab goes to the background: every window of the
    // runtime is in the background at once, and each document is told in a task of its own, in tree
    // order (nsDocShell::ActivenessMaybeChanged on each docshell: SetIsBackground, then
    // PostVisibilityUpdateEvent).
    hide() {
      if (visibility.background) return;
      visibility.background = true;
      __mrtEachWindow("windowHidden");
    },
    // The page wakes (Page::wake) as its tab is shown again: every window leaves the background at
    // once (SetIsBackground(false)), and each document is told in a task of its own, in the same
    // order. What waited is due: the timeouts the 1 s gate held at once (UpdateBackgroundState
    // schedules the executor again), and the animation frame callbacks at the new PresShell's refresh
    // driver's first tick, a frame on, so after each document's visibilitychange.
    show() {
      if (!visibility.background) return;
      visibility.background = false;
      for (const t of timers.values()) {
        if (t.frame && t.at < clock.now + 16) t.at = clock.now + 16;
        else if (t.at < clock.now) t.at = clock.now;
      }
      __mrtEachWindow("windowShown");
    },
    windowShown() {
      visibility.batches.delete(global);
      visibility.gates.delete(global);
      addTimer(() => {
        documentHidden = false;
        dispatchEvent.call(document, new Event("visibilitychange", { bubbles: true }));
      }, 0, [], false);
    },
    // this window went to the background: its timeouts' executor is scheduled again, 1 s on
    // (TimeoutManager::UpdateBackgroundState), and a task makes its document hidden and fires
    // visibilitychange at it, bubbling, not cancelable (Document::UpdateVisibilityState)
    windowHidden() {
      visibility.gates.set(global, clock.now + BACKGROUND_TIMEOUT_MS);
      addTimer(() => {
        documentHidden = true;
        dispatchEvent.call(document, new Event("visibilitychange", { bubbles: true }));
      }, 0, [], false);
    },
    // a frame's document's scripts, then its load events, then `loaded` (bridge.js loads them in
    // order, and waits for what blocks the load)
    runDocument(loaded) {
      for (const el of document.querySelectorAll("script")) global.__mrt.insertedScript(el);
      global.__mrt.fireLoadEvents();
      loaded();
    },
    // what blocks the document's load event (bridge.js counts it): here nothing does
    blockOnload() { return () => {}; },
    setReadyState() {},
    // DOMContentLoaded on the document, then load on the window, as after a page's last script
    fireLoadEvents() {
      document.dispatchEvent(new Event("DOMContentLoaded", { bubbles: true }));
      fireWindowLoad();
    },
  });
})(this);
