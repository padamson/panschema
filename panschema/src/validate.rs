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

    // Slot resolution depends on the class, not the record — resolve each
    // class that appears in the set once, however many records instantiate it.
    let mut resolved_by_class: std::collections::BTreeMap<&str, _> =
        std::collections::BTreeMap::new();
    for inst in &set.instances {
        if let Some(class_name) = inst.types.first()
            && let Some(class) = schema.classes.get(class_name)
        {
            resolved_by_class
                .entry(class_name.as_str())
                .or_insert_with(|| resolve_effective_slots_with_provenance(class, schema));
        }
    }

    for inst in &set.instances {
        // A record's class is the collection slot's range that produced it.
        let Some(class_name) = inst.types.first() else {
            continue;
        };
        let Some(class) = schema.classes.get(class_name) else {
            continue;
        };
        let resolved = &resolved_by_class[class_name.as_str()];
        for (slot_name, rs) in resolved {
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
            // The primitive kind each range target demands, resolved once per
            // slot. Kind-checking applies only when *every* target resolves
            // to a known primitive: enums have their own check, class ranges
            // are the reference machinery's, and an unresolvable name is the
            // dangling diagnostic's — never a typing guess here.
            let expected_primitives: Vec<&'static str> = if range_enum.is_none() {
                ranges
                    .iter()
                    .map(|r| crate::primitives::effective_primitive(schema, r))
                    .collect::<Option<Vec<_>>>()
                    .unwrap_or_default()
            } else {
                Vec::new()
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
                    InstanceValue::Reference { target, .. } => {
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
                // Scalar kind vs the range's primitive(s), matching the JSON
                // Schema this same schema projects: `type: string` there
                // rejects `42`, so the native validator must too. A union
                // conforms when any branch's kind matches, mirroring `anyOf`.
                if !expected_primitives.is_empty()
                    && !expected_primitives.iter().any(|p| kind_matches(p, scalar))
                {
                    let shown = scalar_to_display(scalar);
                    let kind = crate::primitives::scalar_kind_phrase(scalar);
                    let expected = match (ranges.as_slice(), expected_primitives.as_slice()) {
                        ([range], [primitive]) => format!(
                            "the slot's range `{range}` expects {}",
                            with_article(primitive)
                        ),
                        _ => format!(
                            "none of the slot's ranges `{}` permit it",
                            ranges.join("`, `")
                        ),
                    };
                    push(format!(
                        "slot `{slot_name}` (class `{class_name}`) value `{shown}` is {kind}, \
                         but {expected}"
                    ));
                    // A wrong-kinded value can't be meaningfully pattern- or
                    // bounds-checked; one problem yields one report.
                    continue;
                }
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

            // Slot-level `is_a` states a subset: every value here must also
            // be a value of the parent slot on this record. No reasoner runs
            // on this path — without the check, citing a value outside the
            // parent set silently widens the parent instead of erroring.
            // Values compare as typed values, not display strings, so a
            // reference and a same-spelled literal stay distinct. An
            // `Unexpected` child value is the range-kind report's problem;
            // a repeated outside value is reported once.
            if let Some(parent_name) = rs.definition.is_a.as_deref()
                && resolved.contains_key(parent_name)
            {
                let parent_values = slot_values(inst, parent_name);
                let mut reported: Vec<&InstanceValue> = Vec::new();
                for value in slot_values(inst, slot_name) {
                    if matches!(value, InstanceValue::Unexpected(_))
                        || parent_values.iter().any(|p| values_match(p, value))
                        || reported.iter().any(|r| values_match(r, value))
                    {
                        continue;
                    }
                    reported.push(value);
                    push(format!(
                        "slot `{slot_name}` (class `{class_name}`) value `{}` is not \
                         among the values of `{parent_name}`, which `{slot_name}` specializes",
                        value_display(value)
                    ));
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

    // A dataset read against no root yielded nothing, and "nothing" is
    // indistinguishable from "conforms" unless it is said out loud.
    if let Some(candidates) = &set.root_candidates {
        out.push(Violation {
            record: "(root)".to_string(),
            detail: format!(
                "the data conforms to none of this schema's `tree_root` classes, or to \
                 more than one equally: {}. Name the collections of exactly one of them \
                 so the dataset can be read.",
                candidates.join(", ")
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
    match instance_set_for(schema, data) {
        Ok(set) => validate_instances(schema, &set),
        Err(v) => vec![v],
    }
}

/// The set `validate_instance_data` checks, or the violation that stops the
/// data being loadable at all. Callers that want to report on more than
/// violations — cross-graph references, say — take this and validate the set
/// themselves rather than building it twice.
pub fn instance_set_for(schema: &SchemaDefinition, data: &Value) -> Result<InstanceSet, Violation> {
    if data.as_mapping().is_none() {
        return Err(Violation {
            record: "(root)".to_string(),
            detail: "instance data must be a mapping (a tree_root container object)".to_string(),
        });
    }
    Ok(InstanceSet::from_linkml_data(schema, data))
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
    // Equals conditions are membership tests — at least one value equals,
    // matching the `sh:hasValue` the SHACL projection emits for the same
    // condition; an empty or reference-only value set never satisfies one.
    if let Some(want) = &cond.equals_string
        && !values
            .iter()
            .any(|v| matches!(v, InstanceValue::Scalar(s) if scalar_display_eq(s, want)))
    {
        return Some(equals_failure(values, &format!("`{want}`")));
    }
    if let Some(want) = cond.equals_number
        && !values
            .iter()
            .any(|v| numeric_value(v).is_some_and(|n| n == want || (n.is_nan() && want.is_nan())))
    {
        return Some(equals_failure(values, &want.to_string()));
    }
    for value in values {
        let InstanceValue::Scalar(scalar) = value else {
            continue;
        };
        if cond.minimum_value.is_some() || cond.maximum_value.is_some() {
            let Some(n) = numeric(scalar) else {
                return Some(format!(
                    "value `{}` is not numeric, but a bound is required",
                    scalar_to_display(scalar)
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

use crate::primitives::kind_matches;

use crate::primitives::with_article;

/// Whether two values denote the same thing, with the same number
/// semantics the kind check applies: `5` and `5.0` are one value, and a
/// NaN matches a NaN (containment is about which values are present, not
/// IEEE comparison). Everything else compares as its typed self, so a
/// reference and a same-spelled literal stay distinct.
fn values_match(a: &InstanceValue, b: &InstanceValue) -> bool {
    if a == b {
        return true;
    }
    match (numeric_value(a), numeric_value(b)) {
        (Some(x), Some(y)) => x == y || (x.is_nan() && y.is_nan()),
        _ => false,
    }
}

/// The numeric reading of a value, when it has one.
fn numeric_value(value: &InstanceValue) -> Option<f64> {
    match value {
        InstanceValue::Scalar(s) => numeric(s),
        InstanceValue::Reference { .. } | InstanceValue::Unexpected(_) => None,
    }
}

/// A value's display form for violation messages — the scalar's display
/// or a reference's target id. Display only: values compare through
/// [`values_match`] so kinds stay distinct even when displays collide.
fn value_display(value: &InstanceValue) -> String {
    match value {
        InstanceValue::Scalar(s) => scalar_to_display(s),
        InstanceValue::Reference { target, .. } => target.clone(),
        InstanceValue::Unexpected(kind) => kind.to_string(),
    }
}

/// The failure message for an unmet equals condition, phrased by how many
/// values the slot held — references and other non-scalars count and
/// display, so a populated slot is never reported as having no value.
/// `want` arrives pre-formatted (backticked for a string, bare for a
/// number).
fn equals_failure(values: &[InstanceValue], want: &str) -> String {
    match values {
        [] => format!("has no value, but must equal {want}"),
        [one] => format!("is `{}`, but must equal {want}", value_display(one)),
        many => format!("none of its {} values equals {want}", many.len()),
    }
}

/// Whether a scalar's display form equals `want`, without allocating for
/// the dominant string case.
fn scalar_display_eq(scalar: &ScalarValue, want: &str) -> bool {
    match scalar {
        ScalarValue::String(text) => text == want,
        other => scalar_to_display(other) == want,
    }
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
    use crate::linkml::{ClassDefinition, SlotDefinition};

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
        let mut schema: SchemaDefinition = serde_norway::from_str(SCHEMA).expect("parse schema");
        // Mirror the load path: `default_range` is materialized before any
        // consumer sees the schema.
        crate::linkml_resolve::materialize_default_range(&mut schema);
        schema
    }

    fn data(yaml: &str) -> Value {
        serde_norway::from_str(yaml).expect("parse data")
    }

    /// The float family follows number semantics: an integer is a valid
    /// float/double/decimal, exactly as the projected JSON Schema accepts
    /// it — the native validator and the emitted contract must agree.
    #[test]
    fn an_integer_at_a_float_slot_conforms() {
        let schema: crate::linkml::SchemaDefinition = serde_norway::from_str(
            "name: s\nclasses:\n  Item:\n    tree_root: true\n    attributes:\n      id:\n        identifier: true\n      ratio:\n        range: float\n",
        )
        .expect("parse schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: x1\nratio: 3\n").expect("parse data");
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let violations = validate_instances(&schema, &set);
        assert!(
            violations.is_empty(),
            "an integer is a valid number; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
    }

    /// An explicit scalar range is enforced the same way the default is —
    /// a string slot rejects an integer whichever way it was typed.
    #[test]
    fn an_integer_at_an_explicitly_string_slot_is_a_violation() {
        let schema: crate::linkml::SchemaDefinition = serde_norway::from_str(
            "name: s\nclasses:\n  Item:\n    tree_root: true\n    attributes:\n      id:\n        identifier: true\n        range: string\n      question:\n        range: string\n",
        )
        .expect("parse schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: x1\nquestion: 42\n").expect("parse data");
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let violations = validate_instances(&schema, &set);
        assert!(
            violations
                .iter()
                .any(|v| v.to_string().contains("question")),
            "explicit ranges are typed too; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
    }

    /// An integral float satisfies an `integer` range — `5.0` denotes the
    /// integer five, exactly as the projected JSON Schema's `type: integer`
    /// accepts it; only a fractional value is wrong-kinded.
    #[test]
    fn an_integral_float_at_an_integer_slot_conforms() {
        let schema: crate::linkml::SchemaDefinition = serde_norway::from_str(
            "name: s\nclasses:\n  Item:\n    tree_root: true\n    attributes:\n      id:\n        identifier: true\n      count:\n        range: integer\n",
        )
        .expect("parse schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: x1\ncount: 5.0\n").expect("parse data");
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let violations = validate_instances(&schema, &set);
        assert!(
            violations.is_empty(),
            "5.0 denotes an integer; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );

        let data: serde_norway::Value =
            serde_norway::from_str("id: x1\ncount: 5.5\n").expect("parse data");
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let violations = validate_instances(&schema, &set);
        assert!(
            violations.iter().any(|v| v.to_string().contains("count")),
            "a fractional value at an integer slot is a violation; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
    }

    /// A union of scalar ranges is kind-checked against every branch: a
    /// value no branch permits is a violation, matching the `anyOf` the
    /// projected JSON Schema emits for the same slot.
    #[test]
    fn a_wrong_kinded_value_at_a_scalar_union_is_a_violation() {
        let schema: crate::linkml::SchemaDefinition = serde_norway::from_str(
            "name: s\nclasses:\n  Item:\n    tree_root: true\n    attributes:\n      id:\n        identifier: true\n      flag:\n        any_of:\n          - range: integer\n          - range: boolean\n",
        )
        .expect("parse schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: x1\nflag: hello\n").expect("parse data");
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let violations = validate_instances(&schema, &set);
        assert!(
            violations.iter().any(|v| {
                let s = v.to_string();
                s.contains("flag") && s.contains("integer") && s.contains("boolean")
            }),
            "no branch permits a string, and the report names the branches; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );

        let data: serde_norway::Value =
            serde_norway::from_str("id: x1\nflag: true\n").expect("parse data");
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let violations = validate_instances(&schema, &set);
        assert!(
            violations.is_empty(),
            "a value one branch permits conforms; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
    }

    /// The kind-mismatch report is grammatical for every primitive: the
    /// expected range takes its own indefinite article.
    #[test]
    fn the_kind_report_gives_the_expected_primitive_its_article() {
        let schema: crate::linkml::SchemaDefinition = serde_norway::from_str(
            "name: s\nclasses:\n  Item:\n    tree_root: true\n    attributes:\n      id:\n        identifier: true\n      count:\n        range: integer\n",
        )
        .expect("parse schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: x1\ncount: hello\n").expect("parse data");
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let violations = validate_instances(&schema, &set);
        assert!(
            violations
                .iter()
                .any(|v| v.to_string().contains("expects an integer")),
            "\"an integer\", not \"a integer\"; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
    }

    /// A root `types:` entry that declares its lexical space via `uri:`
    /// alone (no `typeof:`) enforces that space — the idiomatic spelling
    /// of a custom root type must not silently escape the kind check.
    #[test]
    fn a_uri_only_root_type_enforces_its_lexical_space() {
        let schema: crate::linkml::SchemaDefinition = serde_norway::from_str(
            "name: s\ntypes:\n  Question:\n    uri: xsd:string\nclasses:\n  Item:\n    tree_root: true\n    attributes:\n      id:\n        identifier: true\n      question:\n        range: Question\n",
        )
        .expect("parse schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: x1\nquestion: 42\n").expect("parse data");
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let violations = validate_instances(&schema, &set);
        assert!(
            violations
                .iter()
                .any(|v| v.to_string().contains("question")),
            "xsd:string via `uri:` types the slot; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
    }

    /// A string value at a bounded string-kinded slot (a `date` with a
    /// numeric bound) reports the impossible bound rather than panicking
    /// or conforming — the bound is the schema's mistake to surface.
    #[test]
    fn a_bounded_non_numeric_slot_reports_the_impossible_bound() {
        let schema: crate::linkml::SchemaDefinition = serde_norway::from_str(
            "name: s\nclasses:\n  Item:\n    tree_root: true\n    attributes:\n      id:\n        identifier: true\n      when:\n        range: date\n        minimum_value: 5\n",
        )
        .expect("parse schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: x1\nwhen: 2024-01-01\n").expect("parse data");
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let violations = validate_instances(&schema, &set);
        assert!(
            violations
                .iter()
                .any(|v| v.to_string().contains("is not numeric")),
            "the bogus bound is reported; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
    }

    /// A slot specializing another (slot-level `is_a`) is a subset claim:
    /// every value of the child is also a value of the parent. On the YAML
    /// path no reasoner runs, so a child value missing from the parent
    /// slot's values on the same record is a violation — not a silently
    /// widened parent set.
    #[test]
    fn a_child_slot_value_missing_from_its_parent_slot_is_a_violation() {
        const SPECIALIZING: &str = "\
name: s
classes:
  Benchmark:
    tree_root: true
    attributes:
      id:
        identifier: true
      answers:
        range: Answer
        multivalued: true
  Answer:
    attributes:
      id:
        identifier: true
    slots: [expected_anchors, expected_citations]
slots:
  expected_anchors:
    range: string
    multivalued: true
  expected_citations:
    is_a: expected_anchors
    range: string
    multivalued: true
";
        let schema: crate::linkml::SchemaDefinition =
            serde_norway::from_str(SPECIALIZING).expect("parse schema");
        let data: serde_norway::Value = serde_norway::from_str(
            "id: b1\nanswers:\n  - id: a1\n    expected_anchors: [rec-a, rec-b]\n    expected_citations: [rec-b, rec-x]\n",
        )
        .expect("parse data");
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let violations = validate_instances(&schema, &set);
        assert!(
            violations.iter().any(|v| {
                let s = v.to_string();
                s.contains("rec-x") && s.contains("expected_anchors")
            }),
            "the value outside the parent set must be reported, naming both; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
        assert!(
            !violations.iter().any(|v| v.to_string().contains("rec-b")),
            "a value the parent also holds conforms; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );

        let data: serde_norway::Value = serde_norway::from_str(
            "id: b1\nanswers:\n  - id: a1\n    expected_anchors: [rec-a, rec-b]\n    expected_citations: [rec-a]\n",
        )
        .expect("parse data");
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let violations = validate_instances(&schema, &set);
        assert!(
            violations.is_empty(),
            "a subset conforms; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
    }

    /// The subset check follows the kind check's number semantics: `5`
    /// and `5.0` denote one value, so YAML spelling differences between
    /// the parent's and child's numerals do not fabricate violations.
    #[test]
    fn an_integral_spelling_difference_is_not_a_containment_violation() {
        const SPECIALIZING: &str = "\
name: s
classes:
  Root:
    tree_root: true
    attributes:
      id:
        identifier: true
      items:
        range: Item
        multivalued: true
  Item:
    attributes:
      id:
        identifier: true
    slots: [readings, flagged]
slots:
  readings:
    range: float
    multivalued: true
  flagged:
    is_a: readings
    range: float
    multivalued: true
";
        let schema: crate::linkml::SchemaDefinition =
            serde_norway::from_str(SPECIALIZING).expect("parse schema");
        let data: serde_norway::Value = serde_norway::from_str(
            "id: r1\nitems:\n  - id: i1\n    readings: [5.0, 2.5]\n    flagged: [5]\n",
        )
        .expect("parse data");
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let violations = validate_instances(&schema, &set);
        assert!(
            violations.is_empty(),
            "5 is among [5.0, 2.5]; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
    }

    /// Numeric containment is by value, both ways: a child numeral equal
    /// to no parent numeral is a violation, and a NaN is contained only
    /// when the parent also holds a NaN.
    #[test]
    fn numeric_containment_is_by_value_and_nan_only_matches_nan() {
        const SPECIALIZING: &str = "\
name: s
classes:
  Root:
    tree_root: true
    attributes:
      id:
        identifier: true
      items:
        range: Item
        multivalued: true
  Item:
    attributes:
      id:
        identifier: true
    slots: [readings, flagged]
slots:
  readings:
    range: float
    multivalued: true
  flagged:
    is_a: readings
    range: float
    multivalued: true
";
        let schema: crate::linkml::SchemaDefinition =
            serde_norway::from_str(SPECIALIZING).expect("parse schema");
        let validate = |data: &str| {
            let data: serde_norway::Value = serde_norway::from_str(data).expect("parse data");
            let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
            validate_instances(&schema, &set)
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
        };

        let violations =
            validate("id: r1\nitems:\n  - id: i1\n    readings: [2.5]\n    flagged: [5]\n");
        assert!(
            violations.iter().any(|v| v.contains("flagged")),
            "5 is not among [2.5]; got: {violations:?}"
        );

        let violations =
            validate("id: r1\nitems:\n  - id: i1\n    readings: [1.0]\n    flagged: [.nan]\n");
        assert!(
            violations.iter().any(|v| v.contains("flagged")),
            "a NaN is not among [1.0]; got: {violations:?}"
        );

        let violations =
            validate("id: r1\nitems:\n  - id: i1\n    readings: [.nan]\n    flagged: [.nan]\n");
        assert!(
            violations.is_empty(),
            "a NaN parent contains a NaN child; got: {violations:?}"
        );
    }

    /// The subset check fires for class-ranged slots exactly as it does
    /// for scalar ones: a citation referencing a record the anchors don't
    /// is a violation, whether or not the target record resolves.
    #[test]
    fn a_class_ranged_child_value_outside_its_parent_is_a_violation() {
        const CLASSY: &str = "\
name: s
classes:
  Rec:
    slots: [id]
  Thing:
    tree_root: true
    slots: [id, anchors, citations]
slots:
  id:
    identifier: true
  anchors:
    range: Rec
    multivalued: true
  citations:
    is_a: anchors
    range: Rec
    multivalued: true
";
        let schema: crate::linkml::SchemaDefinition =
            serde_norway::from_str(CLASSY).expect("parse schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: t1\ncitations: [a]\n").expect("parse data");
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let violations = validate_instances(&schema, &set);
        assert!(
            violations.iter().any(|v| {
                let s = v.to_string();
                s.contains("citations") && s.contains("anchors")
            }),
            "a citation with no anchor must violate at a class range too; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );

        let data: serde_norway::Value =
            serde_norway::from_str("id: t1\nanchors: [a, b]\ncitations: [a]\n")
                .expect("parse data");
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let violations = validate_instances(&schema, &set);
        assert!(
            !violations
                .iter()
                .any(|v| v.to_string().contains("specializes")),
            "a citation among the anchors conforms; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
    }

    /// The subset check compares typed values, not display strings: a
    /// literal spelled like a reference's target is not that reference,
    /// so it does not count as contained.
    #[test]
    fn a_literal_spelled_like_a_reference_target_is_not_contained() {
        const SPECIALIZING: &str = "\
name: s
classes:
  Benchmark:
    tree_root: true
    attributes:
      id:
        identifier: true
      recs:
        range: Rec
        multivalued: true
      answers:
        range: Answer
        multivalued: true
  Rec:
    attributes:
      id:
        identifier: true
  Answer:
    attributes:
      id:
        identifier: true
    slots: [expected_anchors, expected_citations]
slots:
  expected_anchors:
    range: Rec
    multivalued: true
  expected_citations:
    is_a: expected_anchors
    range: string
    multivalued: true
";
        let schema: crate::linkml::SchemaDefinition =
            serde_norway::from_str(SPECIALIZING).expect("parse schema");
        let data: serde_norway::Value = serde_norway::from_str(
            "id: b1\nrecs:\n  - id: rec-a\nanswers:\n  - id: a1\n    expected_anchors: [rec-a]\n    expected_citations: [rec-a]\n",
        )
        .expect("parse data");
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let violations = validate_instances(&schema, &set);
        assert!(
            violations.iter().any(|v| {
                let s = v.to_string();
                s.contains("expected_citations") && s.contains("rec-a")
            }),
            "a string literal is not the reference it happens to spell; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
    }

    /// A custom `types:` entry types through its `typeof` chain, and a
    /// range that resolves to no known primitive is never guessed at.
    #[test]
    fn a_custom_type_enforces_its_base_and_an_unknown_range_is_skipped() {
        let schema: crate::linkml::SchemaDefinition = serde_norway::from_str(
            "name: s\ntypes:\n  Score:\n    typeof: integer\nclasses:\n  Item:\n    tree_root: true\n    attributes:\n      id:\n        identifier: true\n      score:\n        range: Score\n      free:\n        range: Mystery\n",
        )
        .expect("parse schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: x1\nscore: not a number\nfree: 12\n").expect("parse data");
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let violations = validate_instances(&schema, &set);
        assert!(
            violations.iter().any(|v| v.to_string().contains("score")),
            "the typeof chain resolves Score to integer; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
        assert!(
            !violations.iter().any(|v| v.to_string().contains("free")),
            "an unresolvable range is the dangling diagnostic's problem, \
             not a typing guess; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
    }

    /// The schema's `default_range` types every slot that states no range
    /// of its own (materialized at load), so data violating the default is
    /// a violation — not a silent pass.
    #[test]
    fn a_value_violating_the_default_range_is_a_violation() {
        let mut schema: crate::linkml::SchemaDefinition = serde_norway::from_str(
            "name: s\ndefault_range: string\nclasses:\n  Item:\n    tree_root: true\n    attributes:\n      id:\n        identifier: true\n        range: string\n      question:\n        required: true\n",
        )
        .expect("parse schema");
        crate::linkml_resolve::materialize_default_range(&mut schema);
        let data: serde_norway::Value =
            serde_norway::from_str("id: x1\nquestion: 42\n").expect("parse data");
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let violations = validate_instances(&schema, &set);
        assert!(
            violations
                .iter()
                .any(|v| v.to_string().contains("question")),
            "an integer at a string-defaulted slot must be reported; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
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
    fn a_wrong_kinded_value_at_a_bounded_slot_yields_one_report() {
        // The kind check fires and suppresses the bounds check — a string at
        // a bounded float slot is one problem, reported once, not a panic
        // and not a double report.
        let v = value_violations("items:\n  - id: a\n    strength: high\n");
        assert_eq!(v.len(), 1, "one problem, one report; got: {v:?}");
        assert!(
            v[0].detail.contains("expects a float"),
            "the kind mismatch names the declared range; got: {}",
            v[0].detail
        );
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
      shipments: {range: Shipment, multivalued: true}
      batches: {range: Batch, multivalued: true}
      tickets: {range: Ticket, multivalued: true}
      questions: {range: Question, multivalued: true}
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
  Shipment:
    attributes:
      id: {identifier: true}
      status: {range: string}
      cancelled_on: {range: string}
      weight: {range: integer}
    rules:
      - title: an open shipment is not cancelled
        preconditions:
          slot_conditions: {status: {equals_string: open}}
        postconditions:
          slot_conditions:
            cancelled_on: {value_presence: ABSENT}
      - title: express shipments weigh exactly five
        preconditions:
          slot_conditions: {status: {equals_string: express}}
        postconditions:
          slot_conditions:
            weight: {equals_number: 5}
  Batch:
    attributes:
      id: {identifier: true}
      kind: {range: string}
      items: {range: string, multivalued: true}
      code: {range: string}
      score: {range: integer}
    rules:
      - title: a bulk batch carries two or three items
        preconditions:
          slot_conditions: {kind: {equals_string: bulk}}
        postconditions:
          slot_conditions:
            items: {minimum_cardinality: 2, maximum_cardinality: 3}
      - title: a coded batch matches the code pattern
        preconditions:
          slot_conditions: {kind: {equals_string: coded}}
        postconditions:
          slot_conditions:
            code: {pattern: \"^B-[0-9]+$\"}
      - title: a scored batch meets the floor
        preconditions:
          slot_conditions: {kind: {equals_string: scored}}
        postconditions:
          slot_conditions:
            score: {minimum_value: 10}
  Question:
    attributes:
      id: {identifier: true}
      answer_kind: {range: string, multivalued: true}
      unconnected_anchors: {range: string, multivalued: true}
      sources: {range: string, multivalued: true}
    rules:
      - title: a closed-world negative states its absence
        preconditions:
          slot_conditions: {answer_kind: {equals_string: closed-world-negative}}
        postconditions:
          slot_conditions:
            unconnected_anchors: {value_presence: PRESENT}
      - title: an attributed question cites the vetted source
        preconditions:
          slot_conditions: {answer_kind: {equals_string: attribution}}
        postconditions:
          slot_conditions:
            sources: {equals_string: cited}
  Ticket:
    attributes:
      id: {identifier: true}
      tier: {range: string}
      owner: {range: string}
      escalated_to: {range: string}
    rules:
      - title: an escalated tier names someone
        preconditions:
          any_of:
            - slot_conditions: {tier: {equals_string: gold}}
            - slot_conditions: {tier: {equals_string: platinum}}
        postconditions:
          any_of:
            - slot_conditions: {owner: {value_presence: PRESENT}}
            - slot_conditions: {escalated_to: {value_presence: PRESENT}}
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
      notes: {range: Note, multivalued: true}
      loops: {range: Loop, multivalued: true}
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
  Note:
    attributes:
      id: {identifier: true}
      about: {range: Claim}
  Loop:
    is_a: Knot
    attributes:
      id: {identifier: true}
  Knot:
    is_a: Loop
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
    fn data_matching_no_tree_root_is_a_violation_naming_the_candidates() {
        // The old behaviour read such a file against whichever root sorted
        // first and reported nothing, so an empty result looked like a pass.
        let mut schema = SchemaDefinition::new("estate");
        schema.default_range = Some("string".to_string());
        for name in ["Enterprise", "ProviderCatalog"] {
            let mut root = ClassDefinition::new(name);
            root.tree_root = true;
            let mut slot = SlotDefinition::new(if name == "Enterprise" {
                "deployments"
            } else {
                "providers"
            });
            slot.range = Some("Thing".to_string());
            slot.multivalued = true;
            root.attributes.insert(slot.name.clone(), slot);
            schema.classes.insert(name.to_string(), root);
        }
        let mut thing = ClassDefinition::new("Thing");
        let mut id = SlotDefinition::new("id");
        id.identifier = true;
        thing.attributes.insert("id".to_string(), id);
        schema.classes.insert("Thing".to_string(), thing);

        let data: serde_norway::Value = serde_norway::from_str("widgets:\n  - id: w1\n").unwrap();
        let violations = validate_instance_data(&schema, &data);
        let detail = violations
            .iter()
            .map(|v| v.detail.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            detail.contains("Enterprise") && detail.contains("ProviderCatalog"),
            "the violation names both candidate roots; got: {detail}"
        );
    }

    #[test]
    fn data_matching_one_of_several_roots_validates_clean() {
        let mut schema = SchemaDefinition::new("estate");
        schema.default_range = Some("string".to_string());
        for (name, slot_name) in [
            ("Enterprise", "deployments"),
            ("ProviderCatalog", "providers"),
        ] {
            let mut root = ClassDefinition::new(name);
            root.tree_root = true;
            let mut slot = SlotDefinition::new(slot_name);
            slot.range = Some("Thing".to_string());
            slot.multivalued = true;
            root.attributes.insert(slot_name.to_string(), slot);
            schema.classes.insert(name.to_string(), root);
        }
        let mut thing = ClassDefinition::new("Thing");
        let mut id = SlotDefinition::new("id");
        id.identifier = true;
        thing.attributes.insert("id".to_string(), id);
        schema.classes.insert("Thing".to_string(), thing);

        let data: serde_norway::Value =
            serde_norway::from_str("providers:\n  - id: aws\n").unwrap();
        assert!(
            validate_instance_data(&schema, &data).is_empty(),
            "a file that names one root's collections conforms; got: {:?}",
            validate_instance_data(&schema, &data)
        );
    }

    #[test]
    fn a_cross_graph_reference_is_not_reported_as_dangling() {
        // The whole point of slice 2: a record may point into another graph.
        // Before this, such a target was either an error or a bare string.
        let mut schema = union_schema();
        schema.prefixes.insert(
            "catalog".to_string(),
            "https://example.org/catalog/".to_string(),
        );
        let d = data("states:\n  - {id: s1, qualifies: 'catalog:claim-7'}\n");
        let v = validate_instance_data(&schema, &d);
        assert!(
            v.is_empty(),
            "a CURIE against a declared prefix points outside by design; got: {v:?}"
        );
    }

    #[test]
    fn a_bare_id_naming_no_record_is_still_dangling() {
        // The exemption must not swallow the check it sits beside.
        let v = union_violations("states:\n  - {id: s1, qualifies: nope}\n");
        assert_eq!(v.len(), 1, "got: {v:?}");
        assert!(v[0].detail.contains("nope"), "got: {}", v[0].detail);
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
    fn a_single_class_range_is_not_branch_checked() {
        // Branch checking is a union concern. A slot with one declared class
        // range keeps the behaviour it had before unions were understood, so
        // a mismatch there is not reported by this check.
        let v = union_violations("notes:\n  - {id: n1, about: q1}\nquestions:\n  - {id: q1}\n");
        assert!(
            v.is_empty(),
            "a single class range is out of scope for branch checking; got: {v:?}"
        );
    }

    #[test]
    fn an_is_a_cycle_does_not_hang_the_branch_check() {
        // Two classes naming each other as parent must terminate the ancestor
        // walk rather than spin.
        let v = union_violations("loops:\n  - {id: l1}\nstates:\n  - {id: s1, qualifies: l1}\n");
        assert_eq!(
            v.len(),
            1,
            "a Loop is neither a Claim nor a Method; got: {v:?}"
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

    /// `equals_string` on a multivalued slot means membership — at least
    /// one value equals — matching the `sh:hasValue` the SHACL projection
    /// emits for the same condition, so the two agree on when a rule
    /// fires. A second value must never disable the rule.
    #[test]
    fn a_multivalued_precondition_fires_on_membership() {
        let v = rule_violations(
            "questions:\n  - {id: q1, answer_kind: [closed-world-negative, attribution], \
             sources: [cited]}\n",
        );
        assert_eq!(
            v.len(),
            1,
            "the rule fires whichever other kinds ride along; got: {v:?}"
        );
        assert!(
            v[0].detail.contains("unconnected_anchors"),
            "got: {}",
            v[0].detail
        );
        let single = rule_violations(
            "questions:\n  - {id: q1, answer_kind: [closed-world-negative], sources: []}\n",
        );
        assert_eq!(
            single.len(),
            1,
            "a single-element list fires the same way; got: {single:?}"
        );
        assert!(
            rule_violations(
                "questions:\n  - {id: q2, answer_kind: [attribution], sources: [cited]}\n"
            )
            .is_empty(),
            "a list without the value does not fire the rule"
        );
    }

    /// The same membership reading applies to a postcondition: the
    /// constraint demands the value be present among the slot's values,
    /// not that it be the only one.
    #[test]
    fn a_multivalued_postcondition_equals_holds_on_membership() {
        assert!(
            rule_violations(
                "questions:\n  - {id: q1, answer_kind: [attribution], sources: [cited, reviewed]}\n"
            )
            .is_empty(),
            "the required value among others conforms"
        );
        let v = rule_violations(
            "questions:\n  - {id: q1, answer_kind: [attribution], sources: [reviewed]}\n",
        );
        assert_eq!(v.len(), 1, "a list missing the value violates; got: {v:?}");
    }

    /// An equals condition on an absent slot is unmet, never vacuously
    /// met: `sh:hasValue` requires at least one value, and "when status is
    /// actual" cannot hold for a record with no status at all.
    #[test]
    fn an_equals_precondition_on_an_absent_slot_does_not_fire() {
        assert!(
            rule_violations("deployments:\n  - {id: d1}\n").is_empty(),
            "no status value means the precondition is unmet"
        );
    }

    /// The equals failure message counts and shows what the slot actually
    /// holds: a slot populated with references is not "has no value" —
    /// its values simply never equal a literal constant.
    #[test]
    fn equals_failure_counts_references_as_the_values_they_are() {
        let one_ref = [InstanceValue::Reference {
            target: "r1".to_string(),
            held: false,
        }];
        assert_eq!(
            equals_failure(&one_ref, "`cited`"),
            "is `r1`, but must equal `cited`"
        );
        let two = [
            InstanceValue::Reference {
                target: "r1".to_string(),
                held: false,
            },
            InstanceValue::Scalar(ScalarValue::String("reviewed".to_string())),
        ];
        assert_eq!(
            equals_failure(&two, "`cited`"),
            "none of its 2 values equals `cited`"
        );
        assert_eq!(
            equals_failure(&[], "`cited`"),
            "has no value, but must equal `cited`"
        );
    }

    /// `equals_string` compares display forms, so a non-string scalar
    /// satisfies a string constant that spells it — `42` has value `"42"`.
    #[test]
    fn an_equals_string_constant_matches_a_non_string_scalars_spelling() {
        let cond = SlotCondition {
            equals_string: Some("42".to_string()),
            ..Default::default()
        };
        let held = [InstanceValue::Scalar(ScalarValue::Integer(42))];
        assert_eq!(slot_condition_failure(&cond, &held), None);
        let other = [InstanceValue::Scalar(ScalarValue::Integer(7))];
        assert!(slot_condition_failure(&cond, &other).is_some());
    }

    /// `equals_number` follows the same number semantics the containment
    /// check uses: a NaN constant is satisfied by a NaN value, matching
    /// `values_match` rather than IEEE comparison.
    #[test]
    fn an_equals_number_nan_constant_is_satisfied_by_a_nan_value() {
        let cond = SlotCondition {
            equals_number: Some(f64::NAN),
            ..Default::default()
        };
        let held = [InstanceValue::Scalar(ScalarValue::Float(f64::NAN))];
        assert_eq!(
            slot_condition_failure(&cond, &held),
            None,
            "the exact required value is present"
        );
        let other = [InstanceValue::Scalar(ScalarValue::Float(1.0))];
        assert!(
            slot_condition_failure(&cond, &other).is_some(),
            "a finite value does not satisfy a NaN constant"
        );
    }

    #[test]
    fn a_postcondition_value_presence_absent_forbids_a_value() {
        let v =
            rule_violations("shipments:\n  - {id: p1, status: open, cancelled_on: 2026-01-01}\n");
        assert_eq!(
            v.len(),
            1,
            "an open shipment must not be cancelled; got: {v:?}"
        );
        assert!(
            v[0].detail.contains("must be absent"),
            "got: {}",
            v[0].detail
        );
        assert!(
            rule_violations("shipments:\n  - {id: p1, status: open}\n").is_empty(),
            "absent satisfies ABSENT"
        );
    }

    #[test]
    fn a_postcondition_equals_number_is_enforced() {
        assert!(
            rule_violations("shipments:\n  - {id: p1, status: express, weight: 5}\n").is_empty(),
            "the exact value satisfies equals_number"
        );
        let v = rule_violations("shipments:\n  - {id: p1, status: express, weight: 6}\n");
        assert_eq!(v.len(), 1, "got: {v:?}");
        assert!(v[0].detail.contains("must equal 5"), "got: {}", v[0].detail);
    }

    #[test]
    fn a_postcondition_cardinality_is_enforced_at_both_bounds() {
        // The boundary counts conform; one either side does not.
        for n in [2, 3] {
            let items = (0..n)
                .map(|i| format!("i{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            assert!(
                rule_violations(&format!(
                    "batches:\n  - {{id: b1, kind: bulk, items: [{items}]}}\n"
                ))
                .is_empty(),
                "{n} items is within two-to-three"
            );
        }
        let v = rule_violations("batches:\n  - {id: b1, kind: bulk, items: [i0]}\n");
        assert_eq!(v.len(), 1, "got: {v:?}");
        assert!(v[0].detail.contains("fewer than"), "got: {}", v[0].detail);
        let v = rule_violations("batches:\n  - {id: b1, kind: bulk, items: [i0, i1, i2, i3]}\n");
        assert_eq!(v.len(), 1, "got: {v:?}");
        assert!(v[0].detail.contains("more than"), "got: {}", v[0].detail);
    }

    #[test]
    fn a_postcondition_pattern_is_enforced() {
        assert!(
            rule_violations("batches:\n  - {id: b1, kind: coded, code: B-12}\n").is_empty(),
            "a matching code conforms"
        );
        let v = rule_violations("batches:\n  - {id: b1, kind: coded, code: X-1}\n");
        assert_eq!(v.len(), 1, "got: {v:?}");
        assert!(
            v[0].detail.contains("does not match required pattern"),
            "got: {}",
            v[0].detail
        );
    }

    #[test]
    fn a_postcondition_with_only_a_minimum_still_bounds_the_value() {
        // A one-sided bound must be checked; the value exactly on the floor
        // conforms, and one below it does not.
        assert!(
            rule_violations("batches:\n  - {id: b1, kind: scored, score: 10}\n").is_empty(),
            "a value exactly on the floor conforms"
        );
        let v = rule_violations("batches:\n  - {id: b1, kind: scored, score: 9}\n");
        assert_eq!(v.len(), 1, "got: {v:?}");
        assert!(
            v[0].detail.contains("below the required minimum"),
            "got: {}",
            v[0].detail
        );
    }

    #[test]
    fn condition_set_alternatives_gate_a_rule_on_either_side() {
        // `any_of` over whole condition sets, on both halves of the rule:
        // either tier fires it, and either named party satisfies it.
        for tier in ["gold", "platinum"] {
            let v = rule_violations(&format!("tickets:\n  - {{id: t1, tier: {tier}}}\n"));
            assert_eq!(v.len(), 1, "`{tier}` fires the rule; got: {v:?}");
            assert!(
                v[0].detail
                    .contains("satisfies none of its postcondition alternatives"),
                "got: {}",
                v[0].detail
            );
        }
        assert!(
            rule_violations("tickets:\n  - {id: t1, tier: bronze}\n").is_empty(),
            "a tier matching no alternative leaves the rule dormant"
        );
        for named in ["owner: pat", "escalated_to: sam"] {
            assert!(
                rule_violations(&format!("tickets:\n  - {{id: t1, tier: gold, {named}}}\n"))
                    .is_empty(),
                "`{named}` satisfies one postcondition alternative"
            );
        }
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

        // Both boundary values conform: the bounds are inclusive.
        for score in [1, 3, 5] {
            let v = rule_violations(&format!(
                "reviews:\n  - id: r1\n    verdict: approved\n    approved_by: pat\n    score: {score}\n"
            ));
            assert!(v.is_empty(), "score {score} is within 1..=5; got: {v:?}");
        }
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
