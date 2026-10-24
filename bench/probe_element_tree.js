// The element tree of a page for diff_element_tree.py (task 5278): a function body over `w` (the
// page's window) returning [signature, text, children] per element, through open shadow roots.
// The signature is the tag with the attributes that tell a feed's boxes apart (role, aria-label,
// data-pagelet, ...); the text is a leaf's first 30 characters. Wrap it as TESTING-GUIDE "The same
// probe on both sides" says: logged with the tag TREE for MRT_EVAL_AFTER, returned for firefox_probe.py.
function sig(e) {
  let s = e.localName;
  for (const a of ["role", "aria-label", "data-pagelet", "data-visualcompletion", "aria-hidden", "hidden"]) {
    const v = e.getAttribute(a);
    if (v !== null) s += "[" + a + "=" + v.slice(0, 40) + "]";
  }
  if (e.localName === "script" || e.localName === "link") s += "{" + (e.getAttribute("src") || e.getAttribute("href") || "").slice(-40) + "}";
  return s;
}
function t(e) {
  const kids = [];
  for (let c = e.firstElementChild; c; c = c.nextElementSibling) kids.push(t(c));
  if (e.shadowRoot) for (let c = e.shadowRoot.firstElementChild; c; c = c.nextElementSibling) kids.push(t(c));
  const text = e.children.length ? "" : (e.textContent || "").trim().slice(0, 30);
  return kids.length ? [sig(e), text, kids] : [sig(e), text];
}
return t(w.document.documentElement);
