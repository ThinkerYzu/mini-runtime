// The worker of bench/bridge-cases/message-ports.html: answers on the ports it is sent, makes a
// channel and sends one end to its parent (Facebook's channelImport), hands a port back, and talks
// to another worker over a port the page gave both.
const E = e => (e ? e.name + ": " + e.message : "null");
const answerOn = (port, tag) => {
  port.onmessage = m => port.postMessage({ [tag]: m.data, ports: m.ports.length, origin: m.origin, source: m.source,
                                           kind: m.constructor.name, target: m.target === port });
};
let peer = null;
self.onmessage = e => {
  const d = e.data;
  if (d.cmd === "echo-on") {
    // a port in the message's transfer list: the event's ports and the data's own reference
    const port = e.ports[0];
    self.postMessage({ got: [e.ports.length, d.port === port, Object.prototype.toString.call(port), Object.isFrozen(e.ports)] });
    answerOn(port, "echo");
  } else if (d.cmd === "import") {
    // Facebook's channelImport: keep port1, send port2 to the parent
    const ch = new MessageChannel();
    ch.port1.start();
    ch.port1.addEventListener("message", m => {
      ch.port1.postMessage({ imported: m.data });
    });
    self.postMessage({ type: "channelImport", channelName: "c1", port: ch.port2 }, [ch.port2]);
  } else if (d.cmd === "hand-back") {
    // the port goes back where it came from
    self.postMessage({ back: 1 }, [e.ports[0]]);
  } else if (d.cmd === "peer") {
    // a port whose other end is in another worker
    peer = e.ports[0];
    peer.onmessage = m => {
      if (m.data === "ping") peer.postMessage("pong from " + d.name);
      else self.postMessage({ peer: m.data, name: d.name, ports: m.ports.length });
      if (m.ports.length) answerOn(m.ports[0], "forwarded to " + d.name);
    };
  } else if (d.cmd === "ping-peer") {
    peer.postMessage("ping");
  } else if (d.cmd === "forward") {
    // a port received, sent on to the peer worker over another port
    peer.postMessage("forwarding", [e.ports[0]]);
  } else if (d.cmd === "errors") {
    const ch = new MessageChannel();
    const out = [];
    try { self.postMessage({ p: ch.port1 }); out.push("not in transfer: ok"); } catch (x) { out.push("not in transfer: " + E(x)); }
    try { ch.port1.postMessage(1, [ch.port1]); out.push("self: ok"); } catch (x) { out.push("self: " + E(x)); }
    ch.port2.close();
    try { self.postMessage(1, [ch.port2]); out.push("closed: ok"); } catch (x) { out.push("closed: " + E(x)); }
    self.postMessage({ errors: out });
  } else if (d.cmd === "late") {
    // messages posted to the port before this worker got it, held until it is started
    const port = e.ports[0];
    setTimeout(() => {
      const got = [];
      port.onmessage = m => { got.push(m.data); if (got.length === 3) self.postMessage({ late: got }); };
    }, 50);
  } else if (d.cmd === "closed-peer") {
    const port = e.ports[0];
    setTimeout(() => { port.postMessage("after close"); self.postMessage({ posted: 1 }); }, 50);
  }
};
