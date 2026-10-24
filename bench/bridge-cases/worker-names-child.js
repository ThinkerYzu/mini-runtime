// Every name a dedicated worker's global has, by where it lives (tools/gen_worker_names.py reads it).
const own = o => Object.getOwnPropertyNames(o).sort();
const out = { global: own(self) };
for (let p = Object.getPrototypeOf(self), i = 0; p; p = Object.getPrototypeOf(p), i++) out["proto" + i + " " + Object.prototype.toString.call(p)] = own(p);
out.navigator = []; for (let p = Object.getPrototypeOf(navigator); p && p !== Object.prototype; p = Object.getPrototypeOf(p)) out.navigator.push(...own(p));
out.location = own(Object.getPrototypeOf(location));
postMessage(out);
