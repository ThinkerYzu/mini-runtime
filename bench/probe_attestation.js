// YouTube's attestation (BotGuard) on both sides (task 135a, GAPS B38): when the page's prefetched
// challenge reaches window.ytAtN, kevlar's BotGuard wrapper (window.ntpevasrs) and its challenge
// fetcher, what _.bv.prototype.snapshot hands back (the token and the a3M array, which Firefox's VM
// fills with one function), YouTube's own error log ("PMD:Undefined" when a3M is empty), ~1 s timers,
// and the att/get request with its stack. Run as MRT_EVAL_BEFORE (lines in the console dump as PLOG)
// and as REPLAY_INJECT with firefox_probe.py (read with: return Array.from(window.wrappedJSObject.__plog))
(function () {
  if (!/youtube\.com$/.test(location.hostname)) return;
  var log = window.__plog = window.__plog || [];
  var t0 = performance.now();
  function p(m) { m = Math.round(performance.now() - t0) + 'ms ' + m; log.push(m); if (window.__mrt) console.log('PLOG ' + m); }
  document.addEventListener('readystatechange', function () { p('readyState ' + document.readyState + ' scripts=' + document.scripts.length); });
  window.addEventListener('DOMContentLoaded', function () { p('DCL(window, capture-first) scripts=' + document.scripts.length); }, true);
  var n;
  Object.defineProperty(window, 'ytAtN', { configurable: true,
    get: function () { p('get ytAtN ' + (n ? 'set' : 'unset') + ' rs=' + document.readyState); return n; },
    set: function (f) { p('set ytAtN'); n = f && function (v) { p('ytAtN called with ' + (v ? 'R=' + (v.R ? v.R.length : v.R) + ' T=' + !!v.T : v) + ' rs=' + document.readyState); return f.apply(this, arguments); }; } });


  var hooked = false;
  var iv = setInterval(function () {
    var L = window.yt && yt.logging && yt.logging.errors;
    if (L && L.log && !hooked) { hooked = true; var o = L.log; L.log = function (e, lvl) { p('yt error log ' + lvl + ': ' + (e && (e.message || e)) + ' | ' + String(e && e.stack).split('\n').slice(0, 6).join(' / ')); return o.apply(this, arguments); }; p('hooked yt.logging.errors.log'); }
  }, 1);
  setTimeout(function () { clearInterval(iv); }, 3000);
  var oST = window.setTimeout;
  window.setTimeout = function (f, d) { if (performance.now() - t0 < 3000 && d >= 600 && d <= 1400) p('setTimeout ' + d + ' ' + new Error().stack.split('\n').slice(1, 5).map(function(l){return l.replace(/@https:[^ ]*\//,'@')}).join(' / ')); return oST.apply(this, arguments); };
  window.addEventListener('error', function (e) { p('window error ' + e.message); });
  window.addEventListener('unhandledrejection', function (e) { p('unhandled ' + (e.reason && e.reason.message || e.reason)); });

  var bvHooked = false;
  var iv2 = setInterval(function () {
    var K = window.default_kevlar_base;
    if (K && K.bv && K.bv.prototype.snapshot && !bvHooked) { bvHooked = true; var os = K.bv.prototype.snapshot;
      K.bv.prototype.snapshot = function (a) { var t = performance.now(); p('snapshot called keys=' + Object.keys(a || {}).join(','));
        var r = os.apply(this, arguments);
        r.then(function (E) { p('snapshot done in ' + Math.round(performance.now() - t) + 'ms: token len=' + (E && E.length) + ' a3M=' + (a && a.a3M ? a.a3M.length + ':' + a.a3M.map(function (x) { return typeof x; }).join(',') : 'none') + ' b8=' + JSON.stringify(a && a.b8).slice(0, 200)); }, function (e) { p('snapshot rejected ' + e); });
        return r; };
      p('hooked bv.snapshot'); }
  }, 1);
  setTimeout(function () { clearInterval(iv2); }, 5000);
  var nt;
  Object.defineProperty(window, 'ntpevasrs', { configurable: true,
    get: function () { return nt; },
    set: function (o) { p('set ntpevasrs keys=' + (o ? Object.keys(o).join(',') : o));
      if (o && o.bicf) { var ob = o.bicf; o.bicf = function (f) { p('bicf bound'); return ob.call(this, function () { p('VM calls challenge fetcher args=' + JSON.stringify([].slice.call(arguments)).slice(0, 200) + ' stack=' + new Error().stack.split('\n').slice(1, 12).map(function(l){return l.replace(/@https:[^ ]*\//,'@')}).join(' | ')); return f.apply(this, arguments); }); }; }
      if (o && o.blc) { var ol = o.blc; o.blc = function () { var r = ol.apply(this, arguments); p('blc -> ' + JSON.stringify(r).slice(0, 150)); return r; }; }
      if (o && o.bcr) { var oc = o.bcr; o.bcr = function (f) { return oc.call(this, function (v) { p('challenge fetched callback ' + JSON.stringify(v).slice(0, 150)); return f.apply(this, arguments); }); }; }
      nt = o; } });
  var bg;
  Object.defineProperty(window, 'bgens', { configurable: true, get: function () { return bg; }, set: function (v) { p('bgens=' + v); bg = v; } });
  setTimeout(function(){ p('at 5s: ntpevasrs=' + !!nt + ' ytAtRC=' + typeof window.ytAtRC + ' attmp=' + typeof window.attmp); }, 5000);

  var oFetch = window.fetch;
  if (oFetch) window.fetch = function (u) { var s = String(u && u.url || u); if (/att\/get/.test(s)) p('fetch att/get ' + new Error().stack.split('\n').slice(1, 40).map(function(l){return l.replace(/@.*m=kevlar[^:]*/,'@K').replace(/@https:[^ ]*\//,'@')}).join(' | ') + ' DATE=' + Date.now()); return oFetch.apply(this, arguments); };
  var oOpen = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (m, u) { if (/att\/get/.test(String(u))) p('xhr att/get ' + new Error().stack.split('\n').slice(1, 40).map(function(l){return l.replace(/@.*m=kevlar[^:]*/,'@K').replace(/@https:[^ ]*\//,'@')}).join(' | ') + ' DATE=' + Date.now()); return oOpen.apply(this, arguments); };
})();
