//! The DOM bindings, case by case: each case is a page whose script logs with console.log, and the
//! lines it must log. Run with `cargo test --release --offline` (no test harness, see Cargo.toml).

use mrt_js::mrt_style::mrt_dom::Dom;
use mrt_js::{FrameGlobalRelation, GlobalOptions, Page};

/// (name, body markup with a script, expected console lines)
const CASES: &[(&str, &str, &[&str])] = &[
    (
        "unhandled rejections",
        // Gecko's rules: a rejection handled within the task (here in a later microtask) is never
        // reported; one still unhandled after the task's microtasks is reported on the console,
        // then gets unhandledrejection (cancelable), and rejectionhandled once a later task handles it
        r#"<script>
        addEventListener("unhandledrejection", e => console.log("unhandledrejection", e.reason, e.cancelable, e.promise === late));
        addEventListener("rejectionhandled", e => console.log("rejectionhandled", e.reason));
        const late = Promise.reject("late");
        Promise.reject("caught").catch(() => {});
        const p = Promise.reject("in a microtask");
        Promise.resolve().then(() => p.catch(() => {}));
        setTimeout(() => late.catch(() => {}), 100);
        // handled by a timer already due when the event's task runs: reported, but no event
        const late0 = Promise.reject("late0");
        setTimeout(() => late0.catch(() => {}), 0);
        </script>"#,
        // the order release Firefox 157 gives for this page (event listeners log into the title there)
        &["uncaught (in promise): late", "uncaught (in promise): late0", "unhandledrejection late true true", "rejectionhandled late"],
    ),
    (
        "element.style as Gecko's declaration block",
        // the expected lines are release Firefox 157's for the same script
        r#"<script>
        const d = document.createElement("div");
        d.setAttribute("style", "opacity:.55;margin:1px 2px");
        console.log(d.style.opacity, d.style.marginLeft, d.style.margin, d.style.length, d.style[0]);
        d.style.color = "RED";
        console.log(d.style.color, d.getAttribute("style"));
        d.style.width = "bogus";
        console.log(JSON.stringify(d.style.width), d.style.cssFloat === "", d.style.notAProperty);
        d.style.setProperty("top", "5px", "important");
        console.log(d.style.getPropertyPriority("top"), d.style.cssText);
        console.log(d.style.removeProperty("opacity"), d.style.opacity === "", "transition" in d.style, "notAProperty" in d.style, d.style === d.style);
        d.style.cssText = "background:none; padding:0 1px";
        console.log(d.getAttribute("style"));
        </script>"#,
        &[
            "0.55 2px 1px 2px 5 opacity",
            "red opacity: 0.55; margin: 1px 2px; color: red;",
            "\"\" true undefined",
            "important opacity: 0.55; margin: 1px 2px; color: red; top: 5px !important;",
            "0.55 true true false true",
            "background: none; padding: 0px 1px;",
        ],
    ),
    (
        "an SVG element's style",
        // SVGElement includes ElementCSSInlineStyle (YouTube styles its SVG icons, task f03f2); the
        // expected lines are release Firefox 157's
        r#"<script>
        const NS = "http://www.w3.org/2000/svg";
        const svg = document.createElementNS(NS, "svg");
        svg.style.pointerEvents = "none";
        svg.style.height = "12px";
        console.log(typeof svg.style, svg.style === svg.style, svg.getAttribute("style"), "style" in SVGElement.prototype);
        const path = svg.appendChild(document.createElementNS(NS, "path"));
        path.setAttribute("style", "fill: red");
        console.log(path.style.fill, path.style.cssText);
        path.style = "opacity: 0.5";
        console.log(path.getAttribute("style"), path.style.opacity);
        </script>"#,
        &["object true pointer-events: none; height: 12px; true", "red fill: red;", "opacity: 0.5; 0.5"],
    ),
    (
        "doctype and compatMode",
        r#"<script>
        const t = document.doctype;
        console.log(t === document.firstChild, t.name, t.nodeName, JSON.stringify(t.publicId), document.compatMode, t instanceof DocumentType);
        </script>"#,
        &["true html html \"\" CSS1Compat true"],
    ),
    (
        "console format strings",
        // Gecko's ProcessArguments; the expected lines are what release Firefox 157 stores for each
        // call (its console event's arguments joined with spaces)
        r#"<script>
        console.log("%c styled", "color: red");
        console.log("a %s b %d c %i %.2f %%", "x", 4.7, "12", 3.14159, "rest");
        console.log("%o and", 5);
        console.log(String(new CustomEvent("x")), Object.prototype.toString.call(new Event("y")));
        </script>"#,
        &[" styled", "a x b 4 c 12 3.14 % rest", "5  and", "[object CustomEvent] [object Event]"],
    ),
    (
        "window security properties",
        // a page from localhost, as Firefox's test harness loads them: a secure context, not
        // cross-origin isolated, so no SharedArrayBuffer constructor
        r#"<script>console.log(isSecureContext, crossOriginIsolated, typeof SharedArrayBuffer);</script>"#,
        &["true false undefined"],
    ),
    (
        "tree",
        r#"<div id=a>x<b>y</b><!--c--></div><script>
        const a = document.getElementById("a");
        console.log(a.nodeName, a.childNodes.length, a.children.length, a.firstChild.nodeType, a.lastChild.nodeType);
        console.log(a.firstChild.nextSibling.tagName, a.lastChild.previousSibling.firstChild.data);
        console.log(a.parentNode === document.body, document.body.parentNode === document.documentElement);
        console.log(document.documentElement.parentNode === document, document.parentNode);
        console.log(a.textContent, document.textContent, a.ownerDocument === document);
        </script>"#,
        &["DIV 3 1 3 8", "B y", "true true", "true null", "xy null true"],
    ),
    (
        "identity and expandos",
        r#"<p id=p></p><script>
        const p = document.getElementById("p");
        p.mark = 7;
        console.log(p === document.querySelector("p"), document.body.firstChild === p, p.mark);
        </script>"#,
        &["true true 7"],
    ),
    (
        "mutation",
        r#"<ul id=l><li>1</li><li>3</li></ul><script>
        const l = document.getElementById("l");
        const two = document.createElement("LI");
        two.appendChild(document.createTextNode("2"));
        l.insertBefore(two, l.lastChild);
        console.log(l.textContent, two.tagName, two.parentElement === l);
        l.appendChild(l.firstChild);
        console.log(l.textContent);
        l.removeChild(two);
        console.log(l.textContent, two.parentNode, two.isConnected, l.isConnected);
        try { l.removeChild(two); } catch (e) { console.log(e.message.split(":")[0]); }
        try { two.appendChild(document.documentElement); two.appendChild(two); } catch (e) { console.log(e.message.split(":")[0]); }
        l.textContent = "";
        console.log(l.childNodes.length, l.hasChildNodes());
        </script>"#,
        &["123 LI true", "231", "31 null false true", "NotFoundError", "HierarchyRequestError", "0 false"],
    ),
    (
        "attributes",
        r#"<div id=d class="a b" data-x=1></div><script>
        const d = document.getElementById("d");
        console.log(d.getAttribute("DATA-X"), d.hasAttribute("title"), d.getAttribute("title"), d.className);
        d.setAttribute("title", "t"); d.id = "e"; d.className = "c";
        console.log(d.getAttribute("title"), d.id, document.getElementById("e") === d, document.getElementById("d"));
        d.removeAttribute("data-x");
        console.log(d.hasAttribute("data-x"), d.classList.contains("c"), d.classList.toggle("z"), d.className);
        </script>"#,
        &["1 false null a b", "t e true null", "false true true c z"],
    ),
    (
        "selectors",
        r#"<div class=r><span class=c>a</span><span>b</span></div><div class=r><span class=c>c</span></div><script>
        console.log(document.querySelectorAll(".r > .c").length, document.querySelector("div:nth-child(2) span").textContent);
        const r = document.querySelector(".r");
        console.log(r.querySelectorAll("span").length, r.querySelector(":not(.c)").textContent, document.getElementsByClassName("c").length);
        try { document.querySelector("!!"); } catch (e) { console.log(e.message.split(":")[0]); }
        const c = document.querySelector(".c"), lone = document.createElement("p");
        lone.className = "x";
        console.log(c.matches(".r > span"), c.matches("div"), c.closest(".r") === r, c.closest("table"), lone.matches("p.x"), lone.closest("p") === lone);
        try { c.matches("!!"); } catch (e) { console.log(e.message.split(":")[0]); }
        </script>"#,
        &["2 c", "2 b 2", "SyntaxError", "true false true null true true", "SyntaxError"],
    ),
    (
        "style and layout reads",
        r#"<div id=b style="width:100px">x</div><script>
        const b = document.getElementById("b");
        console.log(b.offsetWidth, b.style.width);
        b.style.width = "250px"; b.style.paddingLeft = "5px";
        console.log(b.getAttribute("style"), b.offsetWidth);
        const r = b.getBoundingClientRect();
        console.log(r.width, r.left, r.top);
        </script>"#,
        &["100 100px", "width: 250px; padding-left: 5px; 255", "255 8 8"],
    ),
    (
        // the boxes read from the layout thread as they are asked for (task 9caa0): the same element
        // twice in one DOM generation, an element without a frame, and each after a change
        "layout reads of single elements",
        r#"<div id=a style="width:100px;height:40px">x</div><div id=n style="display:none;height:10px">y</div>
        <div id=s style="width:50px;height:30px"></div><script>
        const a = document.getElementById("a"), n = document.getElementById("n"), s = document.getElementById("s");
        console.log(a.getBoundingClientRect().width, a.getBoundingClientRect().top, a.offsetHeight);
        console.log(n.getBoundingClientRect().width, n.getBoundingClientRect().height, s.getBoundingClientRect().top);
        a.style.height = "60px";
        console.log(a.getBoundingClientRect().height, s.getBoundingClientRect().top);
        n.style.display = "block";
        console.log(n.getBoundingClientRect().top, n.getBoundingClientRect().height, s.getBoundingClientRect().top);
        </script>"#,
        &["100 8 40", "0 0 48", "60 68", "68 10 78"],
    ),
    (
        // a <style> added, given new text, moved and removed after the first layout: the stylist keeps its
        // other sheets and restyles what the change can match (task 9caa0), including a sheet put
        // before the one it overrides, an element matched only by the new sheet's selector, and one
        // its rules no longer match
        "page sheets changed after layout",
        r##"<style>.w { width: 50px }</style><div id=a class=w>a</div><div id=b>b</div><p id=c>c</p><script>
        const w = id => document.getElementById(id).getBoundingClientRect().width;
        console.log(w("a"), w("b") > 100, w("c") > 100);
        const s = document.createElement("style");
        s.textContent = "#b { width: 70px } p { width: 30px }";
        document.head.appendChild(s);
        console.log(w("a"), w("b"), w("c"));
        const first = document.createElement("style");
        first.textContent = ".w { width: 90px } #b { width: 10px }";
        document.head.insertBefore(first, document.head.firstChild);
        console.log(w("a"), w("b"), w("c"));
        s.textContent = "p { width: 40px }";
        console.log(w("a"), w("b"), w("c"));
        // moved: the sheets in a new order, none added or removed (the later rule wins); the first
        // <style> is in the body, after the head's
        const orig = document.body.querySelector("style");
        document.head.insertBefore(orig, first);
        console.log(w("a"), w("b"), w("c"));
        document.body.appendChild(orig);
        console.log(w("a"), w("b"), w("c"));
        document.head.lastChild.remove();
        first.remove();
        console.log(w("a"), w("b") > 100, w("c") > 100);
        </script>"##,
        &["50 true true", "50 70 30", "50 70 30", "50 10 40", "90 10 40", "50 10 40", "50 true true"],
    ),
    (
        // two <style> elements with the same text share one parsed sheet: one of them added, then both
        // removed (task 9caa0: the stylist had the sheet once and was asked to remove it twice)
        "page sheets with the same text",
        r##"<style>#d { width: 60px }</style><div id=d>d</div><script>
        const w = () => document.getElementById("d").getBoundingClientRect().width;
        console.log(w());
        const twin = document.createElement("style");
        twin.textContent = "#d { width: 60px }";
        document.head.appendChild(twin);
        console.log(w());
        twin.remove();
        document.body.querySelector("style").remove();
        console.log(w() > 100);
        </script>"##,
        &["60", "60", "true"],
    ),
    (
        // reads between attribute changes that do not change style or frames keep the last layout
        // (task dcb24: no flush for them), and a change that does still shows: a class a rule
        // matches, an attribute selector, an img's width, then the data attribute last again
        "reads after attribute changes that change nothing",
        r##"<style>.wide { width: 80px } [data-big=y] { width: 120px }</style><div id=d style="width:40px">d</div><img id=i width=10 height=5><script>
        const d = document.getElementById("d"), i = document.getElementById("i");
        const w = () => [d.getBoundingClientRect().width, getComputedStyle(d).width, i.offsetWidth].join(" ");
        console.log(w());
        d.setAttribute("data-x", "1"); console.log(w());
        d.setAttribute("aria-label", "a"); console.log(w());
        d.removeAttribute("style"); d.className = "wide"; d.setAttribute("data-y", "2"); console.log(w());
        d.setAttribute("data-big", "y"); console.log(w());
        i.setAttribute("width", "30"); console.log(w());
        d.setAttribute("data-x", "2"); console.log(w());
        </script>"##,
        &["40 40px 10", "40 40px 10", "40 40px 10", "80 80px 10", "120 120px 10", "120 120px 30", "120 120px 30"],
    ),
    (
        // class and id selectors compared with the attribute's text (task 9caa0): exact in a
        // standards-mode document, ASCII case-insensitive in a quirks-mode one (a DOMParser document
        // without a doctype), through querySelectorAll, querySelector, matches and closest
        "class and id selectors by case",
        r##"<div id=Box class="a  Foo b"><span class=foo id=s>x</span></div><script>
        const n = l => Array.from(l, e => e.id).join("/");
        console.log(n(document.querySelectorAll(".Foo")), n(document.querySelectorAll(".foo")), n(document.querySelectorAll(".FOO")));
        console.log(n(document.querySelectorAll("#Box")), n(document.querySelectorAll("#box")), document.querySelector("div.b > .foo").id);
        const s = document.getElementById("s");
        console.log(s.matches(".foo"), s.matches(".Foo"), s.closest(".Foo").id, s.closest("#box"));
        const q = new DOMParser().parseFromString('<div id=Box class="a Foo"><span class=foo id=s></span></div>', "text/html");
        console.log(q.compatMode, n(q.querySelectorAll(".FOO")), n(q.querySelectorAll("#box")), q.querySelector("#S").id);
        const qs = q.querySelector("span");
        console.log(qs.matches(".FOO"), qs.closest("#BOX").id);
        </script>"##,
        &["Box s ", "Box  s", "true false Box null", "BackCompat Box/s Box s", "true Box"],
    ),
    (
        // the root element's primary frame (nsCSSFrameConstructor::ConstructDocElementFrame); release
        // Firefox prints the same line
        "the root element's box",
        r#"<div style="height:300px">x</div><script>
        const h = document.documentElement, r = h.getBoundingClientRect();
        console.log(h.offsetHeight, h.offsetTop, r.top, r.height, document.body.offsetHeight, document.compatMode);
        </script>"#,
        &["316 0 0 316 300 CSS1Compat"],
    ),
    (
        // translated nsDOMMutationObserver; release Firefox prints the same lines
        "mutation observers: records",
        r##"<div id=d><p id=p>x</p><i id=i></i></div><div id=e></div><script>
const d = document.getElementById("d"), p = document.getElementById("p"), i = document.getElementById("i"), e = document.getElementById("e");
const name = n => n ? (n.id || n.nodeName) : "null";
const fmt = r => [r.type, name(r.target), "+" + Array.from(r.addedNodes).map(name).join("/"), "-" + Array.from(r.removedNodes).map(name).join("/"), name(r.previousSibling), name(r.nextSibling), r.attributeName, r.attributeNamespace, JSON.stringify(r.oldValue)].join(",");
const log = recs => recs.map(fmt).join(" | ");
const mo = new MutationObserver(recs => console.log("d: " + log(recs)));
mo.observe(d, { childList: true, attributes: true, attributeOldValue: true, attributeFilter: ["title", "class"], characterData: true, characterDataOldValue: true, subtree: true });
const me = new MutationObserver(recs => console.log("e: " + log(recs)));
me.observe(e, { childList: true });
d.setAttribute("title", "t1"); d.setAttribute("title", "t2"); d.setAttribute("data-x", "1"); d.removeAttribute("missing"); d.removeAttribute("title");
p.firstChild.data = "y"; p.firstChild.textContent = "z";
const s = document.createElement("span"); s.id = "s"; d.insertBefore(s, i);
e.appendChild(p);
const f = document.createDocumentFragment(); const a = document.createElement("a"); a.id = "a"; const b = document.createElement("b"); b.id = "b";
f.appendChild(a); f.appendChild(b);
const mf = new MutationObserver(recs => console.log("f: " + log(recs)));
mf.observe(f, { childList: true });
d.appendChild(f);
d.textContent = "plain";
e.textContent = "";
d.insertBefore(document.createTextNode("t"), d.firstChild);
console.log("sync " + log(mo.takeRecords()));
</script>"##,
        &["sync attributes,d,+,-,null,null,title,,null | attributes,d,+,-,null,null,title,,\"t1\" | attributes,d,+,-,null,null,title,,\"t2\" | characterData,#text,+,-,null,null,,,\"x\" | characterData,#text,+,-,null,null,,,\"y\" | childList,d,+s,-,p,i,,,null | childList,d,+,-p,null,s,,,null | childList,d,+a/b,-,i,null,,,null | childList,d,+#text,-s/i/a/b,null,null,,,null | childList,d,+#text,-,null,#text,,,null", "e: childList,e,+p,-,null,null,,,null | childList,e,+,-p,null,null,,,null", "f: childList,#document-fragment,+,-a/b,null,null,,,null"],
    ),
    (
        // translated nsDOMMutationObserver; release Firefox prints the same lines
        "mutation observers: transient receivers and errors",
        r##"<div id=d><section id=s><b id=b>x</b></section></div><script>
const d = document.getElementById("d"), s = document.getElementById("s"), b = document.getElementById("b");
const name = n => n ? (n.id || n.nodeName) : "null";
const fmt = r => [r.type, name(r.target), "+" + Array.from(r.addedNodes).map(name).join("/"), "-" + Array.from(r.removedNodes).map(name).join("/"), r.attributeName].join(",");
const mo = new MutationObserver(recs => {
  console.log("cb " + recs.map(fmt).join(" | "));
  if (!window.done) { window.done = 1; b.setAttribute("title", "after"); s.appendChild(document.createElement("u")); Promise.resolve().then(() => console.log("after-delivery microtask")); }
});
mo.observe(d, { childList: true, subtree: true, attributes: true });
d.removeChild(s);
b.setAttribute("title", "removed");
s.appendChild(document.createElement("em"));
const mo2 = new MutationObserver(recs => console.log("cb2 " + recs.map(fmt).join(" | ")));
mo2.observe(b, { attributes: true });
b.setAttribute("class", "c");
mo2.disconnect();
b.setAttribute("lang", "en");
const sr = document.createElement("div"); d.appendChild(sr); const root = sr.attachShadow({ mode: "open" }); root.appendChild(document.createElement("q"));
try { mo.observe(d, {}); } catch (e) { console.log(e.name + ": " + e.message); }
try { mo.observe(d, { attributeOldValue: true, attributes: false }); } catch (e) { console.log(e.name + ": " + e.message); }
try { mo.observe(null, { childList: true }); } catch (e) { console.log(e.name + ": " + e.message); }
try { MutationObserver(() => {}); } catch (e) { console.log(e.name + ": " + e.message); }
try { new MutationObserver(1); } catch (e) { console.log(e.name + ": " + e.message); }
</script>"##,
        &["TypeError: MutationObserver.observe: One of 'childList', 'attributes', 'characterData' must not be false.", "TypeError: MutationObserver.observe: One of 'childList', 'attributes', 'characterData' must not be false.", "TypeError: MutationObserver.observe: Argument 1 is not an object.", "TypeError: MutationObserver constructor: 'new' is required", "TypeError: MutationObserver constructor: Argument 1 is not an object.", "cb childList,d,+,-s, | attributes,b,+,-,title | childList,s,+EM,-, | attributes,b,+,-,class | attributes,b,+,-,lang | childList,d,+DIV,-,", "after-delivery microtask"],
    ),
    (
        // translated nsDOMMutationObserver; release Firefox prints the same lines
        "mutation observers: delivery order",
        r##"<div id=d></div><script>
const d = document.getElementById("d");
const order = [];
const m1 = new MutationObserver(() => { order.push("m1"); });
const m2 = new MutationObserver(() => { order.push("m2"); });
const m3 = new MutationObserver(() => { order.push("m3"); });
m3.observe(d, { attributes: true }); m2.observe(d, { attributes: true }); m1.observe(d, { attributes: true });
Promise.resolve().then(() => order.push("p1"));
d.setAttribute("x", "1");
Promise.resolve().then(() => order.push("p2"));
// Polymer's microtask queue: a text node whose data changes
const t = document.createTextNode(""); let n = 0;
new MutationObserver(() => { order.push("text" + n); }).observe(t, { characterData: true });
t.textContent = String(++n);
setTimeout(() => {
  console.log(order.join(" "));
  const r = new MutationObserver(() => {}); console.log(Object.prototype.toString.call(r), typeof MutationRecord, r instanceof MutationObserver);
  r.observe(d, { attributes: true }); d.setAttribute("y", "2"); const recs = r.takeRecords();
  console.log(recs.length, recs[0] instanceof MutationRecord, recs[0].addedNodes === recs[0].addedNodes, recs[0].addedNodes.length, Object.keys(recs[0]).length, "type" in recs[0]);
}, 0);
</script>"##,
        &["p1 m1 m2 m3 text1 p2", "[object MutationObserver] function true", "1 true true 0 0 true"],
    ),
    (
        // translated nsDOMMutationObserver; release Firefox prints the same lines (with innerHTML and replaceChildren, whose
        // batches the mini runtime's bridge.js calls as natives)
        "mutation observers: innerHTML and replaceChildren batches, slotchange",
        r##"<div id=d><p id=p></p><q id=q></q></div><div id=h></div><script>
const d = document.getElementById("d"), h = document.getElementById("h");
const name = n => n ? (n.id || n.nodeName) : "null";
const fmt = r => [r.type, name(r.target), "+" + Array.from(r.addedNodes).map(name).join("/"), "-" + Array.from(r.removedNodes).map(name).join("/"), name(r.previousSibling), name(r.nextSibling)].join(",");
const log = [];
const mo = new MutationObserver(recs => log.push("mo " + recs.map(fmt).join(" | ")));
mo.observe(d, { childList: true });
__mrtSetInnerHTML(d, __mrtParseFragment('<a id="a"></a><b id="b"></b>'));
const x = document.createElement("x"); x.id = "x"; const y = document.createElement("y"); y.id = "y";
{ const f = document.createDocumentFragment(); f.appendChild(x); f.appendChild(y); __mrtReplaceChildren(d, f); }
__mrtReplaceChildren(d, x);
__mrtReplaceChildren(d, null);
const root = h.attachShadow({ mode: "open" }); root.appendChild(document.createElement("slot"));
root.firstChild.addEventListener("slotchange", () => log.push("slotchange"));
const mh = new MutationObserver(() => log.push("mh"));
mh.observe(h, { childList: true });
h.appendChild(document.createElement("span"));
setTimeout(() => console.log(log.join(" ; ")), 0);
</script>"##,
        &["mo childList,d,+a/b,-p/q,null,null | childList,d,+x/y,-a/b,null,null | childList,d,+x,-x/y,null,null | childList,d,+,-x,null,null ; mh ; slotchange"],
    ),
    (
        // nsDOMTokenList over the class attribute, never className (a Polymer element can shadow
        // className with an accessor that reads undefined); release Firefox prints the same lines
        "classList",
        r#"<div id=a class="x  y x"></div><div id=b></div><script>
        const a = document.getElementById("a"), b = document.getElementById("b");
        console.log(a.classList.length, a.classList.value, a.classList[1], a.classList.item(5), a.classList === a.classList, a.classList instanceof DOMTokenList);
        b.classList.remove("q"); console.log(b.hasAttribute("class"));
        console.log(b.classList.toggle("t"), b.getAttribute("class"), b.classList.toggle("t"), JSON.stringify(b.getAttribute("class")));
        a.classList.add("z", "x"); console.log(a.getAttribute("class"));
        console.log(a.classList.replace("y", "z"), a.getAttribute("class"), a.classList.replace("nope", "w"));
        try { a.classList.add("a b"); } catch (e) { console.log(e.name); }
        try { a.classList.add(""); } catch (e) { console.log(e.name); }
        Object.defineProperty(b, "className", { get() { return undefined; } });
        b.setAttribute("class", "p q"); console.log(b.classList.length, b.classList.contains("q"), [...b.classList].join(","));
        </script>"#,
        &[
            "2 x  y x y null true true",
            "false",
            "true t false \"\"",
            "x y z",
            "true x z false",
            "InvalidCharacterError",
            "SyntaxError",
            "2 true p,q",
        ],
    ),
    (
        "attribute names: case and the parser's prefixes",
        r##"<div id=d DATA-X=1></div><svg id=s xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 24 24"><use xlink:href="#p" xml:lang="en"/></svg><script>
        const d = document.getElementById("d"), s = document.getElementById("s"), u = s.firstElementChild;
        console.log(JSON.stringify(s.getAttributeNames()), JSON.stringify(u.getAttributeNames()));
        console.log(s.getAttribute("viewBox"), s.getAttribute("viewbox"), s.getAttribute("xmlns"), s.getAttribute("xmlns:xlink"));
        console.log(u.getAttribute("xlink:href"), u.getAttribute("href"), u.hasAttribute("xml:lang"), u.getAttribute("XLINK:HREF"));
        s.setAttribute("viewBox", "0 0 10 10"); s.setAttribute("xlink:href", "#q");
        console.log(JSON.stringify(s.getAttributeNames()), s.getAttribute("viewBox"), s.getAttribute("xlink:href"));
        u.setAttribute("xlink:href", "#r"); console.log(u.getAttribute("xlink:href"), JSON.stringify(u.getAttributeNames()));
        s.removeAttribute("xmlns:xlink"); s.removeAttribute("VIEWBOX"); console.log(JSON.stringify(s.getAttributeNames()));
        d.setAttribute("Foo", "1"); console.log(JSON.stringify(d.getAttributeNames()), d.getAttribute("FOO"), d.getAttribute("data-x"), d.hasAttribute("DATA-X"));
        </script>"##,
        &[
            "[\"id\",\"xmlns\",\"xmlns:xlink\",\"viewBox\"] [\"xlink:href\",\"xml:lang\"]",
            "0 0 24 24 null http://www.w3.org/2000/svg http://www.w3.org/1999/xlink",
            "#p null true null",
            "[\"id\",\"xmlns\",\"xmlns:xlink\",\"viewBox\",\"xlink:href\"] 0 0 10 10 #q",
            "#r [\"xlink:href\",\"xml:lang\"]",
            "[\"id\",\"xmlns\",\"viewBox\",\"xlink:href\"]",
            "[\"id\",\"data-x\",\"foo\"] 1 1 true",
        ],
    ),
    (
        "createElementNS",
        r##"<div id=d></div><script>
        const d = document.getElementById("d");
        const SVG = "http://www.w3.org/2000/svg", XHTML = "http://www.w3.org/1999/xhtml";
        const c = document.createElementNS(SVG, "clipPath"), m = document.createElementNS("http://www.w3.org/1998/Math/MathML", "mi");
        console.log(c.tagName, c.localName, c.namespaceURI, m.tagName, m.namespaceURI);
        const h = document.createElementNS(XHTML, "DIV"), n = document.createElementNS(null, "Foo");
        console.log(h.tagName, h.localName, h.namespaceURI, h instanceof HTMLElement, n.tagName, n.namespaceURI);
        const s = document.createElementNS(SVG, "svg"); s.appendChild(c); c.appendChild(document.createElementNS(SVG, "rect")); d.appendChild(s);
        console.log([...d.querySelectorAll("*")].map(e => e.tagName).join(","), d.querySelectorAll("clipPath").length, d.getElementsByTagName("rect").length);
        for (const [ns, q] of [[SVG, ""], [SVG, "1a"], [SVG, "a b"], [null, "x:y"], [SVG, "xmlns"], [SVG, "xml:a"], ["http://www.w3.org/2000/xmlns/", "x"], [SVG, ":a"], [SVG, "a:"]]) {
          try { document.createElementNS(ns, q); console.log("ok", q); } catch (e) { console.log(e.name, JSON.stringify(q)); }
        }
        </script>"##,
        &[
            "clipPath clipPath http://www.w3.org/2000/svg mi http://www.w3.org/1998/Math/MathML",
            "DIV DIV http://www.w3.org/1999/xhtml true Foo null",
            "svg,clipPath,rect 1 1",
            "InvalidCharacterError \"\"",
            "InvalidCharacterError \"1a\"",
            "InvalidCharacterError \"a b\"",
            "NamespaceError \"x:y\"",
            "NamespaceError \"xmlns\"",
            "NamespaceError \"xml:a\"",
            "NamespaceError \"x\"",
            "InvalidCharacterError \":a\"",
            "InvalidCharacterError \"a:\"",
        ],
    ),
    (
        "events",
        r#"<div id=o><p id=i>x</p></div><script>
        const o = document.getElementById("o"), i = document.getElementById("i"), log = [];
        o.addEventListener("ping", e => log.push("o-capture-" + e.eventPhase), true);
        o.addEventListener("ping", e => log.push("o-bubble-" + e.eventPhase));
        i.addEventListener("ping", e => { log.push("i-" + e.eventPhase + "-" + (e.target === i)); });
        window.addEventListener("ping", () => log.push("window"));
        i.dispatchEvent(new Event("ping", { bubbles: true }));
        i.dispatchEvent(new Event("ping"));
        console.log(log.join(" "));
        const once = []; o.addEventListener("x", () => once.push(1), { once: true });
        o.dispatchEvent(new CustomEvent("x", { detail: 1 })); o.dispatchEvent(new CustomEvent("x"));
        console.log(once.length);
        </script>"#,
        &["o-capture-1 i-2-true o-bubble-3 window o-capture-1 i-2-true", "1"],
    ),
    (
        "event handler properties",
        r#"<div id=o><p id=i>x</p></div><script>
        const o = document.getElementById("o"), i = document.getElementById("i"), log = [];
        i.onping = e => log.push("i-prop-" + e.eventPhase);
        i.addEventListener("ping", () => log.push("i-listener"));
        o.onping = e => log.push("o-prop-" + e.eventPhase);
        o.addEventListener("ping", () => log.push("o-capture"), true);
        i.dispatchEvent(new Event("ping", { bubbles: true }));
        o.onping = null;
        i.dispatchEvent(new Event("ping", { bubbles: true }));
        console.log(log.join(" "));
        i.oncancel = () => false;
        console.log(i.dispatchEvent(new Event("cancel", { cancelable: true })));
        window.onload = () => console.log("window.onload");
        </script>"#,
        &["o-capture i-listener i-prop-2 o-prop-3 o-capture i-listener i-prop-2", "false", "window.onload"],
    ),
    (
        "timers, microtasks and load events",
        r#"<script>
        const log = [];
        setTimeout(() => log.push("t20"), 20);
        setTimeout(() => log.push("t10"), 10);
        const iv = setInterval(() => { log.push("iv"); if (log.filter(x => x == "iv").length == 3) clearInterval(iv); }, 7);
        Promise.resolve().then(() => log.push("micro"));
        document.addEventListener("DOMContentLoaded", () => log.push("dcl"));
        window.addEventListener("load", () => log.push("load"));
        setTimeout(() => console.log(log.join(" ")), 100);
        </script>"#,
        &["micro dcl load iv t10 iv t20 iv"],
    ),
    (
        "element interfaces",
        // each tag's interface as Gecko's tag list gives it
        r#"<script>
        const proto = t => Object.getPrototypeOf(document.createElement(t));
        console.log(proto("div") === HTMLDivElement.prototype, proto("h3") === HTMLHeadingElement.prototype, proto("td") === HTMLTableCellElement.prototype);
        console.log(Object.getPrototypeOf(HTMLVideoElement.prototype) === HTMLMediaElement.prototype, Object.getPrototypeOf(HTMLDivElement.prototype) === HTMLElement.prototype);
        console.log(proto("section") === HTMLElement.prototype, proto("x-y") === HTMLElement.prototype, proto("blink") === HTMLUnknownElement.prototype);
        console.log(document.body instanceof HTMLBodyElement, document.body instanceof HTMLDivElement);
        try { new HTMLDivElement(); } catch (e) { console.log(e.message.split(":")[0]); }
        </script>"#,
        &["true true true", "true true", "true true true", "true false", "TypeError"],
    ),
    (
        "a second document and template contents",
        // release Firefox 157's lines (bench/firefox_case.py; until session 42 the first line said
        // the other document's node was not connected)
        r#"<script>
        const d = document.implementation.createHTMLDocument("inert");
        const p = d.createElement("p"); d.body.appendChild(p);
        console.log(d.nodeType, d.title === undefined ? d.head.firstChild.textContent : d.title, d.body.parentNode === d.documentElement, p.ownerDocument === d, p.isConnected);
        console.log(document.body !== d.body, document.implementation.createHTMLDocument().head.childNodes.length);
        const t = document.createElement("template");
        t.content.appendChild(document.createElement("b"));
        const c = t.cloneNode(true);
        console.log(t.childNodes.length, t.content.childNodes.length, c.content.firstChild.tagName, c.content !== t.content);
        </script>"#,
        &["9 inert true true true", "true 0", "0 1 B true"],
    ),
    (
        "custom elements",
        // Gecko's order: an upgrade runs the constructor, then the attributeChangedCallback and
        // connectedCallback it enqueued; createElement and `new` construct at once; a clone is upgraded
        r#"<x-a id=a title=t1></x-a><script>
        const log = [];
        class XA extends HTMLElement {
          static get observedAttributes() { return ["title"]; }
          constructor() { super(); log.push("ctor-" + this.isConnected + "-" + (this instanceof XA)); this.field = 1; }
          connectedCallback() { log.push("conn-" + this.id); }
          disconnectedCallback() { log.push("disc-" + this.id); }
          attributeChangedCallback(n, o, v) { log.push("attr-" + n + "-" + o + "-" + v); }
        }
        const a = document.getElementById("a");
        console.log(a instanceof XA, document.querySelectorAll("x-a:not(:defined)").length, document.querySelectorAll("body:defined").length);
        customElements.define("x-a", XA);
        console.log(log.splice(0).join(" "));
        console.log(a instanceof XA, a.field, document.querySelectorAll("x-a:not(:defined)").length, customElements.get("x-a") === XA, customElements.getName(XA));
        a.setAttribute("title", "t2"); a.setAttribute("lang", "x"); a.removeAttribute("title");
        console.log(log.splice(0).join(" "));
        const b = document.createElement("x-a"); b.id = "b";
        document.body.appendChild(b); b.remove();
        const c = new XA(), d = a.cloneNode();
        console.log(log.splice(0).join(" "), d instanceof XA, d.field, c.parentNode);
        customElements.whenDefined("x-b").then(k => console.log("defined", k === XB));
        class XB extends HTMLElement {}
        const errs = [];
        for (const f of [() => customElements.define("nodash", XB), () => customElements.define("x-a", XB), () => customElements.define("x-c", XA), () => new HTMLElement(), () => new XB()]) {
          try { f(); errs.push("none"); } catch (e) { errs.push(e.message.split(":")[0]); }
        }
        console.log(errs.join(" "));
        customElements.define("x-b", XB);
        </script>"#,
        &[
            "false 1 1",
            "ctor-true-true attr-title-null-t1 conn-a",
            "true 1 0 true x-a",
            "attr-title-t1-t2 attr-title-t2-null",
            "ctor-false-true conn-b disc-b ctor-false-true ctor-false-true true 1 null",
            "SyntaxError NotSupportedError NotSupportedError TypeError TypeError",
            "defined true",
        ],
    ),
    (
        "shadow DOM",
        // declarative and attached shadow roots, named slots, the composed event path with
        // retargeting and closed trees, slotchange, custom elements in a shadow tree; the lines are
        // release Firefox 157's for the same page (bench/firefox_case.py)
        r#"<div id=h><template shadowrootmode=open shadowrootclonable><slot name=a></slot><slot id=d><i>fallback</i></slot><p id=in>x</p></template><span slot=a>A</span>t</div>
<div id=c><template shadowrootmode=closed><b>c</b></template></div>
<p id=p><template shadowrootmode=bogus><b>t</b></template></p>
<script>
const h = document.getElementById("h"), c = document.getElementById("c"), r = h.shadowRoot;
const inP = r.getElementById("in");
console.log(r instanceof ShadowRoot, r instanceof DocumentFragment, r.mode, r.host === h, r.nodeType, r.nodeName, c.shadowRoot, h.children.length, r.childNodes.length, r.clonable, r.serializable, r.delegatesFocus, r.slotAssignment);
console.log(document.getElementById("in"), inP.textContent, r.querySelectorAll("slot").length, inP.getRootNode() === r, inP.getRootNode({composed: true}) === document, inP.isConnected, r.parentNode, inP.parentNode === r, inP.parentElement, document.getElementById("p").firstElementChild.tagName);
const span = h.firstElementChild, d = r.getElementById("d");
console.log(span.assignedSlot === r.querySelector("slot[name=a]"), h.lastChild.assignedSlot === d, d.assignedNodes().length, d.assignedNodes().map(n => n.nodeName).join(), r.querySelector("slot").assignedElements().length, document.createElement("slot").assignedNodes({flatten: true}).length);
const errName = e => (e.name === "Error" ? e.message.split(":")[0] : e.name);
const errs = [];
for (const f of [() => c.attachShadow({mode: "open"}), () => document.createElement("a").attachShadow({mode: "open"}), () => document.createElement("div").attachShadow({}), () => document.createElement("div").attachShadow({mode: "x"}), () => r.cloneNode()]) {
  try { f(); errs.push("none"); } catch (e) { errs.push(errName(e)); }
}
console.log(errs.join(" "));
const copy = h.cloneNode(false);
console.log(copy.shadowRoot && copy.shadowRoot.childNodes.length, copy.childNodes.length, c.cloneNode(true).shadowRoot);

// events across the boundary
const host2 = document.createElement("div");
document.body.appendChild(host2);
const sr = host2.attachShadow({mode: "open"});
const inner = document.createElement("div"), slot = document.createElement("slot");
inner.appendChild(slot);
sr.appendChild(inner);
slot.addEventListener("slotchange", e => console.log("slotchange", e.target === slot, e.composed, slot.assignedNodes().length));
const light = document.createElement("i");
host2.appendChild(light);
host2.appendChild(document.createElement("b"));
const names = new Map([[document, "doc"], [host2, "host"], [sr, "sr"], [inner, "inner"], [slot, "slot"], [light, "light"], [null, "null"]]);
const log = [];
for (const t of [document, host2, sr, inner, slot, light]) t.addEventListener("x", e => log.push(names.get(t) + ":" + names.get(e.target)));
inner.dispatchEvent(new Event("x", {bubbles: true}));
log.push("|");
inner.dispatchEvent(new Event("x", {bubbles: true, composed: true}));
log.push("|");
light.dispatchEvent(new Event("x", {bubbles: true}));
console.log(log.join(" "));
const ev = new Event("x", {bubbles: true, composed: true});
inner.dispatchEvent(ev);
const ev2 = new Event("x", {bubbles: true});
inner.dispatchEvent(ev2);
console.log(names.get(ev.target), names.get(ev2.target), ev.composedPath().length);
const paths = [];
inner.addEventListener("y", e => paths.push(e.composedPath().map(n => names.get(n) || n.nodeName || "window").join(",")));
host2.addEventListener("y", e => paths.push(e.composedPath().map(n => names.get(n) || n.nodeName || "window").join(",")));
inner.dispatchEvent(new Event("y", {bubbles: true, composed: true}));
console.log(paths.join(" / "));

// a closed tree is hidden from outside
const host3 = document.createElement("span");
document.body.appendChild(host3);
const closed = host3.attachShadow({mode: "closed"});
const deep = document.createElement("u");
closed.appendChild(deep);
const seen = [];
host3.addEventListener("z", e => seen.push(e.composedPath().length + " " + (e.target === host3)));
deep.addEventListener("z", e => seen.push(e.composedPath().length + " " + (e.target === deep)));
deep.dispatchEvent(new Event("z", {bubbles: true, composed: true}));
console.log(seen.join(" / "), host3.shadowRoot, closed.host === host3);

// custom elements inside a shadow tree
class XS extends HTMLElement { connectedCallback() { console.log("connected", this.getRootNode() === sr); } }
customElements.define("x-s", XS);
sr.appendChild(document.createElement("x-s"));
// taking over a declarative root empties it
console.log(h.attachShadow({mode: "open"}) === r, r.childNodes.length, span.assignedSlot, span.hasAttributes(), document.createElement("i").hasAttributes());
</script>"#,
        &[
            "true true open true 11 #document-fragment null 1 3 true false false named",
            "null x 2 true true true null true null TEMPLATE",
            "true true 1 #text 1 0",
            "NotSupportedError NotSupportedError TypeError TypeError NotSupportedError",
            "3 0 null",
            "inner:inner sr:inner | inner:inner sr:inner host:host doc:host | light:light slot:light inner:light sr:light host:light doc:light",
            "host null 0",
            "inner,sr,host,BODY,HTML,doc,window / inner,sr,host,BODY,HTML,doc,window",
            "7 true / 5 true null true",
            "connected true",
            "true 0 null true false",
            "slotchange true false 2",
        ],
    ),
    (
        "module scripts",
        // static imports (one module evaluated once per URL), a JSON module, a namespace, module
        // scope and strictness, import.meta, top-level await, deferred order after the classic
        // scripts, dynamic import() and its failures (bare specifier, syntax error, MIME type, a
        // fetch that fails, a failing import below), from a classic script too, and module script
        // elements inserted by script; the lines are release Firefox 157's (bench/firefox_case.py),
        // plus the console errors Firefox reports for each module fetch that fails
        // (ScriptLoader::ReportErrorToConsole), which firefox_case.py does not capture
        r#"<script>console.log("classic 1");</script>
<script type="module">
import { a, f } from "data:text/javascript,export const a = 1; export function f() { return 'f' + a; } window.count = (window.count || 0) + 1;";
import { a as a2 } from "data:text/javascript,export const a = 1; export function f() { return 'f' + a; } window.count = (window.count || 0) + 1;";
import json from "data:application/json,{\"k\":[1,2]}" with { type: "json" };
import * as ns from "data:text/javascript,export default 7; export const z = 2;";
var mv = 1;
console.log("module 1", a, f(), a2, window.count, json.k.length, ns.default, ns.z, Object.prototype.toString.call(ns), typeof window.mv, this === undefined);
console.log(import.meta.url.startsWith("http://"), import.meta.resolve("./x/../y.js") === import.meta.url.replace(/[^/]*$/, "y.js"), import.meta.resolve("HTTPS://A.test/b/../c.js"), Object.keys(import.meta).join());
await Promise.resolve();
console.log("after await");
</script>
<script type="module">
console.log("module 2", document.currentScript == null);
const fails = [];
const t = p => p.then(m => "ok " + m.default, e => e.name + ": " + e.message.replace(/http:\/\/[^ ]*/, "URL"));
Promise.all([
  t(import("bare")),
  t(import("data:text/javascript,export let = ;")),
  t(import("data:text/plain,export default 1")),
  t(import("./missing-module.js")),
  t(import("data:text/javascript,export default 'dyn'")),
  t(import("data:text/javascript,import 'nope'; export default 1")),
]).then(r => console.log(r.join(" | ")));
</script>
<script>console.log("classic 2", typeof mv);
import("data:text/javascript,export default 'from classic'").then(m => console.log(m.default));
const s = document.createElement("script");
s.setAttribute("type", "module");
s.setAttribute("src", "data:text/javascript,console.log('inserted module')");
s.onload = () => console.log("inserted load");
document.body.appendChild(s);
const s2 = document.createElement("script");
s2.setAttribute("type", "module");
s2.setAttribute("src", "./missing-module2.js");
s2.onerror = () => console.log("inserted error");
document.body.appendChild(s2);
</script>"#,
        &[
            "classic 1",
            "classic 2 undefined",
            "module 1 1 f1 1 1 2 7 2 [object Module] undefined true",
            "true true https://a.test/c.js url,resolve",
            "after await",
            "module 2 true",
            "from classic",
            "inserted module",
            "inserted load",
            "Loading failed for the module with source \u{201c}http://localhost/missing-module2.js\u{201d}.",
            "inserted error",
            "Loading failed for the module with source \u{201c}data:text/plain,export default 1\u{201d}.",
            "Loading failed for the module with source \u{201c}http://localhost/missing-module.js\u{201d}.",
            "TypeError: The specifier \u{201c}bare\u{201d} was a bare specifier, but was not remapped to anything. Relative module specifiers must start with \u{201c}./\u{201d}, \u{201c}../\u{201d} or \u{201c}/\u{201d}. | SyntaxError: missing variable name, got '=' | TypeError: error loading dynamically imported module: data:text/plain,export default 1 | TypeError: error loading dynamically imported module: URL | ok dyn | TypeError: The specifier \u{201c}nope\u{201d} was a bare specifier, but was not remapped to anything. Relative module specifiers must start with \u{201c}./\u{201d}, \u{201c}../\u{201d} or \u{201c}/\u{201d}.",
        ],
    ),
    (
        "DOMParser and Element.animate",
        // DOMParser: a whole document, scripts not run, <noscript> as elements, no declarative
        // shadow roots; Element.animate: the timing on the virtual clock (no effect is applied),
        // the finished promise before the finish and cancel events; release Firefox 157's lines
        r#"<div id=t></div>
<script>
const doc = new DOMParser().parseFromString("<!doctype html><title>x</title><p id=a>one<script>window.ran = 1<\/script><noscript><b>n</b></noscript><div><template shadowrootmode=open><i>s</i></template></div>", "text/html");
console.log(doc instanceof Document, doc === document, doc.doctype && doc.doctype.name, doc.documentElement.tagName, doc.body.childElementCount, doc.getElementById("a").textContent, typeof window.ran, doc.querySelector("noscript").firstChild.nodeType, doc.querySelector("div").shadowRoot, doc.querySelector("template") !== null, doc.body.firstChild.isConnected);
const moved = document.importNode(doc.getElementById("a"), true);
document.getElementById("t").appendChild(moved);
console.log(document.getElementById("a") === moved, moved.ownerDocument === document);
try { new DOMParser().parseFromString("x", "text/plain"); } catch (e) { console.log(e.name); }

const el = document.getElementById("t");
const a = el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: 100, delay: 20 });
console.log(a instanceof Animation, a.playState, a.currentTime, a.effect.getComputedTiming().endTime, el.getAnimations().length, document.getAnimations().length);
a.onfinish = e => console.log("finish", a.playState, a.currentTime, el.getAnimations().length);
a.finished.then(x => console.log("finished", x === a));
const b = el.animate({ opacity: [0, 1] }, 1000);
b.finished.catch(e => console.log("b rejected", e.name));
b.addEventListener("cancel", () => console.log("cancel", b.playState, b.currentTime));
setTimeout(() => b.cancel(), 50);
const c = el.animate([], { duration: Infinity });
console.log(c.effect.getComputedTiming().endTime, c.playState);
c.pause();
console.log(c.playState, c.currentTime);
</script>"#,
        &[
            "true false html HTML 2 onewindow.ran = 1n undefined 1 null true true",
            "true true",
            "TypeError",
            "true running 0 120 1 1",
            "Infinity running",
            "paused 0",
            "b rejected AbortError",
            "cancel idle null",
            "finished true",
            "finish finished 120 1",
        ],
    ),
    (
        "Element.animate with the page's Animation replaced",
        // animate builds the runtime's own KeyframeEffect and Animation, not whatever the page put in
        // the globals (YouTube's web-animations polyfill, task c5403); release Firefox 157's lines
        r#"<div id=t></div>
<script>
const el = document.getElementById("t");
const NativeAnimation = Animation, NativeEffect = KeyframeEffect;
window.Animation = function () { throw new Error("page Animation"); };
window.KeyframeEffect = function () { throw new Error("page KeyframeEffect"); };
const a = el.animate([{ opacity: 0 }, { opacity: 1 }], 100);
console.log(a instanceof NativeAnimation, a.effect instanceof NativeEffect, a.playState, el.getAnimations().length);
</script>"#,
        &["true true running 1"],
    ),
    (
        "streams and text decoding",
        // the Streams standard (ReadableStream, its reader, pulls, WritableStream, TransformStream,
        // pipes, tee, async iteration, errors, cancel), streaming UTF-8 decoding and TextDecoderStream;
        // release Firefox 157's lines. The asynchronous results are printed in a fixed order by a later
        // task: the microtask steps of each operation are not Gecko's (GAPS B27). ASCII only, as
        // firefox_case.py's page has no charset
        r#"<script>
const log = [];
const out = {};
const put = (k, ...v) => { out[k] = v.join(" "); };
const cps = s => [...s].map(c => (c.codePointAt(0) > 126 ? "U+" + c.codePointAt(0).toString(16) : c)).join("");
const text = "h" + String.fromCodePoint(0xe9) + "llo " + String.fromCodePoint(0x1f600) + "!";
const bytes = new TextEncoder().encode(text);
const d = new TextDecoder();
const parts = [d.decode(bytes.subarray(0, 2), { stream: true }), d.decode(bytes.subarray(2, 9), { stream: true }), d.decode(bytes.subarray(9))];
console.log(bytes.length, parts.map(cps).join("|"), cps(new TextDecoder().decode(new Uint8Array([0xef, 0xbb, 0xbf, 0x41, 0xff, 0xe2, 0x82, 0x42]))));
try { new TextDecoder("utf-8", { fatal: true }).decode(new Uint8Array([0xc3])); } catch (e) { console.log("fatal", e.name); }

const rs = new ReadableStream({
  start(c) { log.push("start " + c.desiredSize); c.enqueue("a"); log.push("after a " + c.desiredSize); },
  pull(c) { log.push("pull"); if (log.filter(x => x === "pull").length === 2) { c.enqueue("b"); c.close(); } },
});
log.push("locked " + rs.locked);
const reader = rs.getReader();
(async () => {
  const r1 = await reader.read();
  const r2 = await reader.read();
  const r3 = await reader.read();
  log.push(JSON.stringify([r1, r2, r3]));
  put("1 reads", log.join(", "));
})();

// split bytes through TextDecoderStream, then a transform and a writable sink
const src = new ReadableStream({ start(c) { c.enqueue(bytes.subarray(0, 2)); c.enqueue(bytes.subarray(2, 8)); c.enqueue(bytes.subarray(8)); c.close(); } });
const upper = new TransformStream({ transform(chunk, c) { c.enqueue(chunk.toUpperCase()); }, flush(c) { c.enqueue("<end>"); } });
const got = [];
src.pipeThrough(new TextDecoderStream()).pipeThrough(upper).pipeTo(new WritableStream({ write(ch) { got.push(cps(ch)); }, close() { got.push("closed"); } }))
  .then(() => put("2 piped", got.join("|"), src.locked));

// tee and async iteration
const [t1, t2] = new ReadableStream({ start(c) { c.enqueue(1); c.enqueue(2); c.close(); } }).tee();
(async () => {
  const a = [], b = [];
  for await (const x of t1) a.push(x);
  for await (const x of t2) b.push(x);
  put("3 tee", a.join(), b.join());
})();

// errors and cancel
const er = new ReadableStream({ start(c) { c.error(new Error("boom")); } });
er.getReader().read().catch(e => put("4 errored", e.message));
let cancelled;
const cr = new ReadableStream({ cancel(r) { cancelled = r; } });
cr.cancel("why").then(() => put("5 cancel", cancelled, cr.locked));
try { const l = new ReadableStream(); l.getReader(); l.getReader(); } catch (e) { console.log("second reader", e.name); }
const w = new WritableStream({ write() { throw new Error("nope"); } });
const wr = w.getWriter();
wr.write("x").catch(e => put("6 write failed", e.message));
wr.closed.catch(e => put("7 writer closed rejected", e.message));
console.log(typeof ReadableStream.from);
setTimeout(() => { for (const k of Object.keys(out).sort()) console.log(k, out[k]); }, 0);
</script>"#,
        &[
            "12 h|U+e9llo |U+1f600! AU+fffdU+fffdB",
            "fatal TypeError",
            "second reader TypeError",
            "function",
            "1 reads start 1, after a 0, locked false, pull, pull, [{\"done\":false,\"value\":\"a\"},{\"done\":false,\"value\":\"b\"},{\"done\":true}]",
            "2 piped H|U+c9LLO |U+1f600!|<end>|closed false",
            "3 tee 1,2 1,2",
            "4 errored boom",
            "5 cancel why false",
            "6 write failed nope",
            "7 writer closed rejected nope",
        ],
    ),
    (
        "two scripts in order",
        r#"<script>var shared = 1; document.body.appendChild(document.createElement("i"));</script>
        <script>console.log(shared + 1, document.querySelectorAll("i").length);</script>"#,
        &["2 1"],
    ),
];

fn run_case(body: &str) -> (Vec<String>, usize) {
    let html = format!("<!doctype html><html><head><title>t</title></head><body>{body}</body></html>");
    let mut page = Page::new(Dom::parse(&html));
    page.run_scripts("case");
    page.finish_loading(10_000);
    (page.console().to_vec(), page.errors())
}

/// The global options and Gecko's zone and compartment rule for frames (SelectZone), checked
/// directly; returns the failures.
fn global_checks() -> usize {
    let mut failures = 0;
    let mut check = |name: &str, ok: bool, detail: String| {
        println!("{} {name}", if ok { "ok  " } else { "FAIL" });
        if !ok {
            failures += 1;
            println!("  {detail}");
        }
    };
    let html = "<!doctype html><html><body></body></html>";
    let opts = |origin: &str, secure: bool| GlobalOptions { origin: origin.into(), secure_context: secure, cross_origin_isolated: false };

    // a cross-origin isolated page gets the SharedArrayBuffer constructor; a plain http one is not
    // a secure context
    let mut page = Page::with_global(Dom::parse(html), &GlobalOptions { origin: "https://a.test".into(), secure_context: true, cross_origin_isolated: true });
    page.eval("console.log(isSecureContext, crossOriginIsolated, typeof SharedArrayBuffer)", "check");
    check("global: cross-origin isolated", page.console() == ["true true function"], format!("{:?}", page.console()));
    drop(page);
    let mut page = Page::with_global(Dom::parse(html), &opts("http://a.test", false));
    page.eval("console.log(isSecureContext)", "check");
    check("global: not a secure context", page.console() == ["false"], format!("{:?}", page.console()));
    drop(page);

    // frames of https://a.test: all in the page's zone; the page's compartment for the same origin;
    // one compartment for each other origin, shared by its frames; an opaque origin shares nothing
    let mut page = Page::with_global(Dom::parse(html), &opts("https://a.test", true));
    let rel = |same_zone, same_page, secure, same_earlier| FrameGlobalRelation {
        same_zone_as_page: same_zone,
        same_compartment_as_page: same_page,
        secure_context: secure,
        same_compartment_as_earlier_frame: same_earlier,
    };
    let frames = [
        ("same origin", opts("https://a.test", true), rel(true, true, true, false)),
        ("cross origin", opts("https://b.test", true), rel(true, false, true, false)),
        ("cross origin again", opts("https://b.test", true), rel(true, false, true, true)),
        ("opaque origin", opts("null", true), rel(true, false, true, false)),
        ("opaque origin again", opts("null", true), rel(true, false, true, false)),
        ("insecure frame", opts("http://c.test", false), rel(true, false, false, false)),
    ];
    for (name, o, want) in frames {
        let got = page.new_frame_global(&o).map(|i| page.frame_global_relation(i));
        check(&format!("frame global: {name}"), got == Some(want), format!("want {want:?}, got {got:?}"));
    }
    failures
}

/// A page hibernated at 100 ms is a background tab (SPEC decision 22, GAPS B79): its document gets
/// `visibilitychange` in a task and is hidden, its animation frames wait, and its timeouts run in
/// batches 1 s apart, each batch running every timeout due by then (Gecko's TimeoutManager with
/// dom.min_background_timeout_value). Returns the failures.
fn hibernation_checks() -> usize {
    let html = r#"<!doctype html><html><body><script>
    var log = [];
    const at = name => () => log.push(name + " @" + performance.now());
    document.addEventListener("visibilitychange", e => {
      log.push(`vis ${document.visibilityState} ${document.hidden} ${e.bubbles} ${e.cancelable} @${performance.now()}`);
      setTimeout(at("t0"), 0);
      requestAnimationFrame(at("raf after"));
    });
    window.addEventListener("visibilitychange", () => log.push("vis at window"));
    setInterval(at("iv"), 300);
    setTimeout(() => { at("B")(); setTimeout(at("C"), 0); }, 1150);
    setTimeout(at("A"), 50);
    setTimeout(() => requestAnimationFrame(at("raf before")), 90);
    setTimeout(() => log.push(`${document.visibilityState} ${document.hidden} @${performance.now()}`), 100);
    </script></body></html>"#;
    let mut page = Page::new(Dom::parse(html));
    page.run_scripts("hibernation");
    page.run_timers_stepwise(100, 50_000);
    page.hibernate();
    page.run_timers_stepwise(5_000, 50_000);
    page.eval("console.log(log.join(' ; '))", "check");
    let want = "A @50 ; visible false @100 ; vis hidden true true false @100 ; vis at window ; iv @1100 ; t0 @1100 ; \
                iv @2100 ; B @2100 ; iv @3100 ; C @3100 ; iv @4100";
    let ok = page.errors() == 0 && page.console() == [want];
    println!("{} hibernation: a background tab", if ok { "ok  " } else { "FAIL" });
    if !ok {
        println!("  want: {want:?}\n  got:  {:?} (errors: {})", page.console(), page.errors());
    }
    usize::from(!ok)
}

/// The style data dropped (Page::drop_kept_style) with the DOM unchanged: the boxes of the last flush
/// still answer layout reads, also of elements not read before it, without another flush (task 9caa0).
/// Returns the failures.
fn kept_boxes_checks() -> usize {
    let html = r#"<!doctype html><html><body><div id=a style="width:100px;height:40px">x</div>
    <div id=b style="width:70px;height:20px">y</div><p id=n style="display:none">z</p></body></html>"#;
    let mut page = Page::new(Dom::parse(html));
    page.run_scripts("kept boxes");
    page.eval("console.log(document.getElementById('a').getBoundingClientRect().width)", "check");
    let flushes = page.flushes();
    page.drop_kept_style();
    page.eval("const r = id => document.getElementById(id).getBoundingClientRect(); console.log(r('a').width, r('b').width, r('b').top, r('n').width)", "check");
    let want = ["100", "100 70 48 0"];
    let ok = page.errors() == 0 && page.console() == want && page.flushes() == flushes;
    println!("{} layout reads after the style data is dropped", if ok { "ok  " } else { "FAIL" });
    if !ok {
        println!("  want: {want:?}, {flushes} flushes\n  got:  {:?}, {} flushes (errors: {})", page.console(), page.flushes(), page.errors());
    }
    usize::from(!ok)
}

/// A hibernated page woken at 2,000 ms (Page::wake) is a visible tab again: its
/// document gets `visibilitychange` in a task and is visible, what waited runs at once (the timeouts
/// the 1 s gate held, the animation frame callbacks at the first tick), and timeouts run at their own
/// times after that. Returns the failures.
fn wake_checks() -> usize {
    let html = r#"<!doctype html><html><body><script>
    var log = [];
    const at = name => () => log.push(name + " @" + performance.now());
    document.addEventListener("visibilitychange", e => {
      log.push(`vis ${document.visibilityState} ${document.hidden} @${performance.now()}`);
      if (document.hidden) requestAnimationFrame(at("raf"));
    });
    setInterval(at("iv"), 700);
    setTimeout(at("late"), 2300);
    </script></body></html>"#;
    let mut page = Page::new(Dom::parse(html));
    page.run_scripts("wake");
    page.run_timers_stepwise(100, 50_000);
    page.hibernate();
    page.run_timers_stepwise(2_500, 50_000);
    page.eval("console.log('woken @' + performance.now())", "check");
    page.wake();
    page.run_timers_stepwise(3_600, 50_000);
    page.eval("console.log(log.join(' ; '))", "check");
    // (the page has no timer before 100 ms, so its clock is at 0 when it hibernates: batches at 1,000
    // and 2,000; the timeout due at 2,300 waits for the gate at 3,000 and runs at its own time once woken)
    let want = ["woken @2000".to_string(), "vis hidden true @0 ; iv @1000 ; iv @2000 ; vis visible false @2000 ; raf @2016 ; late @2300 ; iv @2700 ; iv @3400".to_string()];
    let ok = page.errors() == 0 && page.console() == want;
    println!("{} hibernation: woken", if ok { "ok  " } else { "FAIL" });
    if !ok {
        println!("  want: {want:?}\n  got:  {:?} (errors: {})", page.console(), page.errors());
    }
    usize::from(!ok)
}

/// The owner's input (prelude queueInputTask, task 0f15b) runs ahead of the tasks queued before it
/// came, here two timeouts already overdue; the other tasks keep their order, so the 0 ms timeouts
/// queued since run after those two, and after the input's second task. Returns the failures.
fn input_order_checks() -> usize {
    let html = r#"<!doctype html><html><body><script>
    var log = [];
    setTimeout(() => log.push("A"), 10);
    setTimeout(() => log.push("B"), 20);
    </script></body></html>"#;
    let mut page = Page::new(Dom::parse(html));
    page.run_scripts("input order");
    page.eval(
        "__mrt.setClock(100);
        const ahead = __mrt.taskOrder();
        __mrt.queueInputTask(() => {
          log.push('input 1');
          setTimeout(() => log.push('t0'), 0);
          __mrt.queueInputTask(() => log.push('input 2'), ahead);
        }, ahead);
        setTimeout(() => log.push('after'), 0);",
        "check",
    );
    page.run_timers_stepwise(1_000, 50_000);
    page.eval("console.log(log.join(' ; '))", "check");
    let want = "input 1 ; input 2 ; A ; B ; after ; t0";
    let ok = page.errors() == 0 && page.console() == [want];
    println!("{} input ahead of the tasks queued before it", if ok { "ok  " } else { "FAIL" });
    if !ok {
        println!("  want: {want:?}\n  got:  {:?} (errors: {})", page.console(), page.errors());
    }
    usize::from(!ok)
}

fn main() {
    let failures = std::thread::Builder::new()
        .stack_size(2 << 30)
        .spawn(|| {
            let mut failures = global_checks() + hibernation_checks() + wake_checks() + input_order_checks() + kept_boxes_checks();
            for (name, body, want) in CASES {
                let (got, errors) = run_case(body);
                let ok = errors == 0 && got.iter().map(String::as_str).eq(want.iter().copied());
                println!("{} {name}", if ok { "ok  " } else { "FAIL" });
                if !ok {
                    failures += 1;
                    println!("  want: {want:?}\n  got:  {got:?} (errors: {errors})");
                }
            }
            failures
        })
        .unwrap()
        .join()
        .unwrap();
    mrt_js::shutdown();
    println!("{} cases, {failures} failed", CASES.len());
    if failures > 0 {
        std::process::exit(1);
    }
}
