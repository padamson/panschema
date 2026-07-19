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
use crate::linkml::{SchemaDefinition, SlotDefinition};
use crate::linkml_resolve::{effective_cardinality, resolve_effective_slots};
use serde_yaml::Value;
use std::collections::HashSet;
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

    for inst in &set.instances {
        // A record's class is the collection slot's range that produced it.
        let Some(class_name) = inst.types.first() else {
            continue;
        };
        let Some(class) = schema.classes.get(class_name) else {
            continue;
        };
        let present: HashSet<&str> = inst.slot_values.iter().map(|sv| sv.slot.as_str()).collect();
        for (slot_name, slot) in &resolve_effective_slots(class, schema) {
            if is_required(slot) && !present.contains(slot_name.as_str()) {
                out.push(Violation {
                    record: inst.id.clone(),
                    detail: format!("required slot `{slot_name}` (class `{class_name}`) is absent"),
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

fn is_required(slot: &SlotDefinition) -> bool {
    effective_cardinality(slot).required
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
