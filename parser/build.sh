#!/bin/sh
# Compile the vendored parser. mrt-dom/build.rs selects and validates the SDK.
set -eu
cd "$(dirname "$0")"
CXX=${CXX:-c++}
if [ -n "${MRT_PARSER_INCLUDE:-}" ]; then
  INCLUDE=$MRT_PARSER_INCLUDE
  CONFDEFS=${MRT_PARSER_CONFDEFS:?}
elif [ -n "${MRT_SDK_DIR:-}" ]; then
  python3 ../tools/sdk/sdk.py validate "$MRT_SDK_DIR"
  INCLUDE=$MRT_SDK_DIR/include
  CONFDEFS=$MRT_SDK_DIR/include/js-confdefs.h
elif [ "${MRT_ENGINE_SOURCE:-}" = 1 ]; then
  JS=${MRT_JS_OBJDIR:-${JS:?set MRT_JS_OBJDIR or JS}}
  INCLUDE=$JS/dist/include
  CONFDEFS=$JS/js/src/js-confdefs.h
else
  echo 'Set MRT_SDK_DIR, or explicitly select MRT_ENGINE_SOURCE=1 and MRT_JS_OBJDIR' >&2
  exit 1
fi
OBJ=${OBJ:-obj}
mkdir -p "$OBJ"
if [ "$#" = 0 ]; then
  set -- shim/atoms.cpp shim/nsHtml5String.cpp vendor/nsHtml5Tokenizer.cpp vendor/nsHtml5TreeBuilder.cpp vendor/nsHtml5ElementName.cpp vendor/nsHtml5AttributeName.cpp vendor/nsHtml5HtmlAttributes.cpp vendor/nsHtml5StackNode.cpp vendor/nsHtml5StateSnapshot.cpp vendor/nsHtml5NamedCharacters.cpp vendor/nsHtml5NamedCharactersAccel.cpp vendor/nsHtml5Portability.cpp vendor/nsHtml5UTF16Buffer.cpp vendor/nsHtml5TokenizerALU.cpp vendor/nsHtml5TokenizerSIMDStubs.cpp shim/mrt_parser.cpp
fi
for f do
  "$CXX" -std=c++20 -O2 -fPIC -fno-exceptions -fno-rtti -w -Ishim -Ivendor \
    -isystem "$INCLUDE" -include "$CONFDEFS" -c "$f" -o "$OBJ/$(basename "$f" .cpp).o"
done
