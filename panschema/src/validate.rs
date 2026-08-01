//! Native LinkML instance-data validator.
//!
//! Checks a LinkML **instance-data** file (an A-box — a `tree_root` container
//! of records) against its schema's constraints and reports every violation.
//! Per ADR-008 it validates the **instance model** ([`InstanceSet`]), not any
//! on-disk format: [`validate_instances`] is the format-agnostic core, and a
//! thin per-format adapter ([`validate_instance_data`] for a LinkML file) reads
//! the data into the model first. The model's typed, slot-keyed `slot_values`
//! carry the untouched value kinds later slices need for `pattern`/bounds/enum
//! checks — fidelity the display-oriented `literals` (stringified) and the
//! still-incomplete JSON-Schema projection can't provide.

use crate::instances::{InstanceSet, InstanceValue, ScalarValue, scalar_to_display};
use crate::linkml::{
    EnumDefinition, RuleConditions, SchemaDefinition, SlotCondition, ValuePresence,
};
use crate::linkml_resolve::{effective_cardinality, resolve_effective_slots_with_provenance};
use regex::Regex;
use serde_norway::Value;
use std::fmt;

/// A single way the data fails to conform to the schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The offending record's identifier, or a positional label when it has no
    /// identifier (e.g. ``Wine#2``).
    pub record: String,
    /// What is wrong, as a ready-to-print clause.
    pub detail: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "instance `{}`: {}", self.record, self.detail)
    }
}

/// Validate an already-read [`InstanceSet`] against `schema`, returning every
/// violation (empty when the data conforms). This is the **format-agnostic
/// core** (ADR-008): it consumes the instance model, so any reader's
/// `InstanceSet` — LinkML data today, OWL individuals or JSON later — validates
/// through it. Deterministic: violations are ordered by record (the set is
/// sorted by id), then by slot, then the reference-integrity violations.
///
/// Slice 1 checks: a required slot absent from a record, and a reference whose
/// target names no record in the set.
pub fn validate_instances(schema: &SchemaDefinition, set: &InstanceSet) -> Vec<Violation> {
    let mut out = Vec::new();
    // Each record's class, for resolving what a reference actually points at.
    let class_of: std::collections::BTreeMap<&str, &str> = set
        .instances
        .iter()
        .filter_map(|i| i.types.first().map(|t| (i.id.as_str(), t.as_str())))
        .collect();

    for inst in &set.instances {
        // A record's class is the collection slot's range that produced it.
        let Some(class_name) = inst.types.first() else {
            continue;
        };
        let Some(class) = schema.classes.get(class_name) else {
            continue;
        };
        for (slot_name, rs) in &resolve_effective_slots_with_provenance(class, schema) {
            let slot = &rs.definition;
            // A union slot has several range targets and no scalar `range:`.
            let ranges = &rs.induced.ranges;
            let card = effective_cardinality(slot);
            let count = inst
                .slot_values
                .iter()
                .find(|sv| &sv.slot == slot_name)
                .map_or(0, |sv| sv.values.len());
            let mut push = |detail: String| {
                out.push(Violation {
                    record: inst.id.clone(),
                    detail,
                })
            };

            if count == 0 {
                if card.required {
                    push(format!(
                        "required slot `{slot_name}` (class `{class_name}`) is absent"
                    ));
                }
                // No values to size-check.
                continue;
            }
            if !card.multivalued && count > 1 {
                push(format!(
                    "single-valued slot `{slot_name}` (class `{class_name}`) has {count} values"
                ));
            }
            if let Some(min) = card.min
                && (count as u32) < min
            {
                push(format!(
                    "slot `{slot_name}` (class `{class_name}`) has {count} value(s), fewer than its minimum of {min}"
                ));
            }
            if let Some(max) = card.max
                && (count as u32) > max
            {
                push(format!(
                    "slot `{slot_name}` (class `{class_name}`) has {count} value(s), exceeding its maximum of {max}"
                ));
            }

            // Per-value constraints: enum membership, numeric bounds, pattern.
            let range_enum = slot
                .range
                .as_deref()
                .and_then(|r| schema.enums.get(r).map(|e| (r, e)));
            let has_bound = slot.minimum_value.is_some() || slot.maximum_value.is_some();
            // Compile the slot's pattern once (not per value); an invalid regex
            // in the schema is reported here rather than crashing the validator.
            let pattern = match slot.pattern.as_deref() {
                Some(p) => match Regex::new(p) {
                    Ok(re) => Some(re),
                    Err(_) => {
                        push(format!(
                            "slot `{slot_name}` (class `{class_name}`) has an invalid pattern `{p}`"
                        ));
                        None
                    }
                },
                None => None,
            };
            for value in inst
                .slot_values
                .iter()
                .find(|sv| &sv.slot == slot_name)
                .map(|sv| sv.values.as_slice())
                .unwrap_or_default()
            {
                let scalar = match value {
                    InstanceValue::Scalar(s) => s,
                    // A value the reader couldn't fit to the slot's range kind
                    // (an object where a scalar is declared, or a non-reference
                    // scalar where a class is) — a range-kind mismatch.
                    InstanceValue::Unexpected(kind) => {
                        let range = if ranges.is_empty() {
                            slot.range.as_deref().unwrap_or("?").to_string()
                        } else {
                            ranges.join(" or ")
                        };
                        push(format!(
                            "slot `{slot_name}` (class `{class_name}`) has {kind} value, which isn't valid for its range `{range}`"
                        ));
                        continue;
                    }
                    // Existence is the integrity pass's job; what this checks
                    // is whether the target's class satisfies one branch of a
                    // union range. A target that names no record is skipped so
                    // one problem yields one report.
                    InstanceValue::Reference(target) => {
                        if ranges.len() > 1
                            && let Some(actual) = class_of.get(target.as_str())
                            && !ranges.iter().any(|r| class_satisfies(schema, actual, r))
                        {
                            push(format!(
                                "slot `{slot_name}` (class `{class_name}`) references `{target}`, \
                                 a `{actual}`, which is none of `{}`",
                                ranges.join("`, `")
                            ));
                        }
                        continue;
                    }
                };
                if let Some((enum_name, enum_def)) = range_enum
                    && !enum_permits(enum_def, scalar)
                {
                    let shown = scalar_to_display(scalar);
                    push(format!(
                        "slot `{slot_name}` (class `{class_name}`) value `{shown}` is not a permissible value of enum `{enum_name}`"
                    ));
                }
                // Pattern: partial match (unanchored `find`), matching the
                // semantics panschema's SHACL `sh:pattern` and Postgres `~`
                // projections use.
                if let Some(re) = &pattern
                    && let ScalarValue::String(s) = scalar
                    && !re.is_match(s)
                {
                    push(format!(
                        "slot `{slot_name}` (class `{class_name}`) value `{s}` does not match pattern `{}`",
                        slot.pattern.as_deref().unwrap_or_default()
                    ));
                }
                if has_bound {
                    match numeric(scalar) {
                        Some(n) => {
                            if let Some(min) = slot.minimum_value
                                && n < min
                            {
                                push(format!(
                                    "slot `{slot_name}` (class `{class_name}`) value {n} is below its minimum of {min}"
                                ));
                            }
                            if let Some(max) = slot.maximum_value
                                && n > max
                            {
                                push(format!(
                                    "slot `{slot_name}` (class `{class_name}`) value {n} is above its maximum of {max}"
                                ));
                            }
                        }
                        None => push(format!(
                            "slot `{slot_name}` (class `{class_name}`) value `{}` is not numeric, but the slot declares a numeric bound",
                            scalar_to_display(scalar)
                        )),
                    }
                }
            }
        }

        // Conditional constraints: a rule whose precondition holds imposes its
        // postcondition on this record. Rules are read off the class directly,
        // as every other projection of `rules` does.
        for (i, rule) in class.rules.iter().enumerate() {
            let applies = rule
                .preconditions
                .as_ref()
                .is_none_or(|pre| conditions_hold(pre, inst));
            if !applies {
                continue;
            }
            let Some(post) = &rule.postconditions else {
                continue;
            };
            let label = rule.title.clone().unwrap_or_else(|| format!("#{}", i + 1));
            for (slot_name, cond) in &post.slot_conditions {
                let values = slot_values(inst, slot_name);
                if let Some(reason) = slot_condition_failure(cond, values) {
                    out.push(Violation {
                        record: inst.id.clone(),
                        detail: format!(
                            "rule `{label}` (class `{class_name}`) applies, but slot \
                             `{slot_name}` {reason}"
                        ),
                    });
                }
            }
            if !post.any_of.is_empty() && !post.any_of.iter().any(|alt| conditions_hold(alt, inst))
            {
                out.push(Violation {
                    record: inst.id.clone(),
                    detail: format!(
                        "rule `{label}` (class `{class_name}`) applies, but the record \
                         satisfies none of its postcondition alternatives"
                    ),
                });
            }
        }
    }

    // Cross-record reference integrity: a typed reference to an id no record
    // in the set defines.
    for d in crate::diagnostics::dangling_instance_references(set) {
        out.push(Violation {
            record: d.referrer.clone(),
            detail: d.detail(),
        });
    }

    // A field the class never declared. Not dropped by the reader — it renders
    // and emits as a property minted in the schema's own namespace — so an
    // unreported one lets a typo invent an ontology term.
    for u in &set.undeclared_fields {
        out.push(Violation {
            record: u.record.clone(),
            detail: format!(
                "field `{}` is not declared by class `{}`; it renders and emits as an \
                 undeclared property",
                u.field, u.class
            ),
        });
    }

    // Identifier uniqueness: an id claimed by more than one record.
    for id in &set.duplicate_ids {
        out.push(Violation {
            record: id.clone(),
            detail: format!("identifier `{id}` is used by more than one record"),
        });
    }

    out
}

/// Read a LinkML instance-data tree into the instance model and validate it —
/// the per-format adapter over [`validate_instances`] (ADR-008). A data file
/// that isn't a container mapping is a single structural violation rather than
/// a panic; anything well-formed becomes an [`InstanceSet`] and validates
/// through the format-agnostic core.
pub fn validate_instance_data(schema: &SchemaDefinition, data: &Value) -> Vec<Violation> {
    if data.as_mapping().is_none() {
        return vec![Violation {
            record: "(root)".to_string(),
            detail: "instance data must be a mapping (a tree_root container object)".to_string(),
        }];
    }
    let set = InstanceSet::from_linkml_data(schema, data);
    validate_instances(schema, &set)
}

/// A record's values for `slot`, or an empty slice when it has none.
fn slot_values<'a>(inst: &'a crate::instances::Instance, slot: &str) -> &'a [InstanceValue] {
    inst.slot_values
        .iter()
        .find(|sv| sv.slot == slot)
        .map(|sv| sv.values.as_slice())
        .unwrap_or_default()
}

/// Whether a rule condition holds for a record: every entry in its
/// `slot_conditions` must hold, and — when present — at least one `any_of`
/// alternative must too.
fn conditions_hold(cond: &RuleConditions, inst: &crate::instances::Instance) -> bool {
    if !cond.any_of.is_empty() && !cond.any_of.iter().any(|alt| conditions_hold(alt, inst)) {
        return false;
    }
    cond.slot_conditions
        .iter()
        .all(|(slot, sc)| slot_condition_failure(sc, slot_values(inst, slot)).is_none())
}

/// Why `cond` does not hold for a slot's `values`, phrased as a clause that
/// completes "slot `x` …", or `None` when it holds.
///
/// One function serves both halves of a rule: a precondition only asks
/// whether this is `None`, while a postcondition turns the reason into the
/// violation's text. A condition's `range` is a type assertion rather than a
/// value test and is not evaluated here — the slot's own declared range is
/// already checked for every record.
fn slot_condition_failure(cond: &SlotCondition, values: &[InstanceValue]) -> Option<String> {
    if cond.required && values.is_empty() {
        return Some("is required but absent".to_string());
    }
    match cond.value_presence {
        Some(ValuePresence::Present) if values.is_empty() => {
            return Some("must have a value but is absent".to_string());
        }
        Some(ValuePresence::Absent) if !values.is_empty() => {
            return Some("must be absent but has a value".to_string());
        }
        _ => {}
    }
    if let Some(min) = cond.minimum_cardinality
        && (values.len() as u32) < min
    {
        return Some(format!(
            "has {} value(s), fewer than the required minimum of {min}",
            values.len()
        ));
    }
    if let Some(max) = cond.maximum_cardinality
        && (values.len() as u32) > max
    {
        return Some(format!(
            "has {} value(s), more than the permitted maximum of {max}",
            values.len()
        ));
    }
    if !cond.any_of.is_empty()
        && !cond
            .any_of
            .iter()
            .any(|alt| slot_condition_failure(alt, values).is_none())
    {
        return Some("satisfies none of the permitted alternatives".to_string());
    }
    for value in values {
        let InstanceValue::Scalar(scalar) = value else {
            continue;
        };
        let shown = scalar_to_display(scalar);
        if let Some(want) = &cond.equals_string
            && &shown != want
        {
            return Some(format!("is `{shown}`, but must equal `{want}`"));
        }
        if let Some(want) = cond.equals_number
            && numeric(scalar) != Some(want)
        {
            return Some(format!("is `{shown}`, but must equal {want}"));
        }
        if cond.minimum_value.is_some() || cond.maximum_value.is_some() {
            let Some(n) = numeric(scalar) else {
                return Some(format!(
                    "value `{shown}` is not numeric, but a bound is required"
                ));
            };
            if let Some(min) = cond.minimum_value
                && n < min
            {
                return Some(format!("value {n} is below the required minimum of {min}"));
            }
            if let Some(max) = cond.maximum_value
                && n > max
            {
                return Some(format!("value {n} is above the required maximum of {max}"));
            }
        }
        if let Some(p) = &cond.pattern {
            match Regex::new(p) {
                Ok(re) => {
                    if let ScalarValue::String(text) = scalar
                        && !re.is_match(text)
                    {
                        return Some(format!(
                            "value `{text}` does not match required pattern `{p}`"
                        ));
                    }
                }
                Err(_) => return Some(format!("is constrained by an invalid pattern `{p}`")),
            }
        }
    }
    None
}

/// Whether `class` satisfies a range naming `target`: the same class, or a
/// descendant of it through `is_a`. Mixins are not walked — a union branch
/// names a class an instance is expected to *be*, and `is_a` is the relation
/// that answers that.
fn class_satisfies(schema: &SchemaDefinition, class: &str, target: &str) -> bool {
    let mut current = class;
    let mut hops = 0;
    loop {
        if current == target {
            return true;
        }
        // Bounded so a malformed `is_a` cycle cannot spin here.
        hops += 1;
        if hops > schema.classes.len() {
            return false;
        }
        match schema.classes.get(current).and_then(|c| c.is_a.as_deref()) {
            Some(parent) => current = parent,
            None => return false,
        }
    }
}

/// Whether `scalar`'s string form is one of the enum's permissible values —
/// matched against either the value key or its `text`.
fn enum_permits(enum_def: &EnumDefinition, scalar: &ScalarValue) -> bool {
    let value = scalar_to_display(scalar);
    enum_def.permissible_values.contains_key(&value)
        || enum_def
            .permissible_values
            .values()
            .any(|pv| pv.text == value)
}

/// The numeric value of a scalar for bound-checking, or `None` for a
/// non-numeric scalar (a string/bool where a bound was declared).
fn numeric(scalar: &ScalarValue) -> Option<f64> {
    match scalar {
        ScalarValue::Integer(i) => Some(*i as f64),
        ScalarValue::Float(f) => Some(*f),
        ScalarValue::String(_) | ScalarValue::Boolean(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = "\
name: WineCatalog
default_range: string
classes:
  WineCatalog:
    tree_root: true
    attributes:
      wines:
        range: Wine
        multivalued: true
      wineries:
        range: Winery
        multivalued: true
  Wine:
    attributes:
      id:
        identifier: true
      name:
        required: true
      produced_by:
        range: Winery
  Winery:
    attributes:
      id:
        identifier: true
      name:
        required: true
";

    fn schema() -> SchemaDefinition {
        serde_norway::from_str(SCHEMA).expect("parse schema")
    }

    fn data(yaml: &str) -> Value {
        serde_norway::from_str(yaml).expect("parse data")
    }

    #[test]
    fn conforming_data_has_no_violations() {
        let d = data(
            "\
wines:
  - id: chateauMorgon
    name: Château Morgon
    produced_by: morgonEstate
wineries:
  - id: morgonEstate
    name: Morgon Estate
",
        );
        assert!(validate_instance_data(&schema(), &d).is_empty());
    }

    #[test]
    fn duplicate_identifier_across_records_is_a_violation() {
        // Three wines claim the same id; the reader dedupes them for display, so
        // the validator reads the collision from `duplicate_ids`. The clashing
        // id is reported once, however many records claim it.
        let d = data(
            "wines:\n  - id: w1\n    name: A\n  - id: w1\n    name: B\n  - id: w1\n    name: C\n",
        );
        let v = validate_instance_data(&schema(), &d);
        assert_eq!(v.len(), 1, "one duplicated identifier, reported once");
        assert_eq!(v[0].record, "w1");
        assert!(
            v[0].detail.contains("used by more than one record"),
            "got: {}",
            v[0].detail
        );
    }

    #[test]
    fn a_field_the_class_does_not_declare_is_a_violation() {
        // An undeclared field is not dropped: it renders in the docs and is
        // emitted as an RDF property minted in the schema's own namespace. So a
        // typo invents an ontology property, and the slot actually meant stays
        // absent. Report it rather than letting the writers assert it.
        let d = data("wines:\n  - id: w1\n    name: Morgon\n    colour: red\n");
        let v = validate_instance_data(&schema(), &d);
        assert_eq!(
            v.len(),
            1,
            "one undeclared field, reported once; got: {v:?}"
        );
        assert_eq!(v[0].record, "w1");
        assert!(
            v[0].detail.contains("colour") && v[0].detail.contains("Wine"),
            "the violation must name the field and the class; got: {}",
            v[0].detail
        );
    }

    #[test]
    fn an_inlined_object_sharing_a_top_level_records_id_is_not_a_duplicate() {
        // The same winery is inlined in a wine and listed as a top-level record;
        // that's one entity referenced two ways, not two records — no violation.
        let d = data(
            "\
wines:
  - id: w1
    name: A
    produced_by:
      id: morgonEstate
      name: Morgon Estate
wineries:
  - id: morgonEstate
    name: Morgon Estate
",
        );
        assert!(validate_instance_data(&schema(), &d).is_empty());
    }

    #[test]
    fn missing_required_slot_is_a_violation_naming_record_and_slot() {
        // The wine omits its required `name`.
        let d = data(
            "\
wines:
  - id: chateauMorgon
    produced_by: morgonEstate
wineries:
  - id: morgonEstate
    name: Morgon Estate
",
        );
        let violations = validate_instance_data(&schema(), &d);
        assert_eq!(violations.len(), 1, "one missing required slot");
        assert_eq!(violations[0].record, "chateauMorgon");
        assert!(
            violations[0].detail.contains("name") && violations[0].detail.contains("Wine"),
            "detail names the missing slot and class; got: {}",
            violations[0].detail
        );
    }

    #[test]
    fn dangling_reference_is_a_violation() {
        let d = data(
            "\
wines:
  - id: chateauMorgon
    name: Château Morgon
    produced_by: ghostWinery
wineries:
  - id: morgonEstate
    name: Morgon Estate
",
        );
        let violations = validate_instance_data(&schema(), &d);
        assert_eq!(violations.len(), 1, "one dangling reference");
        assert_eq!(violations[0].record, "chateauMorgon");
        assert!(violations[0].detail.contains("ghostWinery"));
    }

    #[test]
    fn identifier_supplied_as_map_key_satisfies_the_identifier_slot() {
        // wineries as an identifier-keyed mapping: the id isn't a field, but
        // the required identifier slot is satisfied by the key.
        let d = data(
            "\
wineries:
  morgonEstate:
    name: Morgon Estate
",
        );
        assert!(
            validate_instance_data(&schema(), &d).is_empty(),
            "the map key supplies the identifier; name is present"
        );
    }

    #[test]
    fn optional_slot_absent_is_not_a_violation() {
        // `produced_by` is optional; a wine without it still conforms — an
        // absent optional slot must not be flagged like a required one.
        let d = data("wines:\n  - id: soloWine\n    name: Solo\n");
        assert!(validate_instance_data(&schema(), &d).is_empty());
    }

    #[test]
    fn missing_required_slot_in_identifier_keyed_collection_is_flagged() {
        // wineries as an identifier-keyed mapping: `badWinery` supplies its id
        // via the key but omits the required `name`, which must still be caught.
        let d = data("wineries:\n  badWinery: {}\n");
        let violations = validate_instance_data(&schema(), &d);
        assert_eq!(violations.len(), 1, "the required name is missing");
        assert_eq!(violations[0].record, "badWinery");
        assert!(
            violations[0].detail.contains("name"),
            "detail names the missing slot; got: {}",
            violations[0].detail
        );
    }

    #[test]
    fn non_mapping_data_is_one_structural_violation() {
        let d = data("- just\n- a\n- list\n");
        let violations = validate_instance_data(&schema(), &d);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].detail.contains("must be a mapping"));
    }

    /// A schema exercising each cardinality bound: a single-valued slot and a
    /// multivalued slot bounded `2..3`.
    const CARD_SCHEMA: &str = "\
name: C
default_range: string
classes:
  Root:
    tree_root: true
    attributes:
      items:
        range: Item
        multivalued: true
  Item:
    attributes:
      id:
        identifier: true
      color:
        range: string
      tags:
        range: string
        multivalued: true
        minimum_cardinality: 2
        maximum_cardinality: 3
";

    fn card_schema() -> SchemaDefinition {
        serde_norway::from_str(CARD_SCHEMA).expect("parse card schema")
    }

    #[test]
    fn cardinality_bounds_conform() {
        // `tags` at exactly its maximum of 3 conforms — pins the `>` boundary
        // (a count equal to the max must not be flagged as exceeding it).
        let d = data("items:\n  - id: a\n    color: red\n    tags: [x, y, z]\n");
        assert!(
            validate_instances(
                &card_schema(),
                &InstanceSet::from_linkml_data(&card_schema(), &d)
            )
            .is_empty()
        );
    }

    #[test]
    fn single_valued_slot_given_a_list_is_a_violation() {
        let d = data("items:\n  - id: a\n    color: [red, blue]\n    tags: [x, y]\n");
        let v = validate_instance_data(&card_schema(), &d);
        assert_eq!(v.len(), 1, "color is single-valued");
        assert!(
            v[0].detail.contains("single-valued") && v[0].detail.contains("color"),
            "got: {}",
            v[0].detail
        );
    }

    #[test]
    fn multivalued_below_minimum_is_a_violation() {
        let d = data("items:\n  - id: a\n    tags: [x]\n");
        let v = validate_instance_data(&card_schema(), &d);
        assert_eq!(v.len(), 1, "tags has one value, minimum is two");
        assert!(
            v[0].detail.contains("fewer than its minimum") && v[0].detail.contains("tags"),
            "got: {}",
            v[0].detail
        );
    }

    #[test]
    fn multivalued_above_maximum_is_a_violation() {
        let d = data("items:\n  - id: a\n    tags: [w, x, y, z]\n");
        let v = validate_instance_data(&card_schema(), &d);
        assert_eq!(v.len(), 1, "tags has four values, maximum is three");
        assert!(
            v[0].detail.contains("exceeding its maximum") && v[0].detail.contains("tags"),
            "got: {}",
            v[0].detail
        );
    }

    /// An enum-ranged slot and a `0.0..1.0`-bounded numeric slot.
    const VALUE_SCHEMA: &str = "\
name: V
default_range: string
classes:
  Root:
    tree_root: true
    attributes:
      items:
        range: Item
        multivalued: true
  Item:
    attributes:
      id:
        identifier: true
      color:
        range: ColorEnum
      strength:
        range: float
        minimum_value: 0.0
        maximum_value: 1.0
      level:
        range: float
        minimum_value: 1.0
      code:
        range: string
        pattern: \"^[A-Z]{3}$\"
enums:
  ColorEnum:
    permissible_values:
      red:
      white:
";

    fn value_schema() -> SchemaDefinition {
        serde_norway::from_str(VALUE_SCHEMA).expect("parse value schema")
    }

    fn value_violations(yaml: &str) -> Vec<Violation> {
        validate_instance_data(&value_schema(), &data(yaml))
    }

    #[test]
    fn numeric_values_exactly_on_the_bounds_conform() {
        // strength at exactly its minimum (0.0) and exactly its maximum (1.0)
        // both conform — pins the `<`/`>` boundaries.
        assert!(value_violations("items:\n  - id: lo\n    strength: 0.0\n").is_empty());
        assert!(value_violations("items:\n  - id: hi\n    strength: 1.0\n").is_empty());
    }

    #[test]
    fn single_bounded_slot_below_its_only_minimum_is_a_violation() {
        // `level` declares only a minimum — pins that either bound alone
        // engages the numeric checks.
        let v = value_violations("items:\n  - id: a\n    level: 0.5\n");
        assert_eq!(v.len(), 1);
        assert!(
            v[0].detail.contains("below its minimum") && v[0].detail.contains("level"),
            "got: {}",
            v[0].detail
        );
    }

    #[test]
    fn value_matching_the_pattern_conforms() {
        // `code` must match `^[A-Z]{3}$`.
        assert!(value_violations("items:\n  - id: a\n    code: ABC\n").is_empty());
    }

    #[test]
    fn value_not_matching_the_pattern_is_a_violation() {
        let v = value_violations("items:\n  - id: a\n    code: abcd\n");
        assert_eq!(v.len(), 1);
        assert!(
            v[0].detail.contains("does not match pattern") && v[0].detail.contains("code"),
            "got: {}",
            v[0].detail
        );
    }

    #[test]
    fn object_where_a_scalar_range_is_declared_is_a_range_kind_violation() {
        // `code` has range `string`; an object there is the wrong kind.
        let v = value_violations("items:\n  - id: a\n    code:\n      nested: x\n");
        assert_eq!(v.len(), 1);
        assert!(
            v[0].detail.contains("an object")
                && v[0].detail.contains("code")
                && v[0].detail.contains("range `string`"),
            "got: {}",
            v[0].detail
        );
    }

    #[test]
    fn non_reference_scalar_where_a_class_range_is_declared_is_a_range_kind_violation() {
        // `produced_by` has range `Winery` (a class); a bare number can't be a
        // reference to one.
        let d = data("wines:\n  - id: w1\n    name: W\n    produced_by: 42\n");
        let v = validate_instance_data(&schema(), &d);
        assert_eq!(v.len(), 1);
        assert!(
            v[0].detail.contains("a number")
                && v[0].detail.contains("produced_by")
                && v[0].detail.contains("range `Winery`"),
            "got: {}",
            v[0].detail
        );

        // A boolean at the same class-ranged slot names its kind distinctly.
        let d = data("wines:\n  - id: w2\n    name: W\n    produced_by: true\n");
        let v = validate_instance_data(&schema(), &d);
        assert_eq!(v.len(), 1);
        assert!(v[0].detail.contains("a boolean"), "got: {}", v[0].detail);
    }

    #[test]
    fn invalid_pattern_in_the_schema_is_reported_not_panicked() {
        // `[` is an unterminated character class — the validator reports it
        // rather than crashing when compiling the regex.
        let schema: SchemaDefinition = serde_norway::from_str(
            "name: P\ndefault_range: string\nclasses:\n  Root:\n    tree_root: true\n    attributes:\n      items:\n        range: Item\n        multivalued: true\n  Item:\n    attributes:\n      id:\n        identifier: true\n      code:\n        range: string\n        pattern: \"[\"\n",
        )
        .expect("parse schema");
        let v = validate_instance_data(&schema, &data("items:\n  - id: a\n    code: x\n"));
        assert_eq!(v.len(), 1);
        assert!(
            v[0].detail.contains("invalid pattern"),
            "got: {}",
            v[0].detail
        );
    }

    #[test]
    fn enum_and_bounds_conform() {
        assert!(
            value_violations("items:\n  - id: a\n    color: red\n    strength: 0.5\n").is_empty()
        );
    }

    #[test]
    fn value_outside_enum_is_a_violation() {
        let v = value_violations("items:\n  - id: a\n    color: blue\n");
        assert_eq!(v.len(), 1);
        assert!(
            v[0].detail
                .contains("permissible value of enum `ColorEnum`")
                && v[0].detail.contains("blue"),
            "got: {}",
            v[0].detail
        );
    }

    #[test]
    fn numeric_below_minimum_is_a_violation() {
        let v = value_violations("items:\n  - id: a\n    strength: -0.5\n");
        assert_eq!(v.len(), 1);
        assert!(
            v[0].detail.contains("below its minimum"),
            "got: {}",
            v[0].detail
        );
    }

    #[test]
    fn numeric_above_maximum_is_a_violation() {
        let v = value_violations("items:\n  - id: a\n    strength: 1.5\n");
        assert_eq!(v.len(), 1);
        assert!(
            v[0].detail.contains("above its maximum"),
            "got: {}",
            v[0].detail
        );
    }

    #[test]
    fn non_numeric_value_at_a_bounded_slot_is_reported_not_panicked() {
        let v = value_violations("items:\n  - id: a\n    strength: high\n");
        assert_eq!(v.len(), 1);
        assert!(v[0].detail.contains("not numeric"), "got: {}", v[0].detail);
    }

    /// Conditional-requirement rules, in the shape a consumer schema actually
    /// writes them: a precondition matching one slot's value makes other slots
    /// required. Mirrors a deployment catalogue where an `actual` deployment
    /// must name its environment and provider, while a `planned` one need not.
    const RULE_SCHEMA: &str = "
id: https://example.org/rules
name: rules
default_range: string
enums:
  DeploymentStatus:
    permissible_values:
      planned:
      actual:
      possible:
classes:
  Catalog:
    tree_root: true
    attributes:
      deployments:
        range: Deployment
        multivalued: true
      reviews:
        range: Review
        multivalued: true
  Deployment:
    attributes:
      id:
        identifier: true
      status:
        range: DeploymentStatus
      in_environment:
        range: string
      on_provider:
        range: string
    rules:
      - title: actual deployments are bound
        preconditions:
          slot_conditions:
            status:
              equals_string: actual
        postconditions:
          slot_conditions:
            in_environment:
              required: true
            on_provider:
              required: true
  Review:
    attributes:
      id:
        identifier: true
      verdict:
        range: string
      approved_by:
        range: string
      score:
        range: integer
    rules:
      - preconditions:
          slot_conditions:
            verdict:
              any_of:
                - equals_string: approved
                - equals_string: rejected
        postconditions:
          slot_conditions:
            approved_by:
              value_presence: PRESENT
            score:
              minimum_value: 1
              maximum_value: 5
";

    fn rule_schema() -> SchemaDefinition {
        serde_norway::from_str(RULE_SCHEMA).expect("parse rule schema")
    }

    fn rule_violations(yaml: &str) -> Vec<Violation> {
        validate_instance_data(&rule_schema(), &data(yaml))
    }

    #[test]
    fn a_record_failing_a_rules_postcondition_is_a_violation() {
        let v = rule_violations("deployments:\n  - id: d1\n    status: actual\n");
        assert_eq!(
            v.len(),
            2,
            "an actual deployment missing both bindings violates the rule twice; got: {v:?}"
        );
        assert!(
            v.iter().any(|x| x.detail.contains("in_environment"))
                && v.iter().any(|x| x.detail.contains("on_provider")),
            "both governed slots should be named; got: {v:?}"
        );
        assert!(
            v.iter().all(|x| x.record == "d1"),
            "violations belong to the offending record; got: {v:?}"
        );
    }

    #[test]
    fn a_record_satisfying_a_rules_postcondition_conforms() {
        let v = rule_violations(
            "deployments:\n  - id: d1\n    status: actual\n    in_environment: prod\n    on_provider: aws\n",
        );
        assert!(v.is_empty(), "the rule is satisfied; got: {v:?}");
    }

    /// A slot whose range is an `any_of` class union, in the shape a
    /// provenance layer writes it: `qualifies` accepts a Claim or a Method,
    /// and `Hypothesis is_a Claim`.
    const UNION_SCHEMA: &str = "
id: https://example.org/union
name: union
default_range: string
slots:
  qualifies:
    any_of:
      - range: Claim
      - range: Method
classes:
  Root:
    tree_root: true
    attributes:
      states: {range: State, multivalued: true}
      claims: {range: Claim, multivalued: true}
      hypotheses: {range: Hypothesis, multivalued: true}
      methods: {range: Method, multivalued: true}
      questions: {range: Question, multivalued: true}
  State:
    attributes:
      id: {identifier: true}
    slots: [qualifies]
  Claim:
    attributes:
      id: {identifier: true}
  Hypothesis:
    is_a: Claim
  Method:
    attributes:
      id: {identifier: true}
  Question:
    attributes:
      id: {identifier: true}
";

    fn union_schema() -> SchemaDefinition {
        let mut schema: SchemaDefinition =
            serde_norway::from_str(UNION_SCHEMA).expect("parse union schema");
        let named: Vec<(String, crate::linkml::ClassDefinition)> = schema
            .classes
            .iter()
            .map(|(key, class)| {
                let mut c = class.clone();
                c.name = key.clone();
                (key.clone(), c)
            })
            .collect();
        schema.classes = named.into_iter().collect();
        schema
    }

    fn union_violations(yaml: &str) -> Vec<Violation> {
        validate_instance_data(&union_schema(), &data(yaml))
    }

    #[test]
    fn a_union_reference_outside_the_permitted_classes_is_a_violation() {
        let v =
            union_violations("states:\n  - {id: s1, qualifies: q1}\nquestions:\n  - {id: q1}\n");
        assert_eq!(
            v.len(),
            1,
            "a Question is neither a Claim nor a Method; got: {v:?}"
        );
        assert!(
            v[0].detail.contains("Claim") && v[0].detail.contains("Method"),
            "the message should name the permitted classes; got: {}",
            v[0].detail
        );
    }

    #[test]
    fn a_union_reference_to_a_permitted_class_conforms() {
        let v = union_violations("states:\n  - {id: s1, qualifies: c1}\nclaims:\n  - {id: c1}\n");
        assert!(v.is_empty(), "a Claim is a permitted branch; got: {v:?}");
    }

    #[test]
    fn a_union_reference_to_a_subclass_of_a_permitted_class_conforms() {
        let v =
            union_violations("states:\n  - {id: s1, qualifies: h1}\nhypotheses:\n  - {id: h1}\n");
        assert!(
            v.is_empty(),
            "Hypothesis is_a Claim, so it satisfies the Claim branch; got: {v:?}"
        );
    }

    #[test]
    fn a_dangling_union_reference_is_reported_once() {
        // The integrity pass already reports a target that names no record.
        // The branch check must not pile a second violation on top of it.
        let v = union_violations("states:\n  - {id: s1, qualifies: nope}\n");
        assert_eq!(v.len(), 1, "exactly one report for one problem; got: {v:?}");
        assert!(v[0].detail.contains("nope"), "got: {}", v[0].detail);
    }

    #[test]
    fn an_unusable_value_at_a_union_slot_names_the_permitted_classes() {
        let v = union_violations("states:\n  - {id: s1, qualifies: 42}\n");
        assert_eq!(v.len(), 1, "got: {v:?}");
        assert!(
            v[0].detail.contains("Claim") && v[0].detail.contains("Method"),
            "a range-kind mismatch at a union slot should name the members, not `?`; got: {}",
            v[0].detail
        );
    }

    #[test]
    fn an_untitled_rule_is_named_by_its_position() {
        let v = rule_violations("deployments:\n  - id: d1\n    status: actual\n");
        assert!(
            v.iter()
                .all(|x| x.detail.contains("actual deployments are bound")),
            "a titled rule is named by its title; got: {v:?}"
        );
        let mut schema = rule_schema();
        schema
            .classes
            .get_mut("Deployment")
            .expect("Deployment")
            .rules[0]
            .title = None;
        let d = data("deployments:\n  - id: d1\n    status: actual\n");
        let v = validate_instance_data(&schema, &d);
        assert!(
            v.iter().all(|x| x.detail.contains("`#1`")),
            "an untitled rule falls back to its 1-based position; got: {v:?}"
        );
    }

    #[test]
    fn a_precondition_any_of_fires_on_either_alternative() {
        // `approved` and `rejected` both trigger the rule; `pending` does not.
        for verdict in ["approved", "rejected"] {
            let v = rule_violations(&format!(
                "reviews:\n  - id: r1\n    verdict: {verdict}\n    score: 3\n"
            ));
            assert_eq!(
                v.len(),
                1,
                "`{verdict}` should fire the rule, leaving approved_by absent; got: {v:?}"
            );
            assert!(
                v[0].detail.contains("approved_by") && v[0].detail.contains("absent"),
                "got: {}",
                v[0].detail
            );
        }
        let v = rule_violations("reviews:\n  - id: r1\n    verdict: pending\n");
        assert!(
            v.is_empty(),
            "`pending` matches neither alternative, so the rule stays dormant; got: {v:?}"
        );
    }

    #[test]
    fn a_postcondition_value_presence_and_bounds_are_enforced() {
        let v = rule_violations(
            "reviews:\n  - id: r1\n    verdict: approved\n    approved_by: pat\n    score: 9\n",
        );
        assert_eq!(v.len(), 1, "score 9 exceeds the rule's maximum; got: {v:?}");
        assert!(
            v[0].detail.contains("above the required maximum"),
            "got: {}",
            v[0].detail
        );

        let v = rule_violations(
            "reviews:\n  - id: r1\n    verdict: approved\n    approved_by: pat\n    score: 0\n",
        );
        assert_eq!(
            v.len(),
            1,
            "score 0 is below the rule's minimum; got: {v:?}"
        );
        assert!(
            v[0].detail.contains("below the required minimum"),
            "got: {}",
            v[0].detail
        );

        let v = rule_violations(
            "reviews:\n  - id: r1\n    verdict: approved\n    approved_by: pat\n    score: 3\n",
        );
        assert!(v.is_empty(), "a fully satisfied rule conforms; got: {v:?}");
    }

    #[test]
    fn a_record_whose_precondition_does_not_hold_is_left_alone() {
        // The whole point of a conditional requirement: a planned deployment
        // may omit the bindings an actual one must carry.
        let v = rule_violations("deployments:\n  - id: d1\n    status: planned\n");
        assert!(
            v.is_empty(),
            "the precondition does not match, so the rule does not apply; got: {v:?}"
        );
    }
}
