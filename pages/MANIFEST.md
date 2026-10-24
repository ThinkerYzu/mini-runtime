# Test pages

The real pages are not in git (they are third-party content, and 5 MB). Each was made on 2026-10-01 with
`snapshot.py <url> <name>.html`: scripts removed, linked stylesheets inlined, `src` and `srcset` removed, no image or
font downloads. A page changes over time, so a new snapshot gives different numbers; compare against this list of sizes
and checksums, and keep the Firefox and mini-runtime numbers in `bench/` as the record of what was measured.

| File | Source | Bytes | SHA-256 (first 16 hex digits) |
|---|---|---|---|
| `wikipedia.html` | https://en.wikipedia.org/wiki/Firefox | 1,263,025 | `1abc45605724dfa0…` |
| `wiki-long.html` | https://en.wikipedia.org/wiki/United_States | 2,478,975 | `2b8a65e15f90d293…` |
| `mdn-css.html` | https://developer.mozilla.org/en-US/docs/Web/CSS | 379,912 | `cad496a92fe292a1…` |
| `commons.html` | https://commons.wikimedia.org/wiki/Main_Page | 237,674 | `f95a2e67d6a4161e…` |
| `hackernews.html` | https://news.ycombinator.com/ | 40,919 | `8a03626c5541bcbd…` |
| `synthetic.html` | (generated: 4000 flex rows of 5 spans; kept in git) | 1,198,702 | `3be2554071a9d3f2…` |
| `synthetic-js.html` | (generated: the same tree as `synthetic.html`, built by script from 100,000 JS records; kept in git) | 1,131 | `57dbdb4c1f6f3218…` |
| `todo-js.html` | (hand-written, stage 3: a todo app, 800 items with click listeners, simulated clicks, deletions, timer batches; kept in git) | 2,362 | `bf070c0b13074946…` |
| `table-sort-js.html` | (hand-written, stage 3: a 2,000-row table from data, sorted twice by moving rows from header click listeners; kept in git) | 1,919 | `19830a344f09f27c…` |
| `layout-js.html` | (hand-written, stage 3: an equal-height card grid, 360 cards; reads offsetHeight, writes heights, reads again; kept in git) | 1,634 | `5f812d8a3820dcf9…` |
