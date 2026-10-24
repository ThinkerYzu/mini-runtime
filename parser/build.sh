#!/bin/sh
# Compile the parser core objects. usage: build.sh [file.cpp ...]  (default: all)
set -e
cd "$(dirname "$0")"
CXX=${CXX:-$HOME/.mozbuild/clang/bin/clang++}
JS=${JS:?Set JS to your Firefox SpiderMonkey object directory}
FLAGS="-std=c++20 -O2 -fno-exceptions -fno-rtti -Wno-everything -Ishim -Ivendor -I$JS/dist/include -include $JS/js/src/js-confdefs.h"
OBJ=${OBJ:-obj}
mkdir -p $OBJ
FILES=${@:-"shim/atoms.cpp shim/nsHtml5String.cpp vendor/nsHtml5Tokenizer.cpp vendor/nsHtml5TreeBuilder.cpp vendor/nsHtml5ElementName.cpp vendor/nsHtml5AttributeName.cpp vendor/nsHtml5HtmlAttributes.cpp vendor/nsHtml5StackNode.cpp vendor/nsHtml5StateSnapshot.cpp vendor/nsHtml5NamedCharacters.cpp vendor/nsHtml5NamedCharactersAccel.cpp vendor/nsHtml5Portability.cpp vendor/nsHtml5UTF16Buffer.cpp vendor/nsHtml5TokenizerALU.cpp vendor/nsHtml5TokenizerSIMDStubs.cpp shim/mrt_parser.cpp"}
for f in $FILES; do
  echo "== $f"
  $CXX $FLAGS -c $f -o $OBJ/$(basename $f .cpp).o 2>&1 | grep -E "error|fatal" | head -${ERRS:-12} || true
done
