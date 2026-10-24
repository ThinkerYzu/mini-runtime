// The shared worker of bench/bridge-cases/message-ports.html: a port sent over a connection's port
// is answered on.
self.onconnect = e => {
  const port = e.ports[0];
  port.onmessage = m => {
    const p = m.ports[0];
    p.onmessage = x => p.postMessage({ shared: x.data });
    port.postMessage({ gotPort: m.ports.length, same: m.data.port === p });
  };
};
