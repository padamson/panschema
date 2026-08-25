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

/// The range a slot resolves to *for a specific class*, after that
/// class's `slot_usage` has been applied — LinkML's induced-slot view.
///
/// `slot_usage` can narrow an inherited range three ways, all captured
/// here: replace an inherited `any_of` union with a smaller one,
/// intersect the union down to a single `range` (so the wide inherited
/// union no longer lingers), or set `maximum_cardinality: 0` to say
/// "this class permits no value." Without this view a consumer reading
/// the raw `definition` sees the inherited union even where the class
/// narrowed it (the scalar-`range`-over-`any_of` case leaves both
/// populated on the definition).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InducedRange {
    /// The effective range targets for this class: one entry for a
    /// scalar range, several for an `any_of` union, empty when the
    /// slot is suppressed or names no resolvable range.
    pub ranges: Vec<String>,
    /// `maximum_cardinality: 0` — the class declares the slot but
    /// permits no value. Renderers show "has no value" and draw
    /// no range edge; `ranges` is empty in this case.
    pub suppressed: bool,
}

/// A slot definition paired with where it came from and its induced
/// per-class range. Output of [`resolve_effective_slots_with_provenance`];
/// consumers that don't care about origin use [`resolve_effective_slots`]
/// instead.
#[derive(Debug, Clone)]
pub struct ResolvedSlot {
    pub definition: SlotDefinition,
    pub provenance: Provenance,
    pub induced: InducedRange,
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

/// The class's effective `designates_type` slot name, when it carries
/// one — the slot whose authored value names a record's class.
pub fn designator_slot_of(class: &ClassDefinition, schema: &SchemaDefinition) -> Option<String> {
    designator_in(resolve_effective_slots(class, schema).iter())
}

/// The `designates_type` slot among an already-resolved slot set — the
/// one selection rule, shared by every consumer however it caches its
/// resolution.
pub fn designator_in<'a>(
    slots: impl IntoIterator<Item = (&'a String, &'a SlotDefinition)>,
) -> Option<String> {
    slots
        .into_iter()
        .find(|(_, slot)| slot.designates_type)
        .map(|(name, _)| name.clone())
}

/// Whether `class` satisfies a range naming `target`: the same class, or a
/// descendant of it through `is_a`. Mixins are not walked — a range (or a
/// type designator) names a class an instance is expected to *be*, and
/// `is_a` is the relation that answers that.
pub fn class_satisfies(schema: &SchemaDefinition, class: &str, target: &str) -> bool {
    let mut current = class;
    // Revisiting a class means a malformed `is_a` cycle; stop rather than spin.
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    loop {
        if current == target {
            return true;
        }
        if !seen.insert(current) {
            return false;
        }
        match schema.classes.get(current).and_then(|c| c.is_a.as_deref()) {
            Some(parent) => current = parent,
            None => return false,
        }
    }
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

/// Fill each specializing slot's unset fields from its `is_a` parent
/// chain — LinkML's slot inheritance. Nearer ancestors win; a field the
/// child states is never touched. Runs at load time before
/// [`materialize_default_range`] (an inherited explicit range beats the
/// file's default) and once more after imports merge, so a parent
/// declared in another file contributes to fields still unset. For
/// `range` the ordering holds across files too: a specializing slot's
/// default fill is deferred past the post-merge pass, so a cross-file
/// parent's range always beats the child file's `default_range`.
///
/// Only `Option`- and list-valued metaslots inherit. The boolean
/// metaslots (`required`, `multivalued`, …) do not: the IR cannot
/// distinguish a stated `false` from silence, and stomping an explicit
/// `false` would be worse than not inheriting — recorded as a divergence
/// in `docs/linkml-coverage.md`.
pub fn resolve_slot_inheritance(schema: &mut SchemaDefinition) {
    // Snapshot of every slot definition by name, so fills read stable
    // pre-inheritance ancestors while the live definitions mutate.
    // Top-level slots shadow same-named attributes, matching `find_slot`.
    let mut defs: BTreeMap<String, SlotDefinition> = BTreeMap::new();
    for class in schema.classes.values() {
        for (name, def) in &class.attributes {
            defs.entry(name.clone()).or_insert_with(|| def.clone());
        }
    }
    for (name, def) in &schema.slots {
        defs.insert(name.clone(), def.clone());
    }

    let fill = |own_name: &str, slot: &mut SlotDefinition| {
        let mut seen: Vec<String> = vec![own_name.to_string()];
        let mut next = slot.is_a.clone();
        while let Some(parent_name) = next {
            if seen.contains(&parent_name) {
                break;
            }
            let Some(parent) = defs.get(&parent_name) else {
                break;
            };
            inherit_unset(slot, parent);
            next = parent.is_a.clone();
            seen.push(parent_name);
        }
    };
    for (name, slot) in schema.slots.iter_mut() {
        fill(name, slot);
    }
    for class in schema.classes.values_mut() {
        for (name, attribute) in class.attributes.iter_mut() {
            fill(name, attribute);
        }
    }
}

/// Copy `parent`'s value into `child` for every inheritable metaslot the
/// child leaves unset.
fn inherit_unset(child: &mut SlotDefinition, parent: &SlotDefinition) {
    macro_rules! inherit_opt {
        ($field:ident) => {
            if child.$field.is_none() {
                child.$field = parent.$field.clone();
            }
        };
    }
    inherit_opt!(range);
    inherit_opt!(description);
    inherit_opt!(pattern);
    inherit_opt!(ifabsent);
    inherit_opt!(minimum_cardinality);
    inherit_opt!(maximum_cardinality);
    inherit_opt!(minimum_value);
    inherit_opt!(maximum_value);
    inherit_opt!(inlined);
    inherit_opt!(inlined_as_list);
    if child.any_of.is_empty() && !parent.any_of.is_empty() {
        child.any_of = parent.any_of.clone();
    }
}

/// Fill the schema's `default_range` into its own rangeless slot
/// definitions — top-level `slots:` entries and every class's inline
/// `attributes`. Runs once per schema file at load time, *before* imports
/// merge: LinkML scopes `default_range` to the declaring schema, so each
/// file's default lands on exactly the slots that file declares, and no
/// later consumer — resolver, writer, or validator — needs to know
/// defaults exist.
///
/// A slot that states anything more specific is left alone: an explicit
/// `range`, an `any_of` union whose branches carry ranges, or
/// `maximum_cardinality: 0` (the author saying "no value", where a range
/// would contradict the induced view). An `any_of` whose branches carry
/// only facets — patterns, bounds — constrains values it never types, so
/// the default fills its range like any other rangeless slot.
///
/// A still-rangeless slot with `is_a` is not filled here: its parent may
/// live in a file this pass cannot see, and an inherited range beats any
/// default — LinkML's derivation order is ancestors first, default last.
/// Such a slot instead records this file's default with itself, and
/// [`materialize_deferred_default_range`] completes the fill after the
/// cross-file inheritance pass — from that recorded per-file default, so
/// the scoping is exact whether the slot's chain resolves or not.
pub fn materialize_default_range(schema: &mut SchemaDefinition) {
    let Some(default) = schema.default_range.clone() else {
        return;
    };
    for_each_slot_definition(schema, |slot| {
        if default_range_would_fill(slot) {
            if slot.is_a.is_none() {
                slot.range = Some(default.clone());
            } else {
                slot.annotations
                    .insert(DEFERRED_DEFAULT_KEY.to_string(), default.clone());
            }
        }
    });
}

/// The transient marker [`materialize_default_range`] leaves on a
/// specializing slot it deferred, carrying the declaring file's default.
/// It travels with the definition through the imports merge and is
/// removed by [`materialize_deferred_default_range`] before anything else
/// observes the schema.
const DEFERRED_DEFAULT_KEY: &str = "panschema:deferred_default_range";

/// The post-merge completion of [`materialize_default_range`]: each slot
/// the per-file pass deferred takes its recorded default now, unless the
/// cross-file inheritance pass gave it a range — ancestors first, default
/// last, with each slot's default coming from its own declaring file.
/// Slots that were never deferred (an OWL/Turtle-sourced property with no
/// `rdfs:range`, whose file has no default) are untouched and stay
/// genuinely rangeless.
pub fn materialize_deferred_default_range(schema: &mut SchemaDefinition) {
    for_each_slot_definition(schema, |slot| {
        // `remove` unconditionally, so the transient marker is cleared
        // whatever shape user YAML managed to smuggle under its key.
        let deferred = slot.annotations.remove(DEFERRED_DEFAULT_KEY);
        if let Some(serde_norway::Value::String(default)) = deferred
            && default_range_would_fill(slot)
        {
            slot.range = Some(default);
        }
    });
}

/// One traversal over every slot definition the default-range passes
/// touch — top-level `slots:` and class `attributes:` — so the immediate
/// and deferred passes can never visit different containers.
fn for_each_slot_definition(schema: &mut SchemaDefinition, mut f: impl FnMut(&mut SlotDefinition)) {
    for slot in schema.slots.values_mut() {
        f(slot);
    }
    for class in schema.classes.values_mut() {
        for attribute in class.attributes.values_mut() {
            f(attribute);
        }
    }
}

/// Whether a declared `default_range` would type this slot — the one
/// definition of "rangeless" shared by [`materialize_default_range`] and
/// the untyped-slot diagnostic, so what the loader fills and what the
/// diagnostic reports can never drift apart.
pub fn default_range_would_fill(slot: &SlotDefinition) -> bool {
    slot.range.is_none()
        && !slot.any_of.iter().any(|branch| branch.range.is_some())
        && slot.maximum_cardinality != Some(0)
}

/// Recursive worker for [`resolve_effective_slots`]. `visited` holds the
/// classes currently on the recursion stack so a circular `is_a` or `mixin`
/// chain terminates (silently dropping the would-be-cyclic contribution)
/// rather than overflowing.
///
/// Classes are identified by address rather than by `name`, because a name
/// is not reliably unique: a schema deserialized without the reader's
/// name back-fill leaves every name empty, which under name-keying made
/// each class look like a repeat of the last and silently truncated the
/// `is_a` chain — inherited slots, identifiers included, went missing. The
/// map owns its classes for the whole walk, so their addresses are stable
/// and distinguish them whatever their names say.
fn resolve_slots_walk(
    class: &ClassDefinition,
    schema: &SchemaDefinition,
    visited: &mut BTreeSet<usize>,
) -> BTreeMap<String, ResolvedSlot> {
    let mut slots: BTreeMap<String, ResolvedSlot> = BTreeMap::new();

    // Mark this class as in-progress. If insert returns false, we've
    // already visited this class along the current path — stop.
    if !visited.insert(std::ptr::from_ref(class) as usize) {
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
                    // The parent's induced range carries through — a
                    // narrowing the parent applied via `slot_usage` is
                    // what the child inherits; recomputing from the
                    // definition would resurrect the lingering union.
                    induced: rs.induced,
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
                    induced: rs.induced,
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
                induced: base_induced(def),
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
                    induced: base_induced(def),
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
                target.induced = induced_after_override(&target.definition, override_def);
            }
            // A `slot_usage` with no inherited base acts as the
            // slot's introduction at this class.
            None => {
                slots.insert(
                    name.clone(),
                    ResolvedSlot {
                        induced: base_induced(override_def),
                        definition: override_def.clone(),
                        provenance: Provenance::Direct,
                    },
                );
            }
        }
    }

    // Pop this class on the way out — sibling/cousin paths to it
    // through different ancestors are NOT cycles.
    visited.remove(&(std::ptr::from_ref(class) as usize));
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
    // Enrollment check: every field is named (no `..`), so adding a field
    // to `SlotDefinition` fails to compile here until this merge decides
    // whether `slot_usage` carries it — instead of silently dropping it,
    // which is how `inverse` and `slot_uri` went unenrolled.
    let SlotDefinition {
        name: _,
        description: _,
        deprecated: _,
        aliases: _,
        see_also: _,
        examples: _,
        range: _,
        domain: _,
        ifabsent: _,
        required: _,
        multivalued: _,
        designates_type: _,
        minimum_cardinality: _,
        maximum_cardinality: _,
        pattern: _,
        identifier: _,
        key: _,
        inlined: _,
        inlined_as_list: _,
        slot_uri: _,
        inverse: _,
        is_a: _,
        symmetric: _,
        asymmetric: _,
        reflexive: _,
        irreflexive: _,
        transitive: _,
        minimum_value: _,
        maximum_value: _,
        any_of: _,
        exact_mappings: _,
        close_mappings: _,
        related_mappings: _,
        narrow_mappings: _,
        broad_mappings: _,
        annotations: _,
    } = *source;

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
    merge_opt!(is_a);
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
    if source.designates_type {
        target.designates_type = true;
    }
    // LinkML's two uniqueness forms are mutually exclusive: `identifier` is
    // globally unique, `key` is unique within its container. So an override
    // that sets one clears the other. Without the clear, a class narrowing a
    // shared `identifier` slot to a `key` ends up carrying both, and a
    // consumer asking "are these records scoped to their dataset?" — which
    // reads `key && !identifier` — sees neither form and scopes nothing.
    //
    // This is what lets a schema share one id slot across its record classes
    // and still split reference entities from scoped ones per class, which is
    // the only way to express that split short of splitting the slot in two.
    if source.identifier {
        target.identifier = true;
        target.key = false;
    }
    if source.key {
        target.key = true;
        target.identifier = false;
    }
}

/// The induced range of a slot from its own definition, before any
/// `slot_usage` narrowing — an `any_of` union's member ranges, or the
/// single `range`. `maximum_cardinality: 0` suppresses it (the class
/// permits no value), which empties `ranges`.
fn base_induced(def: &SlotDefinition) -> InducedRange {
    if def.maximum_cardinality == Some(0) {
        return InducedRange {
            ranges: Vec::new(),
            suppressed: true,
        };
    }
    let ranges = if !def.any_of.is_empty() {
        def.any_of.iter().filter_map(|m| m.range.clone()).collect()
    } else {
        def.range.iter().cloned().collect()
    };
    InducedRange {
        ranges,
        suppressed: false,
    }
}

/// The induced range after a class refines a slot via `slot_usage`,
/// per LinkML induced-slot semantics. The decision keys off the
/// *override* (not the merged definition), so the inherited union
/// can't leak through:
/// - `maximum_cardinality: 0` → suppressed, no ranges.
/// - override `any_of` → replaces the inherited union with its members.
/// - override scalar `range` → intersects the inherited union down to
///   that single range (the wide union no longer applies).
/// - override touches neither (e.g. only tightens `required`) → the
///   merged definition's base induced range stands.
fn induced_after_override(merged: &SlotDefinition, override_def: &SlotDefinition) -> InducedRange {
    if merged.maximum_cardinality == Some(0) {
        return InducedRange {
            ranges: Vec::new(),
            suppressed: true,
        };
    }
    let ranges = if !override_def.any_of.is_empty() {
        override_def
            .any_of
            .iter()
            .filter_map(|m| m.range.clone())
            .collect()
    } else if let Some(range) = &override_def.range {
        vec![range.clone()]
    } else {
        return base_induced(merged);
    };
    InducedRange {
        ranges,
        suppressed: false,
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

/// Resolve a slot's effective domain class names. LinkML lets the
/// domain be expressed two ways: the slot's own `domain:`, or — the
/// common case — one or more classes listing the slot in their `slots:`
/// (the computed `domain_of` inverse). A slot can therefore have
/// *several* domains (e.g. `executes` used by both `Analysis` and
/// `Experimentation`). Prefer the slot's explicit `domain:` (a single
/// entry); otherwise return every class (in deterministic `BTreeMap`
/// order) that references the slot. Empty when no class uses it.
pub fn resolve_slot_domains(
    schema: &SchemaDefinition,
    slot_name: &str,
    slot: &SlotDefinition,
) -> Vec<String> {
    if let Some(domain) = &slot.domain {
        return vec![domain.clone()];
    }
    schema
        .classes
        .iter()
        .filter(|(_, c)| c.slots.iter().any(|s| s == slot_name))
        .map(|(name, _)| name.clone())
        .collect()
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

    #[test]
    fn resolve_slot_domain_prefers_explicit_then_class_membership() {
        let mut schema = SchemaDefinition::new("dom");

        // An explicit `domain:` on the slot wins (single entry).
        let mut explicit = SlotDefinition::new("explicit");
        explicit.domain = Some("DeclaredDomain".to_string());
        assert_eq!(
            resolve_slot_domains(&schema, "explicit", &explicit),
            vec!["DeclaredDomain".to_string()]
        );

        // No explicit domain → *every* class listing the slot in
        // `slots:`, in deterministic order.
        let used = SlotDefinition::new("used");
        let mut act = ClassDefinition::new("Act");
        act.slots = vec!["used".to_string()];
        schema.classes.insert("Act".to_string(), act);
        let mut exp = ClassDefinition::new("Experiment");
        exp.slots = vec!["used".to_string()];
        schema.classes.insert("Experiment".to_string(), exp);
        assert_eq!(
            resolve_slot_domains(&schema, "used", &used),
            vec!["Act".to_string(), "Experiment".to_string()]
        );

        // A slot no class references has no resolvable domain.
        let orphan = SlotDefinition::new("orphan");
        assert!(resolve_slot_domains(&schema, "orphan", &orphan).is_empty());
    }

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

    /// LinkML's derivation order for a specializing slot: an inherited
    /// range beats the default, and the default fills only what the whole
    /// chain leaves unset.
    #[test]
    fn a_specializing_slots_range_comes_from_ancestors_first_default_last() {
        let mut schema = SchemaDefinition::new("s");
        schema.default_range = Some("string".to_string());
        let mut parent = SlotDefinition::new("anchors");
        parent.range = Some("uriorcurie".to_string());
        schema.slots.insert("anchors".into(), parent);
        let mut child = SlotDefinition::new("citations");
        child.is_a = Some("anchors".to_string());
        schema.slots.insert("citations".into(), child);
        let mut orphan = SlotDefinition::new("labels");
        orphan.is_a = Some("nothing_known".to_string());
        schema.slots.insert("labels".into(), orphan);

        resolve_slot_inheritance(&mut schema);
        materialize_default_range(&mut schema);
        materialize_deferred_default_range(&mut schema);

        assert_eq!(
            schema.slots["citations"].range.as_deref(),
            Some("uriorcurie"),
            "the parent's range wins over the default"
        );
        assert_eq!(
            schema.slots["labels"].range.as_deref(),
            Some("string"),
            "a chain that yields no range falls back to the default, last"
        );
    }

    /// Materializing `default_range` fills exactly the slots that state
    /// nothing more specific: rangeless top-level `slots:` and class
    /// `attributes` gain the default, while an explicit `range`, an
    /// `any_of` union, and a `maximum_cardinality: 0` "no value" slot are
    /// all left alone. Once materialized, the resolver's induced view
    /// carries the default like any declared range.
    #[test]
    fn materializing_the_default_range_fills_only_slots_that_state_nothing() {
        let mut schema = SchemaDefinition::new("s");
        schema.default_range = Some("string".to_string());
        schema
            .slots
            .insert("title".into(), SlotDefinition::new("title"));
        let mut class = ClassDefinition::new("Item");
        class
            .attributes
            .insert("question".into(), SlotDefinition::new("question"));
        let mut explicit = SlotDefinition::new("count");
        explicit.range = Some("integer".to_string());
        class.attributes.insert("count".into(), explicit);
        let mut union = SlotDefinition::new("either");
        let mut branch_a = SlotDefinition::new("a");
        branch_a.range = Some("string".to_string());
        let mut branch_b = SlotDefinition::new("b");
        branch_b.range = Some("integer".to_string());
        union.any_of = vec![branch_a, branch_b];
        class.attributes.insert("either".into(), union);
        let mut facets = SlotDefinition::new("coded");
        facets.any_of = vec![
            {
                let mut p = SlotDefinition::new("");
                p.pattern = Some("^a".to_string());
                p
            },
            {
                let mut p = SlotDefinition::new("");
                p.pattern = Some("^b".to_string());
                p
            },
        ];
        class.attributes.insert("coded".into(), facets);
        let mut no_value = SlotDefinition::new("legacy");
        no_value.maximum_cardinality = Some(0);
        class.attributes.insert("legacy".into(), no_value);
        schema.classes.insert("Item".into(), class);

        materialize_default_range(&mut schema);

        assert_eq!(
            schema.slots["title"].range.as_deref(),
            Some("string"),
            "a rangeless top-level slot takes the default"
        );
        let item = &schema.classes["Item"];
        assert_eq!(
            item.attributes["question"].range.as_deref(),
            Some("string"),
            "a rangeless attribute takes the default"
        );
        assert_eq!(
            item.attributes["count"].range.as_deref(),
            Some("integer"),
            "an explicit range is untouched"
        );
        assert!(
            item.attributes["either"].range.is_none(),
            "an any_of union whose branches carry ranges does not gain a scalar range"
        );
        assert_eq!(
            item.attributes["coded"].range.as_deref(),
            Some("string"),
            "an any_of whose branches carry only facets constrains values it never \
             types, so the default fills it like any rangeless slot"
        );
        assert!(
            item.attributes["legacy"].range.is_none(),
            "a no-value slot does not gain a range its induced view denies"
        );

        let resolved = resolve_effective_slots_with_provenance(&schema.classes["Item"], &schema);
        assert_eq!(
            resolved["question"].induced.ranges,
            vec!["string".to_string()],
            "the induced view carries the materialized default like any declared range"
        );
    }

    /// A `slot_usage` override can introduce a specialization: `is_a` set
    /// there lands on the effective definition, where the validator
    /// enforces it for that class's records. The scope is the class —
    /// RDF's `rdfs:subPropertyOf` is a global axiom, so a class-scoped
    /// override deliberately does not emit one.
    #[test]
    fn slot_usage_can_declare_a_slot_specialization() {
        let mut schema = SchemaDefinition::new("s");
        schema
            .slots
            .insert("anchors".into(), SlotDefinition::new("anchors"));
        schema
            .slots
            .insert("citations".into(), SlotDefinition::new("citations"));
        let mut class = ClassDefinition::new("Answer");
        class.slots = vec!["anchors".into(), "citations".into()];
        let mut usage = SlotDefinition::new("citations");
        usage.is_a = Some("anchors".to_string());
        class.slot_usage.insert("citations".into(), usage);
        schema.classes.insert("Answer".into(), class);

        let resolved = resolve_effective_slots(&schema.classes["Answer"], &schema);
        assert_eq!(resolved["citations"].is_a.as_deref(), Some("anchors"));
    }

    /// A specializing slot inherits its parent chain's unset metaslots —
    /// nearest ancestor first — while everything the child states stays
    /// its own. A cyclic chain terminates rather than spinning.
    #[test]
    fn a_slot_inherits_unset_fields_from_its_parent_chain() {
        let mut schema = SchemaDefinition::new("s");
        let mut grandparent = SlotDefinition::new("references");
        grandparent.range = Some("integer".to_string());
        grandparent.pattern = Some("^r".to_string());
        schema.slots.insert("references".into(), grandparent);
        let mut parent = SlotDefinition::new("anchors");
        parent.is_a = Some("references".to_string());
        parent.range = Some("string".to_string());
        parent.description = Some("anchor set".to_string());
        schema.slots.insert("anchors".into(), parent);
        let mut child = SlotDefinition::new("citations");
        child.is_a = Some("anchors".to_string());
        child.description = Some("cited subset".to_string());
        schema.slots.insert("citations".into(), child);

        resolve_slot_inheritance(&mut schema);

        let citations = &schema.slots["citations"];
        assert_eq!(
            citations.range.as_deref(),
            Some("string"),
            "the nearest ancestor's range wins"
        );
        assert_eq!(
            citations.description.as_deref(),
            Some("cited subset"),
            "a field the child states is never touched"
        );
        assert_eq!(
            citations.pattern.as_deref(),
            Some("^r"),
            "a field no nearer ancestor states comes from the grandparent"
        );

        let mut cyclic = SchemaDefinition::new("c");
        let mut a = SlotDefinition::new("a");
        a.is_a = Some("b".to_string());
        let mut b = SlotDefinition::new("b");
        b.is_a = Some("a".to_string());
        b.range = Some("string".to_string());
        cyclic.slots.insert("a".into(), a);
        cyclic.slots.insert("b".into(), b);
        resolve_slot_inheritance(&mut cyclic);
        assert_eq!(
            cyclic.slots["a"].range.as_deref(),
            Some("string"),
            "a cycle terminates after one lap, keeping what it gathered"
        );
    }

    /// A parent's `any_of` union inherits onto a child that states none,
    /// and never onto a child that states its own.
    #[test]
    fn a_parent_union_inherits_only_where_the_child_states_none() {
        let mut schema = SchemaDefinition::new("s");
        let mut parent = SlotDefinition::new("value");
        let mut int_branch = SlotDefinition::new("i");
        int_branch.range = Some("integer".to_string());
        let mut bool_branch = SlotDefinition::new("b");
        bool_branch.range = Some("boolean".to_string());
        parent.any_of = vec![int_branch, bool_branch];
        schema.slots.insert("value".into(), parent);
        let mut bare = SlotDefinition::new("bare");
        bare.is_a = Some("value".to_string());
        schema.slots.insert("bare".into(), bare);
        let mut own = SlotDefinition::new("own");
        own.is_a = Some("value".to_string());
        let mut string_branch = SlotDefinition::new("s");
        string_branch.range = Some("string".to_string());
        own.any_of = vec![string_branch];
        schema.slots.insert("own".into(), own);

        resolve_slot_inheritance(&mut schema);

        assert_eq!(
            schema.slots["bare"].any_of.len(),
            2,
            "a union-less child takes the parent's union"
        );
        assert_eq!(
            schema.slots["own"].any_of.len(),
            1,
            "a child's own union is never overwritten"
        );
        assert_eq!(
            schema.slots["own"].any_of[0].range.as_deref(),
            Some("string")
        );
    }

    /// An inherited explicit range beats the file's `default_range`:
    /// inheritance resolves first, so the default fills only slots that
    /// neither state nor inherit a range.
    #[test]
    fn an_inherited_range_beats_the_default_range() {
        let mut schema = SchemaDefinition::new("s");
        schema.default_range = Some("string".to_string());
        let mut parent = SlotDefinition::new("anchors");
        parent.range = Some("integer".to_string());
        schema.slots.insert("anchors".into(), parent);
        let mut child = SlotDefinition::new("citations");
        child.is_a = Some("anchors".to_string());
        schema.slots.insert("citations".into(), child);

        resolve_slot_inheritance(&mut schema);
        materialize_default_range(&mut schema);

        assert_eq!(
            schema.slots["citations"].range.as_deref(),
            Some("integer"),
            "inheritance runs before the default fill"
        );
    }

    /// An attribute-declared child inherits from a top-level parent — the
    /// chain resolves across both declaration sites, matching `find_slot`.
    #[test]
    fn an_attribute_child_inherits_from_a_top_level_parent() {
        let mut schema = SchemaDefinition::new("s");
        let mut parent = SlotDefinition::new("anchors");
        parent.range = Some("integer".to_string());
        schema.slots.insert("anchors".into(), parent);
        let mut class = ClassDefinition::new("Answer");
        let mut child = SlotDefinition::new("citations");
        child.is_a = Some("anchors".to_string());
        class.attributes.insert("citations".into(), child);
        schema.classes.insert("Answer".into(), class);

        resolve_slot_inheritance(&mut schema);

        assert_eq!(
            schema.classes["Answer"].attributes["citations"]
                .range
                .as_deref(),
            Some("integer")
        );
    }

    /// Without a `default_range`, materialization is a no-op — the default
    /// is opt-in, not an implicit string.
    #[test]
    fn no_default_range_leaves_a_rangeless_slot_alone() {
        let mut schema = SchemaDefinition::new("s");
        let mut class = ClassDefinition::new("Item");
        class
            .attributes
            .insert("question".into(), SlotDefinition::new("question"));
        schema.classes.insert("Item".into(), class);

        materialize_default_range(&mut schema);

        assert!(
            schema.classes["Item"].attributes["question"]
                .range
                .is_none()
        );
    }

    /// A slot whose `any_of` branches carry only facets (no `range:`)
    /// resolves the same whether or not the schema declares a
    /// `default_range` — an unrelated schema-level setting must not change
    /// how an untouched slot renders or validates.
    #[test]
    fn a_facet_only_union_resolves_identically_with_and_without_a_default() {
        let build = |default: Option<&str>| {
            let mut schema = SchemaDefinition::new("s");
            schema.default_range = default.map(str::to_string);
            let mut class = ClassDefinition::new("Item");
            let mut score = SlotDefinition::new("score");
            score.range = Some("integer".to_string());
            let mut low = SlotDefinition::new("low");
            low.maximum_value = Some(5.0);
            let mut sentinel = SlotDefinition::new("sentinel");
            sentinel.minimum_value = Some(-1.0);
            score.any_of = vec![low, sentinel];
            class.attributes.insert("score".into(), score);
            schema.classes.insert("Item".into(), class);
            materialize_default_range(&mut schema);
            resolve_effective_slots_with_provenance(&schema.classes["Item"], &schema)["score"]
                .induced
                .clone()
        };

        assert_eq!(
            build(None),
            build(Some("string")),
            "declaring a default the slot never uses must not alter its induced view"
        );
    }

    /// A schema that shares one id slot across its record classes can only
    /// say "this class's records belong to their dataset" per class — i.e.
    /// through `slot_usage`. Dropping `key` there leaves such a schema no
    /// way to split reference entities from scoped ones without splitting
    /// the slot, which is a modeling change forced by the tool.
    #[test]
    fn slot_usage_can_mark_a_shared_id_slot_as_a_key_for_one_class() {
        let mut schema = SchemaDefinition::new("s");
        let mut shared = SlotDefinition::new("id");
        shared.identifier = true;
        schema.slots.insert("id".into(), shared);

        // Reference entity: stays on the shared identifier, global.
        let mut grape = ClassDefinition::new("Grape");
        grape.slots.push("id".into());
        schema.classes.insert("Grape".into(), grape);

        // Scoped record: the same slot, narrowed to a per-container key.
        let mut assessment = ClassDefinition::new("VintageAssessment");
        assessment.slots.push("id".into());
        let mut scoped = SlotDefinition::new("id");
        scoped.key = true;
        assessment.slot_usage.insert("id".into(), scoped);
        schema
            .classes
            .insert("VintageAssessment".into(), assessment);

        let resolved = resolve_effective_slots(&schema.classes["VintageAssessment"], &schema);
        assert!(
            resolved["id"].key,
            "`key: true` set through slot_usage must survive resolution; got: {:?}",
            resolved["id"]
        );
        assert!(
            !resolved["id"].identifier,
            "narrowing a global identifier to a per-container key must clear \
             `identifier` — the two are LinkML's mutually exclusive uniqueness \
             forms, and a slot carrying both scopes as neither; got: {:?}",
            resolved["id"]
        );

        let reference = resolve_effective_slots(&schema.classes["Grape"], &schema);
        assert!(
            reference["id"].identifier && !reference["id"].key,
            "a class that did not override must keep the base slot's identity; got: {:?}",
            reference["id"]
        );
    }

    /// The mirror of the case above: a base `key` narrowed to a global
    /// `identifier` must clear `key`, or the slot again carries both.
    #[test]
    fn slot_usage_promoting_a_key_to_an_identifier_clears_the_key() {
        let mut schema = SchemaDefinition::new("s");
        let mut shared = SlotDefinition::new("id");
        shared.key = true;
        schema.slots.insert("id".into(), shared);

        let mut global = ClassDefinition::new("Grape");
        global.slots.push("id".into());
        let mut promote = SlotDefinition::new("id");
        promote.identifier = true;
        global.slot_usage.insert("id".into(), promote);
        schema.classes.insert("Grape".into(), global);

        let resolved = resolve_effective_slots(&schema.classes["Grape"], &schema);
        assert!(
            resolved["id"].identifier && !resolved["id"].key,
            "promoting a key to an identifier must clear `key`; got: {:?}",
            resolved["id"]
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

    /// Helper: a global slot whose range is an `any_of` union over the
    /// given member class names.
    fn union_slot(name: &str, members: &[&str]) -> SlotDefinition {
        let mut slot = SlotDefinition::new(name);
        slot.any_of = members
            .iter()
            .map(|m| {
                let mut branch = SlotDefinition::new(name);
                branch.range = Some((*m).to_string());
                branch
            })
            .collect();
        slot
    }

    /// A scimantic-shaped fixture: abstract `Act` carries `hasInput` /
    /// `hasOutput` as wide `any_of` unions; subclasses narrow them via
    /// `slot_usage` (single range, smaller union, or `max_cardinality: 0`).
    fn act_facets_schema() -> SchemaDefinition {
        let mut schema = SchemaDefinition::new("acts");
        schema.slots.insert(
            "hasInput".into(),
            union_slot(
                "hasInput",
                &[
                    "Question",
                    "Result",
                    "Dataset",
                    "Annotation",
                    "SourceDocument",
                ],
            ),
        );
        schema.slots.insert(
            "hasOutput".into(),
            union_slot("hasOutput", &["Result", "Dataset", "Annotation"]),
        );

        let mut act = ClassDefinition::new("Act");
        act.slots = vec!["hasInput".into(), "hasOutput".into()];
        schema.classes.insert("Act".into(), act);

        // Helper to add a subclass with slot_usage overrides.
        let mut add_subclass = |name: &str, usage: Vec<(&str, SlotDefinition)>| {
            let mut c = ClassDefinition::new(name);
            c.is_a = Some("Act".into());
            for (slot, def) in usage {
                c.slot_usage.insert(slot.into(), def);
            }
            schema.classes.insert(name.into(), c);
        };

        // Analysis: scalar range narrows each union to one member.
        let mut analysis_in = SlotDefinition::new("hasInput");
        analysis_in.range = Some("Dataset".into());
        let mut analysis_out = SlotDefinition::new("hasOutput");
        analysis_out.range = Some("Result".into());
        add_subclass(
            "Analysis",
            vec![("hasInput", analysis_in), ("hasOutput", analysis_out)],
        );

        // EvidenceExtraction: a smaller any_of replaces the inherited union.
        add_subclass(
            "EvidenceExtraction",
            vec![(
                "hasInput",
                union_slot("hasInput", &["Annotation", "SourceDocument"]),
            )],
        );

        // QuestionFormation: a different narrowed union.
        add_subclass(
            "QuestionFormation",
            vec![("hasInput", union_slot("hasInput", &["Question", "Result"]))],
        );

        // EvidenceAssessment: max_cardinality 0 suppresses the output.
        let mut no_output = SlotDefinition::new("hasOutput");
        no_output.maximum_cardinality = Some(0);
        add_subclass("EvidenceAssessment", vec![("hasOutput", no_output)]);

        // DesignOfExperiment: max_cardinality 0 suppresses the input.
        let mut no_input = SlotDefinition::new("hasInput");
        no_input.maximum_cardinality = Some(0);
        add_subclass("DesignOfExperiment", vec![("hasInput", no_input)]);

        schema
    }

    fn induced_of(schema: &SchemaDefinition, class: &str, slot: &str) -> InducedRange {
        resolve_effective_slots_with_provenance(&schema.classes[class], schema)[slot]
            .induced
            .clone()
    }

    #[test]
    fn induced_base_union_passes_through_unrefined_class() {
        let schema = act_facets_schema();
        // Act itself doesn't narrow — the full union is induced.
        assert_eq!(
            induced_of(&schema, "Act", "hasInput").ranges,
            vec![
                "Question".to_string(),
                "Result".into(),
                "Dataset".into(),
                "Annotation".into(),
                "SourceDocument".into()
            ]
        );
    }

    #[test]
    fn induced_scalar_slot_usage_intersects_inherited_union() {
        let schema = act_facets_schema();
        // The lingering inherited union must NOT survive — the scalar
        // range narrows it to a single member.
        assert_eq!(
            induced_of(&schema, "Analysis", "hasInput").ranges,
            vec!["Dataset".to_string()]
        );
        assert_eq!(
            induced_of(&schema, "Analysis", "hasOutput").ranges,
            vec!["Result".to_string()]
        );
    }

    #[test]
    fn induced_slot_usage_any_of_replaces_inherited_union() {
        let schema = act_facets_schema();
        assert_eq!(
            induced_of(&schema, "EvidenceExtraction", "hasInput").ranges,
            vec!["Annotation".to_string(), "SourceDocument".into()]
        );
        assert_eq!(
            induced_of(&schema, "QuestionFormation", "hasInput").ranges,
            vec!["Question".to_string(), "Result".into()]
        );
    }

    #[test]
    fn induced_max_cardinality_zero_suppresses_without_dropping_slot() {
        let schema = act_facets_schema();

        let assessment = induced_of(&schema, "EvidenceAssessment", "hasOutput");
        assert!(assessment.suppressed, "max_cardinality 0 marks suppressed");
        assert!(
            assessment.ranges.is_empty(),
            "a suppressed slot has no ranges"
        );

        let design = induced_of(&schema, "DesignOfExperiment", "hasInput");
        assert!(design.suppressed);
        assert!(design.ranges.is_empty());

        // The suppressed slot is still part of the class's declared set —
        // suppression hides its value, it doesn't drop the slot.
        let resolved = resolve_effective_slots(&schema.classes["EvidenceAssessment"], &schema);
        assert!(
            resolved.contains_key("hasOutput"),
            "suppressed slot stays in the resolved set"
        );
    }

    #[test]
    fn induced_scalar_range_slot_has_single_member() {
        // A plain scalar-range slot (no any_of, no slot_usage) induces
        // a one-member range list.
        let mut schema = SchemaDefinition::new("s");
        let mut c = ClassDefinition::new("C");
        let mut field = SlotDefinition::new("field");
        field.range = Some("string".into());
        c.attributes.insert("field".into(), field);
        schema.classes.insert("C".into(), c);

        assert_eq!(
            induced_of(&schema, "C", "field").ranges,
            vec!["string".to_string()]
        );
    }

    #[test]
    fn induced_narrowing_inherited_through_a_second_is_a_hop() {
        // Analysis narrows hasInput; a class is_a Analysis without
        // re-refining inherits the narrowed [Dataset], not the union.
        let mut schema = act_facets_schema();
        let mut sub = ClassDefinition::new("SubAnalysis");
        sub.is_a = Some("Analysis".into());
        schema.classes.insert("SubAnalysis".into(), sub);
        assert_eq!(
            induced_of(&schema, "SubAnalysis", "hasInput").ranges,
            vec!["Dataset".to_string()],
            "the parent's narrowing carries through the second is_a hop"
        );
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

    #[test]
    fn inheritance_resolves_even_when_class_names_are_unset() {
        // A schema built by any route that leaves `ClassDefinition.name`
        // empty — deserializing a literal without the reader's name
        // back-fill, or constructing one by hand — must still resolve
        // `is_a`. Keying the cycle guard on the name made every class
        // collide under the empty string, so a parent looked already
        // visited and its slots, including an inherited identifier,
        // silently vanished from the child.
        let mut schema = SchemaDefinition::new("unnamed");
        let mut parent = ClassDefinition::new("");
        parent.attributes.insert("id".to_string(), {
            let mut s = SlotDefinition::new("");
            s.identifier = true;
            s
        });
        let mut child = ClassDefinition::new("");
        child.is_a = Some("Parent".to_string());
        assert_eq!(parent.name, "", "fixture premise: names are unset");
        assert_eq!(child.name, "", "fixture premise: names are unset");
        schema.classes.insert("Parent".to_string(), parent);
        schema.classes.insert("Child".to_string(), child);

        let resolved =
            resolve_effective_slots(schema.classes.get("Child").expect("Child"), &schema);
        assert!(
            resolved.contains_key("id"),
            "the child must inherit the parent's identifier; got: {:?}",
            resolved.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn an_is_a_cycle_still_terminates() {
        // The guard's real job. Two classes naming each other as parent
        // must not recurse forever, with or without names set.
        let mut schema = SchemaDefinition::new("cyclic");
        let mut a = ClassDefinition::new("");
        a.is_a = Some("B".to_string());
        let mut b = ClassDefinition::new("");
        b.is_a = Some("A".to_string());
        schema.classes.insert("A".to_string(), a);
        schema.classes.insert("B".to_string(), b);
        let _ = resolve_effective_slots(schema.classes.get("A").expect("A"), &schema);
    }
}
