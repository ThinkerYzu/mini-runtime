// firefox_probe.py probe for tools/gen_css_properties.py: the attribute names of Firefox's
// CSSStyleProperties interface, in its order (prefs as the release build sets them).
const w = window.wrappedJSObject;
return { props: Object.getOwnPropertyNames(w.CSSStyleProperties.prototype).filter(n => n !== "constructor") };
