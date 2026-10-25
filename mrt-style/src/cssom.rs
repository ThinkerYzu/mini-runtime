//! `element.style` over a Stylo declaration block, as Gecko's nsDOMCSSDeclaration does through
//! servo/ports/geckolib/glue.rs (Servo_DeclarationBlock_*): the `style` attribute is parsed into a
//! PropertyDeclarationBlock, read or changed there, and serialized back. So a value reads back the
//! way Gecko serializes it (`opacity: .55` reads `0.55`), shorthands expand and serialize, and an
//! invalid value leaves the block unchanged.
//!
//! `quirks_mode` is the document's: 0 no quirks, 1 limited quirks, 2 quirks.
//!
//! The mini runtime keeps the attribute text, not the block, so every call parses it again; the
//! caller writes a changed text back through `setAttribute`.

use style::servo_arc::Arc as ServoArc;
use style::properties::{
    parse_one_declaration_into, parse_style_attribute, Importance, PropertyDeclarationBlock, PropertyId,
    SourcePropertyDeclaration,
};
use style::stylesheets::{CssRuleType, Origin, UrlExtraData};
use style_traits::ParsingMode;

fn url_data() -> UrlExtraData {
    UrlExtraData(ServoArc::new(url::Url::parse("about:blank").unwrap()))
}

fn block(css: &str, quirks_mode: i32) -> PropertyDeclarationBlock {
    parse_style_attribute(css, &url_data(), None, crate::quirks_of(quirks_mode), CssRuleType::Style)
}

fn to_text(block: &PropertyDeclarationBlock) -> String {
    let mut out = String::new();
    block.to_css(&mut out).unwrap();
    out
}

/// Whether `name` is a CSS property (a longhand, a shorthand or a custom property) in this build:
/// what the CSS2Properties object answers for `name in element.style`.
pub fn is_property(name: &str) -> bool {
    crate::ensure_prefs();
    PropertyId::parse_enabled_for_all_content(name).is_ok()
}

/// Servo_DeclarationBlock_GetCssText: the block serialized (`cssText`).
pub fn css_text(css: &str, quirks_mode: i32) -> String {
    crate::ensure_prefs();
    to_text(&block(css, quirks_mode))
}

/// get_property_value: the property's value as Gecko serializes it, "" if absent or not a property.
pub fn property_value(css: &str, name: &str, quirks_mode: i32) -> String {
    crate::ensure_prefs();
    let Ok(id) = PropertyId::parse_enabled_for_all_content(name) else { return String::new() };
    let mut out = String::new();
    block(css, quirks_mode).property_value_to_css(&id, &mut out).unwrap();
    out
}

/// Servo_DeclarationBlock_GetPropertyIsImportant: "important" or "".
pub fn property_priority(css: &str, name: &str, quirks_mode: i32) -> &'static str {
    crate::ensure_prefs();
    let Ok(id) = PropertyId::parse_enabled_for_all_content(name) else { return "" };
    match block(css, quirks_mode).property_priority(&id) {
        Importance::Important => "important",
        Importance::Normal => "",
    }
}

/// Servo_DeclarationBlock_Count and GetNthProperty: the number of longhand declarations, and the
/// name of the `index`-th (`length` and `item()`).
pub fn length(css: &str, quirks_mode: i32) -> usize {
    crate::ensure_prefs();
    block(css, quirks_mode).declarations().len()
}

pub fn item(css: &str, index: usize, quirks_mode: i32) -> String {
    crate::ensure_prefs();
    block(css, quirks_mode).declarations().get(index).map(|d| d.id().name().into_owned()).unwrap_or_default()
}

/// set_property (parse_property_into, then set_property_to_declarations): the new attribute text,
/// or None when nothing changes (an unknown property, a value that does not parse, the same value).
pub fn set_property(css: &str, name: &str, value: &str, important: bool, quirks_mode: i32) -> Option<String> {
    crate::ensure_prefs();
    let id = PropertyId::parse_enabled_for_all_content(name).ok()?;
    if let Some(non_custom) = id.non_custom_id() {
        if !non_custom.allowed_in_rule(CssRuleType::Style.into()) {
            return None;
        }
    }
    let mut source = SourcePropertyDeclaration::default();
    parse_one_declaration_into(&mut source, id, value, Origin::Author, &url_data(), None, ParsingMode::DEFAULT, crate::quirks_of(quirks_mode), CssRuleType::Style)
        .ok()?;
    let importance = if important { Importance::Important } else { Importance::Normal };
    let mut decls = block(css, quirks_mode);
    let mut updates = Default::default();
    if !decls.prepare_for_update(&source, importance, &mut updates) {
        return None;
    }
    decls.update(source.drain(), importance, &mut updates);
    Some(to_text(&decls))
}

/// remove_property: (the value it had, the new attribute text or None when it was not there).
pub fn remove_property(css: &str, name: &str, quirks_mode: i32) -> (String, Option<String>) {
    crate::ensure_prefs();
    let Ok(id) = PropertyId::parse_enabled_for_all_content(name) else { return (String::new(), None) };
    let mut decls = block(css, quirks_mode);
    let mut old = String::new();
    decls.property_value_to_css(&id, &mut old).unwrap();
    let Some(first) = decls.first_declaration_to_remove(&id) else { return (old, None) };
    decls.remove_property(&id, first);
    (old, Some(to_text(&decls)))
}

/// `CSS.supports(property, value)`, as Gecko's CSS::Supports calls Servo_CSSSupports2: whether `value`
/// parses as a value of `property` (in no-quirks mode, as an author style declaration).
pub fn supports_declaration(property: &str, value: &str) -> bool {
    crate::ensure_prefs();
    let Ok(id) = PropertyId::parse_enabled_for_all_content(property) else {
        return false;
    };
    let mut declarations = SourcePropertyDeclaration::default();
    parse_one_declaration_into(&mut declarations, id, value, Origin::Author, &url_data(), None, ParsingMode::DEFAULT, style::context::QuirksMode::NoQuirks, CssRuleType::Style).is_ok()
}

/// `CSS.supports(conditionText)`, as Gecko's CSS::Supports calls Servo_CSSSupports: the text parsed
/// whole as a supports condition, or as a bare declaration (`parse_condition_or_declaration`), then
/// evaluated as an author condition in no-quirks mode. False when it does not parse.
pub fn supports_condition(condition: &str) -> bool {
    crate::ensure_prefs();
    use style::parser::ParserContext;
    use style::stylesheets::supports_rule::parse_condition_or_declaration;
    // (cssparser 0.38 takes the text itself: no ParserInput)
    let mut input = cssparser::Parser::new(condition);
    let cond = match input.parse_entirely(parse_condition_or_declaration) {
        Ok(c) => c,
        Err(..) => return false,
    };
    // NOTE(emilio): The supports API is not associated to any stylesheet,
    // so the fact that there is no namespace map here is fine.
    let url_data = url_data();
    let context = ParserContext::new(
        Origin::Author,
        &url_data,
        Some(CssRuleType::Style),
        ParsingMode::DEFAULT,
        style::context::QuirksMode::NoQuirks,
        /* namespaces = */ Default::default(),
        None,
        None,
        /* attr_taint */ Default::default(),
    );
    cond.eval(&context)
}

// ---- style sheets and their rules (CSSStyleSheet, CSSRuleList, CSSRule) ----

use style::shared_lock::{Locked, SharedRwLock, SharedRwLockReadGuard, ToCssWithGuard};
use style::stylesheets::import_rule::{ImportLayer, ImportRule, ImportSheet, ImportSupportsCondition};
use style::stylesheets::{AllowImportRules, CssRule, CssRuleTypes, CssRules, RulesMutateError, StylesheetContents, StylesheetLoader};
use style_traits::ToCss;

/// The `@import` rules of a sheet the CSSOM reads, as Gecko's Loader would give them, but never
/// loaded: the mini runtime's style system does not take `@import` (DESIGN, "Linked style sheets").
struct NoLoad;

impl StylesheetLoader for NoLoad {
    fn request_stylesheet(
        &self,
        url: style::values::CssUrl,
        location: cssparser::SourceLocation,
        lock: &SharedRwLock,
        _media: ServoArc<Locked<style::media_queries::MediaList>>,
        supports: Option<ImportSupportsCondition>,
        layer: ImportLayer,
    ) -> ServoArc<Locked<ImportRule>> {
        ServoArc::new(lock.wrap(ImportRule { url, stylesheet: ImportSheet::new_pending(), supports, layer, source_location: location }))
    }
}

/// A sheet's rules as Gecko's CSSOM holds them (a StyleSheet's inner, which Servo_CssRules_* read and
/// change): parsed once, then changed in place by `apply`, so that a script adding rules one by one
/// (X: 1,208 calls into one `<style>`) parses each rule once. A cache the runtime keeps beside the
/// page state, which is the sheet's text and the ops (mrt_dom::RuleOp).
pub struct CssomSheet {
    lock: SharedRwLock,
    contents: ServoArc<StylesheetContents>,
    /// A constructed sheet: `@import` rules are refused (Gecko's StyleSheet::InsertRuleInternal
    /// and ReplaceSync).
    constructed: bool,
}

/// RulesMutateError as the DOMException Gecko throws for it.
fn error_name(e: RulesMutateError) -> &'static str {
    match e {
        RulesMutateError::Syntax => "SyntaxError",
        RulesMutateError::IndexSize => "IndexSizeError",
        RulesMutateError::HierarchyRequest => "HierarchyRequestError",
        RulesMutateError::InvalidState => "InvalidStateError",
    }
}

/// The rules of a sheet made from `css` and then changed by `ops` (mrt_dom::RuleOp), under `lock`:
/// what Gecko's sheet holds after the same calls. `constructed` refuses `@import` rules, as Gecko does
/// in a constructed sheet; elsewhere they are kept, never loaded, so that the rules' indexes are the
/// CSSOM's. Each op succeeded when the script made it, in the same context, so it does again.
pub fn parse_with_ops(
    css: &str,
    ops: &[mrt_dom::RuleOp],
    url_data: UrlExtraData,
    lock: &SharedRwLock,
    error_reporter: Option<&dyn style::error_reporting::ParseErrorReporter>,
    quirks_mode: i32,
    constructed: bool,
) -> ServoArc<StylesheetContents> {
    crate::ensure_prefs();
    let allow = if constructed { AllowImportRules::No } else { AllowImportRules::Yes };
    let contents = StylesheetContents::from_str(css, url_data, Origin::Author, lock, Some(&NoLoad), error_reporter, crate::quirks_of(quirks_mode), allow, None);
    for op in ops {
        let _ = apply_op(&contents, lock, op, allow);
    }
    contents
}

/// Decode the bridge's form-encoded declaration request without embedded NULs in
/// the JS/C++ string transport. Property values can contain arbitrary Unicode.
pub fn declaration_request(text: &str, flags: u8) -> Option<mrt_dom::DeclarationOp> {
    let fields: std::collections::HashMap<_, _> = url::form_urlencoded::parse(text.as_bytes()).collect();
    let path = fields.get("path")?.split(',').map(str::parse::<u32>).collect::<Result<Vec<_>, _>>().ok()?;
    Some(mrt_dom::DeclarationOp { path, kind: flags & 15, name: fields.get("name")?.to_string(),
        value: fields.get("value")?.to_string(), important: flags & 16 != 0 })
}

// CSSStyleRule.style edits operate on the parsed block, avoiding serialization and
// reparsing of unrelated declarations (notably shorthand values containing var()).
fn rule_declaration(contents: &StylesheetContents, lock: &SharedRwLock, edit: &mrt_dom::DeclarationOp) -> Result<String, &'static str> {
    let declarations = {
        let guard = lock.read();
        let mut path = edit.path.iter();
        let mut rule = contents.rules.read_with(&guard).0.get(*path.next().ok_or("IndexSizeError")? as usize).cloned().ok_or("IndexSizeError")?;
        for index in path { rule = rule.children(&guard).get(*index as usize).cloned().ok_or("IndexSizeError")?; }
        let CssRule::Style(rule) = rule else { return Err("InvalidStateError") };
        let block = rule.read_with(&guard).block.clone();
        block
    };
    let mut guard = lock.write();
    let decls = declarations.write_with(&mut guard);
    let id = PropertyId::parse_enabled_for_all_content(&edit.name).ok();
    let mut result = String::new();
    match edit.kind {
        0 => if let Some(id) = id { decls.property_value_to_css(&id, &mut result).unwrap(); },
        1 => if let Some(id) = id { if decls.property_priority(&id) == Importance::Important { result.push_str("important"); } },
        2 => if let Some(id) = id {
            if id.non_custom_id().is_none_or(|id| id.allowed_in_rule(CssRuleType::Style.into())) {
                let mut source = SourcePropertyDeclaration::default();
                if parse_one_declaration_into(&mut source, id, &edit.value, Origin::Author, &contents.url_data, None,
                    ParsingMode::DEFAULT, contents.quirks_mode, CssRuleType::Style).is_ok() {
                    let importance = if edit.important { Importance::Important } else { Importance::Normal };
                    let mut updates = Default::default();
                    if decls.prepare_for_update(&source, importance, &mut updates) { decls.update(source.drain(), importance, &mut updates); }
                }
            }
        },
        3 => if let Some(id) = id {
            decls.property_value_to_css(&id, &mut result).unwrap();
            if let Some(index) = decls.first_declaration_to_remove(&id) { decls.remove_property(&id, index); }
        },
        4 => result = to_text(decls),
        5 => result = decls.declarations().len().to_string(),
        6 => result = edit.value.parse::<usize>().ok().and_then(|i| decls.declarations().get(i)).map(|d| d.id().name().into_owned()).unwrap_or_default(),
        7 => *decls = parse_style_attribute(&edit.value, &contents.url_data, None, contents.quirks_mode, CssRuleType::Style),
        _ => return Err("InvalidStateError"),
    }
    Ok(result)
}

/// One RuleOp on `contents`: the new rule's JSON for an insert, "" for a delete, or the DOMException's
/// name.
fn apply_op(contents: &StylesheetContents, lock: &SharedRwLock, op: &mrt_dom::RuleOp, allow: AllowImportRules) -> Result<String, &'static str> {
    match op {
        mrt_dom::RuleOp::Insert(index, rule) => {
            let index = *index as usize;
            let parsed = {
                let guard = lock.read();
                let rules = contents.rules.read_with(&guard);
                rules.parse_rule_for_insert(lock, rule, contents, index, CssRuleTypes::default(), None, Some(&NoLoad), allow).map_err(error_name)?
            };
            let mut json = String::new();
            rule_json(&parsed, &lock.read(), &mut json);
            let mut guard = lock.write();
            contents.rules.write_with(&mut guard).0.insert(index, parsed);
            Ok(json)
        }
        mrt_dom::RuleOp::Declaration(edit) => rule_declaration(contents, lock, edit),
        mrt_dom::RuleOp::Delete(index) => {
            let mut guard = lock.write();
            let rules: &mut CssRules = contents.rules.write_with(&mut guard);
            rules.remove_rule(*index as usize).map(|()| String::new()).map_err(error_name)
        }
    }
}

impl CssomSheet {
    /// The sheet the text `css` and then `ops` give, in the document's quirks mode (Gecko parses an
    /// owned sheet in its document's compatibility mode, a constructed one in its constructor
    /// document's).
    pub fn parse(css: &str, ops: &[mrt_dom::RuleOp], quirks_mode: i32, constructed: bool) -> CssomSheet {
        let lock = SharedRwLock::new();
        let contents = parse_with_ops(css, ops, url_data(), &lock, None, quirks_mode, constructed);
        CssomSheet { lock, contents, constructed }
    }

    pub fn declaration(&mut self, edit: &mrt_dom::DeclarationOp) -> Result<String, &'static str> {
        rule_declaration(&self.contents, &self.lock, edit)
    }

    /// The number of top-level rules (`cssRules.length`).
    pub fn len(&self) -> usize {
        self.contents.rules.read_with(&self.lock.read()).0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The top-level rules for bridge.js's CSSRule objects, as a JSON array (`rule_json`).
    pub fn rules_json(&self) -> String {
        let guard = self.lock.read();
        let mut out = String::from("[");
        for (i, r) in self.contents.rules.read_with(&guard).0.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            rule_json(r, &guard, &mut out);
        }
        out.push(']');
        out
    }

    /// Gecko's StyleSheet::InsertRule and DeleteRule through Servo_CssRules_InsertRule and
    /// Servo_CssRules_DeleteRule (CSSOM's "insert a CSS rule" and "remove a CSS rule"): an insert's
    /// rule parsed in this sheet's context at its index (where `@import` and `@namespace` may go).
    /// The new rule's JSON for an insert, "" for a delete, or the DOMException's name; the caller
    /// keeps the op that succeeded.
    pub fn apply(&mut self, op: &mrt_dom::RuleOp) -> Result<String, &'static str> {
        crate::ensure_prefs();
        let allow = if self.constructed { AllowImportRules::No } else { AllowImportRules::Yes };
        apply_op(&self.contents, &self.lock, op, allow)
    }
}

/// A rule's `cssText`.
fn rule_text(rule: &CssRule, guard: &SharedRwLockReadGuard) -> String {
    let mut s = String::new();
    rule.to_css(guard, &mut s).unwrap();
    s
}

/// `s` as a JSON string.
fn json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// A rule for bridge.js's CSSRule objects: `{"i": interface, "t": type, "c": cssText}` and what its
/// interface reads: `s` selectorText, `m` media text, `x` conditionText, `n` name, `h` href, `p`
/// prefix, `u` namespace URI, `k` keyText, `r` the child rules (CSSGroupingRule's and
/// CSSKeyframesRule's `cssRules`). `type` is CSSOM's number; the rules after CSSOM's list report 0.
fn rule_json(rule: &CssRule, guard: &SharedRwLockReadGuard, out: &mut String) {
    let (iface, ty) = match rule {
        CssRule::Style(_) => ("CSSStyleRule", 1),
        CssRule::Import(_) => ("CSSImportRule", 3),
        CssRule::Media(_) => ("CSSMediaRule", 4),
        CssRule::FontFace(_) => ("CSSFontFaceRule", 5),
        CssRule::Page(_) => ("CSSPageRule", 6),
        CssRule::Keyframes(_) => ("CSSKeyframesRule", 7),
        CssRule::Margin(_) => ("CSSMarginRule", 9),
        CssRule::Namespace(_) => ("CSSNamespaceRule", 10),
        CssRule::CounterStyle(_) => ("CSSCounterStyleRule", 11),
        CssRule::Supports(_) => ("CSSSupportsRule", 12),
        CssRule::Document(_) => ("CSSMozDocumentRule", 13),
        CssRule::FontFeatureValues(_) => ("CSSFontFeatureValuesRule", 14),
        CssRule::LayerBlock(_) => ("CSSLayerBlockRule", 0),
        CssRule::LayerStatement(_) => ("CSSLayerStatementRule", 0),
        CssRule::Container(_) => ("CSSContainerRule", 0),
        CssRule::FontPaletteValues(_) => ("CSSFontPaletteValuesRule", 0),
        CssRule::Property(_) => ("CSSPropertyRule", 0),
        CssRule::Scope(_) => ("CSSScopeRule", 0),
        CssRule::StartingStyle(_) => ("CSSStartingStyleRule", 0),
        CssRule::PositionTry(_) => ("CSSPositionTryRule", 0),
        CssRule::NestedDeclarations(_) => ("CSSNestedDeclarations", 0),
        CssRule::CustomMedia(_) => ("CSSCustomMediaRule", 0),
        CssRule::ViewTransition(_) => ("CSSViewTransitionRule", 0),
        CssRule::AppearanceBase(_) => ("CSSRule", 0),
    };
    out.push_str("{\"i\":\"");
    out.push_str(iface);
    out.push_str(&format!("\",\"t\":{ty},\"c\":"));
    json_string(&rule_text(rule, guard), out);
    let field = |out: &mut String, key: &str, value: &str| {
        out.push_str(&format!(",\"{key}\":"));
        json_string(value, out);
    };
    let css = |v: &dyn Fn(&mut style_traits::CssWriter<String>) -> std::fmt::Result| {
        let mut s = String::new();
        v(&mut style_traits::CssWriter::new(&mut s)).unwrap();
        s
    };
    let children = |out: &mut String, rules: &CssRules| {
        out.push_str(",\"r\":[");
        for (i, r) in rules.0.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            rule_json(r, guard, out);
        }
        out.push(']');
    };
    match rule {
        CssRule::Style(r) => {
            let r = r.read_with(guard);
            field(out, "s", &css(&|w| cssparser::ToCss::to_css(&r.selectors, w)));
            if let Some(rules) = &r.rules {
                children(out, rules.read_with(guard));
            }
        }
        CssRule::Media(r) => {
            field(out, "m", &css(&|w| r.media_queries.read_with(guard).to_css(w)));
            children(out, r.rules.read_with(guard));
        }
        CssRule::Supports(r) => {
            field(out, "x", &css(&|w| r.condition.to_css(w)));
            children(out, r.rules.read_with(guard));
        }
        CssRule::Container(r) => {
            field(out, "x", &css(&|w| r.conditions.to_css(w)));
            children(out, r.rules.read_with(guard));
        }
        CssRule::Document(r) => children(out, r.rules.read_with(guard)),
        CssRule::LayerBlock(r) => {
            field(out, "n", &r.name.as_ref().map(|n| css(&|w| n.to_css(w))).unwrap_or_default());
            children(out, r.rules.read_with(guard));
        }
        CssRule::Scope(r) => children(out, r.rules.read_with(guard)),
        CssRule::StartingStyle(r) => children(out, r.rules.read_with(guard)),
        // (the URL as written, as Gecko's CSSImportRule::GetHref: Servo's CssUrl keeps it only for
        // its serialization, `url("...")`)
        CssRule::Import(r) => {
            let url = css(&|w| r.read_with(guard).url.to_css(w));
            let written = url.strip_prefix("url(\"").and_then(|u| u.strip_suffix("\")")).map(|u| u.replace("\\\"", "\"").replace("\\\\", "\\"));
            field(out, "h", &written.unwrap_or(url));
        }
        CssRule::Namespace(r) => {
            field(out, "p", &r.prefix.as_ref().map(|p| p.to_string()).unwrap_or_default());
            field(out, "u", &r.url.to_string());
        }
        CssRule::Keyframes(r) => {
            let r = r.read_with(guard);
            field(out, "n", &r.name.as_atom().to_string());
            out.push_str(",\"r\":[");
            for (i, k) in r.keyframes.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                let k = k.read_with(guard);
                out.push_str("{\"i\":\"CSSKeyframeRule\",\"t\":8,\"c\":");
                let mut text = String::new();
                k.to_css(guard, &mut text).unwrap();
                json_string(&text, out);
                field(out, "k", &css(&|w| k.selector.to_css(w)));
                out.push('}');
            }
            out.push(']');
        }
        _ => {}
    }
    out.push('}');
}

#[cfg(test)]
mod tests {
    use super::*;
    use mrt_dom::RuleOp;

    #[test]
    fn insert_and_delete_rules() {
        let mut sheet = CssomSheet::parse("@import url(a.css); p { color: red }", &[], 0, false);
        assert_eq!(sheet.len(), 2);
        assert!(sheet.apply(&RuleOp::Insert(2, "b { color: blue }".into())).unwrap().contains("\"s\":\"b\""));
        assert_eq!(sheet.apply(&RuleOp::Insert(3, "}{".into())), Err("SyntaxError"));
        assert_eq!(sheet.apply(&RuleOp::Insert(9, "i {}".into())), Err("IndexSizeError"));
        // an @import after a style rule; an @namespace into a sheet with style rules
        assert_eq!(sheet.apply(&RuleOp::Insert(2, "@import url(b.css);".into())), Err("HierarchyRequestError"));
        assert_eq!(sheet.apply(&RuleOp::Insert(1, "@namespace url(x);".into())), Err("InvalidStateError"));
        assert_eq!(sheet.apply(&RuleOp::Delete(3)), Err("IndexSizeError"));
        assert_eq!(sheet.apply(&RuleOp::Delete(1)), Ok(String::new()));
        assert_eq!(sheet.len(), 2);
        // a constructed sheet refuses @import
        let mut constructed = CssomSheet::parse("@import url(a.css); p {}", &[], 0, true);
        assert_eq!(constructed.len(), 1);
        assert_eq!(constructed.apply(&RuleOp::Insert(0, "@import url(a.css);".into())), Err("SyntaxError"));
    }

    #[test]
    fn declaration_edits_replay_without_roundtripping_other_properties() {
        let css = "h2 { font: var(--f); font-weight: bold } @media all { h2 { height: 10px } }";
        let edit = |path, name: &str, value: &str| RuleOp::Declaration(Box::new(mrt_dom::DeclarationOp {
            path, kind: 2, name: name.into(), value: value.into(), important: false,
        }));
        let ops = [edit(vec![0], "color", "blue"), edit(vec![1, 0], "height", "33px"),
                   RuleOp::Insert(0, "p {}".into()), edit(vec![1], "width", "29px")];
        let mut live = CssomSheet::parse(css, &[], 0, false);
        for op in &ops { live.apply(op).unwrap(); }
        let restored = CssomSheet::parse(css, &ops, 0, false);
        assert_eq!(live.rules_json(), restored.rules_json());
        assert!(live.rules_json().contains("font-style: ;"));
        assert!(live.rules_json().contains("height: 33px"));
        assert!(live.rules_json().contains("width: 29px"));
    }

    #[test]
    fn ops_replay_to_the_same_rules() {
        // a shorthand with var() under a longhand: its serialization does not parse back to the
        // same rule, the ops replayed over the text do
        let css = "h2 { font: var(--f); font-weight: bold }";
        let ops = [RuleOp::Insert(1, "a { color: red }".into()), RuleOp::Insert(0, "b {}".into()), RuleOp::Delete(2)];
        let mut live = CssomSheet::parse(css, &[], 0, false);
        for op in &ops {
            live.apply(op).unwrap();
        }
        let replayed = CssomSheet::parse(css, &ops, 0, false);
        assert_eq!(live.rules_json(), replayed.rules_json());
        assert!(replayed.rules_json().contains("font-style: ;"));
        assert_eq!(replayed.len(), 2);
    }
}
