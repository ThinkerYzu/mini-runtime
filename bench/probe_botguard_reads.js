// What YouTube's BotGuard VM reads from the page (task 135a, GAPS B38), for diffing the runtime against
// Firefox. Run before the page's scripts: MRT_EVAL_BEFORE for the runtime (the list is printed to the
// console as one "BGREADS <json>" line), REPLAY_INJECT with firefox_probe.py for Firefox (return
// Array.from(window.wrappedJSObject.__bgreads)).
//
// Every accessor and method of the web-API interfaces (each global that is not an ECMAScript built-in:
// its prototype's and its own members), the window's own accessors and a few reflection built-ins are
// wrapped. A call is recorded only when BotGuard's code (www.google.com/js/th/...) is on the stack, with
// its receiver's class, its result summarized, deduplicated in first-seen order with a count. Recording
// stops when the first snapshot resolves. The wrappers stringify as native functions, so that Firefox's
// verdict is not changed by the probe.
(function () {
  if (!/youtube\.com$/.test(location.hostname)) return;
  var out = window.__bgreads = [];
  var seen = new Map();
  var on = true;
  var wrappers = new WeakMap();
  var getStack = function () { return new Error().stack || ""; };
  var BUILTINS = new Set(("Object Function Array Number parseFloat parseInt Infinity NaN undefined Boolean String Symbol Date " +
    "Promise RegExp Error AggregateError EvalError RangeError ReferenceError SyntaxError TypeError URIError globalThis JSON Math " +
    "Intl ArrayBuffer Atomics Uint8Array Int8Array Uint16Array Int16Array Uint32Array Int32Array Float32Array Float64Array " +
    "Float16Array Uint8ClampedArray BigUint64Array BigInt64Array BigInt DataView Map Set WeakMap WeakSet Proxy Reflect " +
    "FinalizationRegistry WeakRef SharedArrayBuffer decodeURI decodeURIComponent encodeURI encodeURIComponent escape unescape " +
    "eval isFinite isNaN Iterator InternalError WebAssembly DisposableStack AsyncDisposableStack SuppressedError " +
    "uneval").split(" "));
  function cls(o) {
    try { return Object.prototype.toString.call(o).slice(8, -1); } catch (e) { return "?"; }
  }
  function summary(v) {
    var t = typeof v;
    if (v === null) return "null";
    if (t === "undefined") return "undefined";
    if (t === "string") return "str:" + JSON.stringify(v.length > 100 ? v.slice(0, 100) + "..." + v.length : v);
    if (t === "number" || t === "boolean" || t === "bigint") return t + ":" + String(v);
    if (t === "function") return "fn:" + (v.name || "");
    if (t === "symbol") return "sym";
    var c = cls(v);
    if (Array.isArray(v)) return "array[" + v.length + "]" + (v.length && v.length <= 40 && v.every(function (x) { return typeof x === "string"; }) ? ":" + JSON.stringify(v).slice(0, 400) : "");
    return "obj:" + c;
  }
  var busy = false;  // the probe's own calls while it instruments a frame
  function rec(op, self, args, v, threw) {
    if (!on || busy) return;
    // the direct caller (the frame under getStack, rec and the wrapper) is BotGuard's code: calls the APIs make among
    // themselves (the runtime's bridge.js) are not the VM's
    var st = getStack().split("\n");
    if (!/\/js\/th\//.test(st[3] || "")) return;
    var a = "";
    try { a = Array.prototype.map.call(args, function (x) { return typeof x === "function" ? "fn" : typeof x === "object" && x !== null ? cls(x) : typeof x === "string" ? JSON.stringify(x.slice(0, 60)) : String(x); }).join(","); } catch (e) {}
    var key = op + "(" + a + ") this=" + cls(self) + " -> " + (threw ? "throws " + String(threw && threw.name) + ":" + String(threw && threw.message).slice(0, 80) : summary(v));
    var n = seen.get(key);
    if (n === undefined) { seen.set(key, out.length); out.push([key, 1]); } else out[n][1]++;
  }
  function wrap(f, label, kind) {
    if (typeof f !== "function" || wrappers.has(f)) return f;
    var w = function () {
      var v;
      try { v = new.target ? Reflect.construct(f, arguments, new.target) : f.apply(this, arguments); }
      catch (e) { rec(kind + " " + label, this, arguments, undefined, e); throw e; }
      rec(kind + " " + label, this, arguments, v);
      return v;
    };
    try { Object.defineProperty(w, "name", { value: f.name }); Object.defineProperty(w, "length", { value: f.length }); } catch (e) {}
    wrappers.set(w, f);
    return w;
  }
  function wrapMembers(o, owner) {
    if (!o) return;
    Reflect.ownKeys(o).forEach(function (k) {
      // eval stays as it is: a wrapper would make the VM's direct eval an indirect one
      if (typeof k !== "string" || k === "constructor" || k === "prototype" || (o === window && k === "eval")) return;
      var d;
      try { d = Object.getOwnPropertyDescriptor(o, k); } catch (e) { return; }
      if (!d || !d.configurable) return;
      var label = owner + "." + k;
      try {
        if (d.get || d.set) {
          Object.defineProperty(o, k, { get: d.get && wrap(d.get, label, "get"), set: d.set && wrap(d.set, label, "set"), enumerable: d.enumerable, configurable: true });
        } else if (typeof d.value === "function" && !/^[A-Z]/.test(k)) {
          Object.defineProperty(o, k, { value: wrap(d.value, label, "call"), writable: d.writable, enumerable: d.enumerable, configurable: true });
        }
      } catch (e) {}
    });
  }
  // a window's interfaces and namespaces, its own members and prototype chain, the instances that
  // hold their members themselves (the runtime defines some as plain objects), and its reflection
  // built-ins; tag names the realm ("" for the page, "frame:" for a frame the VM reaches)
  var done = new Set();
  var realms = new WeakSet();
  function instrument(W, tag) {
    if (!W || realms.has(W)) return;
    realms.add(W);
    var wots = W.Function.prototype.toString;
    W.Object.getOwnPropertyNames(W).forEach(function (k) {
      if (BUILTINS.has(k) || k.indexOf("__") === 0) return;
      var d = W.Object.getOwnPropertyDescriptor(W, k);
      var v = d && d.value;
      if (typeof v === "function" && /^[A-Z]/.test(k) && v.prototype) {
        if (!done.has(v.prototype)) { done.add(v.prototype); wrapMembers(v.prototype, tag + k); }
        wrapMembers(v, tag + k + "(static)");
      } else if (v && typeof v === "object" && /^[A-Z]/.test(k) && !done.has(v)) {
        done.add(v); wrapMembers(v, tag + k);  // namespaces such as CSS
      }
    });
    wrapMembers(W, tag + "window");
    for (var p = W.Object.getPrototypeOf(W); p && p !== W.Object.prototype; p = W.Object.getPrototypeOf(p)) {
      if (!done.has(p)) { done.add(p); wrapMembers(p, tag + "window-proto:" + cls(p)); }
    }
    ["navigator", "screen", "document", "location", "history", "performance", "localStorage", "sessionStorage", "visualViewport"].forEach(function (k) {
      var v;
      try { v = W[k]; } catch (e) { return; }
      if (v && typeof v === "object" && !done.has(v)) { done.add(v); wrapMembers(v, tag + k + "(own)"); }
    });
    [[W.Object, "Object", ["getOwnPropertyNames", "getOwnPropertyDescriptor", "getOwnPropertyDescriptors", "keys", "getPrototypeOf"]],
     [W.Reflect, "Reflect", ["ownKeys", "getOwnPropertyDescriptor", "has", "get"]]].forEach(function (e) {
      e[2].forEach(function (k) { e[0][k] = wrap(e[0][k], tag + e[1] + "." + k, "call"); });
    });
    W.Function.prototype.toString = wrap(function toString() {
      if (wrappers.has(this)) return "function " + (wrappers.get(this).name || "") + "() {\n    [native code]\n}";
      return wots.call(this);
    }, tag + "Function.prototype.toString", "call");
    // a frame's window, once the VM has reached it
    var fd = W.Object.getOwnPropertyDescriptor(W.HTMLIFrameElement.prototype, "contentWindow");
    if (fd && fd.get) {
      var g = fd.get;
      W.Object.defineProperty(W.HTMLIFrameElement.prototype, "contentWindow", { get: function () {
        var v = g.call(this);
        if (v && !busy && !realms.has(v)) { busy = true; try { if (/\/js\/th\//.test(getStack())) instrument(v, "frame:"); } catch (e) {} busy = false; }
        return v;
      }, enumerable: fd.enumerable, configurable: true });
      wrappers.set(W.Object.getOwnPropertyDescriptor(W.HTMLIFrameElement.prototype, "contentWindow").get, wrappers.get(g) || g);
    }
  }
  // the runtime makes the HTML element interfaces on first use (EnsureHTMLInterface): name each
  // one first, so that it is there to be wrapped
  ("Anchor Area Audio BR Base Body Button Canvas Data DataList Details Dialog Directory Div DList Embed FieldSet Font Form " +
   "Frame FrameSet HR Head Heading Html IFrame Image Input LI Label Legend Link Map Marquee Media Menu Meta Meter Mod " +
   "Object OList OptGroup Option Output Paragraph Param Picture Pre Progress Quote Script Select Slot Source Span Style " +
   "Table TableCaption TableCell TableCol TableRow TableSection Template TextArea Time Title Track UList Unknown Video").split(" ").forEach(function (n) {
    try { void window["HTML" + n + "Element"]; } catch (e) {}
  });
  instrument(window, "");
  // stop at the first snapshot (kevlar's _.bv.prototype.snapshot)
  var iv = setInterval(function () {
    var K = window.default_kevlar_base;
    if (!K || !K.bv || !K.bv.prototype.snapshot) return;
    clearInterval(iv);
    var os = K.bv.prototype.snapshot;
    K.bv.prototype.snapshot = function (a) {
      var r = os.apply(this, arguments);
      if (on) r.then(function (E) {
        on = false;
        out.push(["SNAPSHOT token " + (E && E.length) + " a3M " + (a && a.a3M ? a.a3M.map(function (x) { return typeof x; }).join(",") : "none"), 1]);
        if (window.__mrt) console.log("BGREADS " + JSON.stringify(out));
      });
      return r;
    };
  }, 1);
})();
