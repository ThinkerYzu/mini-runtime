// The worker of bench/bridge-cases/worker.html: answers each command the page posts, and reports
// what its global looks like.
"use strict";
const names = o => Object.getOwnPropertyNames(o).filter(n => !n.startsWith("__")).sort();
const chain = o => { const r = []; for (let p = Object.getPrototypeOf(o); p; p = Object.getPrototypeOf(p)) r.push(Object.prototype.toString.call(p)); return r; };
const E = e => e ? (e.constructor ? e.constructor.name : "?") + " " + e.name + ": " + e.message : String(e);
let received = 0;
self.addEventListener("message", e => {
  received++;
  const d = e.data;
  const reply = r => postMessage(r);
  switch (d && d.cmd) {
    case "shape":
      reply({
        types: [typeof document, typeof window, typeof self, self === globalThis, typeof importScripts, typeof postMessage,
                typeof close, typeof setTimeout, typeof setInterval, typeof clearTimeout, typeof queueMicrotask,
                typeof structuredClone, typeof atob, typeof btoa, typeof TextEncoder, typeof TextDecoder, typeof URL,
                typeof URLSearchParams, typeof console, typeof Event, typeof EventTarget, typeof MessageEvent,
                typeof ErrorEvent, typeof DOMException, typeof performance, typeof navigator, typeof location,
                typeof requestAnimationFrame, typeof localStorage, typeof sessionStorage, typeof alert, typeof Node],
        name: self.name,
        tag: Object.prototype.toString.call(self),
        chain: chain(self),
        globalNames: names(self).filter(n => /^(on|name|self|location|navigator|postMessage|close|importScripts|dump)/.test(n)),
        protoNames: names(Object.getPrototypeOf(self)),
        scopeNames: names(Object.getPrototypeOf(Object.getPrototypeOf(self))),
        location: [location.pathname, location.search, location.hash, location.protocol, Object.prototype.toString.call(location), String(location) === location.href],
        navigator: [Object.prototype.toString.call(navigator), navigator.onLine, typeof navigator.userAgent, typeof navigator.hardwareConcurrency, navigator.language === undefined ? "undef" : typeof navigator.language],
        onmessage: [self.onmessage, self.onerror, self.onmessageerror],
        received,
      });
      break;
    case "echo":
      reply({ echo: d.value, type: e.type, ctor: e.constructor.name, origin: e.origin, lastEventId: e.lastEventId,
              source: e.source, ports: e.ports.length, bubbles: e.bubbles, cancelable: e.cancelable, target: e.target === self });
      break;
    case "clone": {
      const v = d.value;
      reply({ clone: [v.map instanceof Map, v.map && Array.from(v.map), v.date instanceof Date, v.date && v.date.getTime(),
                      v.nested, v.u8 instanceof Uint8Array, v.u8 && Array.from(v.u8), v.ab instanceof ArrayBuffer,
                      v.ab && v.ab.byteLength, v.re instanceof RegExp, v.re && v.re.source, v.set instanceof Set,
                      v.set && Array.from(v.set), v.self === v, v.big] });
      break;
    }
    case "transfer": {
      const ab = d.value;
      reply({ transferred: ab.byteLength, first: new Uint8Array(ab)[0] });
      break;
    }
    case "import": {
      const r = [];
      try { importScripts("worker-helper.js"); r.push("ok " + self.helperValue); } catch (err) { r.push("throws " + E(err)); }
      try { importScripts("worker-missing.js"); r.push("ok"); } catch (err) { r.push("throws " + E(err)); }
      try { importScripts(); r.push("none ok"); } catch (err) { r.push("none throws " + E(err)); }
      // another origin (Reddit's reCAPTCHA worker imports gstatic.com's script, GAPS B97): it loads,
      // and what it throws, compiling or running, is a NetworkError (muted errors); the same scripts
      // of this origin throw their own errors
      const other = location.href.replace("localhost", "127.0.0.1").replace(/[^/]*$/, "");
      try { importScripts(other + "worker-import-other.js"); r.push("other ok " + self.otherValue); } catch (err) { r.push("other throws " + E(err)); }
      try { importScripts(other + "worker-import-throw.js"); r.push("other throw ok"); } catch (err) { r.push("other throw throws " + E(err)); }
      try { importScripts("worker-import-throw.js"); r.push("own throw ok"); } catch (err) { r.push("own throw throws " + E(err)); }
      try { importScripts(other + "worker-syntax.js"); r.push("other syntax ok"); } catch (err) { r.push("other syntax throws " + E(err)); }
      try { importScripts("worker-syntax.js"); r.push("own syntax ok"); } catch (err) { r.push("own syntax throws " + E(err)); }
      reply({ import: r });
      break;
    }
    case "throw":
      throw new Error("boom " + d.n);
    case "throw-handled":
      self.onerror = function (msg, file, line, col, err) {
        reply({ onerror: [typeof msg, msg, String(file).replace(/^.*\//, ""), line, col, E(err)] });
        self.onerror = null;
        return true;
      };
      throw new TypeError("handled");
    case "throw-listener": {
      const l = ev => {
        reply({ errorEvent: [ev.constructor.name, ev.type, ev.message, String(ev.filename).replace(/^.*\//, ""), ev.lineno, ev.colno, E(ev.error), ev.cancelable, ev.bubbles] });
        self.removeEventListener("error", l);
      };
      self.addEventListener("error", l);
      throw new RangeError("listened");
    }
    case "reject":
      Promise.reject(new Error("rejected"));
      reply({ rejected: true });
      break;
    case "timers": {
      const order = [];
      setTimeout(() => order.push("t10"), 10);
      setTimeout(() => order.push("t0"), 0);
      Promise.resolve().then(() => order.push("micro"));
      queueMicrotask(() => order.push("qm"));
      const iv = setInterval(() => { order.push("iv"); if (order.filter(x => x === "iv").length === 3) clearInterval(iv); }, 5);
      setTimeout(() => reply({ timers: order }), 50);
      break;
    }
    case "many":
      for (let i = 0; i < 3; i++) postMessage({ many: i });
      break;
    case "close":
      postMessage({ closing: 1 });
      close();
      postMessage({ closing: 2 });
      setTimeout(() => postMessage({ closing: "timer" }), 0);
      break;
    case "uncloneable":
      try { postMessage({ f: () => 1 }); } catch (err) { reply({ uncloneable: E(err) }); }
      break;
    case "count":
      reply({ count: received });
      break;
    default:
      reply({ unknown: JSON.stringify(d) });
  }
});
postMessage({ started: true, received });
