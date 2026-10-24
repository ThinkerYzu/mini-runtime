# Measurement tools

Recorded-data checks are disabled unless `MRT_RECORDED_TESTS=1`. Existing result files are not used merely because they happen to be present. Record the data yourself before opting in. Do not commit live-site captures, profiles, request logs or private results.

## Record your own inputs

Build the required binaries as described in [build setup](build.md). Static-page comparisons need source HTML under `pages/`: the synthetic pages are included; generate other snapshots with `python3 pages/snapshot.py URL pages/NAME.html` using pages you may capture. Both Firefox and the runtime must read the same snapshots.

From a Firefox source tree, record the baseline you need (replace executable and checkout paths):

```sh
HEADLESS=1 FIREFOX_BIN=/path/to/firefox ./mach python /path/to/mini-runtime/bench/firefox_static.py
HEIGHT_ONLY=1 HEADLESS=1 FIREFOX_BIN=/path/to/firefox ./mach python /path/to/mini-runtime/bench/firefox_static.py
FIREFOX_BIN=/path/to/firefox xvfb-run -a ./mach python /path/to/mini-runtime/bench/firefox_js.py
FIREFOX_BIN=/path/to/firefox xvfb-run -a ./mach python /path/to/mini-runtime/bench/firefox_hibernate.py
FIREFOX_BIN=/path/to/firefox xvfb-run -a ./mach python /path/to/mini-runtime/bench/firefox_boxes.py
```

These write `bench/firefox_static.json`, `firefox_height.json`, `firefox_js.json`, `firefox_hibernate.json` and `firefox_boxes/*.json`, respectively. Some memory/hibernation cases require a Firefox build exposing the relevant measurement hooks. Choose matching builds and settings for each comparison.

Opt-in applies to `compare_memory.py`, `compare_height.py`, `compare_js.py`, `compare_hibernate.py`, `compare_boxes.py`, `compare_realsites.py`, `compare_frames.py`, `compare_workers.py`, `compare_console.py`, `textrun_profile.py` and `oracle/cmp.py`. The recording tools themselves remain explicit commands; this setting never starts recording automatically.

For live-site comparisons, record matching Firefox and runtime runs yourself using `bench/firefox_realsites.py` and `bench/mini_realsites.py`. Console comparisons use their `realsites-console/` outputs; frame comparisons need Firefox and runtime replay reports; worker comparisons additionally need saved Firefox memory reports (`KEEP_REPORTS=1 NO_USE=1` and the matching `REPORTS_DIR`). Oracle comparisons use `OPAGES=/path/to/mini-runtime/bench/oracle/opages OOUT=/path/to/mini-runtime/bench/oracle/oout ONLY=PAGE FIREFOX_BIN=... ./mach python /path/to/mini-runtime/bench/oracle/oracle.py` from the Firefox tree to record the matching `oout/` baseline. Consult each script's usage header for its recording options. These are optional experiments, not prerequisites for the local viewer demo or regression tests.

## Run the comparisons

From the repository root, explicitly enable recorded inputs for each comparison:

```
cd mrt-layout && cargo build --release --offline --bins
for p in synthetic hackernews commons mdn-css wikipedia wiki-long; do ./target/release/layoutcount ../pages/$p.html; done
cd .. && MRT_RECORDED_TESTS=1 python3 bench/compare_memory.py --bin mrt-layout/target/release    # memory by layer against Firefox
MRT_RECORDED_TESTS=1 python3 bench/compare_height.py mrt-layout/target/release/layoutcount       # page heights against Firefox
cd mrt-js && cargo build --release --offline --bins && cd .. && MRT_RECORDED_TESTS=1 python3 bench/compare_js.py   # JS and DOM of scripted pages against Firefox
MRT_RECORDED_TESTS=1 python3 bench/compare_hibernate.py                                          # stage 4: hibernate, wake, save against Gecko's hide/show

# Firefox side (from a Firefox source tree, release build of Firefox):
FIREFOX_BIN=/path/to/firefox ./mach python /path/to/mini-runtime/bench/firefox_static.py
FIREFOX_BIN=/path/to/firefox ./mach python /path/to/mini-runtime/bench/firefox_js.py          # JS on
FIREFOX_BIN=/path/to/firefox ./mach python /path/to/mini-runtime/bench/firefox_hibernate.py   # Gecko hide and show
```

`layoutcount` and `stylecount` count live bytes with a wrapper around the system allocator. The viewport is 1489 by 844 to match the Firefox runs.

## HTML parser

Firefox's tokenizer and tree builder, copied from the Firefox tree by `vendor.sh` into `vendor/` (about 19,500 lines, generated from Java by Firefox's own translator), plus `shim/` (about 1,200 lines written here, and a copy of the 2,700-line static atom list from a Firefox build). The shim replaces the XPCOM types (atoms, strings, handles) and Firefox's tree-operation glue. The tree builder reports what it builds through the C interface in `shim/mrt_parser.h`; the DOM implements it.

```
cd parser
./vendor.sh /path/to/firefox/source       # refresh vendor/ from a Firefox tree
./build.sh                               # compile everything into obj/ (needs the js build in obj-js for the mfbt headers)
clang++ obj/*.o -o obj/test_sink && ./obj/test_sink                 # parse a built-in document and print the tree
./obj/test_sink ../pages/wikipedia.html                             # parse a file: element count and time
```

The stage 1 binary (`mrt/`, Blitz) was removed after stage 2 replaced it; its measurements are in `bench/mrt_stage1.jsonl`, and its source remains in git history (`git show 6aa576d^:mrt`).

Build prerequisites are in [build setup](build.md). These tools compare memory and layout against Firefox; they are separate from attaching a standalone HTTP viewer. Snapshot files under `pages/` may need to be regenerated with `pages/snapshot.py`. Commands targeting Firefox run from that source tree with Marionette available.

Hibernation drops frame and style data while retaining the DOM and JavaScript state; wake rebuilds layout. `jscount page.html --hibernate` and `--cycles N` exercise this path and DOM serialization. These experiments do not establish a memory saving for every site or device. Measure the runtime, API processes and viewer browser together when assessing the interactive system.
