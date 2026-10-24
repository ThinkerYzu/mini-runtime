// What the sites get from the APIs the runtime approximates (GAPS B15, task c1f2, session 102), for
// REPLAY_INJECT with firefox_probe.py; read the counts back with probe_approximated_apis_read.js.
// Counted per API and per caller (the first page frame of the call's stack, or for an observer's
// callbacks the place it was made):
//   IntersectionObserver: observe calls, callbacks, entries, and entries after a target's first one
//     (the runtime sends only the first) and of them how many flip isIntersecting
//   ResizeObserver: observe calls, callbacks and entries (the runtime sends none)
//   innerText: reads, and reads whose value is not textContent (the runtime answers textContent);
//     "ws" counts those that are equal once whitespace is collapsed
//   elementFromPoint, elementsFromPoint: calls, and calls with a non-empty answer (the runtime: none)
// Same-origin frames add to the top window's counts.
(function () {
  let log;
  try { log = window.top.__plog; } catch (e) {}
  if (!log) {
    log = {};
    Object.defineProperty(window, "__plog", { value: log, configurable: true });
  }
  const callerOf = stack => {
    const lines = String(stack).split("\n").slice(1).filter(l => l && !l.includes("probe_approximated_apis"));
    const line = lines[0] || "?";
    // fn@url:line:col, the URL without its query
    return line.replace(/\?[^:@]*(?=:\d+:\d+$)/, "").slice(-160);
  };
  const count = (api, caller, key, n = 1) => {
    const k = api + " | " + caller;
    const row = log[k] || (log[k] = {});
    row[key] = (row[key] || 0) + n;
  };

  const wrapObserver = (name, perEntry) => {
    const Orig = window[name];
    if (!Orig) return;
    const origObserve = Orig.prototype.observe;
    const made = new WeakMap();
    Orig.prototype.observe = function (target) {
      count(name, made.get(this) || callerOf(new Error().stack), "observe");
      return origObserve.apply(this, arguments);
    };
    class Wrapped extends Orig {
      constructor(cb, options) {
        const caller = callerOf(new Error().stack);
        const seen = new WeakMap();
        super(function (entries, observer) {
          count(name, caller, "callbacks");
          count(name, caller, "entries", entries.length);
          for (const e of entries) perEntry(e, seen, key => count(name, caller, key));
          return cb.apply(this, arguments);
        }, options);
        made.set(this, caller);
      }
    }
    Object.defineProperty(Wrapped, "name", { value: name });
    Object.defineProperty(window, name, { value: Wrapped, writable: true, configurable: true });
  };
  wrapObserver("IntersectionObserver", (e, seen, add) => {
    if (!seen.has(e.target)) { seen.set(e.target, e.isIntersecting); return; }
    add("later");
    if (seen.get(e.target) !== e.isIntersecting) add("later_flips");
    seen.set(e.target, e.isIntersecting);
  });
  wrapObserver("ResizeObserver", (e, seen, add) => {
    if (seen.has(e.target)) add("later");
    seen.set(e.target, true);
  });

  const it = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "innerText");
  if (it && it.get) {
    Object.defineProperty(HTMLElement.prototype, "innerText", Object.assign({}, it, {
      get() {
        const v = it.get.call(this);
        const caller = callerOf(new Error().stack);
        count("innerText", caller, "reads");
        const t = this.textContent;
        if (v !== t) {
          count("innerText", caller, "differs");
          const squash = s => s.replace(/\s+/g, " ").trim();
          if (squash(v) === squash(t)) count("innerText", caller, "ws");
        }
        return v;
      },
    }));
  }

  for (const [proto, protoName] of [[Document.prototype, "Document"], [ShadowRoot.prototype, "ShadowRoot"]]) {
    for (const m of ["elementFromPoint", "elementsFromPoint"]) {
      const orig = proto[m];
      if (!orig) continue;
      proto[m] = function () {
        const r = orig.apply(this, arguments);
        const caller = callerOf(new Error().stack);
        count(protoName + "." + m, caller, "calls");
        if (m === "elementFromPoint" ? r !== null : r.length > 0) count(protoName + "." + m, caller, "answered");
        return r;
      };
    }
  }
})();
