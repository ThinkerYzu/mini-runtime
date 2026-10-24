// Marionette adapter; executor is shared with the HTTP viewer (prepended by the driver).
const [req] = arguments;
const done = arguments[arguments.length - 1];
const executor = globalThis.__mrtGL || (globalThis.__mrtGL = createMrtWebGL(window.wrappedJSObject));
executor.execute(req).then(ok => done({ok}), e => done({err: String(e)}));
