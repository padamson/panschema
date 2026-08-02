//! First-class A-box instance model.
//!
//! An [`InstanceSet`] is a flat, id-keyed collection of typed instance
//! records — the hub every instance consumer (the instance graph today; RDF
//! and data validation next) goes through, independent of where the
//! instances came from. Today they come from OWL `NamedIndividual`s
//! (`from_owl_annotations`); the LinkML instance-data reader populates the
//! same model.

use crate::linkml::{SchemaDefinition, SlotDefinition};
use std::collections::BTreeMap;

/// A typed reference from one instance to another — an object-property
/// assertion whose value is another instance's identifier (a graph edge).
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    pub property: String,
    /// The target instance's `id` (not its IRI).
    pub target: String,
}

/// A format-neutral scalar value read from instance data, retaining its kind so
/// a validator can check numeric bounds without re-parsing and distinguish a
/// literal from a reference.
#[derive(Debug, Clone, PartialEq)]
pub enum ScalarValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
}

/// One authored value of a slot: a scalar literal, a reference to another
/// instance by its `id`, or a value whose YAML kind didn't fit the slot's range
/// kind (an object where a scalar is declared, or a non-identifier scalar where
/// a class reference is declared). The mismatch is recorded rather than dropped
/// so a validator can flag it; the payload is a human phrase for the actual
/// kind (e.g. `"an object"`).
#[derive(Debug, Clone, PartialEq)]
pub enum InstanceValue {
    Scalar(ScalarValue),
    Reference(String),
    Unexpected(&'static str),
}

/// A slot's authored value(s) on an instance, keyed by slot **name** (not the
/// display label) so a consumer can resolve the slot's constraints. Multivalued
/// slots carry several values.
#[derive(Debug, Clone, PartialEq)]
pub struct SlotValue {
    pub slot: String,
    pub values: Vec<InstanceValue>,
}

/// One A-box instance: a typed record identified by `id`.
#[derive(Debug, Clone, PartialEq)]
pub struct Instance {
    pub id: String,
    /// Full IRI for display (curie-expanded); `None` when unknown.
    pub iri: Option<String>,
    /// `true` when `iri` is a curie whose prefix wasn't declared.
    pub uri_unresolved: bool,
    pub label: String,
    pub description: Option<String>,
    /// Class ids this is an instance of (resolvable to class cards).
    pub types: Vec<String>,
    /// Literal-valued property assertions: `(property label, value)`.
    pub literals: Vec<(String, String)>,
    /// Object-valued assertions to other instances.
    pub references: Vec<Reference>,
    /// The complete authored assignments, keyed by slot name and typed — the
    /// validation view (see ADR-008). Distinct from the display-oriented
    /// `literals`/`references`: this includes the identifier and label slots and
    /// keeps each value's kind. Empty for readers that don't populate it yet
    /// (e.g. the OWL-individual reader).
    pub slot_values: Vec<SlotValue>,
}

/// A flat, id-keyed A-box. Deterministic: instances are sorted by `id`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InstanceSet {
    pub instances: Vec<Instance>,
    /// Identifiers claimed by more than one top-level (collection) record —
    /// the reader dedupes records by id for display, so a validator reads
    /// duplicates here rather than seeing the extra records. Sorted, each id
    /// listed once. Empty for readers that don't track it (e.g. OWL).
    pub duplicate_ids: Vec<String>,
    /// Fields present in the data that the record's class doesn't declare.
    /// These are **not** dropped — they render and emit as properties minted
    /// in the schema's namespace — so a validator needs to see them. Sorted.
    /// Empty for readers that don't track it (e.g. OWL).
    pub undeclared_fields: Vec<UndeclaredField>,
    /// The `tree_root` container's own scalar values — a data file's
    /// top-level `title:` / `description:` and the like, describing the
    /// dataset itself rather than any record. In the data file's order.
    /// Empty for readers without a container (e.g. OWL).
    pub metadata: Vec<(String, String)>,
}

/// A field the data carries that its record's class never declared.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct UndeclaredField {
    /// The record carrying it.
    pub record: String,
    /// The class that doesn't declare it.
    pub class: String,
    /// The field name as written in the data.
    pub field: String,
}

impl InstanceSet {
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Build from the `panschema:individual*` annotations the OWL reader
    /// emits (a worked example authored as `owl:NamedIndividual`s). An
    /// object-valued assertion — whose value is a known individual's IRI —
    /// becomes a typed [`Reference`]; a literal-valued one becomes a literal
    /// assertion.
    pub fn from_owl_annotations(schema: &SchemaDefinition) -> Self {
        use std::collections::HashMap;

        let Some(ids_csv) = schema.annotations.get("panschema:individuals") else {
            return Self::default();
        };
        let ids: Vec<&str> = ids_csv
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();

        // IRI → instance id, so an object assertion (value = target IRI)
        // resolves to the target instance.
        let mut iri_to_id: HashMap<&str, &str> = HashMap::new();
        for id in &ids {
            if let Some(iri) = schema
                .annotations
                .get(&format!("panschema:individual:{id}:_iri"))
            {
                iri_to_id.insert(iri.as_str(), id);
            }
        }

        let mut instances = Vec::new();
        for id in &ids {
            let label = schema
                .annotations
                .get(&format!("panschema:individual:{id}:_label"))
                .cloned()
                .unwrap_or_else(|| capitalize_first(id));

            let types: Vec<String> = schema
                .annotations
                .get(&format!("panschema:individual:{id}"))
                .map(|csv| {
                    csv.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(local_name)
                        .filter(|tid| schema.classes.contains_key(*tid))
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();

            let prefix = format!("panschema:individual:{id}:");
            let mut literals: Vec<(String, String)> = Vec::new();
            let mut references: Vec<Reference> = Vec::new();
            for (key, value) in &schema.annotations {
                let Some(prop) = key.strip_prefix(&prefix) else {
                    continue;
                };
                // Skip reserved sub-keys (`_iri`/`_label`/`_comment`) and the
                // per-property `:_label` companion keys.
                if prop.starts_with('_') || prop.ends_with(":_label") {
                    continue;
                }
                let prop_label = schema
                    .annotations
                    .get(&format!("{key}:_label"))
                    .cloned()
                    .or_else(|| {
                        schema
                            .slots
                            .get(prop)
                            .and_then(|s| s.annotations.get("panschema:label").cloned())
                    })
                    .unwrap_or_else(|| prop.to_string());

                if let Some(target) = iri_to_id.get(value.as_str()) {
                    references.push(Reference {
                        property: prop_label,
                        target: target.to_string(),
                    });
                } else {
                    literals.push((prop_label, value.clone()));
                }
            }
            literals.sort();
            references.sort_by(|a, b| (&a.property, &a.target).cmp(&(&b.property, &b.target)));

            let description = schema
                .annotations
                .get(&format!("panschema:individual:{id}:_comment"))
                .cloned();
            let (iri, uri_unresolved) = crate::graph_writer::resolve_node_uri(
                schema,
                schema
                    .annotations
                    .get(&format!("panschema:individual:{id}:_iri"))
                    .map(String::as_str),
            );

            instances.push(Instance {
                id: id.to_string(),
                iri,
                uri_unresolved,
                label,
                description,
                types,
                literals,
                references,
                // OWL-individual validation isn't a wired use case yet; the
                // display fields above suffice for the instance graph. See
                // ADR-008 ("uneven reader coverage").
                slot_values: Vec::new(),
            });
        }

        instances.sort_by(|a, b| a.id.cmp(&b.id));
        // The OWL individual list is a set of ids; uniqueness tracking is a
        // LinkML-data concern (see ADR-008 "uneven reader coverage").
        Self {
            instances,
            duplicate_ids: Vec::new(),
            undeclared_fields: Vec::new(),
            metadata: Vec::new(),
        }
    }

    /// Build from a LinkML **instance-data file**: a `tree_root` container
    /// object whose slots are typed collections of records conforming to the
    /// schema. Each collection slot's items become records of that slot's
    /// range class; a record's identifier is its `identifier`-slot value or,
    /// for an identifier-keyed collection, its map key. Within a record a
    /// type/enum-ranged value is a literal, and a class-ranged value is a
    /// typed [`Reference`] — a scalar referencing another instance by id (a
    /// graph edge), or an inlined mapping becoming its own nested record plus
    /// an edge to it. Handles both list and identifier-keyed-dict collections.
    pub fn from_linkml_data(schema: &SchemaDefinition, data: &serde_norway::Value) -> Self {
        let Some((root_name, root)) = schema.classes.iter().find(|(_, c)| c.tree_root) else {
            return Self::default();
        };
        let Some(container) = data.as_mapping() else {
            return Self::default();
        };
        let root_slots = crate::linkml_resolve::resolve_effective_slots(root, schema);

        let mut metadata: Vec<(String, String)> = Vec::new();
        let mut loader = LinkmlLoader {
            schema,
            instances: Vec::new(),
            seen: std::collections::HashSet::new(),
            top_level_seen: std::collections::HashSet::new(),
            duplicate_ids: Vec::new(),
            undeclared_fields: Vec::new(),
        };
        // What each container slot held, so a root that is itself a record
        // can reference the records it contains under that slot's name.
        let mut contained: Vec<(String, Vec<String>)> = Vec::new();
        // The root's own non-collection fields, replayed through the normal
        // record builder rather than reimplementing id/label/scalar handling.
        let mut root_fields = serde_norway::Mapping::new();

        for (key, value) in container {
            let Some(slot_name) = key.as_str() else {
                continue;
            };
            let Some(slot) = root_slots.get(slot_name) else {
                continue;
            };
            // Resolve the range the way record ingestion does — a slot that
            // leans on `default_range` (an `identifier` usually does) has no
            // explicit range of its own, and skipping those would drop both
            // the root's id and any such scalar from the metadata.
            let range = slot
                .range
                .clone()
                .or_else(|| schema.default_range.clone())
                .unwrap_or_default();
            // Class-ranged container slots hold instance records; the
            // container's scalar attributes (a catalog title, a
            // description) describe the dataset itself and surface as its
            // metadata rather than vanishing.
            if schema.classes.contains_key(&range) {
                let ids = loader.collect_collection(&range, value);
                contained.push((slot_name.to_string(), ids));
            } else if let Some(scalar) = scalar_value(value) {
                metadata.push((slot_name.to_string(), scalar_to_display(&scalar)));
                root_fields.insert(key.clone(), value.clone());
            }
        }

        // A container that declares an identifier is a domain individual —
        // nimbus's `Enterprise`, not wine's catalogue vessel — so it emits
        // as a record in its own right, referencing what it contains. A
        // vessel has no identifier and stays unemitted.
        if root_slots.values().any(|slot| slot.identifier) {
            let root_value = serde_norway::Value::Mapping(root_fields);
            if let Some(root_id) = loader.build_record(root_name, None, &root_value)
                && let Some(inst) = loader.instances.iter_mut().find(|i| i.id == root_id)
            {
                for (slot, ids) in &contained {
                    for id in ids {
                        inst.references.push(Reference {
                            property: slot.clone(),
                            target: id.clone(),
                        });
                        push_slot_value(
                            &mut inst.slot_values,
                            slot,
                            InstanceValue::Reference(id.clone()),
                        );
                    }
                }
                inst.references
                    .sort_by(|a, b| (&a.property, &a.target).cmp(&(&b.property, &b.target)));
                inst.slot_values.sort_by(|a, b| a.slot.cmp(&b.slot));
            }
        }
        loader.instances.sort_by(|a, b| a.id.cmp(&b.id));
        loader.duplicate_ids.sort();
        loader.undeclared_fields.sort();
        Self {
            instances: loader.instances,
            duplicate_ids: loader.duplicate_ids,
            undeclared_fields: loader.undeclared_fields,
            metadata,
        }
    }
}

/// Walks a LinkML instance-data tree, accumulating typed records.
/// Deduplicates by id so an instance that appears both inline and in a
/// collection is emitted once.
struct LinkmlLoader<'a> {
    schema: &'a SchemaDefinition,
    instances: Vec<Instance>,
    seen: std::collections::HashSet<String>,
    /// Ids of top-level (collection) records, to detect a second record
    /// claiming an identifier already used by another.
    top_level_seen: std::collections::HashSet<String>,
    duplicate_ids: Vec<String>,
    undeclared_fields: Vec<UndeclaredField>,
}

impl LinkmlLoader<'_> {
    /// A collection value is either a list of records or an identifier-keyed
    /// mapping of records.
    /// Build every record in a container slot, returning their ids in order
    /// so an emitted root can reference the records it holds.
    fn collect_collection(&mut self, class_name: &str, value: &serde_norway::Value) -> Vec<String> {
        let mut ids = Vec::new();
        match value {
            serde_norway::Value::Sequence(items) => {
                for item in items {
                    if let Some(id) = self.build_record(class_name, None, item) {
                        ids.push(id.clone());
                        self.note_top_level_id(id);
                    }
                }
            }
            serde_norway::Value::Mapping(map) => {
                for (key, record) in map {
                    if let Some(id) = self.build_record(class_name, key.as_str(), record) {
                        ids.push(id.clone());
                        self.note_top_level_id(id);
                    }
                }
            }
            _ => {}
        }
        ids
    }

    /// Record a top-level record's id; a second use of an id already claimed by
    /// a top-level record is a duplicate identifier (listed once).
    fn note_top_level_id(&mut self, id: String) {
        if !self.top_level_seen.insert(id.clone()) && !self.duplicate_ids.contains(&id) {
            self.duplicate_ids.push(id);
        }
    }

    /// Materialize one record of `class_name` and return its id (so an inlined
    /// object can be referenced by its container). `dict_key`, when present,
    /// is the record's identifier from an identifier-keyed collection.
    fn build_record(
        &mut self,
        class_name: &str,
        dict_key: Option<&str>,
        record: &serde_norway::Value,
    ) -> Option<String> {
        let class = self.schema.classes.get(class_name)?;
        let map = record.as_mapping()?;
        // Resolved *with provenance* so each slot's induced range is available:
        // an `any_of` union has several range targets and no scalar `range:`,
        // so reading `range` alone would see nothing and fall back to the
        // schema's default_range — silently turning references into literals.
        let resolved =
            crate::linkml_resolve::resolve_effective_slots_with_provenance(class, self.schema);
        let slots: BTreeMap<String, SlotDefinition> = resolved
            .iter()
            .map(|(name, rs)| (name.clone(), rs.definition.clone()))
            .collect();

        let id_slot = slots
            .iter()
            .find(|(_, s)| s.identifier)
            .map(|(name, _)| name.clone());
        // A name/label/title slot supplies the display label, LinkML-conventionally.
        let label_slot = slots
            .keys()
            .find(|n| matches!(n.as_str(), "name" | "label" | "title"))
            .cloned();

        let string_field = |name: Option<&str>| {
            name.and_then(|n| map.get(n))
                .and_then(serde_norway::Value::as_str)
                .map(str::to_string)
        };

        let id = dict_key
            .map(str::to_string)
            .or_else(|| string_field(id_slot.as_deref()))
            .or_else(|| string_field(label_slot.as_deref()))
            .unwrap_or_else(|| format!("{class_name}-{}", self.instances.len() + 1));
        let label = string_field(label_slot.as_deref()).unwrap_or_else(|| capitalize_first(&id));

        let mut literals: Vec<(String, String)> = Vec::new();
        let mut references: Vec<Reference> = Vec::new();
        let mut slot_values: Vec<SlotValue> = Vec::new();
        for (field_key, field_value) in map {
            let Some(field) = field_key.as_str() else {
                continue;
            };
            let slot = slots.get(field);
            if slot.is_none() {
                self.undeclared_fields.push(UndeclaredField {
                    record: id.clone(),
                    class: class_name.to_string(),
                    field: field.to_string(),
                });
            }
            // The slot's range targets: the induced set when the schema
            // supplies one (a union contributes every member), else the
            // declared scalar range, else the schema default.
            let induced = resolved
                .get(field)
                .map(|rs| rs.induced.ranges.clone())
                .unwrap_or_default();
            let ranges: Vec<String> = if induced.is_empty() {
                slot.and_then(|s| s.range.clone())
                    .or_else(|| self.schema.default_range.clone())
                    .into_iter()
                    .collect()
            } else {
                induced
            };
            let property = slot
                .and_then(|s| s.annotations.get("panschema:label").cloned())
                .unwrap_or_else(|| field.to_string());
            // The identifier, label, and description slots are recorded in the
            // typed `slot_values` (the validation view needs their presence) but
            // not repeated in the display `literals`/`references`, since the id,
            // label, and description surface as their own fields.
            let display = Some(field) != id_slot.as_deref()
                && Some(field) != label_slot.as_deref()
                && field != "description";
            self.ingest_field(
                field,
                &ranges,
                &property,
                field_value,
                display,
                &mut literals,
                &mut references,
                &mut slot_values,
            );
        }
        // An identifier supplied as an identifier-keyed collection's map key is
        // an authored value too — record it so a validator sees it present.
        if let (Some(key), Some(id_name)) = (dict_key, id_slot.as_deref())
            && !slot_values.iter().any(|sv| sv.slot == id_name)
        {
            slot_values.push(SlotValue {
                slot: id_name.to_string(),
                values: vec![InstanceValue::Scalar(ScalarValue::String(key.to_string()))],
            });
        }
        literals.sort();
        references.sort_by(|a, b| (&a.property, &a.target).cmp(&(&b.property, &b.target)));
        slot_values.sort_by(|a, b| a.slot.cmp(&b.slot));

        if self.seen.insert(id.clone()) {
            self.instances.push(Instance {
                id: id.clone(),
                iri: None,
                uri_unresolved: false,
                label,
                description: string_field(Some("description")),
                types: vec![class_name.to_string()],
                literals,
                references,
                slot_values,
            });
        }
        Some(id)
    }

    /// Route one slot value into the typed `slot_values` (always) and, when
    /// `display`, the display `literals`/`references`. A scalar becomes a
    /// literal; a class-ranged scalar an id reference, a class-ranged mapping a
    /// nested record plus a reference. Recurses over sequence elements.
    #[allow(clippy::too_many_arguments)]
    /// Record one authored field value, classified by the slot's range
    /// targets.
    ///
    /// A union of classes makes string values references, so the instance
    /// graph draws edges and the integrity pass can check the targets. A
    /// union mixing classes with types or enums keeps strings as scalars —
    /// a string could legitimately be either, and validate resolves the
    /// ambiguity per branch. An inlined object is only built when exactly
    /// one range target is a class; with several it is ambiguous which one
    /// the author meant.
    fn ingest_field(
        &mut self,
        slot: &str,
        ranges: &[String],
        property: &str,
        value: &serde_norway::Value,
        display: bool,
        literals: &mut Vec<(String, String)>,
        references: &mut Vec<Reference>,
        slot_values: &mut Vec<SlotValue>,
    ) {
        if let serde_norway::Value::Sequence(items) = value {
            for item in items {
                self.ingest_field(
                    slot,
                    ranges,
                    property,
                    item,
                    display,
                    literals,
                    references,
                    slot_values,
                );
            }
            return;
        }
        // A null carries no value — treat as absent, not a kind mismatch.
        if matches!(value, serde_norway::Value::Null) {
            return;
        }

        let class_targets: Vec<&String> = ranges
            .iter()
            .filter(|r| self.schema.classes.contains_key(*r))
            .collect();
        let all_classes = !class_targets.is_empty() && class_targets.len() == ranges.len();

        let reference_to =
            |target: String, references: &mut Vec<Reference>, slot_values: &mut Vec<SlotValue>| {
                push_slot_value(slot_values, slot, InstanceValue::Reference(target.clone()));
                if display {
                    references.push(Reference {
                        property: property.to_string(),
                        target,
                    });
                }
            };

        match value {
            // An inlined mapping is its own record; recurse and edge to it.
            // With several class targets it is ambiguous which one the author
            // meant, so it falls through to be recorded as unusable.
            serde_norway::Value::Mapping(_) if class_targets.len() == 1 => {
                let class = class_targets[0].clone();
                if let Some(target) = self.build_record(&class, None, value) {
                    reference_to(target, references, slot_values);
                }
            }
            // A string at an all-class range references a record by id.
            serde_norway::Value::String(text) if all_classes => {
                reference_to(text.clone(), references, slot_values);
            }
            // A number or boolean can never be a reference — record the
            // mismatch rather than dropping it, keeping the display
            // references well-formed.
            other if all_classes => push_slot_value(
                slot_values,
                slot,
                InstanceValue::Unexpected(yaml_kind(other)),
            ),
            // Scalar range, or a union mixing classes with types/enums: keep
            // the value as authored and let validation judge the branches.
            other => {
                if let Some(scalar) = scalar_value(other) {
                    if display {
                        literals.push((property.to_string(), scalar_to_display(&scalar)));
                    }
                    push_slot_value(slot_values, slot, InstanceValue::Scalar(scalar));
                } else {
                    push_slot_value(
                        slot_values,
                        slot,
                        InstanceValue::Unexpected(yaml_kind(other)),
                    );
                }
            }
        }
    }
}

/// A human phrase for a YAML value's kind, for a range-kind-mismatch message.
/// Only reached for a value that fit neither a scalar nor a reference: a
/// number/boolean at a class-ranged slot, or an object at a scalar-ranged slot.
/// (Null is treated as absent, and a sequence is flattened, before this.)
fn yaml_kind(value: &serde_norway::Value) -> &'static str {
    match value {
        serde_norway::Value::Bool(_) => "a boolean",
        serde_norway::Value::Number(_) => "a number",
        serde_norway::Value::Mapping(_) => "an object",
        _ => "a value",
    }
}

/// Append `value` to the `slot`'s entry in `slot_values`, grouping a
/// multivalued slot's elements under one [`SlotValue`].
fn push_slot_value(slot_values: &mut Vec<SlotValue>, slot: &str, value: InstanceValue) {
    if let Some(sv) = slot_values.iter_mut().find(|sv| sv.slot == slot) {
        sv.values.push(value);
    } else {
        slot_values.push(SlotValue {
            slot: slot.to_string(),
            values: vec![value],
        });
    }
}

/// A format-neutral typed scalar from a YAML value; non-scalars yield `None`.
fn scalar_value(value: &serde_norway::Value) -> Option<ScalarValue> {
    match value {
        serde_norway::Value::String(s) => Some(ScalarValue::String(s.clone())),
        serde_norway::Value::Bool(b) => Some(ScalarValue::Boolean(*b)),
        serde_norway::Value::Number(n) => n
            .as_i64()
            .map(ScalarValue::Integer)
            .or_else(|| n.as_f64().map(ScalarValue::Float)),
        _ => None,
    }
}

/// Render a typed scalar as its display string.
pub(crate) fn scalar_to_display(value: &ScalarValue) -> String {
    match value {
        ScalarValue::String(s) => s.clone(),
        ScalarValue::Integer(i) => i.to_string(),
        ScalarValue::Float(f) => f.to_string(),
        ScalarValue::Boolean(b) => b.to_string(),
    }
}

/// Local name of an IRI: the part after the last `#` or `/`, else the whole
/// string. Resolves a type IRI to a class id.
fn local_name(iri: &str) -> &str {
    iri.rsplit(['#', '/']).next().unwrap_or(iri)
}

/// Capitalize the first character (ASCII), leaving the rest untouched — the
/// display label fallback when an individual has no `rdfs:label`.
fn capitalize_first(id: &str) -> String {
    let mut chars = id.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::Reader;
    use crate::linkml::ClassDefinition;
    use crate::owl_reader::OwlReader;

    #[test]
    fn empty_when_the_schema_has_no_individuals() {
        let schema = SchemaDefinition::new("s");
        assert!(InstanceSet::from_owl_annotations(&schema).is_empty());
    }

    #[test]
    fn from_owl_annotations_builds_typed_records_with_refs_and_literals() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/instance_graph.ttl");
        let schema = OwlReader::new().read(&fixture).expect("read fixture");
        let set = InstanceSet::from_owl_annotations(&schema);

        assert!(
            !set.is_empty(),
            "a schema with individuals yields a non-empty set"
        );
        assert_eq!(set.instances.len(), 3, "three individuals → three records");

        let wine = set
            .instances
            .iter()
            .find(|i| i.id == "chateauMorgon")
            .expect("wine instance");
        assert_eq!(wine.types, ["Wine"], "typed as its rdf:type class");
        // The object assertion is a typed reference (an edge), by target id.
        assert_eq!(wine.references.len(), 1);
        assert_eq!(wine.references[0].property, "from region");
        assert_eq!(wine.references[0].target, "beaujolais");
        // The datatype assertion is a literal, not a reference.
        assert_eq!(wine.literals, [("color".to_string(), "red".to_string())]);

        // An individual with no rdfs:label gets the capitalize-first label.
        let napa = set.instances.iter().find(|i| i.id == "napa").expect("napa");
        assert_eq!(napa.label, "Napa");
    }

    /// A `tree_root` container schema whose slots are typed collections of
    /// records — the canonical LinkML instance-data shape the reader ingests.
    const WINE_SCHEMA: &str = "\
name: WineCatalog
classes:
  WineCatalog:
    tree_root: true
    attributes:
      title:
        range: string
      description:
        range: string
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
        range: string
      color:
        range: string
      produced_by:
        range: Winery
  Winery:
    attributes:
      id:
        identifier: true
      name:
        range: string
";

    fn wine_schema() -> SchemaDefinition {
        serde_norway::from_str(WINE_SCHEMA).expect("parse wine schema")
    }

    /// A schema shaped like a provenance layer whose acts and states point at
    /// each other through `any_of` class unions with no outer `range:` — the
    /// polymorphic-range case. `qualifies` is never narrowed; `hasInput` is
    /// narrowed per subclass, once to a scalar range and once to a smaller
    /// union.
    const UNION_SCHEMA: &str = "\
name: provenance
default_range: string
slots:
  hasInput:
    multivalued: true
    any_of:
      - range: Question
      - range: SourceDocument
  qualifies:
    any_of:
      - range: Claim
      - range: Method
classes:
  ProvenanceRecord:
    tree_root: true
    attributes:
      acts: {range: Act, multivalued: true}
      searches: {range: LiteratureSearch, multivalued: true}
      extractions: {range: Extraction, multivalued: true}
      states: {range: State, multivalued: true}
      questions: {range: Question, multivalued: true}
      docs: {range: SourceDocument, multivalued: true}
      claims: {range: Claim, multivalued: true}
      hypotheses: {range: Hypothesis, multivalued: true}
      methods: {range: Method, multivalued: true}
  Act:
    attributes:
      id: {identifier: true}
    slots: [hasInput]
  LiteratureSearch:
    is_a: Act
    slot_usage:
      hasInput: {range: Question}
  Extraction:
    is_a: Act
    slot_usage:
      hasInput:
        any_of:
          - range: SourceDocument
          - range: Question
  State:
    attributes:
      id: {identifier: true}
    slots: [qualifies]
  Question:
    attributes:
      id: {identifier: true}
  SourceDocument:
    attributes:
      id: {identifier: true}
  Claim:
    attributes:
      id: {identifier: true}
  Hypothesis:
    is_a: Claim
  Method:
    attributes:
      id: {identifier: true}
";

    /// Parse a fixture the way the reader delivers a schema: names back-filled
    /// from their map keys. Deserializing the literal alone leaves every
    /// `name` empty, which is not a shape production code ever sees.
    fn union_schema() -> SchemaDefinition {
        let mut schema: SchemaDefinition =
            serde_norway::from_str(UNION_SCHEMA).expect("parse union schema");
        let named: Vec<(String, ClassDefinition)> = schema
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

    fn union_set(yaml: &str) -> InstanceSet {
        let data: serde_norway::Value = serde_norway::from_str(yaml).expect("parse data");
        InstanceSet::from_linkml_data(&union_schema(), &data)
    }

    fn find<'a>(set: &'a InstanceSet, id: &str) -> &'a Instance {
        set.instances
            .iter()
            .find(|i| i.id == id)
            .unwrap_or_else(|| {
                panic!(
                    "no record `{id}`; available: {:?}",
                    set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
                )
            })
    }

    const UNION_DATA: &str = "\
acts:
  - {id: a1, hasInput: [q1, d1]}
states:
  - {id: s1, qualifies: c1}
questions:
  - {id: q1}
docs:
  - {id: d1}
claims:
  - {id: c1}
";

    /// A container that is a real domain individual — it declares an
    /// identifier — in the shape nimbus's `Enterprise` root takes.
    const ROOT_SCHEMA: &str = "\
name: estate
default_range: string
classes:
  Enterprise:
    tree_root: true
    attributes:
      id: {identifier: true}
      name: {range: string}
      deployments: {range: Deployment, multivalued: true}
  Deployment:
    attributes:
      id: {identifier: true}
";

    fn root_schema(with_identifier: bool) -> SchemaDefinition {
        let mut schema: SchemaDefinition =
            serde_norway::from_str(ROOT_SCHEMA).expect("parse root schema");
        let named: Vec<(String, ClassDefinition)> = schema
            .classes
            .iter()
            .map(|(key, class)| {
                let mut c = class.clone();
                c.name = key.clone();
                (key.clone(), c)
            })
            .collect();
        schema.classes = named.into_iter().collect();
        if !with_identifier {
            // A pure vessel: same container, no identifier slot.
            let root = schema.classes.get_mut("Enterprise").expect("Enterprise");
            root.attributes.remove("id");
        }
        schema
    }

    const ROOT_DATA: &str = "\
id: acme
name: Acme Corp
deployments:
  - {id: d1}
  - {id: d2}
";

    #[test]
    fn a_tree_root_that_declares_an_identifier_is_itself_a_record() {
        let data: serde_norway::Value = serde_norway::from_str(ROOT_DATA).expect("parse");
        let set = InstanceSet::from_linkml_data(&root_schema(true), &data);

        let root = set
            .instances
            .iter()
            .find(|i| i.id == "acme")
            .unwrap_or_else(|| {
                panic!(
                    "the root must emit as a record; got: {:?}",
                    set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
                )
            });
        assert_eq!(root.types, vec!["Enterprise".to_string()]);
        assert_eq!(root.label, "Acme Corp", "its label slot still applies");
        assert_eq!(
            set.instances.len(),
            3,
            "the root joins the records it contains, without duplicating them"
        );
    }

    #[test]
    fn the_root_references_the_records_it_contains() {
        // A collection slot on the root is a declared slot with a class
        // range like any other, so it draws edges under its own predicate.
        let data: serde_norway::Value = serde_norway::from_str(ROOT_DATA).expect("parse");
        let set = InstanceSet::from_linkml_data(&root_schema(true), &data);
        let root = set.instances.iter().find(|i| i.id == "acme").expect("root");
        let mut edges: Vec<(&str, &str)> = root
            .references
            .iter()
            .map(|r| (r.property.as_str(), r.target.as_str()))
            .collect();
        edges.sort_unstable();
        assert_eq!(
            edges,
            vec![("deployments", "d1"), ("deployments", "d2")],
            "each contained record is referenced under the slot that holds it"
        );
    }

    #[test]
    fn a_container_without_an_identifier_stays_unemitted() {
        // A pure vessel — wine's catalogue shape — must not start producing
        // a spurious node just because the root is now emittable.
        let data: serde_norway::Value = serde_norway::from_str(ROOT_DATA).expect("parse");
        let set = InstanceSet::from_linkml_data(&root_schema(false), &data);
        assert_eq!(
            set.instances.len(),
            2,
            "only the contained records; got: {:?}",
            set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
        );
        assert!(!set.instances.iter().any(|i| i.id == "acme"));
    }

    #[test]
    fn an_emitted_root_keeps_its_scalars_as_dataset_metadata() {
        // Emitting the record must not cost the metadata block its content.
        let data: serde_norway::Value = serde_norway::from_str(ROOT_DATA).expect("parse");
        let set = InstanceSet::from_linkml_data(&root_schema(true), &data);
        assert!(
            set.metadata
                .iter()
                .any(|(k, v)| k == "name" && v == "Acme Corp"),
            "got: {:?}",
            set.metadata
        );
    }

    #[test]
    fn un_narrowed_any_of_union_values_ingest_as_references() {
        let set = union_set(UNION_DATA);
        let a1 = find(&set, "a1");
        let mut targets: Vec<&str> = a1.references.iter().map(|r| r.target.as_str()).collect();
        targets.sort_unstable();
        assert_eq!(
            targets,
            vec!["d1", "q1"],
            "an any_of union of classes makes its values references, not literals"
        );
        let has_input = a1
            .slot_values
            .iter()
            .find(|sv| sv.slot == "hasInput")
            .expect("hasInput recorded");
        assert!(
            has_input
                .values
                .iter()
                .all(|v| matches!(v, InstanceValue::Reference(_))),
            "got: {:?}",
            has_input.values
        );

        let s1 = find(&set, "s1");
        assert_eq!(
            s1.references
                .iter()
                .map(|r| (r.property.as_str(), r.target.as_str()))
                .collect::<Vec<_>>(),
            vec![("qualifies", "c1")],
            "a never-narrowed union slot references too"
        );
    }

    #[test]
    fn an_inlined_object_at_a_multi_class_union_is_reported_not_guessed() {
        // With several class members it is ambiguous which one an inlined
        // object instantiates, so the value is recorded as unusable rather
        // than silently built as one of them.
        let set = union_set("acts:\n  - {id: a1, hasInput: [{id: x1}]}\n");
        let a1 = find(&set, "a1");
        let has_input = a1
            .slot_values
            .iter()
            .find(|sv| sv.slot == "hasInput")
            .expect("hasInput recorded");
        assert!(
            matches!(has_input.values.as_slice(), [InstanceValue::Unexpected(_)]),
            "an ambiguous inlined object is recorded as unusable; got: {:?}",
            has_input.values
        );
        assert!(
            a1.references.is_empty(),
            "and draws no edge; got: {:?}",
            a1.references
        );
        assert!(
            !set.instances.iter().any(|i| i.id == "x1"),
            "nor is a record invented for it"
        );
    }

    #[test]
    fn an_inlined_object_at_a_single_class_range_is_built() {
        // The unambiguous counterpart: exactly one class member, so the
        // object is built and linked.
        let set = union_set("searches:\n  - {id: ls1, hasInput: {id: q9}}\n");
        assert_eq!(
            find(&set, "ls1")
                .references
                .iter()
                .map(|r| r.target.as_str())
                .collect::<Vec<_>>(),
            vec!["q9"],
            "a single class target makes the inlined object a record"
        );
        assert!(set.instances.iter().any(|i| i.id == "q9"), "and it exists");
    }

    #[test]
    fn slot_usage_any_of_narrowing_ingests_references() {
        let set = union_set(
            "\
searches:
  - {id: ls1, hasInput: q1}
extractions:
  - {id: ex1, hasInput: [d1]}
questions:
  - {id: q1}
docs:
  - {id: d1}
",
        );
        assert_eq!(
            find(&set, "ls1")
                .references
                .iter()
                .map(|r| r.target.as_str())
                .collect::<Vec<_>>(),
            vec!["q1"],
            "a slot_usage scalar narrowing still yields a reference"
        );
        assert_eq!(
            find(&set, "ex1")
                .references
                .iter()
                .map(|r| r.target.as_str())
                .collect::<Vec<_>>(),
            vec!["d1"],
            "a slot_usage any_of narrowing yields a reference"
        );
    }

    #[test]
    fn container_scalar_slots_become_dataset_metadata() {
        // A data file's top-level `title:`/`description:` describe the
        // dataset itself. They are not records, but they must not vanish —
        // they surface as the dataset's metadata.
        let schema = wine_schema();
        let data: serde_norway::Value = serde_norway::from_str(
            "title: Tasting catalog\ndescription: A curated cellar\nwines: []\nwineries: []\n",
        )
        .expect("parse data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert_eq!(
            set.metadata,
            vec![
                ("title".to_string(), "Tasting catalog".to_string()),
                ("description".to_string(), "A curated cellar".to_string()),
            ],
            "container scalars, in the data file's order"
        );
    }

    #[test]
    fn a_dataset_without_container_scalars_has_no_metadata() {
        let schema = wine_schema();
        let data: serde_norway::Value =
            serde_norway::from_str("wines: []\nwineries: []\n").expect("parse data");
        assert!(
            InstanceSet::from_linkml_data(&schema, &data)
                .metadata
                .is_empty()
        );
    }

    #[test]
    fn empty_when_the_data_has_no_container_records() {
        let schema = wine_schema();
        let data: serde_norway::Value =
            serde_norway::from_str("wines: []\nwineries: []\n").expect("parse data");
        assert!(InstanceSet::from_linkml_data(&schema, &data).is_empty());
    }

    #[test]
    fn from_linkml_data_reads_tree_root_container_records() {
        let schema = wine_schema();
        let data: serde_norway::Value = serde_norway::from_str(
            "\
wines:
  - id: chateauMorgon
    name: Château Morgon
    color: red
    produced_by: morgonEstate
wineries:
  - id: morgonEstate
    name: Morgon Estate
",
        )
        .expect("parse data");

        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert_eq!(set.instances.len(), 2, "two records → two instances");

        // Deterministic id ordering, like the OWL path.
        assert_eq!(set.instances[0].id, "chateauMorgon");
        assert_eq!(set.instances[1].id, "morgonEstate");

        let wine = &set.instances[0];
        assert_eq!(wine.types, ["Wine"], "typed by its container slot's range");
        assert_eq!(wine.label, "Château Morgon", "the name slot is the label");
        // A class-ranged scalar value is a typed reference (an edge), by id.
        assert_eq!(wine.references.len(), 1);
        assert_eq!(wine.references[0].property, "produced_by");
        assert_eq!(wine.references[0].target, "morgonEstate");
        // A type-ranged value is a literal; the identifier and label slots are
        // not repeated as literals.
        assert_eq!(wine.literals, [("color".to_string(), "red".to_string())]);

        // The typed, slot-keyed validation view (ADR-008) records *every*
        // authored slot — including the id and name the display fields consume —
        // keyed by slot name and sorted, with the class-ranged value as a
        // reference rather than a scalar.
        assert_eq!(
            wine.slot_values,
            vec![
                SlotValue {
                    slot: "color".to_string(),
                    values: vec![InstanceValue::Scalar(ScalarValue::String(
                        "red".to_string()
                    ))],
                },
                SlotValue {
                    slot: "id".to_string(),
                    values: vec![InstanceValue::Scalar(ScalarValue::String(
                        "chateauMorgon".to_string()
                    ))],
                },
                SlotValue {
                    slot: "name".to_string(),
                    values: vec![InstanceValue::Scalar(ScalarValue::String(
                        "Château Morgon".to_string()
                    ))],
                },
                SlotValue {
                    slot: "produced_by".to_string(),
                    values: vec![InstanceValue::Reference("morgonEstate".to_string())],
                },
            ]
        );

        let winery = &set.instances[1];
        assert_eq!(winery.types, ["Winery"]);
        assert_eq!(winery.label, "Morgon Estate");
        assert!(winery.references.is_empty());
    }

    #[test]
    fn from_linkml_data_handles_inlined_as_dict_collection() {
        let schema = wine_schema();
        // wineries as an identifier-keyed mapping (CompactDict), not a list.
        let data: serde_norway::Value = serde_norway::from_str(
            "\
wineries:
  morgonEstate:
    name: Morgon Estate
",
        )
        .expect("parse data");

        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert_eq!(set.instances.len(), 1);
        assert_eq!(set.instances[0].id, "morgonEstate", "the map key is the id");
        assert_eq!(set.instances[0].label, "Morgon Estate");
        // The identifier supplied as the map key is recorded in slot_values, so
        // a validator sees the identifier slot present (ADR-008).
        assert_eq!(
            set.instances[0].slot_values,
            vec![
                SlotValue {
                    slot: "id".to_string(),
                    values: vec![InstanceValue::Scalar(ScalarValue::String(
                        "morgonEstate".to_string()
                    ))],
                },
                SlotValue {
                    slot: "name".to_string(),
                    values: vec![InstanceValue::Scalar(ScalarValue::String(
                        "Morgon Estate".to_string()
                    ))],
                },
            ]
        );
    }

    #[test]
    fn empty_without_a_tree_root_class() {
        let mut schema = wine_schema();
        for class in schema.classes.values_mut() {
            class.tree_root = false;
        }
        let data: serde_norway::Value =
            serde_norway::from_str("wines:\n  - id: x\n").expect("parse data");
        assert!(InstanceSet::from_linkml_data(&schema, &data).is_empty());
    }

    /// Multivalued slots, the description field, and the no-name label
    /// fallback — the branches the wine happy-path doesn't exercise.
    #[test]
    fn from_linkml_data_handles_multivalued_slots_and_description() {
        const SCHEMA: &str = "\
name: Graph
default_range: string
classes:
  Container:
    tree_root: true
    attributes:
      nodes:
        range: Node
        multivalued: true
  Node:
    attributes:
      id:
        identifier: true
      description: {}
      active:
        range: boolean
      score:
        range: integer
      weight:
        range: float
      tags:
        range: string
        multivalued: true
      links:
        range: Node
        multivalued: true
";
        let schema: SchemaDefinition = serde_norway::from_str(SCHEMA).expect("schema");
        let data: serde_norway::Value = serde_norway::from_str(
            "\
nodes:
  - id: a
    description: The first node.
    active: true
    score: 5
    weight: 1.5
    tags:
      - alpha
      - beta
    links:
      - b
      - c
      - id: d
  - id: b
  - id: c
",
        )
        .expect("data");

        let set = InstanceSet::from_linkml_data(&schema, &data);
        // The inlined object under `links` becomes its own record.
        assert_eq!(set.instances.len(), 4, "a, b, c, and the inlined d");
        assert!(
            set.instances.iter().any(|i| i.id == "d"),
            "inlined d exists"
        );

        let a = set.instances.iter().find(|i| i.id == "a").expect("node a");

        // A record with no name/label/title slot falls back to a
        // capitalize-first label of its id.
        assert_eq!(a.label, "A");
        // The description field is captured once, as the record's description —
        // not duplicated into the literal assertions.
        assert_eq!(a.description.as_deref(), Some("The first node."));
        // Boolean and numeric scalars render as literal assertions, alongside
        // one literal per element of a multivalued type-ranged slot.
        assert_eq!(
            a.literals,
            [
                ("active".to_string(), "true".to_string()),
                ("score".to_string(), "5".to_string()),
                ("tags".to_string(), "alpha".to_string()),
                ("tags".to_string(), "beta".to_string()),
                ("weight".to_string(), "1.5".to_string()),
            ]
        );
        // A multivalued class-ranged slot yields one reference edge per element,
        // including the inlined object (edged to by its id).
        assert_eq!(
            a.references.len(),
            3,
            "two id refs + one inlined → three edges"
        );
        assert_eq!(a.references[0].target, "b");
        assert_eq!(a.references[1].target, "c");
        assert_eq!(a.references[2].target, "d");
        assert!(
            a.references.iter().all(|r| r.property == "links"),
            "each edge carries the slot as its property label"
        );

        // The typed slot_values retain each scalar's kind (bool, integer) and
        // group a multivalued slot's elements under one entry.
        let slot = |name: &str| a.slot_values.iter().find(|sv| sv.slot == name).cloned();
        assert_eq!(
            slot("active").expect("active").values,
            [InstanceValue::Scalar(ScalarValue::Boolean(true))]
        );
        assert_eq!(
            slot("score").expect("score").values,
            [InstanceValue::Scalar(ScalarValue::Integer(5))]
        );
        assert_eq!(
            slot("weight").expect("weight").values,
            [InstanceValue::Scalar(ScalarValue::Float(1.5))]
        );
        assert_eq!(
            slot("tags").expect("tags").values,
            [
                InstanceValue::Scalar(ScalarValue::String("alpha".to_string())),
                InstanceValue::Scalar(ScalarValue::String("beta".to_string())),
            ],
            "a multivalued slot's elements group under one entry"
        );
        assert_eq!(
            slot("links").expect("links").values,
            [
                InstanceValue::Reference("b".to_string()),
                InstanceValue::Reference("c".to_string()),
                InstanceValue::Reference("d".to_string()),
            ]
        );
    }
}
