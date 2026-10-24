#!/bin/sh
# Copy the parts of Firefox's HarfBuzz (gfx/harfbuzz/src) that shaping needs into vendor/harfbuzz/:
# every header, the OT/ headers, and the .cc files listed in sources.txt. No edits are made to the files;
# the build settings Firefox puts in gfx/harfbuzz/src/moz.build are repeated in build.rs.
# usage: vendor.sh [path/to/firefox/tree]
set -e
SRC=${1:-${FIREFOX_SRC:?Pass a Firefox source directory or set FIREFOX_SRC}}/gfx/harfbuzz/src
cd "$(dirname "$0")"
rm -rf vendor/harfbuzz
mkdir -p vendor/harfbuzz/OT
cp "$SRC"/*.h "$SRC"/*.hh vendor/harfbuzz/
(cd "$SRC" && find OT -name '*.hh' -o -name '*.h') | while read f; do
  mkdir -p "vendor/harfbuzz/$(dirname $f)"; cp "$SRC/$f" "vendor/harfbuzz/$f"
done
for f in $(cat sources.txt); do cp "$SRC/$f" vendor/harfbuzz/; done
cp "$SRC/../COPYING" vendor/harfbuzz/COPYING
(cd "$SRC" && git log -1 --format=%H -- . 2>/dev/null || true) > vendor/harfbuzz/FIREFOX_REV
du -sh vendor/harfbuzz
