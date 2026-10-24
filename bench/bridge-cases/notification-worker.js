// notification.html's worker: Notification in a dedicated worker (no requestPermission there)
(async function () {
  const out = [];
  const J = v => JSON.stringify(v);
  const E = e => (e.constructor ? e.constructor.name : "?") + " " + e.name + ": " + e.message;
  const tick = ms => new Promise(r => setTimeout(r, ms));
  out.push(J([typeof Notification, Object.getOwnPropertyNames(Notification).sort(), Object.getOwnPropertyNames(Notification.prototype).sort()]));
  try { out.push("permission " + J([Notification.permission, Notification.maxActions])); } catch (e) { out.push("permission " + E(e)); }
  try {
    const n = new Notification("worker", { body: "b", icon: "i.png", data: [1] });
    out.push("new " + J([n.title, n.body, n.icon, n.data]));
    const order = [];
    n.onerror = e => order.push("error " + e.isTrusted);
    n.onshow = () => order.push("show");
    setTimeout(() => order.push("timeout 0"), 0);
    await tick(300);
    out.push("events " + J(order));
  } catch (e) {
    out.push("new " + E(e));
  }
  postMessage(out.map(l => l.split(location.origin).join("ORIGIN")));
})();
