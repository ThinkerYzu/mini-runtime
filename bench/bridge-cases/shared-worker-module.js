// A module shared worker of bench/bridge-cases/shared-worker.html.
import { helper } from "./worker-module-dep.js";
let importScriptsResult;
try { importScripts("worker-helper.js"); importScriptsResult = "ok"; } catch (e) { importScriptsResult = e.constructor.name + " " + e.name + ": " + e.message; }
onconnect = e => e.ports[0].postMessage(["module", helper, String(import.meta.url).replace(/^.*\//, ""), importScriptsResult, self.name]);
