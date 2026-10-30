# stylo 0.22.0, vendored for the mini runtime

This is the `stylo` crate 0.22.0 from crates.io, the style engine Firefox syncs, with a few changes.
`mrt-style`, `mrt-layout` and `mrt-js` use it through `[patch.crates-io]`.

Why: the runtime builds Stylo in its Servo configuration. In that configuration, a property marked
`engine = "gecko"` gets no parser and no computed value, so the runtime dropped every declaration of
it: in style sheets, in `style` attributes and in `element.style`. 116 of release Firefox's 464
hyphenated property names were refused that way. Instagram reveals its app by removing
`content-visibility: hidden` from the root, so its page stayed blank (tab-hibernation session 75).

The code for those properties is already in this crate; only the generator leaves it out. So the
changes switch Gecko's own code on, they do not reimplement it.

## Changes

| File | Change |
|---|---|
| `properties/longhands.toml`, `properties/shorthands.toml` | `engine = "gecko"` removed from 88 properties: the 67 Gecko-only properties found on the nine replayed sites, the rest of their logical groups, the longhands of their shorthands, and four more shorthands whose longhands were then all there (`scroll-margin-block`, `scroll-margin-inline`, `page-break-before`, `contain-intrinsic-size`). The list is in tab-hibernation's DESIGN, "Gecko-only properties in the Servo build". 33 of Firefox's names are still refused (GAPS B76) |
| `properties/longhands.toml` | `view-transition-name` and `view-transition-class` have `servo_restyle_damage = "repaint"` (tab-hibernation task 8facd). Unannotated, they got the default `rebuild_box` (relayout): Google Images sets `view-transition-name: none` on the root and takes it off around each preview click, so the runtime reframed the whole document twice per click. Gecko's `nsStyleUIReset::CalcDifference` gives these properties `nsChangeHint_RepaintFrame` or `nsChangeHint_NeutralChange` |
| `properties/helpers/animated_properties.mako.rs` | `ContentVisibility`'s `Animate`, `ComputeSquaredDistance` and `ToAnimatedZero` (Gecko's own) no longer gated to the gecko feature |
| `properties/shorthands.rs` | `column_rule`, `text_wrap`, `_webkit_text_stroke`, `font_synthesis` and `text_emphasis` no longer gated to the gecko feature |
| `values/specified/font.rs` | `FontSynthesisStyle` derives `Serialize` and `Deserialize`, as the Servo configuration needs for a computed value |
| `properties/properties.mako.rs` | The Servo size of `ComputedValues` is 240 bytes, not 232: the `XUL` style struct (`-moz-box-*`, which `-webkit-box-*` alias) is now generated |
| `stylesheets/supports_rule.rs` | `font-format()` and `font-tech()` in `@supports` and `CSS.supports` answer as Gecko's `gfxPlatform::IsFontFormatSupported` with release 157's prefs, not `false` (mrt-style also sets `layout.css.font-tech.enabled`, which Gecko's build always has on) |
| `stylesheets/rule_parser.rs` | `@container` rules are parsed in the Servo build too (tab-hibernation session 107: CNN inserts 28 with `insertRule`, which threw SyntaxError). `container-type` stays off (`layout.container-queries.enabled`, GAPS B76), so no element is a query container and no `@container` rule matches, as in Firefox on a page with no containers |
| `matching.rs` | `finish_restyle` does not call `process_animations` in the Servo build (session 77). The runtime has no animation timeline: Stylo got a new animation set at each style build and a time of 0, so a transition it started kept its start value in the element's rules (Google Docs' toolbar, tab-hibernation GAPS B78 and D92). Styles are the after-change styles; what a timeline would add is GAPS B81 |
| `values/specified/ui.rs` | `PointerEvents`'s SVG keywords (`visiblePainted`, `visibleFill`, `visibleStroke`, `visible`, `painted`, `fill`, `stroke`, `all`) no longer gated to the gecko feature (tab-hibernation task 8d43b): the Servo build dropped `pointer-events: all`, which X's new-posts pill sets inside a `pointer-events: none` wrapper, so the bridge tab's copy let clicks through to the post below. Hit testing reads only `!= none`, as Gecko does for HTML. Eight more enums still gate keywords to gecko (GAPS B76) |
| `servo/media_features.rs` | Gecko's `scripting` media feature, always `enabled`: the runtime styles documents whose scripts run, and html.css hides `<noscript>` with `@media (scripting)` (tab-hibernation task 3843: X's `<noscript>` text was laid out above its app). Its atom is added to `vendor/stylo_atoms/static_atoms.txt` (`stylo_atoms` 0.22.0, vendored for that one line and patched in like `stylo`) |

To see every change: `diff -r ~/.cargo/registry/src/index.crates.io-*/stylo-0.22.0 vendor/stylo` (and `stylo_atoms-0.22.0` against `vendor/stylo_atoms`).

## Switching on another Gecko-only property

1. Remove its `engine = "gecko"` line, and those of the other properties in its logical group (the
   generator checks that a group is balanced).
2. Build `mrt-style`. Code gated with `#[cfg(feature = "gecko")]` that the property needs shows up as
   errors; un-gate it when it uses no Gecko bindings. A property whose code calls Gecko bindings
   needs that part translated.
3. If a new style struct appears, update the Servo `size_of_test!(ComputedValues, ...)`.
