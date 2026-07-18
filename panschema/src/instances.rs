//! First-class A-box instance model.
//!
//! An [`InstanceSet`] is a flat, id-keyed collection of typed instance
//! records — the hub every instance consumer (the instance graph today; RDF
//! and data validation next) goes through, independent of where the
//! instances came from. Today they come from OWL `NamedIndividual`s
//! (`from_owl_annotations`); the LinkML instance-data reader populates the
//! same model.

use crate::linkml::SchemaDefinition;

/// A typed reference from one instance to another — an object-property
/// assertion whose value is another instance's identifier (a graph edge).
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    pub property: String,
    /// The target instance's `id` (not its IRI).
    pub target: String,
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
}

/// A flat, id-keyed A-box. Deterministic: instances are sorted by `id`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InstanceSet {
    pub instances: Vec<Instance>,
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
            });
        }

        instances.sort_by(|a, b| a.id.cmp(&b.id));
        Self { instances }
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
    pub fn from_linkml_data(schema: &SchemaDefinition, data: &serde_yaml::Value) -> Self {
        let Some(root) = schema.classes.values().find(|c| c.tree_root) else {
            return Self::default();
        };
        let Some(container) = data.as_mapping() else {
            return Self::default();
        };
        let root_slots = crate::linkml_resolve::resolve_effective_slots(root, schema);

        let mut loader = LinkmlLoader {
            schema,
            instances: Vec::new(),
            seen: std::collections::HashSet::new(),
        };
        for (key, value) in container {
            let Some(slot_name) = key.as_str() else {
                continue;
            };
            let Some(range) = root_slots.get(slot_name).and_then(|s| s.range.as_deref()) else {
                continue;
            };
            // Only class-ranged container slots hold instance records; a scalar
            // attribute on the container (e.g. a catalog title) is not one.
            if schema.classes.contains_key(range) {
                loader.collect_collection(range, value);
            }
        }
        loader.instances.sort_by(|a, b| a.id.cmp(&b.id));
        Self {
            instances: loader.instances,
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
}

impl LinkmlLoader<'_> {
    /// A collection value is either a list of records or an identifier-keyed
    /// mapping of records.
    fn collect_collection(&mut self, class_name: &str, value: &serde_yaml::Value) {
        match value {
            serde_yaml::Value::Sequence(items) => {
                for item in items {
                    self.build_record(class_name, None, item);
                }
            }
            serde_yaml::Value::Mapping(map) => {
                for (key, record) in map {
                    self.build_record(class_name, key.as_str(), record);
                }
            }
            _ => {}
        }
    }

    /// Materialize one record of `class_name` and return its id (so an inlined
    /// object can be referenced by its container). `dict_key`, when present,
    /// is the record's identifier from an identifier-keyed collection.
    fn build_record(
        &mut self,
        class_name: &str,
        dict_key: Option<&str>,
        record: &serde_yaml::Value,
    ) -> Option<String> {
        let class = self.schema.classes.get(class_name)?;
        let map = record.as_mapping()?;
        let slots = crate::linkml_resolve::resolve_effective_slots(class, self.schema);

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
                .and_then(serde_yaml::Value::as_str)
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
        for (field_key, field_value) in map {
            let Some(field) = field_key.as_str() else {
                continue;
            };
            // The identifier, label, and description slots are captured above,
            // not repeated as literal assertions.
            if Some(field) == id_slot.as_deref()
                || Some(field) == label_slot.as_deref()
                || field == "description"
            {
                continue;
            }
            let slot = slots.get(field);
            let range = slot
                .and_then(|s| s.range.clone())
                .or_else(|| self.schema.default_range.clone());
            let property = slot
                .and_then(|s| s.annotations.get("panschema:label").cloned())
                .unwrap_or_else(|| field.to_string());
            self.collect_field(
                range.as_deref(),
                &property,
                field_value,
                &mut literals,
                &mut references,
            );
        }
        literals.sort();
        references.sort_by(|a, b| (&a.property, &a.target).cmp(&(&b.property, &b.target)));

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
            });
        }
        Some(id)
    }

    /// Route one slot value to a literal, a reference edge, or (for an inlined
    /// mapping) a nested record plus an edge. Recurses over sequence elements.
    fn collect_field(
        &mut self,
        range: Option<&str>,
        property: &str,
        value: &serde_yaml::Value,
        literals: &mut Vec<(String, String)>,
        references: &mut Vec<Reference>,
    ) {
        if let serde_yaml::Value::Sequence(items) = value {
            for item in items {
                self.collect_field(range, property, item, literals, references);
            }
            return;
        }
        if range.is_some_and(|r| self.schema.classes.contains_key(r)) {
            let class = range.expect("is_some_and guarantees a class range");
            let target = match value {
                // A scalar references an existing instance by id.
                serde_yaml::Value::String(s) => Some(s.clone()),
                // An inlined mapping is its own record; recurse and edge to it.
                serde_yaml::Value::Mapping(_) => self.build_record(class, None, value),
                _ => None,
            };
            if let Some(target) = target {
                references.push(Reference {
                    property: property.to_string(),
                    target,
                });
            }
        } else if let Some(v) = scalar_to_string(value) {
            literals.push((property.to_string(), v));
        }
    }
}

/// Render a YAML scalar as a display string. Non-scalars yield `None`.
fn scalar_to_string(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        _ => None,
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
        serde_yaml::from_str(WINE_SCHEMA).expect("parse wine schema")
    }

    #[test]
    fn empty_when_the_data_has_no_container_records() {
        let schema = wine_schema();
        let data: serde_yaml::Value =
            serde_yaml::from_str("wines: []\nwineries: []\n").expect("parse data");
        assert!(InstanceSet::from_linkml_data(&schema, &data).is_empty());
    }

    #[test]
    fn from_linkml_data_reads_tree_root_container_records() {
        let schema = wine_schema();
        let data: serde_yaml::Value = serde_yaml::from_str(
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

        let winery = &set.instances[1];
        assert_eq!(winery.types, ["Winery"]);
        assert_eq!(winery.label, "Morgon Estate");
        assert!(winery.references.is_empty());
    }

    #[test]
    fn from_linkml_data_handles_inlined_as_dict_collection() {
        let schema = wine_schema();
        // wineries as an identifier-keyed mapping (CompactDict), not a list.
        let data: serde_yaml::Value = serde_yaml::from_str(
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
    }

    #[test]
    fn empty_without_a_tree_root_class() {
        let mut schema = wine_schema();
        for class in schema.classes.values_mut() {
            class.tree_root = false;
        }
        let data: serde_yaml::Value =
            serde_yaml::from_str("wines:\n  - id: x\n").expect("parse data");
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
      tags:
        range: string
        multivalued: true
      links:
        range: Node
        multivalued: true
";
        let schema: SchemaDefinition = serde_yaml::from_str(SCHEMA).expect("schema");
        let data: serde_yaml::Value = serde_yaml::from_str(
            "\
nodes:
  - id: a
    description: The first node.
    active: true
    score: 5
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
    }
}
