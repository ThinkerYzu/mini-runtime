# Firefox oracles for form controls, fallback fonts and grid

Scripts written during session 18 to check the mini layout against a real headless Firefox (`obj-hib`).
They are kept as written; some paths are hard-coded (look at the top of each file before running).

| File | What |
|---|---|
| `opages/*.html` | small pages: form controls (`forms.html`, `f2.html`, `f3.html`) and fallback-font cases (`fb*.html`; utf-8 meta tag included) |
| `oracle.py` | loads the pages in Firefox through Marionette and writes the element boxes to `oout/<page>.json` |
| `cmp.py <page>` | runs `layoutcount --boxes` (binary path at the top) on the same page and compares with `oout/<page>.json` |
| `grid_diff_firefox.py`, `grid_diff_firefox_text.py` | random grid pages (without and with text): mini layout against Firefox; 0 mismatches over about 930 pages |
