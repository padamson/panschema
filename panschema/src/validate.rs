//! Native LinkML instance-data validator.
//!
//! Checks a LinkML **instance-data** file (an A-box — a `tree_root` container
//! of records) against its schema's constraints and reports every violation.
//! It walks the raw data tree against each class's effective slots directly,
//! rather than the display-oriented [`crate::instances::InstanceSet`] (which
//! stringifies values) or the still-incomplete JSON-Schema projection — so it
//! sees the untouched typed values later slices need for `pattern`/bounds/enum
//! checks. Cross-record reference integrity reuses the check the instance
//! reader already provides.

use crate::instances::InstanceSet;
use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};
use crate::linkml_resolve::{effective_cardinality, resolve_effective_slots};
use serde_yaml::Value;
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

/// Validate an instance-data tree against `schema`, returning every violation
/// (empty when the data conforms). Deterministic: violations are ordered by
/// their walk of the container, then reference-integrity violations, so output
/// is stable across runs.
///
/// Slice 1 checks: a required slot absent from a record, and a reference whose
/// target names no record in the data. A data file that isn't a mapping yields
/// a single structural violation rather than panicking.
pub fn validate_instance_data(schema: &SchemaDefinition, data: &Value) -> Vec<Violation> {
    let mut out = Vec::new();

    let Some(container) = data.as_mapping() else {
        out.push(Violation {
            record: "(root)".to_string(),
            detail: "instance data must be a mapping (a tree_root container object)".to_string(),
        });
        return out;
    };

    if let Some(root) = schema.classes.values().find(|c| c.tree_root) {
        let root_slots = resolve_effective_slots(root, schema);
        for (key, value) in container {
            let Some(slot_name) = key.as_str() else {
                continue;
            };
            let Some(range) = root_slots.get(slot_name).and_then(|s| s.range.as_deref()) else {
                continue;
            };
            if let Some(class) = schema.classes.get(range) {
                check_collection(schema, range, class, value, &mut out);
            }
        }
    }

    // Cross-record reference integrity: a typed reference to an id no record
    // defines. Reuses the reader's model (stringified ids are sufficient here).
    let set = InstanceSet::from_linkml_data(schema, data);
    for d in crate::diagnostics::dangling_instance_references(&set) {
        out.push(Violation {
            record: d.referrer.clone(),
            detail: d.detail(),
        });
    }

    out
}

/// A collection value is either a list of records or an identifier-keyed
/// mapping of records; validate each record of `class`.
fn check_collection(
    schema: &SchemaDefinition,
    class_name: &str,
    class: &ClassDefinition,
    value: &Value,
    out: &mut Vec<Violation>,
) {
    match value {
        Value::Sequence(items) => {
            for (index, item) in items.iter().enumerate() {
                check_record(schema, class_name, class, None, index, item, out);
            }
        }
        Value::Mapping(map) => {
            for (index, (key, record)) in map.iter().enumerate() {
                check_record(schema, class_name, class, key.as_str(), index, record, out);
            }
        }
        _ => {}
    }
}

/// Check one record of `class` for required-slot presence. `dict_key`, when
/// present, is the record's identifier from an identifier-keyed collection (so
/// a required identifier slot supplied as the map key isn't flagged absent).
fn check_record(
    schema: &SchemaDefinition,
    class_name: &str,
    class: &ClassDefinition,
    dict_key: Option<&str>,
    index: usize,
    record: &Value,
    out: &mut Vec<Violation>,
) {
    let Some(map) = record.as_mapping() else {
        // A non-mapping record (a bare scalar where an object is expected) is a
        // kind mismatch — reported in a later slice; nothing to check here.
        return;
    };
    let slots = resolve_effective_slots(class, schema);

    let identifier = slots
        .iter()
        .find(|(_, s)| s.identifier)
        .map(|(name, _)| name.clone());
    let record_id = record_identifier(class_name, index, dict_key, &identifier, map);

    for (slot_name, slot) in &slots {
        if is_required(slot) && !slot_present(map, slot_name, slot, dict_key) {
            out.push(Violation {
                record: record_id.clone(),
                detail: format!("required slot `{slot_name}` (class `{class_name}`) is absent"),
            });
        }
    }
}

/// A stable label for a record: its identifier value, else the identifier-keyed
/// map key, else a positional `Class#N`.
fn record_identifier(
    class_name: &str,
    index: usize,
    dict_key: Option<&str>,
    identifier: &Option<String>,
    map: &serde_yaml::Mapping,
) -> String {
    dict_key
        .map(str::to_string)
        .or_else(|| {
            identifier
                .as_deref()
                .and_then(|n| map.get(n))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("{class_name}#{}", index + 1))
}

fn is_required(slot: &SlotDefinition) -> bool {
    effective_cardinality(slot).required
}

/// Whether a slot's value is present on a record. The identifier slot counts as
/// present when supplied as an identifier-keyed collection's map key.
fn slot_present(
    map: &serde_yaml::Mapping,
    slot_name: &str,
    slot: &SlotDefinition,
    dict_key: Option<&str>,
) -> bool {
    map.contains_key(slot_name) || (slot.identifier && dict_key.is_some())
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
        serde_yaml::from_str(SCHEMA).expect("parse schema")
    }

    fn data(yaml: &str) -> Value {
        serde_yaml::from_str(yaml).expect("parse data")
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
}
