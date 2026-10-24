// compression.html's worker: the interfaces in a worker, and a gzip round trip through Responses.
(async () => {
  const s = "worker text ".repeat(200);
  const z = new Uint8Array(await new Response(new Response(s).body.pipeThrough(new CompressionStream("gzip"))).arrayBuffer());
  const back = await new Response(new Blob([z]).stream().pipeThrough(new DecompressionStream("gzip"))).text();
  postMessage([typeof CompressionStream, typeof DecompressionStream, z.length, back === s]);
})().catch(e => postMessage("threw " + e.name + ": " + e.message));
