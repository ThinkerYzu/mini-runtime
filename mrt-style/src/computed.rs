//! What `getComputedStyle` serializes from a computed style: the resolved value of a longhand, a
//! shorthand or a custom property, as Gecko's servo/ports/geckolib/glue.rs does for the values
//! nsComputedDOMStyle hands to Servo (`Servo_GetResolvedValue`, `computed_or_resolved_value`,
//! `Servo_GetCustomPropertyValue`). The values that depend on layout (sizes, margins, padding,
//! offsets) are nsComputedDOMStyle's own, in mrt-layout.

use style::properties::{ComputedValues, Importance, LonghandId, PropertyDeclarationBlock, PropertyDeclarationId, PropertyId, ShorthandId};
use style::values::resolved;

/// A property name as `getPropertyValue` takes it: a longhand, a shorthand or a custom property
/// enabled for content; `None` for anything else (Gecko's `nsCSSProps::LookupProperty` gives
/// `eCSSProperty_UNKNOWN`, and the value is "").
pub fn lookup_property(name: &str) -> Option<PropertyId> {
    PropertyId::parse_enabled_for_all_content(name).ok()
}

// Gecko: servo/ports/geckolib/glue.rs computed_or_resolved_value (with a resolved::Context, as
// Servo_GetResolvedValue gives it)
/// The resolved value of `property` in `style`, serialized.
pub fn resolved_value(style: &ComputedValues, property: &PropertyId) -> String {
    let shorthand = match property.as_shorthand() {
        Ok(shorthand) => shorthand,
        Err(PropertyDeclarationId::Longhand(longhand)) => return longhand_value(style, longhand),
        // Servo_GetCustomPropertyValue
        Err(id @ PropertyDeclarationId::Custom(_)) => return style.computed_value_to_string(id),
    };
    let mut block = PropertyDeclarationBlock::new();
    for longhand in shorthand.longhands() {
        let mut context = resolved::Context { style, for_property: property.clone(), current_longhand: None };
        block.push(style.computed_or_resolved_declaration(longhand, Some(&mut context)), Importance::Normal);
    }
    let mut out = String::new();
    block.shorthand_to_css(shorthand, &mut out).unwrap();
    out
}

/// The resolved value of one longhand (a logical one is read through its physical one).
pub fn longhand_value(style: &ComputedValues, longhand: LonghandId) -> String {
    style.computed_value_to_string(PropertyDeclarationId::Longhand(longhand))
}

/// The properties a computed style lists (`length`, `item(i)`): every longhand enabled for content,
/// by name. Gecko lists those of its nsComputedDOMStylePropertyList in that list's order, and its
/// set differs from Stylo's servo configuration (GAPS B51).
pub fn property_names() -> &'static [&'static str] {
    static NAMES: std::sync::LazyLock<Vec<&'static str>> = std::sync::LazyLock::new(|| {
        let mut names: Vec<&'static str> = ShorthandId::All
            .longhands()
            .chain([LonghandId::Direction, LonghandId::UnicodeBidi])
            .filter(|l| PropertyId::NonCustom((*l).into()).enabled_for_all_content())
            .map(|l| l.name())
            .collect();
        names.sort_unstable();
        names.dedup();
        names
    });
    &NAMES
}
