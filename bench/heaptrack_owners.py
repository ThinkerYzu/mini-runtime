"""The live heap of `realsite` at exit by owner, from heaptrack (GAPS A16).

Run the harness with MRT_EXIT_AFTER_REPORT=1 under heaptrack (TESTING-GUIDE, "Naming the malloc no
report claims"): what is still allocated at exit is what the page held when it was measured. Each
stack goes to the owner of its innermost frame that is not the allocator or a container; the owners
match the report's figures (SpiderMonkey's, the DOM's, kept_style_mb), which is the check.

usage: heaptrack_print -f ht-cnn.zst --print-leaks 1 --print-peaks 0 --print-allocators 0 \\
         --print-temporary 0 --print-histogram 0 -n 1000000 -s 1000 > leaks-cnn.txt
       python3 bench/heaptrack_owners.py leaks-cnn.txt [--other N]
"""
import re, sys, collections
UNITS = {'B': 1, 'K': 1e3, 'M': 1e6, 'G': 1e9}
def size(t):
    m = re.match(r'([\d.]+)([BKMG]?)', t)
    return float(m.group(1)) * UNITS.get(m.group(2) or 'B', 1)
GENERIC = re.compile(r'^(std::|alloc::|core::|hashbrown::|_\$LT\$alloc|_\$LT\$core|_\$LT\$std|_\$LT\$hashbrown|<alloc|<core|<std|servo_arc::|smallvec::|_\$LT\$smallvec|_\$LT\$servo_arc|malloc|calloc|realloc|operator new|js_malloc|js_calloc|js_realloc|js_pod|_\$LT\$mrt_js\.\.procmem|mrt_js::procmem|__rust|<unresolved|_\$LT\$T\$u20\$as|thin_vec|_\$LT\$thin_vec|indexmap::|_\$LT\$indexmap|_\$LT\$\$u5b|\?\?)')
def stacks(path):
    cur = None
    for line in open(path, errors='replace'):
        m = re.match(r'^\s*([\d.]+[BKMG]?) leaked over (\d+) calls from(:?)', line)
        if m:
            if cur and cur[1]: yield cur
            # a line ending in ':' is one stack of the allocator above it
            cur = [size(m.group(1)), [], 0] if m.group(3) else None
            continue
        if cur is not None and line.startswith('    ') and not line.strip().startswith(('at ', 'in ')):
            cur[1].append(line.strip())
    if cur and cur[1]: yield cur
# the owners, tried in order on each frame, innermost frame first
RULES = [
    ('JS: script sources', r'SourceCompression|ScriptSource|SharedImmutableString|UncompressedSource'),
    ('JS: bytecode', r'ImmutableScriptData|SharedImmutableScriptData|BytecodeEmitter|intoScriptStencil|ScriptStencil'),
    ('HTML parser (left after the tree is built)', r'nsHtml5'),
    ('DOM arena', r'mrt_dom::'),
    ('linked sheet text cache', r'mrt_dom_link_sheet|link_sheet'),
    ('style: parsed sheets', r'owner_sheet|page_sheets|rule_parser|declaration_block|PropertyDeclaration|custom_properties::VariableValue::parse|SelectorBuilder|SelectorList|CssUrl|media_list|cssparser|Stylesheet::'),
    ('style: rule tables and invalidation maps', r'selector_map|invalidation_map|Stylist|CascadeData|sync_tables|rule_tree'),
    ('style: computed styles', r'ComputedValues|StyleStructRef|mutate_|style::matching|ElementData|style::traversal'),
    ('layout: frames', r'mrt_layout::'),
    ('text and fonts', r'mrt_text|hb_|gfxFont|gfxShaped|Fc[A-Z]|FT_|freetype|fontconfig'),
    ('ICU', r'icu_|_uhash|uhash_|ures_|ucol|ubrk|initFromResourceBundle|U_ICU|icu::'),
    ('JS: regexp', r'v8::internal|irregexp|RegExp'),
    ('JS: atoms and strings', r'Atom|JSRope|JSLinearString|JSString|NewString|moveString'),
    ('JS: other engine', r'^js::|^JS::|JSContext|mozilla::detail::HashTable|^Wrap\(|JSONFull|JSObject'),
    ('glue (mrt_js.cpp) and bridge', r'^mrt_js|MrtJs|mrt_js::|Mrt[A-Z]|realsite::'),
    ('string_cache atoms (Stylo)', r'string_cache'),
    ('style: other', r'^style::|^selectors::|mrt_style::'),
]
RULES = [(n, re.compile(r)) for n, r in RULES]
def owner(frames):
    real = [re.sub(r'::h[0-9a-f]{16}$', '', f) for f in frames if not GENERIC.match(f)]
    for f in real:
        for name, rx in RULES:
            if rx.search(f):
                return name, real
    return 'other', real
if __name__ == '__main__':
    agg = collections.Counter(); other = collections.Counter(); total = 0
    for s, frames, _ in stacks(sys.argv[1]):
        o, real = owner(frames); agg[o] += s; total += s
        if o == 'other': other[' <- '.join(real[:3])[:200]] += s
    print(f'total {total/1e6:.2f} MB')
    for k, v in agg.most_common(): print(f'{v/1e6:8.2f}  {k}')
    if '--other' in sys.argv:
        for k, v in other.most_common(int(sys.argv[sys.argv.index('--other') + 1])): print(f'   {v/1e6:7.2f}  {k}')
