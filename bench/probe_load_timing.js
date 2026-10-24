// Firefox's own load timings of the page (ms from navigation start), for firefox_probe.py: first response
// byte, DOMContentLoaded, load, first contentful paint, largest contentful paint (null under this setup).
// Session 76's reload times, set against a hibernated mini runtime's wake.
const w = window.wrappedJSObject;
const n = w.performance.getEntriesByType('navigation')[0];
const p = w.performance.getEntriesByType('paint').find(e => e.name == 'first-contentful-paint');
const l = w.performance.getEntriesByType('largest-contentful-paint');
return { ttfb: n && Math.round(n.responseStart), dcl: n && Math.round(n.domContentLoadedEventEnd), load: n && Math.round(n.loadEventEnd), fcp: p && Math.round(p.startTime), lcp: l.length ? Math.round(l[l.length - 1].startTime) : null };
