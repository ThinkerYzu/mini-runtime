// permissions-storage.html's worker: navigator.permissions and navigator.storage in a dedicated worker
(async function () {
  const out = [];
  const J = v => JSON.stringify(v);
  const E = e => (e.constructor ? e.constructor.name : "?") + " " + e.name + ": " + e.message;
  const settle = p => p.then(v => "resolved " + (v instanceof PermissionStatus ? J([v.name, v.state]) : J(Object.keys(v).sort())),
                             e => "rejected " + E(e));
  out.push(J([typeof Permissions, typeof PermissionStatus, typeof StorageManager, "permissions" in navigator, "storage" in navigator,
              Object.prototype.toString.call(navigator.permissions), Object.prototype.toString.call(navigator.storage),
              typeof StorageManager.prototype.persist]));
  for (const name of ["notifications", "geolocation", "push", "background-sync"]) out.push("query " + name + " " + await settle(navigator.permissions.query({ name })));
  out.push("estimate " + await settle(navigator.storage.estimate()));
  out.push("persisted " + await navigator.storage.persisted().then(v => J(v), e => E(e)));
  postMessage(out);
})();
