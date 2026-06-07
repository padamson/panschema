//! Shared LinkML resolution services consumed by every writer.
//!
//! Every output writer (HTML, Rust, graph JSON, future SHACL / SQL)
//! needs the same answers about a LinkML schema: what's the effective
//! set of slots on a class once `is_a` chains, mixins, and
//! `slot_usage` overrides have been resolved? Without one resolver
//! shared across writers, each writer rolls its own walker — three
//! copies, three correctness bugs to find independently.
//!
//! This module is the single source of truth. Writers import
//! [`resolve_effective_slots`] and consume the result; no writer
//! should walk `is_a` / `mixins` / `slot_usage` directly anymore.
//!
//! The implementation is lifted verbatim from the original
//! `rust_writer::resolve_slots`, which was the most complete walker
//! among the three (covered `slot_usage` merge-overlay; the others
//! ignored it). Behaviour is preserved exactly — the 16 unit tests
//! in [`crate::rust_writer`] continue to validate the same code via
//! the new path.

use std::collections::{BTreeMap, BTreeSet};

use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};

/// Walk a class's `is_a` chain and `mixins`, then apply the class's own
/// `attributes`, global `slots:` refs, and `slot_usage` overrides to
/// produce the effective set of slots that show up as fields on the
/// generated struct / HTML class card / graph hover card.
///
/// Precedence (lowest to highest):
/// 1. `is_a` ancestor's slots (recursive)
/// 2. Mixin slots (don't overwrite is_a-inherited slots with same name)
/// 3. This class's inline `attributes`
/// 4. This class's global `slots:` references (don't overwrite #1–3)
/// 5. This class's `slot_usage` overrides (merge-overlay)
pub fn resolve_effective_slots(
    class: &ClassDefinition,
    schema: &SchemaDefinition,
) -> BTreeMap<String, SlotDefinition> {
    let mut visited = BTreeSet::new();
    resolve_slots_walk(class, schema, &mut visited)
}

/// Recursive worker for [`resolve_effective_slots`]. `visited` holds
/// the class names currently on the recursion stack so a circular
/// `is_a` or `mixin` chain terminates (silently dropping the
/// would-be-cyclic contribution) rather than overflowing.
fn resolve_slots_walk(
    class: &ClassDefinition,
    schema: &SchemaDefinition,
    visited: &mut BTreeSet<String>,
) -> BTreeMap<String, SlotDefinition> {
    let mut slots: BTreeMap<String, SlotDefinition> = BTreeMap::new();

    // Mark this class as in-progress. If insert returns false, we've
    // already visited this class along the current path — stop.
    if !visited.insert(class.name.clone()) {
        return slots;
    }

    if let Some(parent_name) = &class.is_a
        && let Some(parent) = schema.classes.get(parent_name)
    {
        for (name, def) in resolve_slots_walk(parent, schema, visited) {
            slots.insert(name, def);
        }
    }

    for mixin_name in &class.mixins {
        if let Some(mixin) = schema.classes.get(mixin_name) {
            for (name, def) in resolve_slots_walk(mixin, schema, visited) {
                slots.entry(name).or_insert(def);
            }
        }
    }

    for (name, def) in &class.attributes {
        slots.insert(name.clone(), def.clone());
    }

    for slot_name in &class.slots {
        if let Some(def) = schema.slots.get(slot_name) {
            slots
                .entry(slot_name.clone())
                .or_insert_with(|| def.clone());
        }
    }

    for (name, override_def) in &class.slot_usage {
        let target = slots
            .entry(name.clone())
            .or_insert_with(|| override_def.clone());
        merge_slot_override(target, override_def);
    }

    // Pop this class on the way out — sibling/cousin paths to it
    // through different ancestors are NOT cycles.
    visited.remove(&class.name);
    slots
}

/// Merge a `slot_usage` override into an inherited/base slot definition.
/// Only `Option` and `Vec` fields get overwritten when the override
/// supplies a non-default value, so a `slot_usage` entry that just
/// refines `range` doesn't accidentally reset `required` or `multivalued`
/// on the inherited slot.
///
/// Bool fields are copied only when the override sets them to `true`.
/// LinkML schemas in practice use `slot_usage` to tighten constraints
/// (make optional → required, single → multivalued), not loosen them; the
/// pre-Option<bool> IR can't distinguish "override sets false explicitly"
/// from "override omits the field." This compromise covers the common
/// case faithfully.
fn merge_slot_override(target: &mut SlotDefinition, source: &SlotDefinition) {
    /// Clone the source field into the target when the source field is
    /// `Some`. Skips when source is `None` so the inherited value wins.
    macro_rules! merge_opt {
        ($field:ident) => {
            if source.$field.is_some() {
                target.$field = source.$field.clone();
            }
        };
    }
    /// Copy a `Copy` source field to the target when the source is `Some`.
    macro_rules! merge_opt_copy {
        ($field:ident) => {
            if source.$field.is_some() {
                target.$field = source.$field;
            }
        };
    }

    merge_opt!(range);
    merge_opt!(description);
    merge_opt!(pattern);
    merge_opt_copy!(minimum_cardinality);
    merge_opt_copy!(maximum_cardinality);

    if !source.any_of.is_empty() {
        target.any_of = source.any_of.clone();
    }
    if source.required {
        target.required = true;
    }
    if source.multivalued {
        target.multivalued = true;
    }
    if source.identifier {
        target.identifier = true;
    }
}

/// Expand a CURIE-shaped value against the schema's `prefixes:`
/// table, falling back to `default_prefix` for bare names.
///
/// `urn:` is treated as an absolute IRI scheme even though it lacks
/// `://`, so `urn:isbn:9780123456789` passes through unchanged
/// instead of being parsed as a CURIE under the (unlikely) `urn`
/// prefix.
pub fn expand_curie(schema: &SchemaDefinition, value: &str) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    if value.starts_with("http://") || value.starts_with("https://") || value.starts_with("urn:") {
        return Some(value.to_string());
    }
    if let Some((prefix, rest)) = value.split_once(':') {
        return schema
            .prefixes
            .get(prefix)
            .map(|base| format!("{base}{rest}"));
    }
    let default = schema.default_prefix.as_deref()?;
    let base = schema.prefixes.get(default)?;
    Some(format!("{base}{value}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A schema exercising `is_a`, mixins, and `slot_usage` overlay all
    /// at once. Pins the public surface of [`resolve_effective_slots`] —
    /// detailed coverage of each individual rule lives in
    /// [`crate::rust_writer`]'s tests, which exercise the same code path
    /// via the writer that originally housed it. This test guards
    /// against an accidental signature change or an interaction bug
    /// between the rules that the single-rule tests would miss.
    #[test]
    fn resolves_is_a_mixin_and_slot_usage_in_one_walk() {
        let mut schema = SchemaDefinition::new("compound");

        // Root class with one direct slot.
        let mut root = ClassDefinition::new("Root");
        let mut root_field = SlotDefinition::new("rootField");
        root_field.range = Some("string".into());
        root.attributes.insert("rootField".into(), root_field);
        schema.classes.insert("Root".into(), root);

        // Mixin contributing one slot.
        let mut mixin = ClassDefinition::new("Auditable");
        let mut created_at = SlotDefinition::new("createdAt");
        created_at.range = Some("datetime".into());
        mixin.attributes.insert("createdAt".into(), created_at);
        schema.classes.insert("Auditable".into(), mixin);

        // Leaf class: inherits Root, mixes in Auditable, adds its own
        // direct slot, refines `rootField`'s range via `slot_usage`.
        let mut leaf = ClassDefinition::new("Leaf");
        leaf.is_a = Some("Root".into());
        leaf.mixins = vec!["Auditable".into()];
        let mut leaf_only = SlotDefinition::new("leafOnly");
        leaf_only.range = Some("integer".into());
        leaf.attributes.insert("leafOnly".into(), leaf_only);

        let mut refined = SlotDefinition::new("rootField");
        refined.range = Some("Identifier".into());
        refined.required = true;
        leaf.slot_usage.insert("rootField".into(), refined);
        schema.classes.insert("Leaf".into(), leaf);

        let resolved = resolve_effective_slots(&schema.classes["Leaf"], &schema);

        // Every contributing rule appears in the output.
        assert_eq!(resolved.len(), 3, "expected rootField, createdAt, leafOnly");
        assert_eq!(
            resolved["rootField"].range.as_deref(),
            Some("Identifier"),
            "slot_usage range refinement should win"
        );
        assert!(
            resolved["rootField"].required,
            "slot_usage required=true should propagate"
        );
        assert_eq!(
            resolved["createdAt"].range.as_deref(),
            Some("datetime"),
            "mixin slot should be flattened in"
        );
        assert_eq!(
            resolved["leafOnly"].range.as_deref(),
            Some("integer"),
            "direct attribute should be present"
        );
    }

    fn schema_with_prov_default() -> SchemaDefinition {
        let mut schema = SchemaDefinition::new("prov_default");
        schema
            .prefixes
            .insert("prov".to_string(), "http://www.w3.org/ns/prov#".to_string());
        schema.default_prefix = Some("prov".to_string());
        schema
    }

    #[test]
    fn expand_curie_expands_known_prefix() {
        let schema = schema_with_prov_default();
        assert_eq!(
            expand_curie(&schema, "prov:Entity").as_deref(),
            Some("http://www.w3.org/ns/prov#Entity")
        );
    }

    #[test]
    fn expand_curie_returns_none_for_unknown_prefix() {
        let schema = schema_with_prov_default();
        assert!(expand_curie(&schema, "fictional:Foo").is_none());
    }

    #[test]
    fn expand_curie_passes_through_absolute_http_urls() {
        let schema = schema_with_prov_default();
        assert_eq!(
            expand_curie(&schema, "http://example.org/foo").as_deref(),
            Some("http://example.org/foo")
        );
        assert_eq!(
            expand_curie(&schema, "https://example.org/bar").as_deref(),
            Some("https://example.org/bar")
        );
        assert_eq!(
            expand_curie(&schema, "urn:isbn:9780123456789").as_deref(),
            Some("urn:isbn:9780123456789")
        );
    }

    #[test]
    fn expand_curie_uses_default_prefix_for_bare_names() {
        let schema = schema_with_prov_default();
        assert_eq!(
            expand_curie(&schema, "Entity").as_deref(),
            Some("http://www.w3.org/ns/prov#Entity")
        );
    }

    #[test]
    fn expand_curie_returns_none_for_bare_name_without_default_prefix() {
        let mut schema = SchemaDefinition::new("no_default");
        schema
            .prefixes
            .insert("prov".to_string(), "http://www.w3.org/ns/prov#".to_string());
        assert!(expand_curie(&schema, "Entity").is_none());
    }

    #[test]
    fn expand_curie_returns_none_for_empty_input() {
        let schema = schema_with_prov_default();
        assert!(expand_curie(&schema, "").is_none());
    }
}
