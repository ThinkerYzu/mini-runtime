// Structured-clone values as JSON, for IndexedDB through the bridge (DESIGN "IndexedDB, forwarded to
// Firefox"). One expression, evaluated by both sides: bridge.js (the page's values going to Firefox
// and the records coming back) and the bridge tab's `idb` sandbox (bench/idb_bridge.js).
//
// Strings, booleans, finite numbers and null are themselves; everything else is an array whose first
// item is a tag. Objects get an id in the order they are first met, so shared and cyclic references
// come back as they were (["r", id]), as JS_WriteStructuredClone keeps them. The types are those the
// structured clone algorithm carries in Firefox. Of the platform objects only a Blob or File is
// carried, with its bytes; a DOM node or a function fails with DataCloneError (`fail`), and the
// caller lets Firefox raise its own error. The objects in a message's transfer list that are not
// buffers (MessagePorts) are ["tp", index] into that list: `transferIndex(v)` gives the index (-1 when
// `v` is not in it), and `decode` takes the received objects as `transferred`.
(function () {
  const VIEWS = ["Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array",
                 "Float32Array", "Float64Array", "BigInt64Array", "BigUint64Array", "DataView"];
  const toB64 = u8 => {
    let bin = "";
    for (let i = 0; i < u8.length; i += 0x8000) bin += String.fromCharCode.apply(null, Array.from(u8.subarray(i, i + 0x8000)));
    return btoa(bin);
  };
  const fromB64 = (b64, Uint8) => {
    const bin = atob(b64), u8 = new Uint8(bin.length);
    for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
    return u8;
  };
  const classOf = v => Object.prototype.toString.call(v).slice(8, -1);

  // `unwrap(v)` gives the object to read (Firefox's side waives Xrays); `bytes(buffer, offset, length)`
  // views a buffer's bytes (Firefox's side makes the view in the page's realm: typed array data cannot
  // be read over Xrays); `isPlatform(v)` is true for an object the clone cannot carry;
  // `readBlob(v, set)` is false when `v` is not a real Blob, else true, and calls `set` with its
  // bytes, now or later (Firefox reads a Blob only asynchronously: the caller waits before it sends
  // the encoding). Throws `fail` for what cannot be cloned, after calling `onFail` with it.
  function encode(value, { unwrap = v => v, bytes = (b, o, l) => new Uint8Array(b, o, l), isPlatform = () => false,
                           readBlob = () => false, fail, onFail = () => {}, transferIndex = () => -1 }) {
    const seen = new Map();
    const enc = v => {
      switch (typeof v) {
        case "string": case "boolean": return v;
        case "undefined": return ["u"];
        case "number": return Number.isFinite(v) && !Object.is(v, -0) ? v : ["n", Object.is(v, -0) ? "-0" : String(v)];
        case "bigint": return ["b", String(v)];
        case "symbol": case "function": onFail(v); throw fail;
      }
      if (v === null) return null;
      v = unwrap(v);
      const transferred = transferIndex(v);
      if (transferred >= 0) return ["tp", transferred];
      if (seen.has(v)) return ["r", seen.get(v)];
      const cls = classOf(v);
      if (cls === "Blob" || cls === "File") {
        // ["bl", id, bytes, type, a File's name and lastModified or null]
        const out = ["bl", seen.size, null];
        if (readBlob(v, u8 => { out[2] = toB64(u8); })) {
          seen.set(v, out[1]);
          const isFile = cls === "File";
          out.push(String(v.type), isFile ? String(v.name) : null, isFile ? v.lastModified : null);
          return out;
        }
      }
      if (isPlatform(v)) { onFail(v); throw fail; }
      const id = seen.size;
      seen.set(v, id);
      switch (cls) {
        case "Array": {
          const items = [];
          for (let i = 0; i < v.length; i++) items.push(Object.prototype.hasOwnProperty.call(v, i) ? enc(v[i]) : ["h"]);
          return ["a", id, items];
        }
        case "Date": return ["d", id, v.getTime()];
        case "RegExp": return ["x", id, v.source, v.flags];
        case "ArrayBuffer": return ["ab", id, toB64(bytes(v, 0, v.byteLength))];
        case "Map": return ["m", id, Array.from(v, ([k, x]) => [enc(k), enc(x)])];
        case "Set": return ["s", id, Array.from(v, x => enc(x))];
        case "Boolean": case "Number": case "String": case "BigInt":
          return ["w", id, cls, cls === "BigInt" ? String(v.valueOf()) : v.valueOf()];
        case "Error": return ["e", id, String(v.name), String(v.message)];
      }
      if (VIEWS.includes(cls)) return ["ta", id, cls, toB64(bytes(v.buffer, v.byteOffset, v.byteLength))];
      // what the structured clone refuses: promises, weak collections, proxies of functions
      if (["Promise", "WeakMap", "WeakSet", "WeakRef", "FinalizationRegistry", "Function", "Symbol"].includes(cls)) { onFail(v); throw fail; }
      // any other object is cloned as a plain one: its own enumerable string-keyed properties
      const props = {};
      for (const k of Object.keys(v)) props[k] = enc(v[k]);
      return ["o", id, props];
    };
    return enc(value);
  }

  // `g` is the global whose constructors make the objects (Firefox's side: the page's window, so the
  // values are the page's own).
  function decode(json, g, transferred = []) {
    const refs = [];
    const dec = x => {
      if (x === null || typeof x !== "object") return x;
      switch (x[0]) {
        case "u": return undefined;
        case "n": return x[1] === "-0" ? -0 : Number(x[1]);
        case "b": return g.BigInt(x[1]);
        case "r": return refs[x[1]];
        case "tp": return transferred[x[1]];
        case "a": {
          const a = new g.Array(x[2].length);
          refs[x[1]] = a;
          x[2].forEach((e, i) => { if (!(Array.isArray(e) && e[0] === "h")) a[i] = dec(e); });
          return a;
        }
        case "d": return (refs[x[1]] = new g.Date(x[2]));
        case "x": return (refs[x[1]] = new g.RegExp(x[2], x[3]));
        case "ab": return (refs[x[1]] = fromB64(x[2], g.Uint8Array).buffer);
        case "m": {
          const m = new g.Map();
          refs[x[1]] = m;
          for (const [k, v] of x[2]) m.set(dec(k), dec(v));
          return m;
        }
        case "s": {
          const s = new g.Set();
          refs[x[1]] = s;
          for (const v of x[2]) s.add(dec(v));
          return s;
        }
        case "w": return (refs[x[1]] = g.Object(x[2] === "BigInt" ? g.BigInt(x[3]) : x[3]));
        case "e": return (refs[x[1]] = new (g[x[2]] || g.Error)(x[3]));
        case "ta": {
          const u8 = fromB64(x[3], g.Uint8Array), T = g[x[2]];
          return (refs[x[1]] = x[2] === "DataView" ? new T(u8.buffer) : new T(u8.buffer, 0, u8.byteLength / T.BYTES_PER_ELEMENT));
        }
        case "bl": {
          // made by the global's own constructors, from arrays and options of its realm
          const parts = new g.Array(), opts = new g.Object();
          parts.push(fromB64(x[2], g.Uint8Array));
          opts.type = x[3];
          if (x[4] === null) return (refs[x[1]] = new g.Blob(parts, opts));
          opts.lastModified = x[5];
          return (refs[x[1]] = new g.File(parts, x[4], opts));
        }
        case "o": {
          const o = new g.Object();
          refs[x[1]] = o;
          for (const k of Object.keys(x[2])) o[k] = dec(x[2][k]);
          return o;
        }
      }
      throw new TypeError("clone codec: unknown tag " + x[0]);
    };
    return dec(json);
  }

  return { encode, decode };
})()
