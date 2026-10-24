#!/bin/sh
# Copy Firefox's user-agent sheets from a Firefox tree into assets/firefox-ua/.
# usage: vendor-ua.sh [path/to/firefox/tree]   (default: the firefox-hibernate worktree)
# The list and the order they are applied in come from Document::FillStyleSetUserAndUASheets
# (dom/base/Document.cpp) and layout/style/BuiltInStyleSheetList.inc.
set -e
SRC=${1:-${FIREFOX_SRC:?Pass a Firefox source directory or set FIREFOX_SRC}}
DST="$(dirname "$0")/assets/firefox-ua"
mkdir -p "$DST"
cp "$SRC/layout/style/res/ua.css" "$SRC/layout/mathml/mathml.css" "$SRC/layout/svg/svg.css" \
   "$SRC/layout/style/res/html.css" "$SRC/layout/style/res/counterstyles.css" \
   "$SRC/layout/style/res/forms.css" "$SRC/layout/style/res/scrollbars.css" \
   "$SRC/layout/style/res/quirk.css" "$DST/"
# The tree's HEAD, for the record (firefox-hibernate is the prototype commit on top of a8d334e3bde6e; it does not touch the sheets).
git -C "$SRC" rev-parse HEAD > "$DST/REVISION"
