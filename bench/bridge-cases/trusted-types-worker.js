// trusted-types.html's worker: the API in a worker (WorkerGlobalScope's trustedTypes); no CSP
const out = [];
try {
  out.push(typeof trustedTypes, trustedTypes instanceof TrustedTypePolicyFactory, typeof TrustedHTML);
  const p = trustedTypes.createPolicy("w", { createScriptURL: s => s + "#w" });
  out.push(p.name, String(p.createScriptURL("x.js")), trustedTypes.isScriptURL(p.createScriptURL("y")));
  setTimeout(p.createScriptURL ? "postMessage('string timer ran')" : "", 0);
} catch (e) {
  out.push("threw " + e.name + " " + e.message);
}
postMessage(out.join(" "));
