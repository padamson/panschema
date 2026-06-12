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

/// How a resolved slot reached the class it was resolved for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InheritancePath {
    /// Chain of `is_a` ancestors walked from the class's parent down
    /// to the defining class: `["B", "A"]` when `D is_a B is_a A`
    /// and `A` defines the slot.
    IsA(Vec<String>),
    /// Name of the mixin listed on the class (the slot may originate
    /// deeper — `from` names the definer, this names the hop).
    Mixin(String),
}

/// Origin of a resolved slot, from the perspective of the class
/// passed to [`resolve_effective_slots_with_provenance`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provenance {
    /// Defined on the class itself (inline `attributes` or a
    /// `slots:` reference).
    Direct,
    /// Contributed by an ancestor or mixin. `from` is the defining
    /// class; an ancestor that itself refined the slot counts as the
    /// definer of what the class actually inherits.
    Inherited { from: String, via: InheritancePath },
    /// Overridden at this class — by `slot_usage`, or by an inline
    /// attribute shadowing an inherited slot. `from` is where the
    /// overridden base came from (the class itself when the base was
    /// direct).
    Refined { from: String, by_slot_usage: bool },
}

impl Provenance {
    /// Human-readable origin for display — `"mixin Named"`,
    /// `"Identifiable via mixin Auditable"`, or just the defining
    /// class name for `is_a` inheritance. `None` when the slot
    /// originates at `here` (direct slots, and refinements of the
    /// class's own slots), so consumers can render nothing for the
    /// common case.
    pub fn origin_label(&self, here: &str) -> Option<String> {
        match self {
            Provenance::Direct => None,
            Provenance::Refined { from, .. } => (from != here).then(|| from.clone()),
            Provenance::Inherited { from, via } => Some(match via {
                InheritancePath::Mixin(m) if m == from => format!("mixin {m}"),
                InheritancePath::Mixin(m) => format!("{from} via mixin {m}"),
                InheritancePath::IsA(_) => from.clone(),
            }),
        }
    }
}

/// A slot definition paired with where it came from. Output of
/// [`resolve_effective_slots_with_provenance`]; consumers that don't
/// care about origin use [`resolve_effective_slots`] instead.
#[derive(Debug, Clone)]
pub struct ResolvedSlot {
    pub definition: SlotDefinition,
    pub provenance: Provenance,
}

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
    resolve_effective_slots_with_provenance(class, schema)
        .into_iter()
        .map(|(name, rs)| (name, rs.definition))
        .collect()
}

/// [`resolve_effective_slots`] plus per-slot [`Provenance`]. Same
/// walk, same precedence; the provenance is rebased at each hop so
/// every entry answers "where did this come from?" relative to the
/// class passed in. Diamond shapes (a slot reachable via both the
/// `is_a` chain and a mixin) deterministically report the `is_a`
/// path — it is processed first and mixins never overwrite.
pub fn resolve_effective_slots_with_provenance(
    class: &ClassDefinition,
    schema: &SchemaDefinition,
) -> BTreeMap<String, ResolvedSlot> {
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
) -> BTreeMap<String, ResolvedSlot> {
    let mut slots: BTreeMap<String, ResolvedSlot> = BTreeMap::new();

    // Mark this class as in-progress. If insert returns false, we've
    // already visited this class along the current path — stop.
    if !visited.insert(class.name.clone()) {
        return slots;
    }

    if let Some(parent_name) = &class.is_a
        && let Some(parent) = schema.classes.get(parent_name)
    {
        for (name, rs) in resolve_slots_walk(parent, schema, visited) {
            slots.insert(
                name,
                ResolvedSlot {
                    provenance: rebase_through_is_a(parent_name, rs.provenance),
                    definition: rs.definition,
                },
            );
        }
    }

    for mixin_name in &class.mixins {
        if let Some(mixin) = schema.classes.get(mixin_name) {
            for (name, rs) in resolve_slots_walk(mixin, schema, visited) {
                slots.entry(name).or_insert_with(|| ResolvedSlot {
                    provenance: rebase_through_mixin(mixin_name, rs.provenance),
                    definition: rs.definition,
                });
            }
        }
    }

    for (name, def) in &class.attributes {
        // An inline attribute shadowing an inherited slot is a
        // refinement-by-redefinition; a fresh name is a direct slot.
        let provenance = match slots.get(name) {
            Some(prev) => Provenance::Refined {
                from: origin_of(&prev.provenance, &class.name),
                by_slot_usage: false,
            },
            None => Provenance::Direct,
        };
        slots.insert(
            name.clone(),
            ResolvedSlot {
                definition: def.clone(),
                provenance,
            },
        );
    }

    for slot_name in &class.slots {
        if let Some(def) = schema.slots.get(slot_name) {
            slots
                .entry(slot_name.clone())
                .or_insert_with(|| ResolvedSlot {
                    definition: def.clone(),
                    provenance: Provenance::Direct,
                });
        }
    }

    for (name, override_def) in &class.slot_usage {
        match slots.get_mut(name) {
            Some(target) => {
                target.provenance = Provenance::Refined {
                    from: origin_of(&target.provenance, &class.name),
                    by_slot_usage: true,
                };
                merge_slot_override(&mut target.definition, override_def);
            }
            // A `slot_usage` with no inherited base acts as the
            // slot's introduction at this class.
            None => {
                slots.insert(
                    name.clone(),
                    ResolvedSlot {
                        definition: override_def.clone(),
                        provenance: Provenance::Direct,
                    },
                );
            }
        }
    }

    // Pop this class on the way out — sibling/cousin paths to it
    // through different ancestors are NOT cycles.
    visited.remove(&class.name);
    slots
}

/// The defining class a provenance points back to, with `here` as
/// the answer for slots that originate at the current class.
fn origin_of(provenance: &Provenance, here: &str) -> String {
    match provenance {
        Provenance::Direct => here.to_string(),
        Provenance::Inherited { from, .. } | Provenance::Refined { from, .. } => from.clone(),
    }
}

/// Rebase a parent-relative provenance to the child inheriting
/// through `is_a`: the parent's direct (or refined) slots are what
/// the child inherits from the parent, and deeper `is_a` chains grow
/// by the parent hop. A mixin path observed at the parent stays a
/// mixin path — the mixin relationship is the fact worth surfacing.
fn rebase_through_is_a(parent: &str, provenance: Provenance) -> Provenance {
    match provenance {
        Provenance::Direct | Provenance::Refined { .. } => Provenance::Inherited {
            from: parent.to_string(),
            via: InheritancePath::IsA(vec![parent.to_string()]),
        },
        Provenance::Inherited {
            from,
            via: InheritancePath::IsA(chain),
        } => {
            let mut full = vec![parent.to_string()];
            full.extend(chain);
            Provenance::Inherited {
                from,
                via: InheritancePath::IsA(full),
            }
        }
        inherited_via_mixin => inherited_via_mixin,
    }
}

/// Rebase a mixin-relative provenance to the consuming class: the
/// hop is always the mixin named in the class's `mixins:` list, and
/// `from` stays on the defining class when the mixin itself
/// inherited the slot.
fn rebase_through_mixin(mixin: &str, provenance: Provenance) -> Provenance {
    let from = match provenance {
        Provenance::Inherited { from, .. } => from,
        Provenance::Direct | Provenance::Refined { .. } => mixin.to_string(),
    };
    Provenance::Inherited {
        from,
        via: InheritancePath::Mixin(mixin.to_string()),
    }
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

/// Effective cardinality of a resolved slot: the answer every writer
/// should display, after explicit `minimum_cardinality` /
/// `maximum_cardinality` bounds and the `required` / `multivalued`
/// flags have been reconciled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cardinality {
    pub required: bool,
    pub multivalued: bool,
    pub min: Option<u32>,
    pub max: Option<u32>,
}

/// Compute the effective cardinality of a slot. Pass a slot that has
/// already been through [`resolve_effective_slots`] — the
/// `slot_usage` overlay happens there, so this is a pure view with no
/// resolution logic of its own.
///
/// Precedence per bound (highest wins): an explicit
/// `minimum_cardinality` decides `required` (`min >= 1`); an explicit
/// `maximum_cardinality` decides `multivalued` (`max > 1`); each flag
/// is the fallback when its bound is absent.
pub fn effective_cardinality(slot: &SlotDefinition) -> Cardinality {
    Cardinality {
        required: slot
            .minimum_cardinality
            .map_or(slot.required, |min| min >= 1),
        multivalued: slot
            .maximum_cardinality
            .map_or(slot.multivalued, |max| max > 1),
        min: slot.minimum_cardinality,
        max: slot.maximum_cardinality,
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

    /// Builds: A defines `name`; B is_a A; C is_a A (mixin-usable);
    /// D is_a B, mixins [C]. The diamond fixture for provenance.
    fn diamond_schema() -> SchemaDefinition {
        let mut schema = SchemaDefinition::new("diamond");
        let mut a = ClassDefinition::new("A");
        a.attributes
            .insert("name".into(), SlotDefinition::new("name"));
        schema.classes.insert("A".into(), a);
        let mut b = ClassDefinition::new("B");
        b.is_a = Some("A".into());
        schema.classes.insert("B".into(), b);
        let mut c = ClassDefinition::new("C");
        c.is_a = Some("A".into());
        schema.classes.insert("C".into(), c);
        let mut d = ClassDefinition::new("D");
        d.is_a = Some("B".into());
        d.mixins = vec!["C".into()];
        schema.classes.insert("D".into(), d);
        schema
    }

    #[test]
    fn provenance_direct_slot_is_direct() {
        let schema = diamond_schema();
        let resolved = resolve_effective_slots_with_provenance(&schema.classes["A"], &schema);
        assert_eq!(resolved["name"].provenance, Provenance::Direct);
        assert_eq!(resolved["name"].provenance.origin_label("A"), None);
    }

    #[test]
    fn provenance_tracks_is_a_chain_to_definer() {
        let schema = diamond_schema();
        let resolved = resolve_effective_slots_with_provenance(&schema.classes["B"], &schema);
        assert_eq!(
            resolved["name"].provenance,
            Provenance::Inherited {
                from: "A".into(),
                via: InheritancePath::IsA(vec!["A".into()]),
            }
        );
        assert_eq!(
            resolved["name"].provenance.origin_label("B").as_deref(),
            Some("A")
        );
    }

    #[test]
    fn provenance_diamond_reports_is_a_path_deterministically() {
        // D reaches A.name both via is_a (B → A) and via mixin C.
        // The is_a path is processed first and mixins never
        // overwrite, so the reported path is the is_a chain.
        let schema = diamond_schema();
        let resolved = resolve_effective_slots_with_provenance(&schema.classes["D"], &schema);
        assert_eq!(
            resolved["name"].provenance,
            Provenance::Inherited {
                from: "A".into(),
                via: InheritancePath::IsA(vec!["B".into(), "A".into()]),
            }
        );
    }

    #[test]
    fn provenance_mixin_slot_names_the_mixin_hop() {
        let mut schema = SchemaDefinition::new("s");
        let mut named = ClassDefinition::new("Named");
        named
            .attributes
            .insert("name".into(), SlotDefinition::new("name"));
        schema.classes.insert("Named".into(), named);
        let mut person = ClassDefinition::new("Person");
        person.mixins = vec!["Named".into()];
        schema.classes.insert("Person".into(), person);

        let resolved = resolve_effective_slots_with_provenance(&schema.classes["Person"], &schema);
        assert_eq!(
            resolved["name"].provenance,
            Provenance::Inherited {
                from: "Named".into(),
                via: InheritancePath::Mixin("Named".into()),
            }
        );
        assert_eq!(
            resolved["name"]
                .provenance
                .origin_label("Person")
                .as_deref(),
            Some("mixin Named")
        );
    }

    #[test]
    fn provenance_mixin_inherited_slot_keeps_definer_names_hop() {
        // The mixin itself inherits the slot: from = the definer,
        // via = the mixin hop the consuming class actually lists.
        let mut schema = SchemaDefinition::new("s");
        let mut base = ClassDefinition::new("Identifiable");
        base.attributes
            .insert("id".into(), SlotDefinition::new("id"));
        schema.classes.insert("Identifiable".into(), base);
        let mut mixin = ClassDefinition::new("Auditable");
        mixin.is_a = Some("Identifiable".into());
        schema.classes.insert("Auditable".into(), mixin);
        let mut doc = ClassDefinition::new("Document");
        doc.mixins = vec!["Auditable".into()];
        schema.classes.insert("Document".into(), doc);

        let resolved =
            resolve_effective_slots_with_provenance(&schema.classes["Document"], &schema);
        assert_eq!(
            resolved["id"].provenance,
            Provenance::Inherited {
                from: "Identifiable".into(),
                via: InheritancePath::Mixin("Auditable".into()),
            }
        );
        assert_eq!(
            resolved["id"]
                .provenance
                .origin_label("Document")
                .as_deref(),
            Some("Identifiable via mixin Auditable")
        );
    }

    #[test]
    fn provenance_slot_usage_marks_refined_with_origin() {
        let mut schema = SchemaDefinition::new("s");
        let mut parent = ClassDefinition::new("Parent");
        parent
            .attributes
            .insert("field".into(), SlotDefinition::new("field"));
        schema.classes.insert("Parent".into(), parent);
        let mut child = ClassDefinition::new("Child");
        child.is_a = Some("Parent".into());
        let mut tighten = SlotDefinition::new("field");
        tighten.required = true;
        child.slot_usage.insert("field".into(), tighten);
        schema.classes.insert("Child".into(), child);

        let resolved = resolve_effective_slots_with_provenance(&schema.classes["Child"], &schema);
        assert_eq!(
            resolved["field"].provenance,
            Provenance::Refined {
                from: "Parent".into(),
                by_slot_usage: true,
            }
        );
        assert!(resolved["field"].definition.required);
        assert_eq!(
            resolved["field"]
                .provenance
                .origin_label("Child")
                .as_deref(),
            Some("Parent"),
            "a refined inherited slot still points at its origin"
        );
    }

    #[test]
    fn provenance_inline_attribute_shadowing_inherited_is_refined() {
        let mut schema = SchemaDefinition::new("s");
        let mut parent = ClassDefinition::new("Parent");
        parent
            .attributes
            .insert("field".into(), SlotDefinition::new("field"));
        schema.classes.insert("Parent".into(), parent);
        let mut child = ClassDefinition::new("Child");
        child.is_a = Some("Parent".into());
        let mut shadow = SlotDefinition::new("field");
        shadow.range = Some("integer".into());
        child.attributes.insert("field".into(), shadow);
        schema.classes.insert("Child".into(), child);

        let resolved = resolve_effective_slots_with_provenance(&schema.classes["Child"], &schema);
        assert_eq!(
            resolved["field"].provenance,
            Provenance::Refined {
                from: "Parent".into(),
                by_slot_usage: false,
            }
        );
    }

    #[test]
    fn provenance_refinement_of_own_slot_renders_no_origin() {
        // slot_usage over the class's own attribute: Refined with
        // from = the class itself, which origin_label suppresses.
        let mut schema = SchemaDefinition::new("s");
        let mut thing = ClassDefinition::new("Thing");
        thing
            .attributes
            .insert("field".into(), SlotDefinition::new("field"));
        let mut tighten = SlotDefinition::new("field");
        tighten.required = true;
        thing.slot_usage.insert("field".into(), tighten);
        schema.classes.insert("Thing".into(), thing);

        let resolved = resolve_effective_slots_with_provenance(&schema.classes["Thing"], &schema);
        assert_eq!(
            resolved["field"].provenance,
            Provenance::Refined {
                from: "Thing".into(),
                by_slot_usage: true,
            }
        );
        assert_eq!(resolved["field"].provenance.origin_label("Thing"), None);
    }

    #[test]
    fn provenance_variant_keeps_definitions_identical_to_plain_resolution() {
        // The two public entry points are the same walk; their
        // definitions must never diverge.
        let schema = diamond_schema();
        for class in schema.classes.values() {
            let plain = resolve_effective_slots(class, &schema);
            let with_prov = resolve_effective_slots_with_provenance(class, &schema);
            assert_eq!(plain.len(), with_prov.len());
            for (name, def) in &plain {
                assert_eq!(def.name, with_prov[name].definition.name);
            }
        }
    }

    #[test]
    fn effective_cardinality_explicit_bounds_override_flags() {
        // Explicit cardinality fields win over the bool flags: a slot
        // flagged required+multivalued but bounded 0..1 is effectively
        // optional and single-valued.
        let mut slot = SlotDefinition::new("s");
        slot.required = true;
        slot.multivalued = true;
        slot.minimum_cardinality = Some(0);
        slot.maximum_cardinality = Some(1);

        let card = effective_cardinality(&slot);
        assert!(!card.required);
        assert!(!card.multivalued);
        assert_eq!(card.min, Some(0));
        assert_eq!(card.max, Some(1));
    }

    #[test]
    fn effective_cardinality_min_one_unbounded_max_keeps_multivalued_flag() {
        // min: 1 forces required; an absent max defers to the
        // multivalued flag.
        let mut slot = SlotDefinition::new("s");
        slot.multivalued = true;
        slot.minimum_cardinality = Some(1);

        let card = effective_cardinality(&slot);
        assert!(card.required);
        assert!(card.multivalued);
        assert_eq!(card.min, Some(1));
        assert_eq!(card.max, None);
    }

    #[test]
    fn effective_cardinality_max_above_one_forces_multivalued() {
        let mut slot = SlotDefinition::new("s");
        slot.maximum_cardinality = Some(5);

        let card = effective_cardinality(&slot);
        assert!(!card.required);
        assert!(card.multivalued);
        assert_eq!(card.max, Some(5));
    }

    #[test]
    fn effective_cardinality_falls_back_to_flags_when_bounds_absent() {
        let mut slot = SlotDefinition::new("s");
        slot.required = true;

        let card = effective_cardinality(&slot);
        assert!(card.required);
        assert!(!card.multivalued);
        assert_eq!(card.min, None);
        assert_eq!(card.max, None);
    }

    #[test]
    fn effective_cardinality_after_slot_usage_required_preserves_inherited_multivalued() {
        // A slot_usage that only tightens `required` must not disturb
        // the inherited multivalued framing once the resolved slot is
        // viewed through the cardinality lens.
        let mut schema = SchemaDefinition::new("s");
        let mut parent = ClassDefinition::new("Parent");
        let mut tags = SlotDefinition::new("tags");
        tags.multivalued = true;
        parent.attributes.insert("tags".into(), tags);
        schema.classes.insert("Parent".into(), parent);

        let mut child = ClassDefinition::new("Child");
        child.is_a = Some("Parent".into());
        let mut tighten = SlotDefinition::new("tags");
        tighten.required = true;
        child.slot_usage.insert("tags".into(), tighten);
        schema.classes.insert("Child".into(), child);

        let resolved = resolve_effective_slots(&schema.classes["Child"], &schema);
        let card = effective_cardinality(&resolved["tags"]);
        assert!(card.required, "slot_usage required=true applies");
        assert!(card.multivalued, "inherited multivalued is preserved");
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
