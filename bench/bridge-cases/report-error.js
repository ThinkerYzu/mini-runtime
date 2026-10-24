// report-error.html's script (see there)
(async function () {
  const log = window.__log = [];
  const O = location.origin;
  const L = (...a) => log.push(a.join(" ").split(O).join("ORIGIN"));
  const J = v => JSON.stringify(v);
  const E = e => e ? (e.constructor ? e.constructor.name : "?") + " " + e.name + ": " + e.message : "null";
  const T = (what, f) => { try { L(what, "ok", J(f())); } catch (e) { L(what, "throws", E(e)); } };
  const tick = ms => new Promise(r => setTimeout(r, ms));

  T("typeof", () => [typeof reportError, typeof window.reportError, typeof Window.prototype.reportError, "reportError" in window]);
  T("descriptor", () => {
    const d = Object.getOwnPropertyDescriptor(window, "reportError");
    return d && [typeof d.value, d.writable, d.enumerable, d.configurable, d.value.length, d.value.name, "prototype" in d.value];
  });
  T("new", () => new reportError(1));
  T("no argument", () => reportError());
  T("wrong this", () => reportError.call({}, 1));

  // each value: the event a listener sees, and what came back
  let seen = [];
  // an Error's place is its own; a DOMException's (made by the binding) and another value's is the
  // caller's, this script
  const place = e => e.error instanceof Error && !(e.error instanceof DOMException) ? [e.filename === e.error.fileName, e.lineno === e.error.lineNumber, e.colno === e.error.columnNumber] : e.filename;
  const record = e => seen.push([e.constructor.name, e.type, e.message, place(e), typeof e.lineno, e.lineno > 0, typeof e.colno, e.colno > 0,
    e.isTrusted, e.bubbles, e.cancelable, e.composed, e.eventPhase, e.target === window, e.currentTarget === window, e.defaultPrevented]);
  window.addEventListener("error", record);
  const err = new Error("boom");
  const values = [["Error", err], ["TypeError", new TypeError("bad type")], ["DOMException", new DOMException("dom msg", "NotFoundError")],
    ["named", Object.assign(new Error("m"), { name: "Custom" })], ["string", "a string"], ["number", 42], ["object", { message: "obj" }],
    ["null", null], ["undefined", undefined], ["empty Error", new Error()], ["array", [1, 2]]];
  for (const [what, v] of values) {
    seen = [];
    let errorSame;
    const same = e => { errorSame = e.error === v; };
    window.addEventListener("error", same);
    T("value " + what, () => { const r = reportError(v); return [r, seen.length, errorSame]; });
    window.removeEventListener("error", same);
    L("event " + what, J(seen));
  }
  T("symbol", () => { seen = []; reportError(Symbol("s")); return seen.map(s => s[2]); });
  T("undefined this", () => { seen = []; reportError.call(undefined, err); return seen.length; });
  window.removeEventListener("error", record);

  // onerror: called with the error's parts; true cancels, false does not
  const onerrorArgs = [];
  window.onerror = function (...a) { onerrorArgs.push([a.length, a[0], a[1] === err.fileName, a[2] === err.lineNumber, a[3] === err.columnNumber, a[4] === err, this === window]); return true; };
  // read after the dispatch: the handler runs after the listeners in the runtime (GAPS B71)
  let last;
  const keep = e => { last = e; };
  window.addEventListener("error", keep);
  T("onerror true", () => { reportError(err); return [onerrorArgs, last.defaultPrevented]; });
  window.onerror = () => false;
  T("onerror false", () => { reportError(err); return last.defaultPrevented; });
  window.onerror = null;
  window.removeEventListener("error", keep);

  // listener order and preventDefault (the second listener still runs), capture at the window
  // (added last: at the target the capture listeners run first)
  const order = [];
  const a = e => { order.push("a " + e.eventPhase); e.preventDefault(); };
  const b = e => order.push("b " + e.defaultPrevented);
  const c = e => order.push("capture " + e.eventPhase);
  window.addEventListener("error", a); window.addEventListener("error", b); window.addEventListener("error", c, true);
  T("order", () => { reportError(err); return order; });
  window.removeEventListener("error", a); window.removeEventListener("error", b); window.removeEventListener("error", c, true);
  T("document listener", () => { let n = 0; const f = () => n++; document.addEventListener("error", f); reportError(err); document.removeEventListener("error", f); return n; });

  // a reportError made while an error event is handled fires no event (sHandlingScriptError)
  const nested = [];
  const n1 = e => { nested.push("outer " + e.message); if (nested.length < 5) reportError(new Error("inner")); nested.push("after inner"); };
  window.addEventListener("error", n1);
  T("nested", () => { reportError(new Error("outer")); return nested; });
  window.removeEventListener("error", n1);

  // a listener that throws: the next one runs, and the exception fires no second event
  const thrown = [];
  const t1 = e => { thrown.push("t1 " + e.message); throw new Error("from listener"); };
  const t2 = e => thrown.push("t2 " + e.message);
  window.addEventListener("error", t1); window.addEventListener("error", t2);
  T("throwing listener", () => { reportError(new Error("first")); return thrown; });
  window.removeEventListener("error", t1); window.removeEventListener("error", t2);
  await tick(10);
  T("throwing listener after", () => thrown);

  // from a timer and a microtask: the event before the call returns
  const timed = [];
  const tl = e => timed.push("event " + e.message);
  window.addEventListener("error", tl);
  await new Promise(r => setTimeout(() => { reportError(new Error("timer")); timed.push("returned"); r(); }, 0));
  await Promise.resolve().then(() => { reportError(new Error("microtask")); timed.push("returned"); });
  window.removeEventListener("error", tl);
  T("timer and microtask", () => timed);

  // a same-origin frame: the event at the window whose method was called
  const frame = document.createElement("iframe");
  document.body.appendChild(frame);
  const fw = frame.contentWindow;
  const where = [];
  const top1 = () => where.push("top"), fr1 = e => where.push("frame " + (e.target === fw) + " " + e.isTrusted);
  window.addEventListener("error", top1); fw.addEventListener("error", fr1);
  T("frame", () => { fw.reportError(err); where.push("|"); reportError.call(fw, err); where.push("|"); fw.reportError.call(window, err); return [where, fw.reportError !== reportError]; });
  window.removeEventListener("error", top1); fw.removeEventListener("error", fr1);

  // [Replaceable]? no: a plain writable method; an assignment replaces it
  T("assign", () => { const old = window.reportError; window.reportError = 5; const v = window.reportError; window.reportError = old; return [v, window.reportError === old]; });

  // a worker: the global's error event and onerror, then the Worker's error event unless cancelled
  const w = new Worker("report-error-worker.js");
  const fromWorker = [];
  w.onerror = e => { fromWorker.push(["Worker error", e.message, e.filename, typeof e.lineno, e.lineno > 0, e.isTrusted, e.error]); e.preventDefault(); };
  const lines = await new Promise(r => { w.onmessage = e => r(e.data); setTimeout(() => r(["timeout"]), 3000); });
  await tick(100);
  for (const l of lines) L("worker", l);
  L("worker errors", J(fromWorker).split(O).join("ORIGIN"));
  L("done");
})();
