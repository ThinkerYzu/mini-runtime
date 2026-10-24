  // ---- WebGL, forwarded to the bridge tab in batches (SPEC decision 23, task 23bb) ----
  //
  // Gecko runs WebGL out of the content process (dom/canvas): ClientWebGLContext holds the API, its
  // object ids and the state it can answer itself; each call is serialized into WebGLChild's pending
  // buffer (Run<RPROC(...)>), which FlushPendingCmds sends as one message, at each paint
  // (OnBeforePaintTransaction -> Present), when the buffer is full (webgl.out-of-process.shmem-size,
  // 100 KB), and before a call that needs an answer (getError, getParameter, readPixels, ...), which
  // then waits on a sync message; HostWebGLContext runs the commands on the real context. Here the
  // queue is the window's (`glQueue`), flushed at the same three points through the bridge's `webgl`
  // request, and the bridge tab (bench/webgl_bridge.js) runs each batch on real WebGL contexts of
  // canvases it makes, objects mapped by the ids given here. The interfaces come from Gecko's WebIDL
  // (webgl_idl.js, generated): constants, attributes, and per operation its argument count, return
  // type and, when it has one overload, its argument types, converted here as a binding would.
  // Overloads, enums and GL errors are left to the bridge's real context, so a GL error is Firefox's.
  // A context lives in the bridge tab; when the tab is dropped (a hibernated page under pressure,
  // SPEC decision 14) the context is lost as on a GPU process loss in Gecko, and restored in the new
  // tab if the page prevented the default of `webglcontextlost` (no state is replayed: the page
  // rebuilds it, as a page must for any context loss). Not built: OffscreenCanvas and WebGL in
  // workers (GAPS B130), ImageBitmap sources, a video's frames (sent as a blank of its size), and the
  // clear of the drawing buffer after a paint without preserveDrawingBuffer (the bridge's canvas is
  // never shown).
  // HTTP viewers (abdd4) use the same queue/executor and mirror canvas ids.
  // With API_PROCESS's `webgl`, the API process's real contexts answer (B169) and
  // the viewer only draws; bounded replay preserves logical contexts across
  // attachment. Only native GPU loss/overflow (or the API process ending) loses
  // them. The original privileged bridge lifecycle above is unchanged.
  // @WEBGL_IDL@
  if (!isWorker) {
    const GL_CONTEXT_LOST_WEBGL = 0x9242, GL_INVALID_ENUM = 0x0500, GL_INVALID_OPERATION = 0x0502;
    const GL_VERTEX_SHADER = 0x8B31, GL_FRAGMENT_SHADER = 0x8B30;
    // webgl.out-of-process.shmem-size: the pending buffer's size, a flush when a command passes it
    const PENDING_LIMIT = 100000;
    const glStats = share("webglStats", () => ({ contexts: 0, commands: 0, flushes: 0, syncs: 0, bytes: 0, throws: 0, lost: 0, restored: 0 }));
    // ids of contexts and objects, unique in the process (the bridge tab keeps one table for all)
    const glIds = share("webglIds", () => ({ next: 1 }));
    const glState = new WeakMap();    // a context -> its state (ClientWebGLContext's)
    const glObjects = new WeakMap();  // a WebGL object -> { ctx, id, iface, gen }
    const glObjectsById = new Map();  // id -> WeakRef of the object (objects the bridge names in an answer)
    const glExtensions = new WeakMap(); // an extension object -> { st, name }
    const glContexts = new Set();     // the window's live contexts (the bridge's loss reaches each)

    // the window's pending commands: [ctx id, extension name or 0, method, arguments, id made or 0],
    // and the bytes of the typed arrays they carry, at 8-byte aligned offsets of one buffer
    const glQueue = { cmds: [], chunks: [], binBytes: 0, size: 0, paint: false };
    const binRef = v => {
      const isBuffer = v instanceof ArrayBuffer || (typeof SharedArrayBuffer === "function" && v instanceof SharedArrayBuffer);
      const bytes = isBuffer ? new Uint8Array(v) : new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
      const o = (glQueue.binBytes + 7) & ~7;
      glQueue.chunks.push([o, bytes.slice()]);
      glQueue.binBytes = o + bytes.length;
      return { $v: isBuffer ? "ArrayBuffer" : Object.prototype.toString.call(v).slice(8, -1), o, b: bytes.length };
    };
    // the canvas size, as HTMLCanvasElement::GetWidthHeight: the attribute as a non-negative integer,
    // else 300 by 150
    const canvasSize = (canvas, name) => {
      const m = /^[\t\n\f\r ]*\+?(\d+)/.exec(canvas.getAttribute(name) || "");
      const v = m ? Number(m[1]) : NaN;
      return Number.isFinite(v) && v <= 0x7fffffff ? v : (name === "width" ? 300 : 150);
    };
    // An argument as it crosses the bridge (webgl_bridge.js's dec): numbers JSON cannot hold, WebGL
    // objects by id, typed arrays and buffers by their place in the batch's bytes, image sources by
    // what the bridge can rebuild them from
    const enc = (v, st) => {
      if (v === undefined) return { $u: 1 };
      if (v === null || typeof v === "boolean" || typeof v === "string") return v;
      if (typeof v === "number") return Number.isFinite(v) && !Object.is(v, -0) ? v : { $n: Object.is(v, -0) ? "-0" : String(v) };
      if (typeof v === "bigint") return { $n: String(v), big: 1 };
      if (typeof v !== "object" && typeof v !== "function") return String(v);
      const obj = glObjects.get(v);
      if (obj) return obj.gen === obj.ctx.gen ? { $o: obj.id } : null;
      if (ArrayBuffer.isView(v) || v instanceof ArrayBuffer || (typeof SharedArrayBuffer === "function" && v instanceof SharedArrayBuffer)) return binRef(v);
      if (Array.isArray(v)) return v.map(x => enc(x, st));
      if (typeof HTMLImageElement === "function" && v instanceof HTMLImageElement) {
        const url = v.currentSrc;
        if (url && isBlobURL(url)) {
          const entry = __mrtBlobURL(2, url, global.origin, true, clockNow());
          if (entry) return { $img: url, bytes: entry[0].toBase64(), type: entry[1] };
        }
        return { $img: url, cors: v.getAttribute("crossorigin") };
      }
      if (typeof HTMLCanvasElement === "function" && v instanceof HTMLCanvasElement) {
        const other = contexts.get(v);
        const s = other && glState.get(other);
        if (s && !s.lost) return { $canvas: s.id };
        // a 2d canvas draws nothing here (B15): its pixels are transparent
        stub("WebGL texture from a 2d canvas");
        return { $blank: [canvasSize(v, "width"), canvasSize(v, "height")] };
      }
      if (typeof HTMLVideoElement === "function" && v instanceof HTMLVideoElement) {
        stub("WebGL texture from a video");
        return { $blank: [v.videoWidth | 0, v.videoHeight | 0] };
      }
      // ImageData (the 2d stub's are plain objects with its members)
      if (typeof v.width === "number" && typeof v.height === "number" && v.data instanceof Uint8ClampedArray) {
        return { $imageData: [v.width, v.height], data: binRef(v.data) };
      }
      stub("WebGL argument " + Object.prototype.toString.call(v));
      return { $d: Object.fromEntries(Object.entries(v).map(([k, x]) => [k, enc(x, st)])) };
    };
    // An answer from the bridge (webgl_bridge.js's enc) as the page's value
    const dec = (v, st) => {
      if (v === null || typeof v !== "object") return v;
      if (Array.isArray(v)) return v.map(x => dec(x, st));
      if ("$u" in v) return undefined;
      if ("$n" in v) return v.big ? BigInt(v.$n) : Number(v.$n);
      if ("$v" in v) {
        const bytes = Uint8Array.fromBase64(v.b64);
        return v.$v === "ArrayBuffer" ? bytes.buffer : new global[v.$v](bytes.buffer);
      }
      if ("$o" in v) {
        const ref = glObjectsById.get(v.$o);
        const known = ref && ref.deref();
        if (known) return known;
        return makeObject(st, v.$t, v.$o);
      }
      if ("$ai" in v) return makeInfo(WebGLActiveInfo, { size: v.$ai[1], type: v.$ai[2], name: v.$ai[0] });
      if ("$pf" in v) return makeInfo(WebGLShaderPrecisionFormat, { rangeMin: v.$pf[0], rangeMax: v.$pf[1], precision: v.$pf[2] });
      if ("$d" in v) return Object.fromEntries(Object.entries(v.$d).map(([k, x]) => [k, dec(x, st)]));
      return v;
    };

    // Sends the window's pending commands as one `webgl` request (FlushPendingCmds), the last one
    // waiting on its answer when `sync`. The answer: the last command's result and the outputs it
    // filled, the commands that threw, and the contexts the bridge does not have (a new tab).
    function flush(sync) {
      if (glQueue.cmds.length === 0) return null;
      const cmds = glQueue.cmds, chunks = glQueue.chunks, binBytes = glQueue.binBytes;
      glQueue.cmds = [];
      glQueue.chunks = [];
      glQueue.binBytes = 0;
      glQueue.size = 0;
      let bin = null;
      if (binBytes > 0) {
        const all = new Uint8Array(binBytes);
        for (const [o, bytes] of chunks) all.set(bytes, o);
        bin = all.toBase64();
      }
      glStats.flushes++;
      glStats.bytes += binBytes;
      if (sync) glStats.syncs++;
      const r = bridge("webgl", { cmds, bin, sync: !!sync });
      for (const [i, name, message] of r.throws || []) {
        if (sync && i === cmds.length - 1) continue;
        glStats.throws++;
        stub(`WebGL ${cmds[i][2]} threw in the bridge: ${name}: ${String(message).slice(0, 80)}`);
      }
      // contexts the bridge tab does not have: it was made again since (bridgeLost has run)
      if (r.viewer) global.__mrt.webglViewer(r.viewer);
      for (const id of r.lost || []) {
        for (const st of glContexts) if (st.id === id && !st.lost) loseContext(st, "bridge");
      }
      return r;
    }
    // A command for the bridge, queued; the first one since the last paint asks for a refresh tick,
    // whose paint flushes the queue (OnBeforePaintTransaction)
    function append(st, ext, method, args, made) {
      glQueue.cmds.push([st.id, ext, method, args, made || 0]);
      glQueue.size += 16 + args.length * 8;
      glStats.commands++;
      if (glQueue.binBytes + glQueue.size > PENDING_LIMIT) flush(false);
      else if (!glQueue.paint) {
        glQueue.paint = true;
        global.__mrt.requestPaint(paint);
      }
    }
    function paint() {
      glQueue.paint = false;
      flush(false);
    }
    // A command that needs its answer: everything pending, then it, then the wait
    function call(st, ext, method, args, outs, made) {
      glQueue.cmds.push([st.id, ext, method, args, made || 0]);
      glStats.commands++;
      const r = flush(true);
      // the call itself threw in the bridge: it throws here, as the binding would have
      if (r.lastThrew) {
        const [name, message] = r.lastThrew;
        if (name === "TypeError") throw new TypeError(message);
        if (name === "RangeError") throw new RangeError(message);
        throw new DOMException(message, name);
      }
      if (outs) {
        (r.outs || []).forEach((b64, i) => {
          if (b64 === null || !outs[i]) return;
          const bytes = Uint8Array.fromBase64(b64);
          new Uint8Array(outs[i].buffer, outs[i].byteOffset, outs[i].byteLength).set(bytes.subarray(0, outs[i].byteLength));
        });
      }
      return r;
    }

    // ---- the objects ----

    const illegalGL = name => ({ [name]: function () { throw new TypeError("Illegal constructor."); } })[name];
    const glBrand = (C, member) => new TypeError(`'${member}' called on an object that does not implement interface ${C}.`);
    const OBJECT_IFACES = WEBGL_GLOBALS.filter(n => !/RenderingContext$/.test(n) && n !== "WebGLActiveInfo" && n !== "WebGLShaderPrecisionFormat");
    const ifaces = {};
    for (const name of WEBGL_GLOBALS) {
      const C = illegalGL(name);
      Object.defineProperty(C.prototype, Symbol.toStringTag, { value: name, configurable: true });
      ifaces[name] = C;
    }
    const WebGLActiveInfo = ifaces.WebGLActiveInfo, WebGLShaderPrecisionFormat = ifaces.WebGLShaderPrecisionFormat;
    const infoData = new WeakMap();
    const makeInfo = (C, data) => {
      const o = Object.create(C.prototype);
      infoData.set(o, Object.assign({ iface: C }, data));
      return o;
    };
    for (const [C, members] of [[WebGLActiveInfo, ["size", "type", "name"]], [WebGLShaderPrecisionFormat, ["rangeMin", "rangeMax", "precision"]]]) {
      for (const member of members) {
        Object.defineProperty(C.prototype, member, {
          get() {
            const d = infoData.get(this);
            if (!d || d.iface !== C) throw glBrand(C.name, `get ${member}`);
            return d[member];
          },
          configurable: true, enumerable: true,
        });
      }
    }
    function makeObject(st, iface, id) {
      const o = Object.create(ifaces[iface].prototype);
      glObjects.set(o, { ctx: st, id, iface, gen: st.gen });
      glObjectsById.set(id, new WeakRef(o));
      return o;
    }
    // a context's state is dropped from the bridge tab with it (a canvas collected with its context)
    const glFinalizer = new FinalizationRegistry(id => {
      glQueue.cmds.push([id, 0, "$drop", [], 0]);
      for (const [oid, ref] of glObjectsById) if (!ref.deref()) glObjectsById.delete(oid);
    });

    // ---- the contexts ----

    // The context made in the bridge tab (ClientWebGLContext::CreateHostContext): its attributes and
    // extensions, or null when Firefox makes none
    function createHost(st) {
      st.w = canvasSize(st.canvas, "width");
      st.h = canvasSize(st.canvas, "height");
      st.dbSize = null;
      const r = call(st, 0, "$create", [st.type, st.attrs, st.w || 1, st.h || 1, global.__mrt.mirrorNodeId(st.canvas)]);
      return r.result;
    }
    // The canvas's size sent before what depends on it (HTMLCanvasElement's attributes reach
    // ClientWebGLContext::SetDimensions; a resize clears the drawing buffer)
    function syncSize(st) {
      const w = canvasSize(st.canvas, "width"), h = canvasSize(st.canvas, "height");
      if (w === st.w && h === st.h) return;
      st.w = w;
      st.h = h;
      st.dbSize = null;
      append(st, 0, "$size", [w || 1, h || 1]);
    }
    const SIZED = new Set(["clear", "drawArrays", "drawElements", "drawArraysInstanced", "drawElementsInstanced", "drawRangeElements",
      "readPixels", "copyTexImage2D", "copyTexSubImage2D", "copyTexSubImage3D", "blitFramebuffer", "drawArraysInstancedANGLE",
      "drawElementsInstancedANGLE", "viewport", "scissor"]);
    // OnContextLoss: lost now, the error CONTEXT_LOST_WEBGL, extensions detached, then the event as a
    // task; a context lost with the bridge restores in a new one if the page prevented the default
    function loseContext(st, reason) {
      if (st.lost) return;
      glStats.lost++;
      st.gen++;
      st.lost = true;
      st.status = reason === "manual" ? "LostManually" : "Lost";
      st.nextError = GL_CONTEXT_LOST_WEBGL;
      // the extensions are detached, but WEBGL_lose_context, which the context keeps (mExtLoseContext)
      const keep = st.extObjects.get("WEBGL_lose_context");
      st.extObjects.clear();
      if (keep) st.extObjects.set("WEBGL_lose_context", keep);
      if (reason === "manual") append(st, 0, "$drop", []);
      global.__mrt.queueTask(() => {
        const ev = new Event("webglcontextlost", { bubbles: true, cancelable: true });
        st.canvas.dispatchEvent(ev);
        if (!ev.defaultPrevented) st.status = "LostForever";
        if (st.status === "Lost") restoreContext(st, "Lost");
      });
    }
    function restoreContext(st, required) {
      if (st.status !== required) {
        if (!st.nextError) st.nextError = GL_INVALID_OPERATION;
        return;
      }
      if (st.awaitingRestore) return;
      st.awaitingRestore = true;
      global.__mrt.queueTask(() => {
        st.awaitingRestore = false;
        st.status = "Ready";
        st.nextError = 0;
        st.gen++;
        st.lost = false;
        const made = createHost(st);
        if (!made) { st.lost = true; st.status = "LostForever"; return; }
        st.made = made;
        glStats.restored++;
        st.canvas.dispatchEvent(new Event("webglcontextrestored", { bubbles: true, cancelable: true }));
      });
    }
    // HTTP attachment preserves contexts through command replay. Only actual
    // GPU loss or replay overflow loses the runtime's logical context.
    global.__mrt.webglViewer = v => {
      for (const st of glContexts)
        if ((v.lost || []).includes(st.id)) loseContext(st, "bridge");
    };
    bridgeLostHooks.push(() => {
      glQueue.cmds = [];
      glQueue.chunks = [];
      glQueue.binBytes = 0;
      glQueue.size = 0;
      for (const st of glContexts) loseContext(st, "bridge");
    });

    // The value an operation gives on a lost context (the binding's default; [WebGLHandlesContextLoss]
    // ones answer as Gecko's do)
    const lostValue = (name, ret) => {
      if (ret === "GLboolean" || ret === "boolean") return false;
      if (name === "getAttribLocation" || name === "getFragDataLocation") return -1;
      if (ret === "GLenum" || ret === "GLint" || ret === "GLuint" || ret === "GLintptr") return 0;
      return ret === "undefined" ? undefined : null;
    };
    // a binding's conversion of an argument to its IDL type
    const toInt64 = v => { const n = Number(v); return Number.isFinite(n) ? Math.trunc(n) : 0; };
    const CONVERT = {
      GLenum: v => Number(v) >>> 0, GLuint: v => Number(v) >>> 0, GLbitfield: v => Number(v) >>> 0,
      GLint: v => Number(v) | 0, GLsizei: v => Number(v) | 0,
      GLintptr: toInt64, GLsizeiptr: toInt64, GLint64: toInt64, GLuint64: toInt64,
      GLfloat: v => Math.fround(Number(v)), GLclampf: v => Math.fround(Number(v)),
      GLboolean: v => !!v, DOMString: v => String(v),
    };
    // The arguments of a call with one overload converted, and its WebGL objects checked, as the binding
    // does before the method runs (its TypeErrors with Gecko's messages)
    function convertArgs(iface, name, types, args) {
      const out = [];
      for (let i = 0; i < args.length; i++) {
        const t = types && types[i];
        const v = args[i];
        if (!t) { out.push(v); continue; }
        const conv = CONVERT[t];
        if (conv) { out.push(conv(v)); continue; }
        const objType = t.replace(/\?$/, "");
        if (ifaces[objType] && OBJECT_IFACES.includes(objType)) {
          if (v === null || v === undefined) {
            if (!t.endsWith("?")) throw new TypeError(`${iface}.${name}: Argument ${i + 1} is not an object.`);
            out.push(null);
            continue;
          }
          const o = glObjects.get(v);
          if (!o || o.iface !== objType) {
            throw new TypeError(typeof v === "object" || typeof v === "function"
              ? `${iface}.${name}: Argument ${i + 1} does not implement interface ${objType}.`
              : `${iface}.${name}: Argument ${i + 1} is not an object.`);
          }
        }
        out.push(v);
      }
      return out;
    }
    // an encoded <img> argument of another origin than the window's (blob: and data: ones are its own)
    const foreignImage = a => {
      if (!a || typeof a.$img !== "string" || a.bytes || /^(data|blob):/i.test(a.$img)) return false;
      try { return new URL(a.$img).origin !== global.origin; } catch (e) { return false; }
    };
    // readPixels and getBufferSubData fill the page's view: it goes as its type and length, and the
    // bridge's array comes back
    const OUT_ARG = { readPixels: 6, getBufferSubData: 2 };

    // One operation of a context or an extension (Run<RPROC(...)> or a sync call)
    function operation(iface, ext, name, spec) {
      const [least, ret, types] = spec;
      const made = /^(WebGL\w+)\??$/.exec(ret);
      const creates = made && OBJECT_IFACES.includes(made[1]) && (name.startsWith("create") || name === "fenceSync");
      const plural = least === 1 ? "argument" : "arguments";
      return {
        [name](...args) {
          let st;
          if (ext) {
            const e = glExtensions.get(this);
            if (!e || e.name !== ext) throw glBrand(ext, name);
            st = e.st;
          } else {
            st = glState.get(this);
            if (!st || st.iface !== iface) throw glBrand(iface, name);
          }
          if (args.length < least) throw new TypeError(`${ext || iface}.${name}: At least ${least} ${plural} required, but only ${args.length} passed`);
          args = convertArgs(ext || iface, name, types, args);
          if (creates) {
            if (name === "createShader" && args[0] !== GL_VERTEX_SHADER && args[0] !== GL_FRAGMENT_SHADER) {
              if (!st.lost && !st.nextError) st.nextError = GL_INVALID_ENUM;
              return null;
            }
            const id = glIds.next++;
            const o = makeObject(st, made[1], id);
            if (!st.lost) append(st, ext || 0, name, args.map(a => enc(a, st)), id);
            return o;
          }
          // the error the context set itself comes first, lost or not (ClientWebGLContext::GetError)
          if (name === "getError" && st.nextError) { const e = st.nextError; st.nextError = 0; return e; }
          if (st.lost || (ext && glExtensions.get(this).gen !== st.gen)) return lostValue(name, ret);
          if (SIZED.has(name)) syncSize(st);
          const outAt = OUT_ARG[name];
          if (outAt !== undefined && ArrayBuffer.isView(args[outAt])) {
            const view = args[outAt];
            const sent = args.map((a, i) => (i === outAt ? { $out: Object.prototype.toString.call(view).slice(8, -1), n: view.length !== undefined ? view.length : view.byteLength } : enc(a, st)));
            call(st, ext || 0, name, sent, [view]);
            return undefined;
          }
          const sent = args.map(a => enc(a, st));
          // another origin's image may be tainted: the upload waits, so that its SecurityError throws here
          if (ret === "undefined" && sent.some(foreignImage)) { call(st, ext || 0, name, sent); return undefined; }
          if (ret === "undefined") { append(st, ext || 0, name, sent); return undefined; }
          if (ret === "Promise<undefined>") { append(st, ext || 0, name, sent); return Promise.resolve(); }
          // Uniform locations need stable ids across dummy/real replay too.
          const id = name === "getUniformLocation" ? glIds.next++ : 0;
          return dec(call(st, ext || 0, name, sent, null, id).result, st);
        },
      }[name];
    }

    // the release-Firefox prefs and [Func]s of the WebGL members: what it exposes
    const PREF_ON = { "webgl.enable-webgl2": true, "webgl.drawing_buffer_color_space": true };
    const prefOn = p => !p || PREF_ON[p] === true;
    const defineConsts = (target, consts) => {
      for (const [k, v] of Object.entries(consts)) Object.defineProperty(target, k, { value: v, enumerable: true });
    };
    const defineOps = (proto, iface, ext, ops) => {
      for (const [name, spec] of Object.entries(ops)) {
        if (!prefOn(spec[3])) continue;
        if (CONTEXT_OWN[name] && !ext) continue;
        Object.defineProperty(proto, name, { value: operation(iface, ext, name, spec), writable: true, configurable: true, enumerable: true });
      }
    };
    // the operations the context answers itself (Gecko's client state)
    const ownMember = (iface, name, fn) => function (...args) {
      const st = glState.get(this);
      if (!st || st.iface !== iface) throw glBrand(iface, name);
      return fn(st, ...args);
    };
    const CONTEXT_OWN = {
      getContextAttributes: st => (st.lost ? null : Object.assign({}, st.made.attrs)),
      isContextLost: st => st.lost,
      getSupportedExtensions: st => (st.lost ? null : st.made.exts.slice()),
      getExtension(st, ...args) {
        if (args.length < 1) throw new TypeError(`${st.iface}.getExtension: At least 1 argument required, but only 0 passed`);
        if (st.lost) return null;
        const wanted = String(args[0]).toLowerCase();
        const name = st.made.exts.find(n => n.toLowerCase() === wanted);
        if (!name) return null;
        let o = st.extObjects.get(name);
        if (o) return o;
        o = Object.create(extProtos[name] || extProtos.$unknown);
        if (!extProtos[name]) stub("WebGL extension without an interface: " + name);
        glExtensions.set(o, { st, name, gen: st.gen });
        st.extObjects.set(name, o);
        append(st, 0, "$ext", [name]);
        return o;
      },
    };
    const extProtos = { $unknown: {} };
    for (const name of WEBGL_EXTENSIONS) {
      const spec = WEBGL_IDL[name];
      const proto = {};
      Object.defineProperty(proto, Symbol.toStringTag, { value: name, configurable: true });
      defineConsts(proto, spec.consts);
      defineOps(proto, name, name, spec.ops);
      extProtos[name] = proto;
    }
    // WEBGL_lose_context's two operations act on the client (EmulateLoseContext, RestoreContext)
    if (extProtos.WEBGL_lose_context) {
      const own = (name, fn) => Object.defineProperty(extProtos.WEBGL_lose_context, name, {
        value: { [name]() { const e = glExtensions.get(this); if (!e || e.name !== "WEBGL_lose_context") throw glBrand("WEBGL_lose_context", name); fn(e.st); } }[name],
        writable: true, configurable: true, enumerable: true });
      own("loseContext", st => {
        if (st.status !== "Ready") { if (!st.nextError) st.nextError = GL_INVALID_OPERATION; return; }
        loseContext(st, "manual");
      });
      own("restoreContext", st => restoreContext(st, "LostManually"));
    }

    for (const iface of ["WebGLRenderingContext", "WebGL2RenderingContext"]) {
      const spec = WEBGL_IDL[iface];
      if (!prefOn(spec.pref)) continue;
      const C = ifaces[iface];
      defineConsts(C, spec.consts);
      defineConsts(C.prototype, spec.consts);
      const proto = C.prototype;
      getter(proto, "canvas", ownMember(iface, "get canvas", st => st.canvas));
      for (const which of ["drawingBufferWidth", "drawingBufferHeight"]) {
        getter(proto, which, ownMember(iface, "get " + which, st => {
          if (st.lost) return 0;
          syncSize(st);
          if (!st.dbSize) st.dbSize = call(st, 0, "$dbsize", []).result;
          return st.dbSize[which === "drawingBufferWidth" ? 0 : 1];
        }));
      }
      for (const which of ["drawingBufferColorSpace", "unpackColorSpace"]) {
        getter(proto, which, ownMember(iface, "get " + which, st => st[which]), ownMember(iface, "set " + which, (st, v) => {
          v = String(v);
          if (v !== "srgb" && v !== "display-p3") return;
          st[which] = v;
          if (!st.lost) append(st, 0, "$set", [which, v]);
        }));
      }
      for (const [name, fn] of Object.entries(CONTEXT_OWN)) {
        Object.defineProperty(proto, name, { value: { [name]: ownMember(iface, name, fn) }[name], writable: true, configurable: true, enumerable: true });
      }
      defineOps(proto, iface, 0, spec.ops);
    }
    // the interfaces, in Gecko's order, but WebGL2's when its pref is off
    for (const name of WEBGL_GLOBALS) if (prefOn(WEBGL_IDL[name].pref)) define(global, name, ifaces[name]);

    // WebGLContextEvent (dom/canvas/WebGLContextEvent.webidl)
    const eventMessages = new WeakMap();
    class WebGLContextEvent extends Event {
      constructor(type, init = {}) {
        if (arguments.length < 1) throw new TypeError("WebGLContextEvent constructor: At least 1 argument required, but only 0 passed");
        super(type, init);
        eventMessages.set(this, init && init.statusMessage !== undefined ? String(init.statusMessage) : "");
      }
      get statusMessage() {
        if (!eventMessages.has(this)) throw glBrand("WebGLContextEvent", "get statusMessage");
        return eventMessages.get(this);
      }
    }
    Object.defineProperty(WebGLContextEvent.prototype, Symbol.toStringTag, { value: "WebGLContextEvent", configurable: true });
    define(global, "WebGLContextEvent", WebGLContextEvent);

    // The WebGL context of a canvas (CanvasRenderingContextHelper::GetOrCreateContext): the same one
    // again for its type, null for another type; the attributes as WebGLContextAttributes converts them
    const CONTEXT_TYPES = { webgl: 1, "experimental-webgl": 1, webgl2: 2 };
    const ATTRIBUTE_TYPES = [["alpha", "b"], ["antialias", "b"], ["depth", "b"], ["failIfMajorPerformanceCaveat", "b"],
      ["powerPreference", "e"], ["premultipliedAlpha", "b"], ["preserveDrawingBuffer", "b"], ["stencil", "b"]];
    const POWER = ["default", "low-power", "high-performance"];
    function contextAttributes(init) {
      const out = {};
      if (init === undefined || init === null) return out;
      if (typeof init !== "object" && typeof init !== "function") return out;
      for (const [k, t] of ATTRIBUTE_TYPES) {
        const v = init[k];
        if (v === undefined) continue;
        if (t === "b") out[k] = !!v;
        else {
          const s = String(v);
          if (!POWER.includes(s)) throw new TypeError(`'${s}' (value of 'powerPreference' member of WebGLContextAttributes) is not a valid value for enumeration WebGLPowerPreference.`);
          out[k] = s;
        }
      }
      return out;
    }
    global.__mrt.webglContext = (canvas, type, init) => {
      const kind = CONTEXT_TYPES[type];
      const iface = kind === 2 ? "WebGL2RenderingContext" : "WebGLRenderingContext";
      const ctx = Object.create(ifaces[iface].prototype);
      const st = { id: glIds.next++, type, kind, iface, canvas, attrs: contextAttributes(init), gen: 0, lost: false, status: "Ready",
                   nextError: 0, made: null, extObjects: new Map(), w: 0, h: 0, dbSize: null,
                   drawingBufferColorSpace: "srgb", unpackColorSpace: "srgb", awaitingRestore: false };
      glState.set(ctx, st);
      const made = createHost(st);
      if (!made) return null;
      st.made = made;
      glStats.contexts++;
      glContexts.add(st);
      glFinalizer.register(ctx, st.id);
      return ctx;
    };
    global.__mrt.webglKind = ctx => { const st = ctx && glState.get(ctx); return st ? st.kind : 0; };
    // toDataURL of a WebGL canvas: the bridge's canvas, as Firefox would draw it
    global.__mrt.webglDataURL = (ctx, args) => {
      const st = glState.get(ctx);
      if (!st || st.lost) return "data:,";
      syncSize(st);
      return call(st, 0, "$dataURL", args.map(a => enc(a, st))).result;
    };
  }
