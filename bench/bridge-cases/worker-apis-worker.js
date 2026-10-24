// The worker of bench/bridge-cases/worker-apis.html: runs the bridge's APIs inside a worker and posts
// one line per check, then "done".
"use strict";
const E = e => e ? (e.constructor ? e.constructor.name : "?") + " " + e.name + ": " + e.message : String(e);
const J = v => JSON.stringify(v);
const out = (...a) => postMessage(a.join(" "));
// a check that has not settled after 3 s is logged as a timeout, so one hang does not hide the rest
const A = async (what, f) => {
  const timeout = new Promise(r => setTimeout(() => r("timeout"), 3000));
  try {
    const v = await Promise.race([Promise.resolve().then(f).then(x => ["ok", x]), timeout]);
    out(what, ...(v === "timeout" ? ["timeout"] : [v[0], J(v[1])]));
  } catch (e) { out(what, "throws", E(e)); }
};
const hex = b => [...new Uint8Array(b)].map(x => x.toString(16).padStart(2, "0")).join("");
const events = (target, types) => new Promise(r => { const got = []; for (const t of types) target.addEventListener(t, e => { got.push(t); if (t === "loadend") r(got); }); });

onmessage = async e => {
  const wsPort = e.data.ws;
  await A("types", () => ["fetch", "XMLHttpRequest", "Blob", "File", "FileReader", "FileReaderSync", "crypto", "indexedDB", "WebSocket", "Worker",
    "Request", "Response", "Headers", "AbortController", "ReadableStream", "BroadcastChannel", "EventSource", "CompressionStream", "TextDecoderStream",
    "FormData", "URLSearchParams", "MessageChannel", "IDBKeyRange", "CryptoKey", "SubtleCrypto", "ProgressEvent"].map(n => n + ":" + typeof self[n]));

  // fetch
  await A("fetch", async () => { const r = await fetch("worker-apis-data.txt"); return [r.status, r.ok, r.type, r.url.replace(location.origin, ""), r.headers.get("content-type"), await r.text()]; });
  await A("fetch 404", async () => { const r = await fetch("no-such-file.txt"); return [r.status, r.ok]; });
  await A("fetch Request", async () => { const r = await fetch(new Request("worker-apis-data.txt", { cache: "no-store" })); return (await r.arrayBuffer()).byteLength; });
  await A("fetch blob()", async () => { const b = await (await fetch("worker-apis-data.txt")).blob(); return [b.size, b.type, await b.text()]; });

  // XHR
  await A("xhr async", () => new Promise(r => {
    const x = new XMLHttpRequest();
    x.open("GET", "worker-apis-data.txt");
    const seen = [];
    x.onreadystatechange = () => seen.push(x.readyState);
    x.onloadend = () => r([x.status, x.responseText, x.responseURL.replace(location.origin, ""), seen]);
    x.send();
  }));
  await A("xhr sync", () => { const x = new XMLHttpRequest(); x.open("GET", "worker-apis-data.txt", false); x.send(); return [x.status, x.responseText]; });
  await A("xhr arraybuffer", () => new Promise(r => {
    const x = new XMLHttpRequest();
    x.open("GET", "worker-apis-data.txt");
    x.responseType = "arraybuffer";
    x.onload = () => r([x.response instanceof ArrayBuffer, x.response.byteLength]);
    x.send();
  }));

  // Blob and FileReader
  const blob = new Blob(["héllo ", new Uint8Array([119, 111, 114, 108, 100])], { type: "text/plain" });
  await A("blob", async () => [blob.size, blob.type, await blob.text(), await blob.slice(1, 4).text()]);
  await A("FileReader text", async () => {
    const fr = new FileReader();
    const p = events(fr, ["loadstart", "progress", "load", "loadend"]);
    fr.readAsText(blob);
    const states = [fr.readyState];
    const order = await p;
    return [states, fr.readyState, fr.result, order];
  });
  await A("FileReader dataURL", () => new Promise(r => { const fr = new FileReader(); fr.onload = () => r(fr.result); fr.readAsDataURL(blob); }));
  await A("FileReader arrayBuffer", () => new Promise(r => { const fr = new FileReader(); fr.onload = () => r([fr.result instanceof ArrayBuffer, fr.result.byteLength]); fr.readAsArrayBuffer(blob); }));
  await A("FileReader binaryString", () => new Promise(r => { const fr = new FileReader(); fr.onload = () => r(fr.result.length); fr.readAsBinaryString(blob); }));
  await A("FileReader twice", () => { const fr = new FileReader(); fr.readAsText(blob); fr.readAsText(blob); });
  await A("FileReader abort", () => new Promise(r => {
    const fr = new FileReader(), got = [];
    for (const t of ["loadstart", "load", "abort", "loadend"]) fr.addEventListener(t, () => got.push(t));
    fr.readAsText(blob);
    fr.abort();
    setTimeout(() => r([got, fr.readyState, fr.result]), 50);
  }));
  await A("FileReader not a blob", () => new FileReader().readAsText("x"));
  await A("FileReaderSync", () => { const s = new FileReaderSync(); return [s.readAsText(blob), s.readAsArrayBuffer(blob).byteLength, s.readAsDataURL(blob), s.readAsBinaryString(blob).length]; });

  // crypto
  await A("getRandomValues", () => { const a = new Uint8Array(16); return [crypto.getRandomValues(a) === a, a.length]; });
  await A("randomUUID", () => /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(crypto.randomUUID()));
  await A("digest", async () => hex(await crypto.subtle.digest("SHA-256", new TextEncoder().encode("abc"))));
  await A("aes-gcm", async () => {
    const key = await crypto.subtle.generateKey({ name: "AES-GCM", length: 128 }, true, ["encrypt", "decrypt"]);
    const iv = new Uint8Array(12);
    const ct = await crypto.subtle.encrypt({ name: "AES-GCM", iv }, key, new TextEncoder().encode("secret"));
    const pt = await crypto.subtle.decrypt({ name: "AES-GCM", iv }, key, ct);
    return [Object.prototype.toString.call(key), key.algorithm, key.usages, ct.byteLength, new TextDecoder().decode(pt)];
  });
  await A("hmac import", async () => {
    const key = await crypto.subtle.importKey("raw", new TextEncoder().encode("k"), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
    return hex(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode("m")));
  });

  // IndexedDB
  let idbStep = "start";
  await A("indexedDB", () => new Promise((resolve, reject) => {
    setTimeout(() => resolve("stalled at " + idbStep), 2500);
    const name = "worker-apis-db";
    const open = indexedDB.open(name, 1);
    open.onupgradeneeded = () => { idbStep = "upgrade"; open.result.createObjectStore("s"); };
    open.onerror = () => reject(open.error);
    open.onblocked = () => reject(new Error("blocked"));
    open.onsuccess = () => {
      idbStep = "opened";
      const db = open.result;
      const tx = db.transaction("s", "readwrite");
      tx.objectStore("s").put({ a: 1, b: blob }, "k");
      tx.onerror = () => reject(tx.error);
      tx.oncomplete = () => {
        idbStep = "put";
        const get = db.transaction("s").objectStore("s").get("k");
        get.onsuccess = async () => {
          idbStep = "got";
          const v = get.result;
          const text = await v.b.text();
          idbStep = "blob read";
          db.close();
          const del = indexedDB.deleteDatabase(name);
          del.onsuccess = () => resolve([db.name, db.version, [...db.objectStoreNames], v.a, v.b instanceof Blob, text]);
        };
      };
    };
  }));

  // WebSocket
  await A("websocket", () => new Promise(r => {
    const ws = new WebSocket(`ws://localhost:${wsPort}/echo`);
    const got = [];
    ws.onopen = () => { got.push("open"); ws.send("hi"); };
    ws.onmessage = m => { got.push("message " + m.data); ws.send("bye"); };
    ws.onclose = c => { got.push("close " + c.code + " " + c.wasClean); r(got); };
    ws.onerror = () => got.push("error");
  }));

  // a nested worker
  await A("nested worker", () => new Promise(r => {
    const w = new Worker("worker-apis-child.js");
    w.onmessage = m => { r(m.data); w.terminate(); };
    w.onerror = ev => { ev.preventDefault(); r(["error", ev.message]); };
    w.postMessage("ping");
    setTimeout(() => r("timeout"), 2000);
  }));
  await A("nested blob worker", () => new Promise(r => {
    const w = new Worker(URL.createObjectURL(new Blob(["postMessage(['nested blob', self.origin])"])));
    w.onmessage = m => r(m.data);
    w.onerror = ev => { ev.preventDefault(); r(["error"]); };
    setTimeout(() => r("timeout"), 2000);
  }));
  await A("nested channel", () => new Promise(r => {
    const w = new Worker("worker-apis-child.js");
    const ch = new MessageChannel();
    w.postMessage("port", [ch.port2]);
    w.onmessage = m => r(m.data.slice(0, 2));
    setTimeout(() => r("timeout"), 2000);
  }));

  // a Blob posted to the page
  postMessage({ blob });
  out("done");
};
