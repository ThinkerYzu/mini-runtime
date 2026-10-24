onmessage = e => {
  const r = indexedDB.open(e.data);
  r.onsuccess = () => {
    const db = r.result;
    db.onversionchange = event => {
      db.close();
      postMessage(["closed", event.oldVersion, event.newVersion]);
    };
    postMessage(["ready"]);
  };
  r.onerror = () => postMessage(["error", r.error.name]);
};
