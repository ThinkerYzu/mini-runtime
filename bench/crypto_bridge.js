// Web Crypto requests from the mini runtime, run by the bridge tab's real SubtleCrypto (SPEC decision
// 21, DESIGN "Web Crypto: keys kept in the runtime"). mini_realsites.py runs this in Marionette's
// content sandbox "crypto", kept between calls (new_sandbox=False). State lives in
// `globalThis.__mrtCrypto`: Firefox's CryptoKey for each of the runtime's key ids.
//
// The runtime keeps every key's material; this sandbox is only a cache of Firefox's objects. A request
// names the keys it uses (`keys`); if one is not here (the sandbox is new, or the bridge was dropped)
// the reply is `{missing}` and the runtime sends the request again with their material, which is
// imported first. A key made here (generateKey, deriveKey, unwrapKey) is made extractable and
// exported at once, so the runtime holds its material too; the runtime reports the `extractable` the
// page asked for and refuses exportKey and wrapKey itself.
//
// Values cross as JSON (`encode` and `decode` here and in bridge.js): {$b: base64, $t: type} for a
// BufferSource, {$k: id} for a CryptoKey, {$o: [[name, value], ...]} for an object (members in order),
// {$u: 1} for undefined; arrays and the rest as themselves. A key in a result is
// {$key: {id, type, extractable, algorithm, usages, material}}.
const [req] = arguments;
const done = arguments[arguments.length - 1];
const W = window.wrappedJSObject;
const S = globalThis.__mrtCrypto || (globalThis.__mrtCrypto = { keys: new Map() });
const subtle = window.crypto.subtle;
const unwrap = v => (v && typeof v === "object" && v.wrappedJSObject) || v;

const toB64 = u8 => {
  let bin = "";
  for (let i = 0; i < u8.length; i++) bin += String.fromCharCode(u8[i]);
  return window.btoa(bin);
};
// a BufferSource of the page's realm, so that Firefox reads it as the page's own (typed array data
// cannot cross the Xray)
function decodeBytes(b64, type) {
  const bin = window.atob(b64), n = bin.length;
  const u8 = new W.Uint8Array(n);
  for (let i = 0; i < n; i++) u8[i] = bin.charCodeAt(i);
  if (type === "ArrayBuffer") return u8.buffer;
  if (type === "DataView") return new W.DataView(u8.buffer);
  return new W[type](u8.buffer, 0, n / W[type].BYTES_PER_ELEMENT);
}
function decode(v) {
  if (v === null || typeof v !== "object") return v;
  if (Array.isArray(v)) {
    const a = new W.Array();
    for (const x of v) a.push(decode(x));
    return a;
  }
  if ("$u" in v) return undefined;
  if ("$b" in v) return decodeBytes(v.$b, v.$t);
  if ("$k" in v) return S.keys.get(v.$k);
  const o = new W.Object();
  for (const [k, x] of v.$o) o[k] = decode(x);
  return o;
}
const classOf = v => Object.prototype.toString.call(v).slice(8, -1);
function encode(v) {
  if (v === undefined) return { $u: 1 };
  if (v === null || typeof v !== "object") return v;
  const cls = classOf(v);
  if (cls === "ArrayBuffer") return { $b: toB64(unwrap(new W.Uint8Array(v))), $t: "ArrayBuffer" };
  if (ArrayBuffer.isView(unwrap(v)) || /Array$/.test(cls) && cls !== "Array") {
    const u = unwrap(v);
    return { $b: toB64(unwrap(new W.Uint8Array(u.buffer, u.byteOffset, u.byteLength))), $t: cls };
  }
  if (cls === "Array") return Array.from(unwrap(v), encode);
  const u = unwrap(v);
  return { $o: Object.keys(u).map(k => [k, encode(u[k])]) };
}

// a key Firefox made for the page: kept under `id`, described for the runtime; `material` is how to
// import it again (null for a key the page imported, whose material the runtime has)
async function describe(key, id, withMaterial) {
  S.keys.set(id, key);
  const k = unwrap(key);
  const out = { id, type: k.type, extractable: k.extractable, algorithm: encode(k.algorithm), usages: Array.from(k.usages), material: null };
  if (withMaterial) {
    let format = "jwk", data;
    try { data = await subtle.exportKey("jwk", key); }
    catch (e) { format = "raw"; data = await subtle.exportKey("raw", key); }
    out.material = { format, data: encode(data), algorithm: out.algorithm, extractable: true, usages: out.usages };
  }
  return out;
}
const importMaterial = m => subtle.importKey(m.format, decode(m.data), decode(m.algorithm), m.extractable, Array.from(m.usages));

// the extractable argument of the calls that make a key, forced true here (the runtime keeps the page's)
const EXTRACTABLE_ARG = { generateKey: 1, deriveKey: 3, unwrapKey: 5 };

(async () => {
  if (req.call === "token") return window.crypto.randomUUID();
  // every key forgotten, as when the bridge is dropped (the crypto case's check of the material)
  if (req.call === "forget") { S.keys.clear(); return null; }
  for (const [id, m] of Object.entries(req.materials || {})) {
    if (!S.keys.has(id)) S.keys.set(id, await importMaterial(m));
  }
  const missing = (req.keys || []).filter(id => !S.keys.has(id));
  if (missing.length) return { missing };
  const args = req.args.map(decode);
  const made = req.call in EXTRACTABLE_ARG;
  if (made) args[EXTRACTABLE_ARG[req.call]] = true;
  // `early`: Firefox settled the promise at the call, so the runtime settles it at once too: a binding
  // error, a task that failed when it was made (WebCryptoTask::DispatchWithPromise and mEarlyRv), or a
  // task done on the main thread (importing a symmetric key); else it came from the crypto thread
  let result, settled = false;
  const p = subtle[req.call](...args);
  p.then(() => { settled = true; }, () => { settled = true; });
  await null;
  const early = settled;
  try { result = await p; }
  catch (e) { return { reject: { name: String(e.name), message: String(e.message) }, early }; }
  // the keys in a result: one key, or a key pair (privateKey and publicKey, in the dictionary's order)
  const newIds = req.newIds || [];
  if (classOf(result) === "CryptoKey") return { result: { $key: await describe(result, newIds[0], made) }, early };
  if (req.call === "generateKey") {
    const r = unwrap(result), members = [];
    for (const [i, name] of Object.keys(r).entries()) members.push([name, { $key: await describe(r[name], newIds[i], true) }]);
    return { result: { $o: members }, early };
  }
  return { result: encode(result), early };
})().then(ok => done({ ok: ok === undefined ? null : ok }), e => done({ err: String(e) }));
