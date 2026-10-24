// A census of the string values a worker's objects hold (GAPS B89), run in a debugger global with
// `Debugger` and the worker's global as `target`: in the runtime through __mrtDebuggerEval
// (MRT_WORKER_CENSUS), in Firefox through a WorkerDebugger (firefox_worker_census.py). The same
// script on both sides, so the two results compare.
//
// Every object of the worker's realm (Debugger.Memory-free: findObjects) is visited: its own data
// properties, a function's closure scopes, a Map's or Set's entries. Property names are atoms (not
// the zone's strings) and are left out. A string value is counted once per distinct value; strings
// reached only from the stack, from engine internals or from objects of other realms are not seen.
// Returns a JSON text: totals, a length histogram, the groups with the most characters (by a
// normalized prefix) and the longest values' prefixes.
// `censusStage` (when defined) stops the walk early, for finding which step a build mishandles: 1
// objects only, 2 their properties, 3 closure scopes, 4 Map and Set entries (the whole walk).
(function () {
  const stage = typeof censusStage === "number" ? censusStage : 4;
  const t0 = Date.now();
  const dbg = new Debugger();
  const g = dbg.addDebuggee(target);
  const values = new Map();  // string -> number of holders seen
  const add = v => { if (typeof v === "string") values.set(v, (values.get(v) || 0) + 1); };
  const envs = new Set();
  let objects = 0, proxies = 0, failed = 0, skipped = 0;
  const classes = new Map();
  // Only SpiderMonkey's own classes are walked: reading a DOM object's properties from a debugger
  // crashed Firefox's content process (session 84), and a DOM object keeps its state in C++ there
  // (the runtime's bridge.js objects of the same interfaces are JS objects, class Object: a few of
  // their strings are counted on the runtime's side only)
  const JS_CLASSES = new Set(["Object", "Array", "Function", "Arguments", "Error", "Date", "RegExp", "Map", "Set",
    "WeakMap", "WeakSet", "Promise", "Boolean", "Number", "String", "Symbol", "BigInt", "ArrayBuffer", "DataView",
    "Generator", "AsyncGenerator", "AsyncFunctionGenerator", "Iterator", "Array Iterator", "Map Iterator",
    "Set Iterator", "String Iterator", "Proxy", "Module", "ModuleNamespace", "WeakRef", "FinalizationRegistry",
    "Error", "TypeError", "RangeError", "SyntaxError", "ReferenceError"]);
  const objs = dbg.findObjects();
  for (const o of objs) {
    objects++;
    classes.set(o.class, (classes.get(o.class) || 0) + 1);
    if (o.isProxy) { proxies++; continue; }
    if (!JS_CLASSES.has(o.class)) { skipped++; continue; }
    if (stage < 2) continue;
    try {
      for (const name of o.getOwnPropertyNames()) {
        let desc;
        try { desc = o.getOwnPropertyDescriptor(name); } catch (e) { failed++; continue; }
        if (desc && "value" in desc) add(desc.value);
      }
      if (stage >= 3 && o.callable && o.environment) {
        for (let env = o.environment; env && !envs.has(env); env = env.parent) {
          envs.add(env);
          if (env.type === "object" || env.type === "with") continue;
          for (const name of env.names()) {
            try { add(env.getVariable(name)); } catch (e) { failed++; }
          }
        }
      }
      if (stage >= 4 && (o.class === "Map" || o.class === "Set")) {
        const raw = o.unsafeDereference();
        const it = o.class === "Map" ? Map.prototype.entries.call(raw) : Set.prototype.values.call(raw);
        for (const entry of it) {
          if (o.class === "Map") { add(entry[0]); add(typeof entry[1] === "string" ? entry[1] : null); }
          else add(entry);
        }
      }
    } catch (e) { failed++; }
  }
  // a value's bytes as the engine stores it: one per character for Latin-1, two otherwise
  const bytes = s => { for (let i = 0; i < s.length; i++) if (s.charCodeAt(i) > 255) return 2 * s.length; return s.length; };
  const buckets = [[8, "<8"], [24, "<24"], [100, "<100"], [1000, "<1k"], [10000, "<10k"], [Infinity, ">=10k"]];
  const hist = {};
  for (const [, label] of buckets) hist[label] = { count: 0, bytes: 0 };
  const groups = new Map();
  let distinct = 0, total = 0;
  for (const [s] of values) {
    distinct++;
    const b = bytes(s);
    total += b;
    hist[buckets.find(([max]) => s.length < max)[1]].count++;
    hist[buckets.find(([max]) => s.length < max)[1]].bytes += b;
    // the group: the first 24 characters with digit runs as 0 and hex runs of 8 or more as h
    const key = s.slice(0, 24).replace(/[0-9a-f]{8,}/gi, "h").replace(/[0-9]+/g, "0");
    const gr = groups.get(key) || { count: 0, bytes: 0 };
    gr.count++; gr.bytes += b;
    groups.set(key, gr);
  }
  const top = [...groups].sort((a, b) => b[1].bytes - a[1].bytes).slice(0, 40).map(([k, v]) => [k, v.count, v.bytes]);
  const longest = [...values.keys()].sort((a, b) => b.length - a.length).slice(0, 20).map(s => [s.length, s.slice(0, 100)]);
  const classTop = [...classes].sort((a, b) => b[1] - a[1]).slice(0, 30);
  return JSON.stringify({ objects, proxies, skipped, classes: classTop, failed, environments: envs.size, distinct, bytes: total, hist, top, longest, ms: Date.now() - t0 });
})()
