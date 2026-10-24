// abort.html's worker: AbortSignal and AbortController on a dedicated worker's global
(async function () {
  const out = [];
  const J = v => JSON.stringify(v);
  const E = e => e && typeof e === "object" ? (e.constructor ? e.constructor.name : "?") + " " + e.name + ": " + e.message : typeof e + " " + String(e);
  const T = (what, f) => { try { out.push(what + " ok " + J(f())); } catch (e) { out.push(what + " throws " + E(e)); } };
  const tick = ms => new Promise(r => setTimeout(r, ms));
  T("global", () => [typeof AbortSignal, typeof AbortController, Object.getPrototypeOf(AbortSignal) === EventTarget, AbortSignal.any.length]);
  const c = new AbortController();
  const seen = [];
  c.signal.addEventListener("abort", e => seen.push([e.isTrusted, c.signal.aborted]));
  c.abort();
  T("abort", () => [E(c.signal.reason), seen]);
  T("new AbortSignal", () => new AbortSignal());
  const order = [];
  setTimeout(() => order.push("timer"), 0);
  const t = AbortSignal.timeout(0);
  t.onabort = () => order.push("timeout " + E(t.reason));
  const d = AbortSignal.any([t]);
  d.onabort = () => order.push("any " + (d.reason === t.reason));
  await tick(30);
  T("timeout", () => order);
  postMessage(out);
})();
