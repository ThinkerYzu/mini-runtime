// inserted by bench/bridge-cases/onload.html before its load: inserts onload-b.js, which the load waits for too
window.__mark("script a ran");
const b = document.createElement("script");
b.src = "onload-b.js";
b.onload = () => window.__mark("script b load event");
document.head.appendChild(b);
