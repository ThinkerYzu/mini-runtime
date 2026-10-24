// A module worker of bench/bridge-cases/worker.html: imports a module, answers a message.
import { helper } from "./worker-module-dep.js";
let importScriptsResult;
try { importScripts("worker-helper.js"); importScriptsResult = "ok"; } catch (e) { importScriptsResult = e.constructor.name + " " + e.name + ": " + e.message; }
postMessage(["module", helper, typeof import.meta, String(import.meta.url).replace(/^.*\//, ""), importScriptsResult, typeof self.importScripts]);
self.onmessage = e => postMessage(["echo", e.data]);
