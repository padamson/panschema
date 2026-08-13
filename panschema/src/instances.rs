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
    /// The target lies outside this dataset — an absolute IRI, or a CURIE
    /// against a prefix the schema declares. Such a reference names no
    /// record here by design, so it is exempt from the dangling check and
    /// reported as a cross-graph edge instead of an error.
    pub external: bool,
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
    /// The IRI of the `tree_root` individual this record belongs to, when the
    /// dataset has one. A bare id mints beneath it, so the same id in two
    /// datasets denotes two individuals. Applied when the data is read and
    /// never written back to it, so already-authored files need no rework.
    /// `None` for a vessel-rooted dataset, which mints as it always has.
    pub scope: Option<String>,
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
    /// References pointing outside this dataset, kept visible rather than
    /// silently unchecked. Sorted.
    pub external_references: Vec<ExternalReference>,
    /// The `tree_root` class this dataset was read against, when one was
    /// chosen. `None` for a schema with no root, or when the choice was
    /// ambiguous — see `root_candidates`.
    pub root: Option<String>,
    /// When a schema declares several `tree_root` classes and the data
    /// conforms to none of them, or to more than one equally well: the
    /// candidates, for a caller to report. `None` when there was nothing to
    /// choose between.
    pub root_candidates: Option<Vec<String>>,
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
                        // Resolved against this schema's own individuals.
                        external: false,
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
                scope: None,
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
            external_references: Vec::new(),
            root: None,
            root_candidates: None,
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
        let Some(container) = data.as_mapping() else {
            return Self::default();
        };
        let (root_name, root) = match select_tree_root(schema, container) {
            RootSelection::None => return Self::default(),
            RootSelection::Ambiguous(candidates) => {
                return Self {
                    root_candidates: Some(candidates),
                    ..Self::default()
                };
            }
            RootSelection::Chosen(name) => match schema.classes.get_key_value(&name) {
                Some(pair) => pair,
                None => return Self::default(),
            },
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
            // A slot with no range (none declared, none defaulted at load)
            // is still a scalar for metadata purposes — skipping it would
            // drop both the root's id and any such scalar from the metadata.
            let range = slot.range.clone().unwrap_or_default();
            // Class-ranged container slots hold instance records; the
            // container's scalar attributes (a catalog title, a
            // description) describe the dataset itself and surface as its
            // metadata rather than vanishing. A *list* of scalars (a
            // multivalued scalar slot on the root) is neither a collection
            // of records nor a single metadata scalar — it replays through
            // the record builder like any record's values, and shows in
            // the metadata as the joined list.
            if schema.classes.contains_key(&range) {
                let ids = loader.collect_collection(&range, value);
                contained.push((slot_name.to_string(), ids));
            } else if let Some(scalar) = scalar_value(value) {
                metadata.push((slot_name.to_string(), scalar_to_display(&scalar)));
                root_fields.insert(key.clone(), value.clone());
            } else if let Some(items) = value.as_sequence() {
                let scalars: Vec<ScalarValue> = items.iter().filter_map(scalar_value).collect();
                if scalars.len() == items.len() && !scalars.is_empty() {
                    metadata.push((
                        slot_name.to_string(),
                        scalars
                            .iter()
                            .map(scalar_to_display)
                            .collect::<Vec<_>>()
                            .join(", "),
                    ));
                    root_fields.insert(key.clone(), value.clone());
                }
            }
        }

        // A container that declares an identifier is a domain individual —
        // an `Enterprise`, not wine's catalogue vessel — so it emits
        // as a record in its own right, referencing what it contains. A
        // vessel has no identifier and stays unemitted.
        let mut emitted_root_id: Option<String> = None;
        if root_slots.values().any(|slot| slot.identifier) {
            let root_value = serde_norway::Value::Mapping(root_fields);
            if let Some(root_id) = loader.build_record(root_name, None, &root_value)
                && let Some(inst) = loader.instances.iter_mut().find(|i| i.id == root_id)
            {
                emitted_root_id = Some(root_id.clone());
                for (slot, ids) in &contained {
                    for id in ids {
                        inst.references.push(Reference {
                            property: slot.clone(),
                            target: id.clone(),
                            external: false,
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
        // The root individual is the scope, and a record identified by a
        // `key` slot — unique within its container, per LinkML — mints
        // beneath it, so the same key in two datasets is two individuals. A
        // record identified by `identifier` claims global uniqueness and
        // mints unscoped in the schema namespace, so the same id anywhere is
        // one individual. A vessel root yields no scope at all.
        if let Some(root_id) = &emitted_root_id
            && let Some(root_inst) = loader.instances.iter().find(|i| &i.id == root_id)
        {
            let scope = crate::rdf_serializers::instance_iri_string(schema, root_inst);
            let mut class_has_key: std::collections::BTreeMap<String, bool> =
                std::collections::BTreeMap::new();
            for inst in &mut loader.instances {
                let keyed = inst.types.first().is_some_and(|class_name| {
                    *class_has_key.entry(class_name.clone()).or_insert_with(|| {
                        schema.classes.get(class_name).is_some_and(|class| {
                            crate::linkml_resolve::resolve_effective_slots(class, schema)
                                .values()
                                .any(|slot| slot.key && !slot.identifier)
                        })
                    })
                });
                if keyed && &inst.id != root_id {
                    inst.scope = Some(scope.clone());
                }
            }
        }

        loader.instances.sort_by(|a, b| a.id.cmp(&b.id));
        loader.duplicate_ids.sort();
        loader.undeclared_fields.sort();
        let mut external_references: Vec<ExternalReference> = loader
            .instances
            .iter()
            .flat_map(|inst| {
                inst.references
                    .iter()
                    .filter(|r| r.external)
                    .map(|r| ExternalReference {
                        referrer: inst.id.clone(),
                        property: r.property.clone(),
                        target: r.target.clone(),
                    })
            })
            .collect();
        external_references.sort();
        external_references.dedup();

        Self {
            instances: loader.instances,
            duplicate_ids: loader.duplicate_ids,
            undeclared_fields: loader.undeclared_fields,
            metadata,
            external_references,
            root: Some(root_name.clone()),
            root_candidates: None,
        }
    }

    /// A human-readable account of every reference that leaves this dataset,
    /// or `None` when none do.
    ///
    /// These edges cannot be resolved here — their targets are records of
    /// another graph — so naming them is the only way an unresolvable one
    /// stays visible rather than passing as a silently unchecked link.
    pub fn external_reference_summary(&self) -> Option<String> {
        if self.external_references.is_empty() {
            return None;
        }
        let mut out = format!(
            "{} cross-graph reference(s) leave this dataset and are not checked here:",
            self.external_references.len()
        );
        for r in &self.external_references {
            out.push_str(&format!(
                "\n  `{}` references `{}` via `{}`",
                r.referrer, r.target, r.property
            ));
        }
        Some(out)
    }
}

/// Which `tree_root` a data file was read against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootSelection {
    /// The schema declares no `tree_root`, so there is nothing to read.
    None,
    /// One root was chosen, by name.
    Chosen(String),
    /// Several roots exist and the data distinguishes none of them — it
    /// conforms to no root, or to two equally well. The candidates, sorted.
    Ambiguous(Vec<String>),
}

/// Choose the `tree_root` a data file conforms to.
///
/// A schema with one root always yields it, whatever the file holds — a
/// container key the root does not declare has always been skipped rather
/// than being fatal, and that stays true.
///
/// With several roots, the file's own top-level keys decide: the root that
/// declares the most of them wins. A schema whose roots hold disjoint
/// collections — an estate's deployments against a catalogue's providers —
/// is decided by the first key either way. Nothing is written into the data
/// and no configuration is consulted, so a dataset stays portable.
///
/// A tie is **not** broken. Picking the alphabetically-first root and reading
/// on is the bug this replaces: a catalogue read against an estate root
/// yields a plausible, wrong, near-empty dataset.
pub fn select_tree_root(
    schema: &SchemaDefinition,
    container: &serde_norway::Mapping,
) -> RootSelection {
    let roots: Vec<(&String, &crate::linkml::ClassDefinition)> =
        schema.classes.iter().filter(|(_, c)| c.tree_root).collect();
    match roots.as_slice() {
        [] => return RootSelection::None,
        [(name, _)] => return RootSelection::Chosen((*name).clone()),
        _ => {}
    }

    let keys: Vec<&str> = container.keys().filter_map(|k| k.as_str()).collect();
    let mut scored: Vec<(usize, &String)> = roots
        .iter()
        .map(|(name, class)| {
            let slots = crate::linkml_resolve::resolve_effective_slots(class, schema);
            let matched = keys.iter().filter(|k| slots.contains_key(**k)).count();
            (matched, *name)
        })
        .collect();
    scored.sort_by_key(|(matched, _)| std::cmp::Reverse(*matched));

    let best = scored[0].0;
    let winners = scored.iter().filter(|(score, _)| *score == best).count();
    if best == 0 || winners > 1 {
        let mut candidates: Vec<String> = roots.iter().map(|(name, _)| (*name).clone()).collect();
        candidates.sort();
        return RootSelection::Ambiguous(candidates);
    }
    RootSelection::Chosen(scored[0].1.clone())
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
                    // A bare scalar in a class-ranged collection is LinkML's
                    // non-inlined form: a reference to a record by its
                    // identifier. The id joins the collection as a reference
                    // — whether it resolves is the reference-integrity
                    // pass's question — rather than being dropped for not
                    // being an inline record.
                    if let Some(scalar) = scalar_value(item) {
                        ids.push(scalar_to_display(&scalar));
                        continue;
                    }
                    if let Some(id) = self.build_record(class_name, None, item) {
                        ids.push(id.clone());
                        self.note_top_level_id(id);
                    }
                }
            }
            serde_norway::Value::Mapping(map) => {
                for (key, record) in map {
                    // A scalar entry is LinkML's compact (SimpleDict) form:
                    // the key maps straight to the class's one non-key slot.
                    // Expand it into the ordinary shape and build as usual,
                    // so downstream never sees the compaction.
                    let expanded;
                    let record = if record.as_mapping().is_none()
                        && let Some(widened) = self.expand_simple_dict_entry(class_name, record)
                    {
                        expanded = widened;
                        &expanded
                    } else {
                        record
                    };
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

    /// LinkML's SimpleDict form widened to the ordinary record shape: a
    /// scalar dict entry fills the class's **one** slot beyond its
    /// key/identifier. With zero or several candidate slots there is no fact
    /// about which one the scalar fills, so nothing is invented and the
    /// entry stays unread.
    fn expand_simple_dict_entry(
        &self,
        class_name: &str,
        value: &serde_norway::Value,
    ) -> Option<serde_norway::Value> {
        let class = self.schema.classes.get(class_name)?;
        let slots = crate::linkml_resolve::resolve_effective_slots(class, self.schema);
        let mut non_identifying = slots
            .iter()
            .filter(|(_, slot)| !slot.identifier && !slot.key)
            .map(|(name, _)| name);
        let primary = non_identifying.next()?;
        if non_identifying.next().is_some() {
            return None;
        }
        let mut map = serde_norway::Mapping::new();
        map.insert(serde_norway::Value::String(primary.clone()), value.clone());
        Some(serde_norway::Value::Mapping(map))
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
        // so reading `range` alone would see nothing — silently turning
        // references into literals.
        let resolved =
            crate::linkml_resolve::resolve_effective_slots_with_provenance(class, self.schema);
        let slots: BTreeMap<String, SlotDefinition> = resolved
            .iter()
            .map(|(name, rs)| (name.clone(), rs.definition.clone()))
            .collect();

        // A record is identified by its `identifier` slot or, failing that,
        // its `key` slot — LinkML's globally- and container-unique forms.
        let id_slot = slots
            .iter()
            .find(|(_, s)| s.identifier)
            .or_else(|| slots.iter().find(|(_, s)| s.key))
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
            // declared scalar range.
            let induced = resolved
                .get(field)
                .map(|rs| rs.induced.ranges.clone())
                .unwrap_or_default();
            let ranges: Vec<String> = if induced.is_empty() {
                slot.and_then(|s| s.range.clone()).into_iter().collect()
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
                // Set once the dataset's root is known, not here.
                scope: None,
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

        let schema = self.schema;
        let reference_to =
            |target: String, references: &mut Vec<Reference>, slot_values: &mut Vec<SlotValue>| {
                let external = points_outside_dataset(schema, &target);
                push_slot_value(slot_values, slot, InstanceValue::Reference(target.clone()));
                if display {
                    references.push(Reference {
                        property: property.to_string(),
                        target,
                        external,
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

/// A reference whose target lies outside this dataset.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct ExternalReference {
    pub referrer: String,
    pub property: String,
    pub target: String,
}

/// Whether a reference target names something outside this dataset: an
/// absolute IRI, or a CURIE against a prefix the schema declares.
///
/// A bare id — even one matching no record — is deliberately **not** external:
/// it is a promise about *this* dataset, and breaking that promise is the
/// dangling-reference error. An undeclared prefix is likewise no licence to
/// skip checks; it is far likelier a typo than an intended cross-graph link.
pub fn points_outside_dataset(schema: &SchemaDefinition, target: &str) -> bool {
    if target.contains("://") {
        return true;
    }
    match target.split_once(':') {
        Some((prefix, _)) => schema.prefixes.contains_key(prefix),
        None => false,
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
    /// identifier — in the shape an estate's `Enterprise` root takes.
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

    /// A schema whose records can point outside their own dataset, in the
    /// shape a cross-graph reference takes: a declared prefix for the other
    /// graph, plus bare ids for records in this one.
    const XREF_SCHEMA: &str = "\
id: https://example.org/estate
name: estate
default_range: string
prefixes:
  catalog: https://example.org/catalog/
classes:
  Root:
    tree_root: true
    attributes:
      deployments: {range: Deployment, multivalued: true}
      providers: {range: Provider, multivalued: true}
  Deployment:
    attributes:
      id: {identifier: true}
      on_provider: {range: Provider}
  Provider:
    attributes:
      id: {identifier: true}
";

    fn xref_set(yaml: &str) -> InstanceSet {
        let mut schema: SchemaDefinition =
            serde_norway::from_str(XREF_SCHEMA).expect("parse xref schema");
        let named: Vec<(String, ClassDefinition)> = schema
            .classes
            .iter()
            .map(|(k, c)| {
                let mut c = c.clone();
                c.name = k.clone();
                (k.clone(), c)
            })
            .collect();
        schema.classes = named.into_iter().collect();
        let data: serde_norway::Value = serde_norway::from_str(yaml).expect("parse data");
        InstanceSet::from_linkml_data(&schema, &data)
    }

    #[test]
    fn a_curie_against_a_declared_prefix_is_an_external_reference() {
        let set = xref_set("deployments:\n  - {id: d1, on_provider: 'catalog:aws'}\n");
        let d1 = set.instances.iter().find(|i| i.id == "d1").expect("d1");
        let r = d1.references.first().expect("a reference");
        assert!(
            r.external,
            "a CURIE against a declared prefix points outside this dataset; got: {r:?}"
        );
        assert!(
            set.external_references
                .iter()
                .any(|e| e.target == "catalog:aws" && e.referrer == "d1"),
            "and it is summarised rather than passing silently; got: {:?}",
            set.external_references
        );
    }

    #[test]
    fn an_absolute_iri_is_an_external_reference() {
        let set =
            xref_set("deployments:\n  - {id: d1, on_provider: 'https://other.example/aws'}\n");
        let r = set
            .instances
            .iter()
            .find(|i| i.id == "d1")
            .and_then(|i| i.references.first())
            .expect("a reference");
        assert!(r.external, "an absolute IRI is outside by construction");
    }

    #[test]
    fn a_bare_id_is_not_external_even_when_it_names_no_record() {
        // The distinction this slice adds must not swallow the existing
        // dangling case: a bare id is a promise about *this* dataset.
        let set = xref_set("deployments:\n  - {id: d1, on_provider: nope}\n");
        let r = set
            .instances
            .iter()
            .find(|i| i.id == "d1")
            .and_then(|i| i.references.first())
            .expect("a reference");
        assert!(!r.external, "a bare id stays an intra-dataset reference");
        assert!(
            set.external_references.is_empty(),
            "and is not summarised as external; got: {:?}",
            set.external_references
        );
    }

    #[test]
    fn the_summary_names_every_cross_graph_target_and_its_referrer() {
        let set = xref_set(
            "deployments:\n  - {id: d1, on_provider: 'catalog:aws'}\n  \
             - {id: d2, on_provider: 'https://other.example/gcp'}\n",
        );
        let summary = set
            .external_reference_summary()
            .expect("references leave the dataset, so there is a summary");
        assert!(
            summary.starts_with("2 cross-graph reference(s)"),
            "it leads with how many edges go unchecked; got: {summary}"
        );
        for expected in [
            "`d1` references `catalog:aws` via `on_provider`",
            "`d2` references `https://other.example/gcp` via `on_provider`",
        ] {
            assert!(
                summary.contains(expected),
                "an unresolvable target stays visible: expected {expected} in {summary}"
            );
        }
    }

    #[test]
    fn a_dataset_with_no_cross_graph_edges_has_no_summary() {
        let set =
            xref_set("providers:\n  - {id: aws}\ndeployments:\n  - {id: d1, on_provider: aws}\n");
        assert!(
            set.external_reference_summary().is_none(),
            "silence is right when nothing leaves the dataset"
        );
    }

    #[test]
    fn an_undeclared_prefix_is_not_treated_as_external() {
        // `mystery:` is not in the schema's prefixes, so this is a typo in a
        // bare id rather than a deliberate cross-graph link — it must stay
        // dangling-checked rather than being waved through.
        let set = xref_set("deployments:\n  - {id: d1, on_provider: 'mystery:aws'}\n");
        let r = set
            .instances
            .iter()
            .find(|i| i.id == "d1")
            .and_then(|i| i.references.first())
            .expect("a reference");
        assert!(
            !r.external,
            "an undeclared prefix is not a licence to skip checks"
        );
    }

    /// Two roots holding disjoint collections — the shape all four consumers
    /// described: a scoped estate root and a shared reference root.
    const TWO_ROOT_SCHEMA: &str = "
id: https://example.org/estate
name: estate
default_prefix: estate
prefixes:
  estate: https://example.org/estate/
default_range: string
classes:
  Enterprise:
    tree_root: true
    attributes:
      id: {identifier: true}
      deployments: {range: Deployment, multivalued: true}
  ProviderCatalog:
    tree_root: true
    attributes:
      id: {identifier: true}
      providers: {range: Provider, multivalued: true}
  Deployment:
    attributes:
      id: {key: true}
  Provider:
    attributes:
      id: {key: true}
";

    fn two_root_set(yaml: &str) -> InstanceSet {
        let mut schema: SchemaDefinition =
            serde_norway::from_str(TWO_ROOT_SCHEMA).expect("parse two-root schema");
        let named: Vec<(String, ClassDefinition)> = schema
            .classes
            .iter()
            .map(|(k, c)| {
                let mut c = c.clone();
                c.name = k.clone();
                (k.clone(), c)
            })
            .collect();
        schema.classes = named.into_iter().collect();
        let data: serde_norway::Value = serde_norway::from_str(yaml).expect("parse data");
        InstanceSet::from_linkml_data(&schema, &data)
    }

    #[test]
    fn each_dataset_is_read_against_the_root_its_keys_conform_to() {
        // `Enterprise` sorts before `ProviderCatalog`, so reading a catalogue
        // against the first root by sort order is exactly the old bug.
        let catalog = two_root_set("id: aws-catalog\nproviders:\n  - id: aws\n  - id: gcp\n");
        assert_eq!(
            catalog.root.as_deref(),
            Some("ProviderCatalog"),
            "the catalogue's keys name ProviderCatalog; got: {:?}",
            catalog.root
        );
        let ids: Vec<&str> = catalog.instances.iter().map(|i| i.id.as_str()).collect();
        assert!(
            ids.contains(&"aws") && ids.contains(&"gcp"),
            "and its records are read, not silently dropped; got: {ids:?}"
        );

        let estate = two_root_set("id: acme\ndeployments:\n  - id: d1\n");
        assert_eq!(
            estate.root.as_deref(),
            Some("Enterprise"),
            "and an estate file still reads as Enterprise"
        );
    }

    /// The schema, with `catalog:` declared, that both halves of the
    /// reference/scoped split resolve against.
    fn two_root_schema_with_catalog_prefix() -> SchemaDefinition {
        let mut schema: SchemaDefinition =
            serde_norway::from_str(TWO_ROOT_SCHEMA).expect("parse two-root schema");
        schema.prefixes.insert(
            "catalog".to_string(),
            "https://example.org/catalog/".to_string(),
        );
        schema
    }

    /// A tree-root container's multivalued scalar slot keeps its values:
    /// they land on the root record like any record's, so they render and
    /// the validator sees them — a list is neither a collection of records
    /// nor a single metadata scalar, and must not fall between the two.
    #[test]
    fn a_container_multivalued_scalar_slot_keeps_its_values() {
        let mut schema = SchemaDefinition::new("s");
        let mut root = ClassDefinition::new("Root");
        root.tree_root = true;
        let mut id = SlotDefinition::new("id");
        id.identifier = true;
        root.attributes.insert("id".to_string(), id);
        let mut keywords = SlotDefinition::new("keywords");
        keywords.multivalued = true;
        keywords.range = Some("string".to_string());
        root.attributes.insert("keywords".to_string(), keywords);
        schema.classes.insert("Root".to_string(), root);

        let data: serde_norway::Value =
            serde_norway::from_str("id: r1\nkeywords: [alpha, beta]\n").unwrap();
        let set = InstanceSet::from_linkml_data(&schema, &data);
        let root_inst = set
            .instances
            .iter()
            .find(|i| i.id == "r1")
            .expect("the identified root emits as a record");
        let keyword_values: Vec<String> = root_inst
            .slot_values
            .iter()
            .find(|sv| sv.slot == "keywords")
            .map(|sv| {
                sv.values
                    .iter()
                    .filter_map(|v| match v {
                        InstanceValue::Scalar(ScalarValue::String(s)) => Some(s.clone()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(
            keyword_values,
            vec!["alpha".to_string(), "beta".to_string()],
            "both list values must survive ingestion"
        );
    }

    /// A list holding a non-scalar at a scalar-ranged root slot is not the
    /// multivalued-scalar shape: no values are invented from the scalar
    /// subset — the field is left out of the root record entirely rather
    /// than partially ingested.
    #[test]
    fn a_mixed_list_at_a_scalar_container_slot_is_not_partially_ingested() {
        let mut schema = SchemaDefinition::new("s");
        let mut root = ClassDefinition::new("Root");
        root.tree_root = true;
        let mut id = SlotDefinition::new("id");
        id.identifier = true;
        root.attributes.insert("id".to_string(), id);
        let mut keywords = SlotDefinition::new("keywords");
        keywords.multivalued = true;
        keywords.range = Some("string".to_string());
        root.attributes.insert("keywords".to_string(), keywords);
        schema.classes.insert("Root".to_string(), root);

        let data: serde_norway::Value =
            serde_norway::from_str("id: r1\nkeywords: [alpha, {stray: 1}]\n").unwrap();
        let set = InstanceSet::from_linkml_data(&schema, &data);
        let root_inst = set
            .instances
            .iter()
            .find(|i| i.id == "r1")
            .expect("the identified root emits as a record");
        assert!(
            !root_inst.slot_values.iter().any(|sv| sv.slot == "keywords"),
            "a mixed list must not be partially ingested; got: {:?}",
            root_inst.slot_values
        );
    }

    #[test]
    fn a_simple_dict_collection_reads_as_records_not_silence() {
        // LinkML's compact form: when a class has exactly one slot beyond its
        // key, a dict entry may map the key straight to that value —
        // `prefixes: {dcterms: http://…}` is the canonical example. This was
        // silently dropped before: a conforming file lost its records.
        let mut schema = SchemaDefinition::new("estate");
        schema.default_range = Some("string".to_string());
        let mut root = ClassDefinition::new("Root");
        root.tree_root = true;
        let mut providers = SlotDefinition::new("providers");
        providers.range = Some("Provider".to_string());
        providers.multivalued = true;
        root.attributes.insert("providers".to_string(), providers);
        schema.classes.insert("Root".to_string(), root);
        let mut provider = ClassDefinition::new("Provider");
        let mut key = SlotDefinition::new("id");
        key.key = true;
        provider.attributes.insert("id".to_string(), key);
        provider
            .attributes
            .insert("name".to_string(), SlotDefinition::new("name"));
        schema.classes.insert("Provider".to_string(), provider);

        let data: serde_norway::Value =
            serde_norway::from_str("providers:\n  aws: Amazon Web Services\n  gcp: Google Cloud\n")
                .unwrap();
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert_eq!(
            set.instances.len(),
            2,
            "both compact entries become records; got: {:?}",
            set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
        );
        let aws = set.instances.iter().find(|i| i.id == "aws").expect("aws");
        assert_eq!(
            aws.label, "Amazon Web Services",
            "the mapped value lands in the class's one non-key slot — here \
             `name`, which supplies the display label"
        );
        assert!(
            aws.slot_values.iter().any(|sv| sv.slot == "name"
                && sv.values.iter().any(|v| matches!(
                    v,
                    InstanceValue::Scalar(ScalarValue::String(s)) if s == "Amazon Web Services"
                ))),
            "and the authored assignment carries it; got: {:?}",
            aws.slot_values
        );
    }

    #[test]
    fn a_scalar_dict_entry_against_a_wider_class_is_not_guessed_into_a_record() {
        // The compact form is only defined for exactly-one-extra-slot
        // classes. With two, there is no fact about which slot the scalar
        // fills, so nothing is invented.
        let mut schema = SchemaDefinition::new("estate");
        schema.default_range = Some("string".to_string());
        let mut root = ClassDefinition::new("Root");
        root.tree_root = true;
        let mut providers = SlotDefinition::new("providers");
        providers.range = Some("Provider".to_string());
        providers.multivalued = true;
        root.attributes.insert("providers".to_string(), providers);
        schema.classes.insert("Root".to_string(), root);
        let mut provider = ClassDefinition::new("Provider");
        let mut key = SlotDefinition::new("id");
        key.key = true;
        provider.attributes.insert("id".to_string(), key);
        provider
            .attributes
            .insert("name".to_string(), SlotDefinition::new("name"));
        provider
            .attributes
            .insert("region".to_string(), SlotDefinition::new("region"));
        schema.classes.insert("Provider".to_string(), provider);

        let data: serde_norway::Value =
            serde_norway::from_str("providers:\n  aws: Amazon Web Services\n").unwrap();
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            set.instances.is_empty(),
            "an ambiguous compact entry builds nothing; got: {:?}",
            set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn key_records_scope_per_dataset_while_identifier_records_stay_global() {
        // The LinkML split: `key` is unique within its container, so two
        // estates' same-keyed deployments are two individuals; `identifier`
        // is unique everywhere, so an identifier-carrying record is the same
        // individual whichever dataset states facts about it.
        let mut schema = SchemaDefinition::new("estate");
        schema.id = Some("https://example.org/estate".to_string());
        schema.default_prefix = Some("estate".to_string());
        schema.prefixes.insert(
            "estate".to_string(),
            "https://example.org/estate/".to_string(),
        );
        schema.default_range = Some("string".to_string());

        let mut root = ClassDefinition::new("Enterprise");
        root.tree_root = true;
        let mut root_id = SlotDefinition::new("id");
        root_id.identifier = true;
        root.attributes.insert("id".to_string(), root_id);
        for (slot_name, range) in [("deployments", "Deployment"), ("providers", "Provider")] {
            let mut slot = SlotDefinition::new(slot_name);
            slot.range = Some(range.to_string());
            slot.multivalued = true;
            root.attributes.insert(slot_name.to_string(), slot);
        }
        schema.classes.insert("Enterprise".to_string(), root);

        // Deployment identifies by key (container-scoped); Provider by
        // identifier (global).
        let mut dep = ClassDefinition::new("Deployment");
        let mut key = SlotDefinition::new("id");
        key.key = true;
        dep.attributes.insert("id".to_string(), key);
        schema.classes.insert("Deployment".to_string(), dep);
        let mut provider = ClassDefinition::new("Provider");
        let mut ident = SlotDefinition::new("id");
        ident.identifier = true;
        provider.attributes.insert("id".to_string(), ident);
        // A plain slot alongside the identifier: only a `key` slot makes a
        // class scope, not the mere presence of non-identifying slots.
        provider
            .attributes
            .insert("name".to_string(), SlotDefinition::new("name"));
        schema.classes.insert("Provider".to_string(), provider);
        let read = |yaml: &str| {
            let data: serde_norway::Value = serde_norway::from_str(yaml).unwrap();
            InstanceSet::from_linkml_data(&schema, &data)
        };
        let acme = read("id: acme\ndeployments:\n  - id: api-gateway\nproviders:\n  - id: aws\n");
        let contoso =
            read("id: contoso\ndeployments:\n  - id: api-gateway\nproviders:\n  - id: aws\n");
        let iri_of = |set: &InstanceSet, id: &str| {
            let inst = set.instances.iter().find(|i| i.id == id).expect("record");
            crate::rdf_serializers::instance_iri_string(&schema, inst)
        };
        assert_ne!(
            iri_of(&acme, "api-gateway"),
            iri_of(&contoso, "api-gateway"),
            "key-identified records are unique per container, so they scope"
        );
        assert_eq!(
            iri_of(&acme, "aws"),
            iri_of(&contoso, "aws"),
            "identifier-identified records are globally unique, so they merge"
        );
        assert_eq!(
            iri_of(&acme, "aws"),
            "https://example.org/estate/aws",
            "and the global record mints in the schema namespace, unscoped"
        );
    }

    #[test]
    fn records_of_an_identified_root_mint_under_that_root() {
        // acme's api-gateway and contoso's are different services that happen
        // to share a local name. Distinct entities need distinct IRIs.
        let schema = two_root_schema_with_catalog_prefix();
        let acme = two_root_set("id: acme\ndeployments:\n  - id: api-gateway\n");
        let contoso = two_root_set("id: contoso\ndeployments:\n  - id: api-gateway\n");
        let iri_of = |set: &InstanceSet| {
            let inst = set
                .instances
                .iter()
                .find(|i| i.id == "api-gateway")
                .expect("the deployment");
            crate::rdf_serializers::instance_iri_string(&schema, inst)
        };
        assert_eq!(iri_of(&acme), "https://example.org/estate/acme/api-gateway");
        assert_eq!(
            iri_of(&contoso),
            "https://example.org/estate/contoso/api-gateway"
        );
        assert_ne!(
            iri_of(&acme),
            iri_of(&contoso),
            "two estates' same-named services must not be one individual"
        );
    }

    #[test]
    fn a_record_named_by_an_absolute_iri_escapes_its_dataset_scope() {
        // A record grounded in an external standard — a DOI, an ORCID, a
        // FoodOn or ChEBI term — already denotes one thing worldwide. Nesting
        // it under the dataset that mentions it would mint a private copy and
        // break exactly the cross-schema co-reference those standards exist
        // for.
        let schema = two_root_schema_with_catalog_prefix();
        for authored in [
            "https://doi.org/10.1000/xyz",
            "urn:uuid:8f1a0e2c-0000-4000-8000-000000000000",
        ] {
            let set = two_root_set(&format!("id: acme\ndeployments:\n  - id: '{authored}'\n"));
            let inst = set
                .instances
                .iter()
                .find(|i| i.id == authored)
                .unwrap_or_else(|| panic!("the record named {authored}"));
            assert_eq!(
                crate::rdf_serializers::instance_iri_string(&schema, inst),
                authored,
                "an externally-grounded id is used as authored, not scoped \
                 beneath the dataset that mentions it"
            );
        }
    }

    #[test]
    fn the_root_itself_stays_the_anchor_rather_than_nesting_under_its_own_scope() {
        let schema = two_root_schema_with_catalog_prefix();
        let acme = two_root_set("id: acme\ndeployments:\n  - id: api-gateway\n");
        let root = acme
            .instances
            .iter()
            .find(|i| i.id == "acme")
            .expect("the root record");
        assert_eq!(
            crate::rdf_serializers::instance_iri_string(&schema, root),
            "https://example.org/estate/acme",
            "the root IS the scope, so it does not nest inside itself"
        );
    }

    #[test]
    fn two_datasets_sharing_a_root_id_keep_denoting_the_same_individuals() {
        // A teaching preview that subsets a worked example shares records on
        // purpose. Same root id, same scope, no opt-out needed.
        let schema = two_root_schema_with_catalog_prefix();
        let preview = two_root_set("id: acme\ndeployments:\n  - id: api-gateway\n");
        let full = two_root_set("id: acme\ndeployments:\n  - id: api-gateway\n  - id: billing\n");
        let iri_of = |set: &InstanceSet, id: &str| {
            let inst = set.instances.iter().find(|i| i.id == id).expect("record");
            crate::rdf_serializers::instance_iri_string(&schema, inst)
        };
        assert_eq!(
            iri_of(&preview, "api-gateway"),
            iri_of(&full, "api-gateway"),
            "the shared record stays one individual across the pair"
        );
    }

    #[test]
    fn records_of_a_vessel_root_mint_exactly_as_before() {
        // Scoping is off by default: a root with no identifier is not a
        // scope-bearing entity, so nothing about its records changes.
        let mut schema: SchemaDefinition =
            serde_norway::from_str(XREF_SCHEMA).expect("parse xref schema");
        schema.default_prefix = Some("estate".to_string());
        schema.prefixes.insert(
            "estate".to_string(),
            "https://example.org/estate/".to_string(),
        );
        let set = xref_set("providers:\n  - id: aws\n");
        let aws = set.instances.iter().find(|i| i.id == "aws").expect("aws");
        assert_eq!(
            crate::rdf_serializers::instance_iri_string(&schema, aws),
            "https://example.org/estate/aws",
            "a vessel root introduces no scope segment"
        );
    }

    #[test]
    fn a_shared_record_authored_by_curie_mints_where_other_datasets_point() {
        // The reference/scoped refactor's join: a catalogue record named by
        // CURIE emits the very IRI a scoped dataset's `catalog:aws` resolves
        // to. LinkML's own rule — a CURIE identifier expands against the
        // schema's prefixes — is what makes this work with no scoping.
        let catalog = two_root_set("id: catalog:main\nproviders:\n  - id: catalog:aws\n");
        let schema = two_root_schema_with_catalog_prefix();
        let aws = catalog
            .instances
            .iter()
            .find(|i| i.id == "catalog:aws")
            .expect("the catalogue's provider");
        assert_eq!(
            crate::rdf_serializers::instance_iri_string(&schema, aws),
            "https://example.org/catalog/aws",
            "a CURIE-named shared record mints into the shared namespace"
        );
    }

    #[test]
    fn the_same_record_authored_bare_mints_elsewhere_and_does_not_join() {
        // The silent half of the same contract: authored bare, the record
        // lands in the schema's namespace, so a `catalog:aws` reference
        // resolves to nothing this dataset produced — both halves valid,
        // nothing joined. Documented because the failure is invisible.
        let catalog = two_root_set("id: aws-catalog\nproviders:\n  - id: aws\n");
        let schema = two_root_schema_with_catalog_prefix();
        let aws = catalog
            .instances
            .iter()
            .find(|i| i.id == "aws")
            .expect("the catalogue's provider");
        assert_eq!(
            crate::rdf_serializers::instance_iri_string(&schema, aws),
            "https://example.org/estate/aws-catalog/aws",
            "a bare id scopes under its own dataset's root, NOT into the \
             shared namespace — so `catalog:aws` does not resolve to it"
        );
    }

    #[test]
    fn a_file_matching_no_root_names_the_candidates_instead_of_guessing() {
        let orphan = two_root_set("widgets:\n  - id: w1\n");
        assert!(
            orphan.instances.is_empty(),
            "nothing is invented from a file that conforms to no root"
        );
        let ambiguity = orphan
            .root_candidates
            .as_ref()
            .expect("an unmatched file records the candidates it could not choose between");
        assert!(
            ambiguity.contains(&"Enterprise".to_string())
                && ambiguity.contains(&"ProviderCatalog".to_string()),
            "both roots are named so the author can see the choice; got: {ambiguity:?}"
        );
        assert!(
            orphan.root.is_none(),
            "and no root is claimed when none was chosen"
        );
    }

    #[test]
    fn a_file_matching_two_roots_equally_is_reported_not_resolved_by_sort_order() {
        // `id` alone is a slot of both roots, so nothing distinguishes them.
        let tie = two_root_set("id: ambiguous\n");
        assert!(
            tie.root.is_none(),
            "a tie must not be broken silently; got: {:?}",
            tie.root
        );
        assert!(
            tie.root_candidates.is_some(),
            "the tie is reported for the author to resolve"
        );
    }

    #[test]
    fn a_single_root_schema_still_reads_a_file_with_keys_it_does_not_declare() {
        // Long-standing behaviour: unknown container keys are skipped, not
        // fatal. Adding selection must not turn that into an error.
        // No key the root declares at all: the sole root is still the root,
        // and the file simply reads as empty. Treating "nothing matched" as
        // ambiguity would make a one-root schema start erroring on data it
        // has always accepted.
        let barren = xref_set("unknown_key: 3\n");
        assert_eq!(
            barren.root.as_deref(),
            Some("Root"),
            "one root is chosen whatever the file holds; got: {:?}",
            barren.root
        );
        assert!(
            barren.root_candidates.is_none(),
            "and nothing is reported as ambiguous when there is one candidate"
        );

        let set = xref_set("providers:\n  - id: aws\nunknown_key: 3\n");
        assert!(
            set.root_candidates.is_none(),
            "one root leaves nothing to choose between"
        );
        assert_eq!(
            set.root.as_deref(),
            Some("Root"),
            "the sole root is still the root"
        );
        assert_eq!(
            set.instances.iter().filter(|i| i.id == "aws").count(),
            1,
            "and the records it does declare are still read"
        );
    }

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
