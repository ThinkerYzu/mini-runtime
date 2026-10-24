// What the sites do with the CSSOM of their style sheets (GAPS B15, task 3f23, session 107), for
// REPLAY_INJECT with firefox_probe.py; read the counts back with probe_approximated_apis_read.js
// (it returns window.__plog). Counted per member and per caller (the first page frame of the call's
// stack): every method call and getter or setter use on StyleSheet, CSSStyleSheet, CSSRuleList,
// CSSRule and its subclasses, StyleSheetList and document.styleSheets. insertRule also counts its
// outcome (ok or the exception's name), the sheet's owner (style, link, constructed), where the
// index puts the rule (end, 0, other) and the rule's kind (style or the at-keyword).
// Same-origin frames add to the top window's counts.
(function () {
  let log;
  try { log = window.top.__plog; } catch (e) {}
  if (!log) {
    log = {};
    Object.defineProperty(window, "__plog", { value: log, configurable: true });
  }
  const callerOf = stack => {
    const lines = String(stack).split("\n").slice(1).filter(l => l && !l.includes("probe_sheet_cssom"));
    const line = lines[0] || "?";
    return line.replace(/\?[^:@]*(?=:\d+:\d+$)/, "").slice(-160);
  };
  const count = (api, caller, key, n = 1) => {
    const k = api + " | " + caller;
    const row = log[k] || (log[k] = {});
    row[key] = (row[key] || 0) + n;
  };
  const ownerKind = sheet => {
    try {
      const o = sheet.ownerNode;
      return o ? o.localName : (sheet.ownerRule ? "import" : "constructed");
    } catch (e) { return "?"; }
  };
  let inside = 0;
  const wrapProto = (iface) => {
    const C = window[iface];
    if (!C) return;
    const proto = C.prototype;
    for (const name of Object.getOwnPropertyNames(proto)) {
      if (name === "constructor") continue;
      const d = Object.getOwnPropertyDescriptor(proto, name);
      const key = iface + "." + name;
      if (typeof d.value === "function") {
        const f = d.value;
        d.value = function () {
          if (inside) return f.apply(this, arguments);
          inside++;
          const caller = callerOf(new Error().stack);
          let extra = null;
          try {
            if (name === "insertRule" && this instanceof CSSStyleSheet) {
              const len = this.cssRules.length, idx = arguments.length > 1 ? Number(arguments[1]) : 0;
              const text = String(arguments[0]).trim();
              extra = [ownerKind(this), idx === len ? "end" : idx === 0 ? "0" : "other", text[0] === "@" ? text.split(/[\s({]/)[0] : "style"];
            }
          } catch (e) {}
          inside--;
          try {
            const r = f.apply(this, arguments);
            count(key, caller, "ok");
            if (extra) for (const x of extra) count(key, caller, x);
            return r;
          } catch (e) {
            count(key, caller, "threw " + (e && e.name));
            if (extra) for (const x of extra) count(key, caller, x);
            throw e;
          }
        };
        Object.defineProperty(proto, name, d);
      } else {
        if (d.get) {
          const g = d.get;
          d.get = function () { if (!inside) count(key, callerOf(new Error().stack), "get"); return g.call(this); };
        }
        if (d.set) {
          const s = d.set;
          d.set = function (v) { if (!inside) count(key, callerOf(new Error().stack), "set"); return s.call(this, v); };
        }
        Object.defineProperty(proto, name, d);
      }
    }
  };
  for (const iface of ["StyleSheet", "CSSStyleSheet", "CSSRuleList", "StyleSheetList", "CSSRule", "CSSStyleRule",
    "CSSGroupingRule", "CSSConditionRule", "CSSMediaRule", "CSSSupportsRule", "CSSImportRule", "CSSFontFaceRule",
    "CSSKeyframesRule", "CSSKeyframeRule", "CSSNamespaceRule", "CSSPageRule", "CSSLayerBlockRule",
    "CSSLayerStatementRule", "CSSContainerRule", "MediaList"]) wrapProto(iface);
  const ss = Object.getOwnPropertyDescriptor(Document.prototype, "styleSheets");
  if (ss) Object.defineProperty(Document.prototype, "styleSheets", Object.assign({}, ss, {
    get() { if (!inside) count("Document.styleSheets", callerOf(new Error().stack), "get"); return ss.get.call(this); },
  }));
  for (const [P, n] of [[HTMLStyleElement, "sheet"], [HTMLLinkElement, "sheet"]]) {
    const d = Object.getOwnPropertyDescriptor(P.prototype, n) || Object.getOwnPropertyDescriptor(Object.getPrototypeOf(P.prototype), n);
    if (!d) continue;
    Object.defineProperty(P.prototype, n, Object.assign({}, d, {
      get() { if (!inside) count(P.name + ".sheet", callerOf(new Error().stack), "get"); return d.get.call(this); },
    }));
  }
})();
