// web-locks.html's worker: Web Locks inside a worker, then a lock held until the page terminates it,
// as Facebook's Messenger worker holds sw-lock-<name>-<id>.
(async function () {
  const J = v => JSON.stringify(v);
  postMessage("worker " + J([typeof LockManager, typeof Lock, navigator.locks instanceof LockManager, Object.prototype.toString.call(navigator.locks),
                             self.LockManager != null && self.navigator.locks instanceof self.LockManager]));
  const v = await navigator.locks.request("w-first", lock => lock.name + " " + lock.mode);
  postMessage("worker first " + v);
  const q = await navigator.locks.query();
  postMessage("worker query " + J([q.held.length, q.pending.length]));
  navigator.locks.request("w-lock", () => { postMessage("holding"); return new Promise(() => {}); });
})();
