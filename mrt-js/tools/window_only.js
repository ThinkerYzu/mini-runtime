// Lists the top-level statements of prelude.js or bridge.js that touch an [Exposed=Window]-only name
// when the file is evaluated: directly, through a computed `global[...]`, or through a local function
// they call at load. In a worker's global those names do not exist (DESIGN, "Workers: the build
// plan"), so each listed statement must be guarded by `if (!isWorker)`; statements already guarded
// are not listed. Code inside functions is not listed: it runs only when called.
//
// usage (the SpiderMonkey shell of the tree; the @...@ placeholders replaced by a line each):
//   sed 's|^  // @HTML_INTERFACES@$|  const HTML_ATTRIBUTE_OWNERS = {};|; s|^// @CLONE_CODEC@$|null|; s|^    // @WORKER_NAMES@$|    const WORKER_GLOBAL_NAMES = null;|' glue/bridge.js > /tmp/b.js
//   $FIREFOX/obj-js/dist/bin/js tools/window_only.js /tmp/b.js
// Output: line:column-endline, the statement's kind, what it touches, its first line. Some are false
// positives (a computed global access to a typed array's constructor); read each.
const src = os.file.readFile(scriptArgs[0]);
const WINDOW_ONLY = new Set(["Window", "Node", "Element", "HTMLElement", "CharacterData", "Text", "Comment", "CDATASection",
  "ProcessingInstruction", "Document", "DocumentType", "DocumentFragment", "ShadowRoot", "MutationObserver", "MutationRecord",
  "CustomElementRegistry", "customElements", "document", "window", "__mrtStyle", "__mrtComputed", "__mrtParseFragment",
  "__mrtCEReactions", "__mrtLink", "__mrtCreateHTMLDocument", "__mrtParseDocument", "__mrtSheet", "__mrtCssom", "__mrtBorderBox",
  "__mrtClientScroll", "__mrtViewport", "__mrtRefresh", "__mrtLinkSheet", "__mrtModuleScript", "__mrtModuleStats",
  "__mrtSetInnerHTML", "__mrtReplaceChildren", "__mrtSessionStorage", "__mrtWindows", "__mrtPostMessage"]);
const isWin = n => n === "global[]" || WINDOW_ONLY.has(n) || /^(HTML|SVG)\w*Element$/.test(n);
const ast = Reflect.parse(src, { loc: true });
let body;
for (const st of ast.body) if (st.type === "ExpressionStatement" && st.expression.type === "CallExpression" && st.expression.callee.type === "FunctionExpression") body = st.expression.callee.body.body;
const lines = src.split("\n");
// identifiers referenced, deep (into functions) or shallow (what runs now)
function refs(n, deep, out) {
  if (!n || typeof n !== "object") return;
  if (Array.isArray(n)) { for (const x of n) refs(x, deep, out); return; }
  switch (n.type) {
    case "FunctionExpression": case "ArrowFunctionExpression": case "FunctionDeclaration":
      if (deep) { refs(n.body, deep, out); refs(n.defaults, deep, out); } return;
    case "Identifier": out.add(n.name); return;
    case "MemberExpression":
      refs(n.object, deep, out);
      if (n.computed) { refs(n.property, deep, out); if (n.object.type === "Identifier" && n.object.name === "global") out.add("global[]"); }
      else if (n.object.type === "Identifier" && n.object.name === "global") out.add(n.property.name);
      return;
    case "Property": if (n.computed) refs(n.key, deep, out); refs(n.value, deep, out); return;
    case "ClassStatement": case "ClassExpression":
      refs(n.superClass, deep, out);
      if (deep) refs(n.body, deep, out);
      return;
  }
  for (const k in n) if (k !== "loc" && k !== "type") refs(n[k], deep, out);
}
// local functions: name -> identifiers their bodies use
const fns = new Map();
for (const st of body) {
  if (st.type === "FunctionDeclaration") { const s = new Set(); refs(st.body, true, s); fns.set(st.id.name, s); }
  if (st.type === "VariableDeclaration") for (const d of st.declarations) {
    if (d.id.type === "Identifier" && d.init && (d.init.type === "ArrowFunctionExpression" || d.init.type === "FunctionExpression")) {
      const s = new Set(); refs(d.init.body, true, s); fns.set(d.id.name, s);
    }
  }
}
// which local functions reach a window-only name
const tainted = new Map();
let changed = true;
while (changed) {
  changed = false;
  for (const [name, used] of fns) {
    if (tainted.has(name)) continue;
    const why = [...used].find(u => isWin(u) || tainted.has(u));
    if (why) { tainted.set(name, why); changed = true; }
  }
}
// what a statement runs now: identifiers outside functions, plus calls of tainted functions
function nowCalls(n, out) {
  if (!n || typeof n !== "object") return;
  if (Array.isArray(n)) { for (const x of n) nowCalls(x, out); return; }
  if (n.type === "FunctionExpression" || n.type === "ArrowFunctionExpression" || n.type === "FunctionDeclaration") return;
  if (n.type === "ClassStatement" || n.type === "ClassExpression") { nowCalls(n.superClass, out); return; }
  if ((n.type === "CallExpression" || n.type === "NewExpression") && n.callee.type === "Identifier" && tainted.has(n.callee.name)) out.add(n.callee.name + "()");
  for (const k in n) if (k !== "loc" && k !== "type") nowCalls(n[k], out);
}
for (const st of body) {
  if (st.type === "FunctionDeclaration") continue;
  // already guarded
  if (st.type === "IfStatement" && st.test.type === "UnaryExpression" && st.test.argument.type === "Identifier" && st.test.argument.name === "isWorker") continue;
  if (st.type === "IfStatement" && st.test.type === "Identifier" && st.test.name === "isWorker") continue;
  const ids = new Set(); refs(st, false, ids);
  const hits = new Set([...ids].filter(isWin));
  nowCalls(st, hits);
  if (hits.size) print(`${st.loc.start.line}:${st.loc.start.column}-${st.loc.end.line} ${st.type} [${[...hits].join(",")}] ${lines[st.loc.start.line - 1].trim().slice(0, 90)}`);
}
