// The shared worker of bench/bridge-cases/shared-worker.html: counts its connections, answers on each
// port, reports its global, throws and closes when asked.
let connections = 0;
const ports = [];
const own = o => Object.getOwnPropertyNames(o).sort();
self.onerror = message => { for (const p of ports) p.postMessage({ scopeError: String(message) }); };
self.onconnect = e => {
  connections++;
  const port = e.ports[0];
  ports.push(port);
  port.onmessage = m => handle(port, m.data);
  port.postMessage({ connected: connections, name: self.name,
                     event: [e.constructor.name, e.type, JSON.stringify(e.data), e.origin, e.lastEventId, e.source === port,
                             e.ports.length, e.bubbles, e.cancelable, e.target === self, Object.prototype.toString.call(port)] });
};
function handle(port, d) {
  if (d.cmd === "echo") port.postMessage({ echo: d.value });
  else if (d.cmd === "all") for (const p of ports) p.postMessage({ all: d.value });
  else if (d.cmd === "shape") port.postMessage({ shape: [typeof self.postMessage, "onmessage" in self, "onconnect" in self, self instanceof SharedWorkerGlobalScope,
    Object.prototype.toString.call(self), typeof DedicatedWorkerGlobalScope, typeof importScripts, location.pathname.replace(/^.*\//, ""),
    typeof self.close, Object.getOwnPropertyNames(SharedWorkerGlobalScope.prototype).sort(), Object.getPrototypeOf(SharedWorkerGlobalScope.prototype) === WorkerGlobalScope.prototype] });
  else if (d.cmd === "names") {
    const out = { global: own(self) };
    for (let p = Object.getPrototypeOf(self), i = 0; p; p = Object.getPrototypeOf(p), i++) out["proto" + i + " " + Object.prototype.toString.call(p)] = own(p);
    out.navigator = []; for (let p = Object.getPrototypeOf(navigator); p && p !== Object.prototype; p = Object.getPrototypeOf(p)) out.navigator.push(...own(p));
    out.location = own(Object.getPrototypeOf(location));
    port.postMessage({ names: out });
  } else if (d.cmd === "clone") port.postMessage({ clone: d.value });
  else if (d.cmd === "throw") setTimeout(() => { throw new Error("boom " + d.value); });
  else if (d.cmd === "close") { port.postMessage({ closing: connections }); close(); }
}
