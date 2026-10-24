// The nested worker of bench/bridge-cases/worker-apis.html: started by worker-apis-worker.js.
onmessage = e => postMessage(["child got", e.data, self.origin, location.pathname, typeof Worker, Object.prototype.toString.call(self)]);
