#!/usr/bin/env python3
"""Make a static, offline copy of a web page for the DOM + style + layout comparison.

usage: snapshot.py <url> <out.html>

The copy has no scripts, its linked stylesheets inlined, and no image or font downloads
(src / srcset removed, @font-face rules dropped). Both Firefox and the mini runtime then
get the same bytes and need nothing from the network.
"""
import re, sys, urllib.parse, urllib.request

def get(url):
    req = urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0"})
    return urllib.request.urlopen(req, timeout=30).read().decode("utf-8", "replace")

def inline_css(html, base):
    def repl(m):
        tag = m.group(0)
        if "stylesheet" not in tag.lower(): return ""
        href = re.search(r'href=["\']([^"\']+)["\']', tag)
        if not href: return ""
        try:
            css = get(urllib.parse.urljoin(base, href.group(1)))
        except Exception as e:
            sys.stderr.write(f"skip css {href.group(1)}: {e}\n"); return ""
        css = re.sub(r"@font-face\s*{[^}]*}", "", css)
        css = re.sub(r"@import[^;]+;", "", css)
        css = re.sub(r"url\([^)]*\)", "none", css)  # no image or font downloads
        return "<style>" + css + "</style>"
    return re.sub(r"<link\b[^>]*>", repl, html, flags=re.I)

url, out = sys.argv[1], sys.argv[2]
html = get(url)
html = re.sub(r"<script\b.*?</script\s*>", "", html, flags=re.I | re.S)
html = re.sub(r"<noscript\b.*?</noscript\s*>", "", html, flags=re.I | re.S)
html = inline_css(html, url)
html = re.sub(r'\s(src|srcset|data-src)=("[^"]*"|\'[^\']*\')', "", html, flags=re.I)
html = re.sub(r"url\(([^)]*)\)", "none", html)
open(out, "w").write(html)
print(out, len(html), "bytes")
