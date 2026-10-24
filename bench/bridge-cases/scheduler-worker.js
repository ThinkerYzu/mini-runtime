// scheduler.html's worker: scheduler on a dedicated worker's global
(async function () {
  const out = [];
  const J = v => JSON.stringify(v);
  const E = e => e && typeof e === "object" ? (e.constructor ? e.constructor.name : "?") + " " + e.name + ": " + e.message : typeof e + " " + String(e);
  const T = (what, f) => { try { out.push(what + " ok " + J(f())); } catch (e) { out.push(what + " throws " + E(e)); } };
  const tick = ms => new Promise(r => setTimeout(r, ms));
  const settle = p => p.then(v => "resolved " + J(v), e => "rejected " + E(e));
  const d = Object.getOwnPropertyDescriptor(WorkerGlobalScope.prototype, "scheduler");
  T("global", () => [typeof scheduler, Object.getOwnPropertyNames(self).includes("scheduler"), d && [typeof d.get, typeof d.set, d.enumerable, d.configurable],
    scheduler === self.scheduler, Object.prototype.toString.call(scheduler), typeof TaskController, typeof TaskSignal, typeof TaskPriorityChangeEvent]);
  out.push("postTask " + await settle(scheduler.postTask(() => "in worker")));
  out.push("yield " + await settle(scheduler.yield()));
  out.push("bad priority " + await settle(scheduler.postTask(() => 1, { priority: "x" })));
  const order = [];
  setTimeout(() => order.push("timer"), 0);
  scheduler.postTask(() => order.push("background"), { priority: "background" });
  scheduler.postTask(() => order.push("visible"));
  scheduler.postTask(() => order.push("blocking"), { priority: "user-blocking" });
  scheduler.yield().then(() => order.push("yield"));
  queueMicrotask(() => order.push("micro"));
  await tick(50);
  out.push("order " + J(order));
  const c = new TaskController({ priority: "background" });
  const moved = [];
  scheduler.postTask(() => moved.push("signal"), { signal: c.signal });
  scheduler.postTask(() => moved.push("visible"));
  c.setPriority("user-blocking");
  await tick(50);
  out.push("setPriority " + J(moved));
  const a = new TaskController();
  const p = scheduler.postTask(() => 1, { signal: a.signal });
  a.abort("w");
  out.push("abort " + await settle(p));
  postMessage(out);
})();
