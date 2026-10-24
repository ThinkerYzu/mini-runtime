// Shared real WebGL batch executor: used in Marionette's kept sandbox and
// the HTTP viewer. Each instance owns its contexts, object ids and image cache.
// W supplies the canvas realm; onCanvas binds GPU canvases to mirror node ids.
//
// A request: {cmds: [[ctx id, extension or 0, method, arguments, id made or 0], ...], bin, sync}.
// `bin` holds the bytes of the batch's typed arrays (base64), each argument naming its place.
// Arguments cross as JSON (glue/webgl.js's enc): {$u} undefined, {$n} a number JSON cannot hold,
// {$o: id} an object, {$v: type, o, b} a view of `bin`, {$out: type, n} an array the call fills
// (readPixels), {$img} an image by URL (or its bytes), {$canvas: ctx id}, {$blank: [w, h]},
// {$imageData: [w, h], data}, {$d} a dictionary. The commands run in order; a command of a context
// this tab does not have (made before a drop of the bridge tab) is skipped and its id listed in
// `lost`. The answer: the last command's result when `sync`, the outputs it filled, and the
// commands that threw, as [index, name, message].
//
// Values in the page's realm: typed array data cannot cross the Xray, so arrays are made with the
// page's constructors (`W`) and read back with its toBase64.
globalThis.createMrtWebGL = function(W, onCanvas) {
const S = {ctxs: new Map(), objs: new Map(), ids: new Map(), next: 1, images: new Map(), canvases: new Map(), lost: new Set()};
let disposed = false;
const tagOf = v => Object.prototype.toString.call(v).slice(8, -1);
const OBJECT_TYPES = new Set(["WebGLBuffer", "WebGLFramebuffer", "WebGLProgram", "WebGLRenderbuffer", "WebGLShader", "WebGLTexture",
  "WebGLUniformLocation", "WebGLVertexArrayObject", "WebGLQuery", "WebGLSampler", "WebGLSync", "WebGLTransformFeedback"]);

// an <img> of the runtime's, loaded in this tab (its cookies and cache; the replay's recording)
function loadImage(arg) {
  const key = arg.bytes ? "bytes:" + arg.$img : arg.$img;
  let p = S.images.get(key);
  if (!p) {
    p = new Promise(resolve => {
      const img = new W.Image();
      if (arg.cors !== null && arg.cors !== undefined) img.setAttribute("crossorigin", arg.cors);
      img.addEventListener("load", () => resolve(img));
      img.addEventListener("error", () => resolve(img));
      img.src = arg.bytes ? "data:" + (arg.type || "").replace(/[,;].*/, "") + ";base64," + arg.bytes : arg.$img;
      W.setTimeout(() => resolve(img), 5000);
    });
    S.images.set(key, p);
  }
  return p;
}

function main(req, bin) {
  const throws = [];
  const lost = new Set();
  let result = null, outs = [];
  const dec = v => {
    if (v === null || typeof v !== "object") return v;
    if (Array.isArray(v)) return W.Array.from(v.map(dec));
    if ("$u" in v) return undefined;
    if ("$n" in v) return v.big ? BigInt(v.$n) : Number(v.$n);
    if ("$o" in v) return S.objs.has(v.$o) ? S.objs.get(v.$o) : null;
    if ("$v" in v) {
      if (v.$v === "ArrayBuffer") return bin.buffer.slice(v.o, v.o + v.b);
      if (v.$v === "DataView") return new W.DataView(bin.buffer, v.o, v.b);
      return new W[v.$v](bin.buffer, v.o, v.b / W[v.$v].BYTES_PER_ELEMENT);
    }
    if ("$out" in v) {
      const a = v.$out === "DataView" ? new W.DataView(new W.ArrayBuffer(v.n)) : new W[v.$out](v.n);
      outs.push(a);
      return a;
    }
    if ("$img" in v) return v.loaded;
    if ("$canvas" in v) { const c = S.ctxs.get(v.$canvas); return c ? c.canvas : blank(1, 1); }
    if ("$blank" in v) return blank(v.$blank[0], v.$blank[1]);
    if ("$imageData" in v) return new W.ImageData(new W.Uint8ClampedArray(bin.buffer, v.data.o, v.data.b), v.$imageData[0], v.$imageData[1]);
    if ("$d" in v) {
      const o = new W.Object();
      for (const [k, x] of Object.entries(v.$d)) o[k] = dec(x);
      return o;
    }
    return v;
  };
  const blank = (w, h) => {
    const c = W.document.createElement("canvas");
    c.width = w;
    c.height = h;
    return c;
  };
  // a result as the runtime decodes it (webgl.js's dec)
  const enc = v => {
    if (v === undefined) return { $u: 1 };
    if (v === null || typeof v === "boolean" || typeof v === "string") return v;
    if (typeof v === "number") return Number.isFinite(v) && !Object.is(v, -0) ? v : { $n: Object.is(v, -0) ? "-0" : String(v) };
    if (typeof v === "bigint") return { $n: String(v), big: 1 };
    const tag = tagOf(v);
    if (OBJECT_TYPES.has(tag)) {
      let id = S.ids.get(v);
      if (id === undefined) {
        id = "h" + S.next++;
        S.ids.set(v, id);
        S.objs.set(id, v);
      }
      return { $o: id, $t: tag };
    }
    if (tag === "WebGLActiveInfo") return { $ai: [v.name, v.size, v.type] };
    if (tag === "WebGLShaderPrecisionFormat") return { $pf: [v.rangeMin, v.rangeMax, v.precision] };
    if (/Array$/.test(tag) && tag !== "Array") return { $v: tag, b64: W.Uint8Array.prototype.toBase64.call(new W.Uint8Array(v.buffer, v.byteOffset, v.byteLength)) };
    if (tag === "Array") return Array.from(v.wrappedJSObject || v, enc);
    const d = {};
    for (const k of Object.keys(v.wrappedJSObject || v)) d[k] = enc(v[k]);
    return { $d: d };
  };
  for (let i = 0; i < req.cmds.length; i++) {
    const [cid, ext, method, args, made] = req.cmds[i];
    const last = i === req.cmds.length - 1;
    outs = [];
    try {
      if (method === "$create") {
        const [type, attrs, w, h, node] = args;
        const canvas = blank(w, h);
        const a = new W.Object();
        for (const [k, x] of Object.entries(attrs)) a[k] = x;
        const gl = canvas.getContext(type, a);
        if (!gl) { result = null; continue; }
        const old = S.ctxs.get(cid);
        if (old) old.gl.getExtension("WEBGL_lose_context")?.loseContext();
        const state = { canvas, gl, node, exts: new Map() };
        S.ctxs.set(cid, state);
        canvas.addEventListener("webglcontextlost", e => {
          e.preventDefault();
          if (S.ctxs.get(cid) === state) S.lost.add(cid);
        });
        if (node) { S.canvases.set(node, canvas); onCanvas?.(node, canvas); }
        result = { attrs: enc(gl.getContextAttributes()).$d, exts: Array.from(gl.getSupportedExtensions()) };
        continue;
      }
      const c = S.ctxs.get(cid);
      if (!c) { lost.add(cid); result = null; continue; }
      if (method === "$drop") {
        S.ctxs.delete(cid);
        if (c.node) S.canvases.delete(c.node);
        c.gl.getExtension("WEBGL_lose_context")?.loseContext();
        continue;
      }
      if (c.gl.isContextLost()) { lost.add(cid); result = null; continue; }
      if (method === "$size") { c.canvas.width = args[0]; c.canvas.height = args[1]; continue; }
      if (method === "$dbsize") { result = [c.gl.drawingBufferWidth, c.gl.drawingBufferHeight]; continue; }
      if (method === "$ext") { c.exts.set(args[0], c.gl.getExtension(args[0])); continue; }
      if (method === "$set") {
        if (!["drawingBufferColorSpace", "unpackColorSpace"].includes(args[0])) throw new TypeError("Unknown WebGL attribute");
        c.gl[args[0]] = args[1]; continue;
      }
      if (method === "$dataURL") { result = c.canvas.toDataURL(...args.map(dec)); continue; }
      const target = ext ? c.exts.get(ext) : c.gl;
      if (!target) { result = null; continue; }
      if (["constructor", "__proto__", "prototype"].includes(method) || typeof target[method] !== "function")
        throw new TypeError("Unknown WebGL method");
      const r = target[method](...args.map(dec));
      if (made) {
        S.objs.set(made, r);
        if (r) S.ids.set(r, made);
      }
      if (last && req.sync) {
        result = enc(r);
        outs = outs.map(a => W.Uint8Array.prototype.toBase64.call(new W.Uint8Array(a.buffer, a.byteOffset, a.byteLength)));
      }
    } catch (e) {
      throws.push([i, e.name || "Error", String(e.message || e)]);
      result = null;
    }
  }
  const lastThrew = req.sync && throws.length && throws[throws.length - 1][0] === req.cmds.length - 1 ? throws[throws.length - 1].slice(1) : null;
  return { result, outs: req.sync ? outs : [], throws, lost: Array.from(lost), lastThrew };
}


return {
  canvases: S.canvases,
  takeLost() { const ids = [...S.lost]; S.lost.clear(); return ids; },
  dispose() {
    disposed = true;
    for (const c of S.ctxs.values()) c.gl.getExtension("WEBGL_lose_context")?.loseContext();
    S.ctxs.clear(); S.objs.clear(); S.ids.clear(); S.images.clear(); S.canvases.clear(); S.lost.clear();
  },
  async execute(req) {
    for (const cmd of req.cmds) for (const a of cmd[3])
      if (a && typeof a === "object" && "$img" in a) a.loaded = await loadImage(a);
    if (disposed) return {result: null, outs: [], throws: [], lost: [...new Set(req.cmds.map(c => c[0]))], lastThrew: null};
    const bin = req.bin ? W.Uint8Array.fromBase64(req.bin) : null;
    return main(req, bin);
  }
};
};
