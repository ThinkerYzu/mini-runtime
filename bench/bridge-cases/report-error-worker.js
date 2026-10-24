// report-error.html's worker: reportError on a dedicated worker's global
(function () {
  const out = [];
  const J = v => JSON.stringify(v);
  const E = e => (e.constructor ? e.constructor.name : "?") + " " + e.name + ": " + e.message;
  const T = (what, f) => { try { out.push(what + " ok " + J(f())); } catch (e) { out.push(what + " throws " + E(e)); } };
  T("typeof", () => [typeof reportError, typeof self.reportError, Object.getOwnPropertyNames(self).includes("reportError"),
    typeof WorkerGlobalScope.prototype.reportError, reportError.length, reportError.name]);
  T("wrong this", () => reportError.call({}, 1));
  T("no argument", () => reportError());
  const seen = [];
  const err = new Error("worker boom");
  self.addEventListener("error", e => seen.push([e.constructor.name, e.message, typeof e.lineno, e.lineno > 0, e.error === err || e.error,
    e.isTrusted, e.cancelable, e.bubbles, e.target === self]));
  self.onerror = function (...a) { seen.push(["onerror", a.length, a[0], typeof a[2], a[4] === err]); return a[0] === "Error: cancelled" || a[0].endsWith("cancelled"); };
  T("Error", () => [reportError(err), seen.splice(0)]);
  T("string", () => [reportError("plain"), seen.splice(0)]);
  T("cancelled", () => [reportError(new Error("cancelled")), seen.splice(0)]);
  postMessage(out.map(l => l.split(location.origin).join("ORIGIN")));
})();
