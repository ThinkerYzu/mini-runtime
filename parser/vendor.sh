#!/bin/sh
# Copy the generated core of Firefox's HTML parser (the tokenizer, tree builder and their tables) from
# the Firefox tree into vendor/. The Firefox-specific glue (supplements, tree operations, stream parser)
# is replaced by our own files in shim/.
set -e
SRC=${1:-${FIREFOX_SRC:?Pass a Firefox source directory or set FIREFOX_SRC}}/parser/html
cd "$(dirname "$0")"
for f in nsHtml5Tokenizer nsHtml5TreeBuilder nsHtml5AttributeName nsHtml5ElementName nsHtml5HtmlAttributes \
         nsHtml5StackNode nsHtml5StateSnapshot nsHtml5NamedCharacters nsHtml5NamedCharactersAccel \
         nsHtml5Portability nsHtml5UTF16Buffer nsHtml5AttributeEntry; do
  for ext in cpp h; do [ -f "$SRC/$f.$ext" ] && cp "$SRC/$f.$ext" vendor/; done
done
for f in jArray.h nsHtml5ArrayCopy.h nsHtml5Macros.h nsHtml5DocumentMode.h nsAHtml5TreeBuilderState.h \
         nsHtml5NamedCharactersInclude.inc nsHtml5TokenizerCppSupplement.h nsHtml5TokenizerHSupplement.h \
         nsHtml5PortabilityHSupplement.h \
         nsHtml5ByteReadable.h nsHtml5ContentCreatorFunction.h nsHtml5TokenizerLoopPoliciesALU.h \
         nsHtml5TokenizerALU.cpp nsHtml5TokenizerSIMDStubs.cpp; do
  [ -f "$SRC/$f" ] && cp "$SRC/$f" vendor/
done
# The element-name table passes Firefox's DOM element factory functions; this runtime has no such factories.
sed -i -E 's/\bNS_NewCustomElement\b/MRT_CUSTOM_ELEMENT/g; s/\bNS_New(HTML|SVG)[A-Za-z0-9_]*Element\b/nullptr/g' vendor/nsHtml5ElementName.cpp vendor/nsHtml5ElementName.h
sed -i -E 's/\bNS_New(HTML|SVG)[A-Za-z0-9_]*Element\b/nullptr/g' vendor/nsHtml5TreeBuilder.cpp
# Firefox-only headers that the generated headers list but the core does not need
sed -i -E '/#include "(mozilla\/ImportScanner|nsContentUtils|nsHtml5OplessBuilder|nsHtml5Parser|nsHtml5PlainTextUtils|nsHtml5StreamParser|nsHtml5TreeOperation|nsHtml5TreeOpExecutor|nsHtml5ViewSourceUtils)\.h"/d' vendor/*.h vendor/*.cpp
rm -f vendor/nsHtml5ContentCreatorFunction.h vendor/nsHtml5TokenizerHSupplement.h vendor/nsHtml5TokenizerCppSupplement.h
ls vendor | wc -l
