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
    /// A reference to another instance by id. `held` marks the edge as
    /// containment rather than citation: the target was materialized at
    /// this edge (an inlined mapping), or the slot is one of the dataset
    /// container's multivalued collection slots — single-class or union —
    /// which hold their records by role (a single-valued container slot
    /// follows the record-level rules). A held edge is still a reference
    /// for range and integrity checks; consumers asking "who cites this
    /// record" skip held edges. Restating an already-materialized record
    /// as an inline mapping is a citation, not containment.
    Reference {
        target: String,
        held: bool,
    },
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

/// Why a union-ranged entry chose no member, phrased to follow
/// "entry `k` …" — one spelling for every reporting site.
const UNION_AMBIGUITY_REASON: &str = "names no single class of the union range — give it a \
                                      field only the intended member declares, or a type \
                                      designator naming one";

/// A container-collection entry that loaded no record — or loaded one
/// without its authored key — because it named no single class of the
/// slot's range, or its shape can never name one.
/// Kept on the set (not any record) so validation surfaces it whether or
/// not a container record emits: no collection entry vanishes silently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnusableCollectionEntry {
    /// The container slot holding the entry.
    pub slot: String,
    /// The dict key — the entry's would-be id — when the collection was
    /// spelled as a dict with a string key.
    pub key: Option<String>,
    /// Why the entry loaded nothing, phrased to follow "entry `k` …".
    pub reason: String,
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
    /// String-keyed ones are **not** dropped — they render and emit as
    /// properties minted in the schema's namespace — while a non-string
    /// key (`key_kind: Some`) can name no slot, so its value is the one
    /// thing here that was discarded. Sorted, each finding once.
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
    /// The id of the emitted dataset-container record itself, when the
    /// chosen root declares an identifier. Its collection-slot edges are
    /// marked held (containment by role), so citation-oriented consumers
    /// need no special case for it.
    pub root_record: Option<String>,
    /// Container-collection entries that loaded no record, with why —
    /// reported by validation so the entries stay visible even when no
    /// container record exists to carry them. Empty for readers that
    /// don't track it (e.g. OWL).
    pub unusable_collection_entries: Vec<UnusableCollectionEntry>,
    /// The container's authored id when it collides with a record's id.
    /// No container is emitted then — attaching its edges to that record
    /// would mislabel an ordinary record as the container — and without a
    /// container, key-scoped records mint unscoped. Distinct from
    /// `duplicate_ids` so a caller can tell this degraded state from a
    /// legitimately vessel-rooted dataset.
    pub root_collision: Option<String>,
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
    /// `Some` when the field's key is not a string — such a field cannot
    /// name a slot, so unlike an ordinary undeclared field its value is
    /// dropped. `None` for a string-keyed undeclared field, which is
    /// kept and emitted as an undeclared property.
    pub key_kind: Option<KeyKind>,
}

/// What kind of non-string key a field had, deciding the remedy the
/// report can honestly offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeyKind {
    /// A number or boolean: `field` shows its exact display, and quoting
    /// it makes it a string.
    Quotable,
    /// A mapping, sequence, or null: nothing quotable exists, and
    /// `field` carries a kind phrase rather than an authored key.
    Unquotable,
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
            root_record: None,
            root_collision: None,
            root_candidates: None,
            unusable_collection_entries: Vec::new(),
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
        // Resolved with provenance so each slot's induced range is available,
        // exactly as record building does below: an `any_of` union carries
        // its range targets there, not in the scalar `range:`.
        let root_resolved =
            crate::linkml_resolve::resolve_effective_slots_with_provenance(root, schema);

        let mut metadata: Vec<(String, String)> = Vec::new();
        let mut loader = LinkmlLoader {
            schema,
            instances: Vec::new(),
            seen: std::collections::HashSet::new(),
            top_level_seen: std::collections::HashSet::new(),
            duplicate_ids: Vec::new(),
            undeclared_fields: Vec::new(),
            effective_slots_by_class: std::collections::BTreeMap::new(),
            unusable_entries: Vec::new(),
        };
        // The container's authored identifier value, when its declared
        // identifier slot carries one — decided up front because it names
        // the container in findings, routes union-ranged container slots
        // below, and later gates whether a container record emits.
        let authored_root_id: Option<String> = root_resolved
            .iter()
            .find(|(_, rs)| rs.definition.identifier)
            .and_then(|(name, _)| container.get(serde_norway::Value::String(name.clone())))
            .and_then(scalar_value)
            .map(|s| scalar_to_display(&s));
        let authored_identifier = authored_root_id.is_some();
        // What each class-ranged container slot carried, so a root that is
        // itself a record can reference those records under that slot's
        // name — each id paired with whether the edge is containment.
        let mut contained: Vec<(String, Vec<(String, bool)>)> = Vec::new();
        // The root's own non-collection fields, replayed through the normal
        // record builder rather than reimplementing id/label/scalar handling.
        let mut root_fields = serde_norway::Mapping::new();

        for (key, value) in container {
            let Some(slot_name) = key.as_str() else {
                loader.note_non_string_key(
                    authored_root_id
                        .clone()
                        .unwrap_or_else(|| root_name.clone()),
                    root_name,
                    key,
                );
                continue;
            };
            let Some(resolved) = root_resolved.get(slot_name) else {
                // An undeclared string field is kept and reported exactly
                // as on any record: the replayed container carries it.
                root_fields.insert(key.clone(), value.clone());
                continue;
            };
            let slot = &resolved.definition;
            // A slot with no range (none declared, none defaulted at load)
            // is still a scalar for metadata purposes — skipping it would
            // drop both the root's id and any such scalar from the metadata.
            let ranges: Vec<String> = if resolved.induced.ranges.is_empty() {
                slot.range.clone().into_iter().collect()
            } else {
                resolved.induced.ranges.clone()
            };
            let all_classes =
                !ranges.is_empty() && ranges.iter().all(|r| schema.classes.contains_key(r));
            let mut class_targets: Vec<&String> = ranges
                .iter()
                .filter(|r| schema.classes.contains_key(*r))
                .collect();
            // Branches repeating one class are one candidate.
            let mut seen_targets = std::collections::BTreeSet::new();
            class_targets.retain(|c| seen_targets.insert(c.as_str()));
            // Class-ranged container slots hold instance records; the
            // container's scalar attributes (a catalog title, a
            // description) describe the dataset itself and surface as its
            // metadata rather than vanishing. A *list* of scalars (a
            // multivalued scalar slot on the root) is neither a collection
            // of records nor a single metadata scalar — it replays through
            // the record builder like any record's values, and shows in
            // the metadata as the joined list.
            if slot.multivalued
                && !class_targets.is_empty()
                && (class_targets.len() == 1 || all_classes)
            {
                // A collection holds its records by role, however each
                // entry is spelled; a union of classes chooses each entry's
                // member individually. A union mixing several classes with
                // types falls through to record-level rules instead — a
                // scalar there could legitimately be a literal.
                let ids = loader.collect_collection(slot_name, &class_targets, value);
                contained.push((slot_name.to_string(), ids));
            } else if class_targets.len() == 1 {
                // Single-valued: the collection arm above took every
                // multivalued shape.
                let range = class_targets[0].clone();
                contained.push((slot_name.to_string(), loader.collect_single(&range, value)));
            } else if !class_targets.is_empty() {
                if authored_identifier {
                    // The field replays through the record builder on the
                    // emitted container, the authored keys choosing each
                    // value's member per record-level rules.
                    root_fields.insert(key.clone(), value.clone());
                } else {
                    // No container record will exist to replay through, but
                    // the records are still data — build them.
                    loader.collect_union_records(slot_name, &class_targets, value);
                }
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

        // A container that declares an identifier *and authors a value for
        // it* is a domain individual — an `Enterprise`, not wine's catalogue
        // vessel — so it emits as a record in its own right, referencing
        // what it contains. A vessel has no identifier and stays unemitted;
        // so does a container that leaves its declared identifier blank,
        // since building it anyway would fabricate an id that shifts with
        // the instance count, and every key-scoped IRI with it.
        let mut emitted_root_id: Option<String> = None;
        let mut root_collision: Option<String> = None;
        if authored_identifier {
            let root_value = serde_norway::Value::Mapping(root_fields);
            match loader.build_record(root_name, None, &root_value) {
                // The container's id is already some record's id. Attaching
                // the container's edges to that record would silently make
                // an ordinary record the dataset container, so report the
                // collision and emit no container at all.
                Some((root_id, false)) => {
                    loader.note_duplicate_id(root_id.clone());
                    root_collision = Some(root_id);
                }
                Some((root_id, true)) => {
                    if let Some(inst) = loader.instances.iter_mut().find(|i| i.id == root_id) {
                        for (slot, ids) in &contained {
                            for (id, held) in ids {
                                inst.references.push(Reference {
                                    property: slot.clone(),
                                    target: id.clone(),
                                    external: points_outside_dataset(schema, id),
                                });
                                push_slot_value(
                                    &mut inst.slot_values,
                                    slot,
                                    InstanceValue::Reference {
                                        target: id.clone(),
                                        held: *held,
                                    },
                                );
                            }
                        }
                        inst.references.sort_by(|a, b| {
                            (&a.property, &a.target).cmp(&(&b.property, &b.target))
                        });
                        inst.slot_values.sort_by(|a, b| a.slot.cmp(&b.slot));
                    }
                    emitted_root_id = Some(root_id);
                }
                None => {}
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
        loader.undeclared_fields.dedup();
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
            root_record: emitted_root_id,
            root_collision,
            unusable_collection_entries: loader.unusable_entries,
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
    let scored: Vec<(usize, &String)> = roots
        .iter()
        .map(|(name, class)| {
            let slots = crate::linkml_resolve::resolve_effective_slots(class, schema);
            let matched = keys.iter().filter(|k| slots.contains_key(**k)).count();
            (matched, *name)
        })
        .collect();
    match unique_best_scored(&scored) {
        Some(name) => RootSelection::Chosen((*name).clone()),
        None => {
            let mut candidates: Vec<String> =
                roots.iter().map(|(name, _)| (*name).clone()).collect();
            candidates.sort();
            RootSelection::Ambiguous(candidates)
        }
    }
}

/// The candidate scoring strictly highest — the one rule for letting
/// authored keys choose a class, shared by dataset-root selection and
/// inline-union disambiguation. Callers pass two or more candidates, so a
/// field where nothing matched is a tie and yields `None`.
fn unique_best_scored<T>(scored: &[(usize, T)]) -> Option<&T> {
    let best = scored.iter().map(|(score, _)| *score).max()?;
    let mut winners = scored.iter().filter(|(score, _)| *score == best);
    match (winners.next(), winners.next()) {
        (Some((_, one)), None) => Some(one),
        _ => None,
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
    /// Each class's effective slots, resolved once per dataset — every
    /// per-class fact (key matching, SimpleDict admission, the type
    /// designator) projects from this one map, and the full inheritance
    /// walk is too costly to repeat per record.
    effective_slots_by_class:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, SlotDefinition>>,
    /// Collection entries that loaded no record, carried to the set.
    unusable_entries: Vec<UnusableCollectionEntry>,
}

impl LinkmlLoader<'_> {
    /// `class_name`'s effective slots, resolved once and cached.
    fn effective_slots(
        &mut self,
        class_name: &str,
    ) -> &std::collections::BTreeMap<String, SlotDefinition> {
        if !self.effective_slots_by_class.contains_key(class_name) {
            let slots = self
                .schema
                .classes
                .get(class_name)
                .map(|class| crate::linkml_resolve::resolve_effective_slots(class, self.schema))
                .unwrap_or_default();
            self.effective_slots_by_class
                .insert(class_name.to_string(), slots);
        }
        &self.effective_slots_by_class[class_name]
    }

    /// The slot a compact SimpleDict entry fills for `class_name`: the
    /// class's **one** effective slot beyond its key/identifier — and
    /// beyond any type designator, which carries the class, not the
    /// payload. With zero or several candidates there is no fact about
    /// which slot the scalar fills, so `None` — nothing is invented.
    fn simple_dict_slot(&mut self, class_name: &str) -> Option<String> {
        let mut open = self
            .effective_slots(class_name)
            .iter()
            .filter(|(_, slot)| !slot.identifier && !slot.key && !slot.designates_type)
            .map(|(name, _)| name);
        match (open.next(), open.next()) {
            (Some(name), None) => Some(name.clone()),
            _ => None,
        }
    }

    /// The `designates_type` slot among `class_name`'s effective slots —
    /// the slot whose authored value names the record's class.
    fn designator_slot(&mut self, class_name: &str) -> Option<String> {
        self.effective_slots(class_name)
            .iter()
            .find(|(_, slot)| slot.designates_type)
            .map(|(name, _)| name.clone())
    }

    /// Whether `slot_name` is an effective slot of `class_name`.
    fn class_carries(&mut self, class_name: &str, slot_name: &str) -> bool {
        self.effective_slots(class_name).contains_key(slot_name)
    }

    /// How many of `keys` are effective slots of `class_name`.
    fn key_match_count(&mut self, class_name: &str, keys: &[&str]) -> usize {
        let slots = self.effective_slots(class_name);
        keys.iter().filter(|k| slots.contains_key(**k)).count()
    }

    /// Report a field key that is not a string: it can name no slot, so
    /// its value is dropped — unlike a kept undeclared property.
    fn note_non_string_key(&mut self, record: String, class: &str, key: &serde_norway::Value) {
        let (field, kind) = match key {
            serde_norway::Value::Number(n) => (n.to_string(), KeyKind::Quotable),
            serde_norway::Value::Bool(b) => (b.to_string(), KeyKind::Quotable),
            other => (yaml_kind(other).to_string(), KeyKind::Unquotable),
        };
        self.undeclared_fields.push(UndeclaredField {
            record,
            class: class.to_string(),
            field,
            key_kind: Some(kind),
        });
    }

    /// Record a collection entry that loaded nothing, and why.
    fn note_unusable(&mut self, slot: &str, key: Option<String>, reason: String) {
        self.unusable_entries.push(UnusableCollectionEntry {
            slot: slot.to_string(),
            key,
            reason,
        });
    }

    /// The union member the authored keys name. Candidates arrive
    /// deduplicated. LinkML's type designator decides first: every
    /// designator key any member declares is consulted; a string value
    /// naming a member (by name, IRI, or CURIE) wins, and two designators
    /// naming different members — or an unusable value on a key every
    /// carrier marks as a designator — leave the entry ambiguous, never
    /// guessed. A key that is an ordinary slot for some member may be
    /// plain data, so an unresolved value there falls to the heuristic:
    /// the member matching the most keys wins, ties stay ambiguous.
    fn disambiguate_class(
        &mut self,
        candidates: &[&String],
        map: &serde_norway::Mapping,
    ) -> Option<String> {
        if let [one] = candidates {
            return Some((*one).clone());
        }
        let mut designator_keys: Vec<String> = Vec::new();
        for candidate in candidates {
            if let Some(key) = self.designator_slot(candidate)
                && !designator_keys.contains(&key)
            {
                designator_keys.push(key);
            }
        }
        let mut chosen: Option<&str> = None;
        for key in &designator_keys {
            let Some(value) = map.get(key.as_str()) else {
                continue;
            };
            let resolved = value.as_str().map(|named| {
                crate::rdf_serializers::class_named_by(self.schema, candidates, named)
            });
            match resolved {
                Some(crate::rdf_serializers::ClassMatch::One(name)) => {
                    if chosen.is_some_and(|already| already != name) {
                        return None;
                    }
                    chosen = Some(name);
                }
                Some(crate::rdf_serializers::ClassMatch::Several) => return None,
                Some(crate::rdf_serializers::ClassMatch::None) | None => {
                    let designator_for_all = candidates.iter().all(|c| {
                        !self.class_carries(c, key)
                            || self.designator_slot(c).as_deref() == Some(key)
                    });
                    if designator_for_all {
                        return None;
                    }
                }
            }
        }
        if let Some(name) = chosen {
            return Some(name.to_string());
        }
        let keys: Vec<&str> = map.keys().filter_map(|k| k.as_str()).collect();
        let scored: Vec<(usize, &str)> = candidates
            .iter()
            .map(|name| (self.key_match_count(name, &keys), name.as_str()))
            .collect();
        unique_best_scored(&scored).map(|name| (*name).to_string())
    }

    /// Build the records of a union field that cannot replay through a
    /// container record — a vessel root's single-valued union slot, or
    /// its mixed class-and-type union: a mapping naming one member
    /// materializes; nothing else has a holder to be recorded on.
    fn collect_union_records(
        &mut self,
        slot: &str,
        candidates: &[&String],
        value: &serde_norway::Value,
    ) {
        match value {
            serde_norway::Value::Sequence(items) => {
                for item in items {
                    self.collect_union_records(slot, candidates, item);
                }
            }
            serde_norway::Value::Mapping(map) => match self.disambiguate_class(candidates, map) {
                Some(class) => {
                    if let Some((id, _)) = self.build_record(&class, None, value) {
                        self.note_top_level_id(id);
                    }
                }
                None => self.note_unusable(slot, None, UNION_AMBIGUITY_REASON.to_string()),
            },
            _ => {}
        }
    }

    /// The records of a class-ranged container collection — one class or
    /// a union, list and identifier-keyed dict spellings alike; the
    /// spelling of a collection must not decide whether its data
    /// survives. Each entry chooses its class on its own: a mapping by
    /// the member its keys name, a compact SimpleDict scalar by the one
    /// member with a single open slot to fill. An entry naming no single
    /// member is recorded as unusable, never built by guessing and never
    /// dropped silently. Returns the contained ids, held by role.
    fn collect_collection(
        &mut self,
        slot: &str,
        candidates: &[&String],
        value: &serde_norway::Value,
    ) -> Vec<(String, bool)> {
        let mut ids = Vec::new();
        match value {
            serde_norway::Value::Sequence(items) => {
                for item in items {
                    match item {
                        // A bare string is LinkML's non-inlined form: a
                        // reference to a record by its identifier. Whether
                        // it resolves is the integrity pass's question.
                        serde_norway::Value::String(text) => ids.push((text.clone(), true)),
                        // An authored arity mistake stays loadable rather
                        // than vanishing behind its extra brackets.
                        serde_norway::Value::Sequence(_) => {
                            ids.extend(self.collect_collection(slot, candidates, item));
                        }
                        serde_norway::Value::Mapping(map) => {
                            match self.disambiguate_class(candidates, map) {
                                Some(class) => {
                                    if let Some((id, _)) = self.build_record(&class, None, item) {
                                        self.note_top_level_id(id.clone());
                                        ids.push((id, true));
                                    }
                                }
                                None => self.note_unusable(
                                    slot,
                                    None,
                                    UNION_AMBIGUITY_REASON.to_string(),
                                ),
                            }
                        }
                        serde_norway::Value::Null => self.note_unusable(
                            slot,
                            None,
                            "is a null; a null names no record".to_string(),
                        ),
                        other => self.note_unusable(
                            slot,
                            None,
                            format!(
                                "is {}; a collection entry must be a record or an id string",
                                yaml_kind(other)
                            ),
                        ),
                    }
                }
            }
            serde_norway::Value::Mapping(map) => {
                for (key, record) in map {
                    let key_str = key.as_str();
                    // A key that is a member's own slot name is almost
                    // certainly an un-wrapped inline record, not an id —
                    // building it would mint a phantom named after a field.
                    if let Some(k) = key_str
                        && candidates.iter().any(|c| self.key_match_count(c, &[k]) > 0)
                    {
                        self.note_unusable(
                            slot,
                            Some(k.to_string()),
                            format!(
                                "is named like a field of the range (`{k}`), not a record \
                                 id — wrap records in a list"
                            ),
                        );
                        continue;
                    }
                    match record {
                        serde_norway::Value::Mapping(entry) => {
                            if key_str.is_none() {
                                self.note_unusable(
                                    slot,
                                    None,
                                    "has a non-string key; the record loads by its own id — \
                                     quote the key to make it the authored one"
                                        .to_string(),
                                );
                            }
                            match self.disambiguate_class(candidates, entry) {
                                Some(class) => {
                                    if let Some((id, _)) =
                                        self.build_record(&class, key_str, record)
                                    {
                                        self.note_top_level_id(id.clone());
                                        ids.push((id, true));
                                    }
                                }
                                None => self.note_unusable(
                                    slot,
                                    key_str.map(str::to_string),
                                    UNION_AMBIGUITY_REASON.to_string(),
                                ),
                            }
                        }
                        serde_norway::Value::Null => self.note_unusable(
                            slot,
                            key_str.map(str::to_string),
                            "is a null; a null names no record".to_string(),
                        ),
                        _ => {
                            let Some(k) = key_str else {
                                self.note_unusable(
                                    slot,
                                    None,
                                    "has a non-string key; quote the key to make it an id"
                                        .to_string(),
                                );
                                continue;
                            };
                            self.build_simple_dict_entry(slot, candidates, k, record, &mut ids);
                        }
                    }
                }
            }
            _ => {}
        }
        ids
    }

    /// A compact SimpleDict entry builds as the one union member with a
    /// single open slot for the scalar; zero or several members leave no
    /// fact about which slot it fills, so the entry is unusable.
    fn build_simple_dict_entry(
        &mut self,
        slot: &str,
        candidates: &[&String],
        key: &str,
        record: &serde_norway::Value,
        ids: &mut Vec<(String, bool)>,
    ) {
        let mut winner: Option<&String> = None;
        for candidate in candidates {
            if self.simple_dict_slot(candidate).is_some() {
                if winner.is_some() {
                    self.note_unusable(
                        slot,
                        Some(key.to_string()),
                        "could fill a slot of more than one union member — spell the \
                         record out with its fields"
                            .to_string(),
                    );
                    return;
                }
                winner = Some(candidate);
            }
        }
        let Some(class) = winner else {
            self.note_unusable(
                slot,
                Some(key.to_string()),
                "is a compact entry, but no union member has a single open slot to fill"
                    .to_string(),
            );
            return;
        };
        let primary = self
            .simple_dict_slot(class)
            .expect("winner admitted a primary slot");
        let mut widened = serde_norway::Mapping::new();
        widened.insert(serde_norway::Value::String(primary), record.clone());
        if let Some((id, _)) =
            self.build_record(class, Some(key), &serde_norway::Value::Mapping(widened))
        {
            self.note_top_level_id(id.clone());
            ids.push((id, true));
        }
    }

    /// Record `id` as claimed by more than one record, listed once.
    fn note_duplicate_id(&mut self, id: String) {
        if !self.duplicate_ids.contains(&id) {
            self.duplicate_ids.push(id);
        }
    }

    /// Record a top-level record's id; a second use of an id already claimed by
    /// a top-level record is a duplicate identifier (listed once).
    fn note_top_level_id(&mut self, id: String) {
        if !self.top_level_seen.insert(id.clone()) {
            self.note_duplicate_id(id);
        }
    }

    /// One record at a single-valued class-ranged container slot: an inline
    /// mapping materializes that record (containment), a bare scalar cites
    /// an existing record by id. A sequence recurses per element, so an
    /// authored arity mistake stays visible to validation instead of
    /// vanishing.
    fn collect_single(
        &mut self,
        class_name: &str,
        value: &serde_norway::Value,
    ) -> Vec<(String, bool)> {
        match value {
            serde_norway::Value::Sequence(items) => items
                .iter()
                .flat_map(|item| self.collect_single(class_name, item))
                .collect(),
            serde_norway::Value::Mapping(_) => self
                .build_record(class_name, None, value)
                .map(|(id, materialized)| {
                    self.note_top_level_id(id.clone());
                    vec![(id, materialized)]
                })
                .unwrap_or_default(),
            other => scalar_value(other)
                .map(|s| vec![(scalar_to_display(&s), false)])
                .unwrap_or_default(),
        }
    }

    /// Materialize one record of `class_name` and return its id (so an inlined
    /// object can be referenced by its container) plus whether this call
    /// materialized it — `false` when the id was already taken, so the caller
    /// is restating an existing record rather than holding a new one.
    /// `dict_key`, when present, is the record's identifier from an
    /// identifier-keyed collection.
    fn build_record(
        &mut self,
        class_name: &str,
        dict_key: Option<&str>,
        record: &serde_norway::Value,
    ) -> Option<(String, bool)> {
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
                self.note_non_string_key(id.clone(), class_name, field_key);
                continue;
            };
            let slot = slots.get(field);
            if slot.is_none() {
                self.undeclared_fields.push(UndeclaredField {
                    record: id.clone(),
                    class: class_name.to_string(),
                    field: field.to_string(),
                    key_kind: None,
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

        let materialized = self.seen.insert(id.clone());
        if materialized {
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
        } else if let Some(existing) = self.instances.iter().find(|i| i.id == id) {
            // Restating an existing record is fine only when nothing is
            // lost: the same record authored identically in two places is
            // one entity referenced two ways. A different class, or values
            // the kept record does not carry, mean a second definition was
            // discarded — report the id rather than let content vanish.
            let class_conflict = existing.types != [class_name.to_string()];
            let value_kept = |slot: &str, v: &InstanceValue| {
                existing
                    .slot_values
                    .iter()
                    .find(|e| e.slot == slot)
                    .is_some_and(|e| {
                        e.values.iter().any(|kept| match (v, kept) {
                            // Containment vs citation is a spelling of the
                            // same edge, not a content difference.
                            (
                                InstanceValue::Reference { target: a, .. },
                                InstanceValue::Reference { target: b, .. },
                            ) => a == b,
                            _ => v == kept,
                        })
                    })
            };
            let content_lost = slot_values
                .iter()
                .any(|sv| sv.values.iter().any(|v| !value_kept(&sv.slot, v)));
            if class_conflict || content_lost {
                self.note_duplicate_id(id.clone());
            }
        }
        Some((id, materialized))
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
    /// ambiguity per branch. An inlined object is built when its authored
    /// fields name one class target — one range class, or the union member
    /// matching the most authored keys; a mapping that fits several
    /// members equally, or none at all, is recorded as unusable rather
    /// than built by guessing.
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

        let all_classes =
            !ranges.is_empty() && ranges.iter().all(|r| self.schema.classes.contains_key(r));
        let mut class_targets: Vec<&String> = ranges
            .iter()
            .filter(|r| self.schema.classes.contains_key(*r))
            .collect();
        // Branches repeating one class are one candidate.
        let mut seen_targets = std::collections::BTreeSet::new();
        class_targets.retain(|c| seen_targets.insert(c.as_str()));

        // A null carries no value — treat as absent, not a kind mismatch —
        // except where only a reference could stand: a null can never
        // reference a record, and dropping it there would silently shorten
        // an authored reference list.
        if matches!(value, serde_norway::Value::Null) {
            if all_classes {
                push_slot_value(slot_values, slot, InstanceValue::Unexpected("a null"));
            }
            return;
        }

        let schema = self.schema;
        let reference_to = |target: String,
                            held: bool,
                            references: &mut Vec<Reference>,
                            slot_values: &mut Vec<SlotValue>| {
            let external = points_outside_dataset(schema, &target);
            push_slot_value(
                slot_values,
                slot,
                InstanceValue::Reference {
                    target: target.clone(),
                    held,
                },
            );
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
            // At a union, the authored fields choose the class; when they
            // don't choose one member, the mapping is recorded as unusable
            // rather than built by guessing.
            serde_norway::Value::Mapping(map) => {
                match self.disambiguate_class(&class_targets, map) {
                    Some(class) => {
                        if let Some((target, materialized)) = self.build_record(&class, None, value)
                        {
                            reference_to(target, materialized, references, slot_values);
                        }
                    }
                    None => push_slot_value(
                        slot_values,
                        slot,
                        InstanceValue::Unexpected(yaml_kind(value)),
                    ),
                }
            }
            // A string at an all-class range references a record by id.
            serde_norway::Value::String(text) if all_classes => {
                reference_to(text.clone(), false, references, slot_values);
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
    // `urn:` is an absolute scheme with no `://`, treated as absolute by
    // every other name resolution here (`expand_curie`, instance minting).
    if target.contains("://") || target.starts_with("urn:") {
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
        // Deliberately coarser than the scalar-kind vocabulary: this
        // phrases a YAML value that fit no slot at all, where the
        // integer/float split carries no information.
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
            "and it is summarized rather than passing silently; got: {:?}",
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
    fn a_urn_is_an_external_reference() {
        // `urn:` is an absolute IRI scheme everywhere else names are
        // resolved (`expand_curie`, instance minting), so classifying it
        // as a bare id here would make a URN anchor a false dangling
        // error and hide it from cross-graph resolution.
        let set = xref_set("deployments:\n  - {id: d1, on_provider: 'urn:uuid:1234'}\n");
        let r = set
            .instances
            .iter()
            .find(|i| i.id == "d1")
            .and_then(|i| i.references.first())
            .expect("a reference");
        assert!(r.external, "a URN points outside this dataset");
        assert!(
            set.external_references
                .iter()
                .any(|e| e.target == "urn:uuid:1234"),
            "and it is summarized; got: {:?}",
            set.external_references
        );
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
            "and is not summarized as external; got: {:?}",
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
    fn an_identified_roots_cross_graph_references_are_external() {
        // A benchmark-shaped dataset records IRIs into a graph that is by
        // definition someone else's: the root's own collection slots hold
        // absolute IRIs naming no record in this file. Those references are
        // outside the dataset exactly as a record slot's would be, so they
        // carry the same external marking — exempt from the dangling check
        // and named in the cross-graph summary.
        let data: serde_norway::Value = serde_norway::from_str(
            "id: acme\nname: Acme Corp\ndeployments:\n  - https://other.example/d1\n",
        )
        .expect("parse");
        let set = InstanceSet::from_linkml_data(&root_schema(true), &data);
        let root = set.instances.iter().find(|i| i.id == "acme").expect("root");
        let r = root.references.first().expect("a reference");
        assert!(
            r.external,
            "an absolute IRI on the root's own slot points outside this dataset; got: {r:?}"
        );
        assert!(
            set.external_references
                .iter()
                .any(|e| e.target == "https://other.example/d1" && e.referrer == "acme"),
            "and it is summarized rather than passing silently; got: {:?}",
            set.external_references
        );
    }

    #[test]
    fn an_identified_roots_bare_id_references_stay_dangling_checked() {
        // The external exemption must not swallow the typo case: a bare id
        // on the root's slot is a promise about *this* dataset, so it stays
        // non-external and the dangling check still owns it.
        let data: serde_norway::Value =
            serde_norway::from_str("id: acme\nname: Acme Corp\ndeployments:\n  - nope\n")
                .expect("parse");
        let set = InstanceSet::from_linkml_data(&root_schema(true), &data);
        let root = set.instances.iter().find(|i| i.id == "acme").expect("root");
        let r = root.references.first().expect("a reference");
        assert!(!r.external, "a bare id stays an intra-dataset reference");
        assert!(
            set.external_references.is_empty(),
            "and is not summarized as external; got: {:?}",
            set.external_references
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
                .all(|v| matches!(v, InstanceValue::Reference { .. })),
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
                    values: vec![InstanceValue::Reference {
                        target: "morgonEstate".to_string(),
                        held: false,
                    }],
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
                InstanceValue::Reference {
                    target: "b".to_string(),
                    held: false,
                },
                InstanceValue::Reference {
                    target: "c".to_string(),
                    held: false,
                },
                InstanceValue::Reference {
                    target: "d".to_string(),
                    held: true,
                },
            ],
            "an inlined element's edge is containment; by-id elements are citations"
        );
    }

    const SHELVED_SCHEMA: &str = "\
name: Estate
default_range: string
classes:
  Root:
    tree_root: true
    attributes:
      id:
        identifier: true
      shelves:
        range: Shelf
        multivalued: true
      main_shelf:
        range: Shelf
  Shelf:
    attributes:
      id:
        identifier: true
      held:
        range: Item
        multivalued: true
      next:
        range: Shelf
  Item:
    attributes:
      id:
        identifier: true
";

    #[test]
    fn a_single_valued_container_slot_materializes_or_cites() {
        let schema: SchemaDefinition = serde_norway::from_str(SHELVED_SCHEMA).expect("schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nmain_shelf:\n  id: s1\n  held:\n    - {id: w1}\n")
                .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);

        let mut ids: Vec<&str> = set.instances.iter().map(|i| i.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, vec!["est", "s1", "w1"], "no per-field phantom records");
        let root = set.instances.iter().find(|i| i.id == "est").expect("est");
        assert_eq!(
            root.slot_values
                .iter()
                .find(|sv| sv.slot == "main_shelf")
                .expect("main_shelf recorded")
                .values,
            [InstanceValue::Reference {
                target: "s1".to_string(),
                held: true,
            }],
            "the container's edge to the record it materialized is containment"
        );

        let schema: SchemaDefinition = serde_norway::from_str(SHELVED_SCHEMA).expect("schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nshelves:\n  - {id: s1}\nmain_shelf: s1\n")
                .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);

        let root = set.instances.iter().find(|i| i.id == "est").expect("est");
        assert_eq!(
            root.slot_values
                .iter()
                .find(|sv| sv.slot == "main_shelf")
                .expect("main_shelf recorded")
                .values,
            [InstanceValue::Reference {
                target: "s1".to_string(),
                held: false,
            }],
            "citing an existing record by id is not containment"
        );
        assert!(
            root.references
                .iter()
                .any(|r| r.property == "main_shelf" && r.target == "s1"),
            "the citation draws a display edge; got: {:?}",
            root.references
        );
    }

    #[test]
    fn a_type_designator_names_the_union_member_outright() {
        // Both members designate via `kind`, so key-matching alone ties.
        const DESIGNATED_UNION_SCHEMA: &str = "\
id: https://example.org/estate
name: Estate
default_prefix: est
prefixes:
  est: https://example.org/estate/
default_range: string
classes:
  Root:
    tree_root: true
    attributes:
      id:
        identifier: true
      things:
        multivalued: true
        any_of:
          - range: Shelf
          - range: Crate
  Shelf:
    attributes:
      id:
        identifier: true
      kind:
        designates_type: true
      held:
        range: string
  Crate:
    attributes:
      id:
        identifier: true
      kind:
        designates_type: true
      weight:
        range: integer
";
        let schema: SchemaDefinition =
            serde_norway::from_str(DESIGNATED_UNION_SCHEMA).expect("schema");

        let data: serde_norway::Value = serde_norway::from_str(
            "id: est\nthings:\n  - {id: x1, kind: Crate, held: misleading}\n",
        )
        .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        let x1 = set.instances.iter().find(|i| i.id == "x1").expect("x1");
        assert_eq!(
            x1.types,
            vec!["Crate"],
            "an authored designator beats a key that scores for another member"
        );

        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nthings:\n  - {id: x2, kind: Basket, held: y}\n")
                .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            !set.instances.iter().any(|i| i.id == "x2"),
            "a designator naming no member must not fall back to guessing; got: {:?}",
            set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
        );
        assert!(
            set.unusable_collection_entries
                .iter()
                .any(|u| u.reason.contains("type designator")),
            "the report names the mechanism that can fix it; got: {:?}",
            set.unusable_collection_entries
        );

        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nthings:\n  - {id: x3, kind: 42, held: y}\n")
                .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            !set.instances.iter().any(|i| i.id == "x3"),
            "a non-string designator is explicit-but-unusable, never a guess; got: {:?}",
            set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
        );

        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nthings:\n  - {id: c2, kind: est:Crate}\n")
                .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        let c2 = set
            .instances
            .iter()
            .find(|i| i.id == "c2")
            .unwrap_or_else(|| {
                panic!("the designator accepts the class IRI or CURIE; got: {set:?}")
            });
        assert_eq!(c2.types, vec!["Crate"]);

        // Two members declaring the same class_uri: an IRI designator
        // naming it names both, and a designation must name one thing.
        let shared_iri_schema = DESIGNATED_UNION_SCHEMA
            .replace("  Shelf:\n", "  Shelf:\n    class_uri: est:Thing\n")
            .replace("  Crate:\n", "  Crate:\n    class_uri: est:Thing\n");
        let schema: SchemaDefinition = serde_norway::from_str(&shared_iri_schema).expect("schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nthings:\n  - {id: x4, kind: est:Thing}\n")
                .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            !set.instances.iter().any(|i| i.id == "x4"),
            "an IRI naming several members is a conflict, never a pick; got: {:?}",
            set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
        );
        assert!(
            !set.unusable_collection_entries.is_empty(),
            "the ambiguous designation is reported"
        );
    }

    #[test]
    fn a_designator_on_one_member_does_not_hijack_the_others() {
        // Shelf designates via `kind`; Crate carries `kind` as an ordinary
        // slot and designates via `category` instead.
        const MIXED_DESIGNATOR_SCHEMA: &str = "\
id: https://example.org/estate
name: Estate
default_prefix: est
prefixes:
  est: https://example.org/estate/
default_range: string
classes:
  Root:
    tree_root: true
    attributes:
      id:
        identifier: true
      things:
        multivalued: true
        any_of:
          - range: Shelf
          - range: Crate
  Shelf:
    attributes:
      id:
        identifier: true
      kind:
        designates_type: true
      held:
        range: string
  Crate:
    attributes:
      id:
        identifier: true
      kind:
        range: string
      category:
        designates_type: true
      weight:
        range: integer
";
        let schema: SchemaDefinition =
            serde_norway::from_str(MIXED_DESIGNATOR_SCHEMA).expect("schema");

        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nthings:\n  - {id: c3, kind: wooden, weight: 3}\n")
                .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        let c3 = set
            .instances
            .iter()
            .find(|i| i.id == "c3")
            .unwrap_or_else(|| {
                panic!("a key that is ordinary for the true member stays data; got: {set:?}")
            });
        assert_eq!(
            c3.types,
            vec!["Crate"],
            "the key heuristic still decides when no designator resolves"
        );

        let data: serde_norway::Value = serde_norway::from_str(
            "id: est\nthings:\n  - {id: c4, kind: wooden, category: Crate}\n",
        )
        .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        let c4 = set.instances.iter().find(|i| i.id == "c4").expect("c4");
        assert_eq!(
            c4.types,
            vec!["Crate"],
            "every member's designator is consulted, not just the first's"
        );

        let data: serde_norway::Value = serde_norway::from_str(
            "id: est\nthings:\n  - {id: x5, kind: Shelf, category: Crate}\n",
        )
        .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            !set.instances.iter().any(|i| i.id == "x5"),
            "two designators naming different members conflict, never a pick; got: {:?}",
            set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
        );

        // A compact SimpleDict entry: excluding the designator, Shelf has
        // exactly one open slot (`held`), so the entry still loads.
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nthings:\n  s5: aisle-9\n").expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        let s5 = set
            .instances
            .iter()
            .find(|i| i.id == "s5")
            .unwrap_or_else(|| panic!("a designator is not an open slot; got: {set:?}"));
        assert_eq!(s5.types, vec!["Shelf"]);

        // `category` is unanswerable and Shelf never carries it, so no
        // member reads it as plain data — the refusal stands even though
        // the key heuristic alone would have scored Crate ahead.
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nthings:\n  - {id: x6, category: Basket}\n")
                .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            !set.instances.iter().any(|i| i.id == "x6"),
            "a member without the key cannot turn the refusal into a guess; got: {:?}",
            set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
        );
        assert!(
            set.unusable_collection_entries
                .iter()
                .any(|u| u.slot == "things"),
            "the unanswerable designation is reported; got: {:?}",
            set.unusable_collection_entries
        );
    }

    #[test]
    fn a_vessel_root_reports_a_designator_miss() {
        const VESSEL_DESIGNATED_SCHEMA: &str = "\
name: Estate
default_range: string
classes:
  Root:
    tree_root: true
    attributes:
      thing:
        any_of:
          - range: Shelf
          - range: Crate
  Shelf:
    attributes:
      id:
        identifier: true
      kind:
        designates_type: true
      held:
        range: string
  Crate:
    attributes:
      id:
        identifier: true
      kind:
        designates_type: true
      weight:
        range: integer
";
        let schema: SchemaDefinition =
            serde_norway::from_str(VESSEL_DESIGNATED_SCHEMA).expect("schema");
        let data: serde_norway::Value =
            serde_norway::from_str("thing: {id: x6, kind: Basket}\n").expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            !set.instances.iter().any(|i| i.id == "x6"),
            "got: {:?}",
            set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
        );
        assert!(
            !set.unusable_collection_entries.is_empty(),
            "the miss is reported even with no container record to carry it"
        );

        // A list under the same slot walks entry by entry: hits build,
        // the miss is still reported.
        let data: serde_norway::Value = serde_norway::from_str(
            "thing:\n  - {id: v1, kind: Crate, weight: 2}\n  - {id: x7, kind: Basket}\n",
        )
        .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        let v1 = set
            .instances
            .iter()
            .find(|i| i.id == "v1")
            .unwrap_or_else(|| panic!("list entries materialize; got: {set:?}"));
        assert_eq!(v1.types, vec!["Crate"]);
        assert!(
            !set.instances.iter().any(|i| i.id == "x7")
                && !set.unusable_collection_entries.is_empty(),
            "the miss inside the list is refused and reported; got: {set:?}"
        );
    }

    #[test]
    fn id_collisions_are_reported_wherever_they_hide() {
        let schema: SchemaDefinition = serde_norway::from_str(SHELVED_SCHEMA).expect("schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: s1\nshelves:\n  - {id: s1}\n  - {id: s2}\n").expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);

        assert_eq!(set.duplicate_ids, vec!["s1"], "the collision is reported");
        assert_eq!(set.root_record, None, "no record is promoted to container");
        let s1 = set.instances.iter().find(|i| i.id == "s1").expect("s1");
        assert!(
            !s1.slot_values.iter().any(|sv| sv.slot == "shelves"),
            "the contained record must not absorb the container's edges; got: {:?}",
            s1.slot_values
        );

        let schema: SchemaDefinition = serde_norway::from_str(SHELVED_SCHEMA).expect("schema");
        let data: serde_norway::Value = serde_norway::from_str(
            "id: est\nshelves:\n  - {id: s1, held: [{id: dup}]}\n  - {id: dup, held: [{id: i9}]}\n",
        )
        .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert_eq!(
            set.duplicate_ids,
            vec!["dup"],
            "the second shelf's authored content was discarded, which must be reported"
        );

        let schema: SchemaDefinition = serde_norway::from_str(SHELVED_SCHEMA).expect("schema");
        // x exists as an Item; the Shelf-ranged slot restates its id with
        // no content of its own, so only the class conflict flags it.
        let data: serde_norway::Value = serde_norway::from_str(
            "id: est\nshelves:\n  - {id: s1, held: [{id: x}]}\n  - {id: s2, next: {id: x}}\n",
        )
        .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert_eq!(set.duplicate_ids, vec!["x"]);

        let schema: SchemaDefinition = serde_norway::from_str(SHELVED_SCHEMA).expect("schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nshelves:\n  - {id: s1}\n  - {id: s1}\n")
                .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert_eq!(
            set.duplicate_ids,
            vec!["s1"],
            "two top-level claims on one id are a collision even with identical content"
        );
    }

    const CLASS_UNION_SCHEMA: &str = "\
name: Estate
default_range: string
classes:
  Root:
    tree_root: true
    attributes:
      id:
        identifier: true
      links:
        range: Link
        multivalued: true
  Link:
    attributes:
      id:
        identifier: true
      related:
        any_of:
          - range: Shelf
          - range: Crate
  Shelf:
    attributes:
      id:
        identifier: true
      held:
        range: string
  Crate:
    attributes:
      id:
        identifier: true
      weight:
        range: integer
";

    #[test]
    fn an_inline_mapping_at_a_class_union_builds_when_fields_name_one_member() {
        let schema: SchemaDefinition = serde_norway::from_str(CLASS_UNION_SCHEMA).expect("schema");
        let data: serde_norway::Value = serde_norway::from_str(
            "id: est\nlinks:\n  - {id: l1, related: {id: s1, held: aisle-3}}\n",
        )
        .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        let s1 = set
            .instances
            .iter()
            .find(|i| i.id == "s1")
            .unwrap_or_else(|| panic!("s1 built as the one covering member; got: {set:?}"));
        assert_eq!(s1.types, vec!["Shelf"], "held names Shelf, not Crate");
        let l1 = set.instances.iter().find(|i| i.id == "l1").expect("l1");
        assert_eq!(
            l1.slot_values
                .iter()
                .find(|sv| sv.slot == "related")
                .expect("related recorded")
                .values,
            [InstanceValue::Reference {
                target: "s1".to_string(),
                held: true,
            }]
        );

        let schema: SchemaDefinition = serde_norway::from_str(CLASS_UNION_SCHEMA).expect("schema");
        // `{id}` fits Shelf and Crate equally, so the class stays ambiguous.
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nlinks:\n  - {id: l1, related: {id: a1}}\n")
                .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            !set.instances.iter().any(|i| i.id == "a1"),
            "no record is built by guessing a class"
        );
        let l1 = set.instances.iter().find(|i| i.id == "l1").expect("l1");
        assert_eq!(
            l1.slot_values
                .iter()
                .find(|sv| sv.slot == "related")
                .expect("related recorded")
                .values,
            [InstanceValue::Unexpected("an object")]
        );

        let schema: SchemaDefinition = serde_norway::from_str(CLASS_UNION_SCHEMA).expect("schema");
        let data: serde_norway::Value = serde_norway::from_str(
            "id: est\nlinks:\n  - {id: l1, related: {id: s1, held: aisle-3, note: oops}}\n",
        )
        .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        let s1 = set
            .instances
            .iter()
            .find(|i| i.id == "s1")
            .unwrap_or_else(|| panic!("held names Shelf over Crate; got: {set:?}"));
        assert_eq!(s1.types, vec!["Shelf"]);
        assert!(
            set.undeclared_fields
                .iter()
                .any(|f| f.record == "s1" && f.field == "note"),
            "the unknown field is a named diagnostic, not a forfeit; got: {:?}",
            set.undeclared_fields
        );
    }

    #[test]
    fn duplicate_union_branches_naming_one_class_still_build() {
        const REPEATED_BRANCH_SCHEMA: &str = "\
name: Estate
default_range: string
classes:
  Root:
    tree_root: true
    attributes:
      id:
        identifier: true
      links:
        range: Link
        multivalued: true
  Link:
    attributes:
      id:
        identifier: true
      related:
        any_of:
          - range: Shelf
          - range: Shelf
  Shelf:
    attributes:
      id:
        identifier: true
      held:
        range: string
";
        let schema: SchemaDefinition =
            serde_norway::from_str(REPEATED_BRANCH_SCHEMA).expect("schema");
        let data: serde_norway::Value = serde_norway::from_str(
            "id: est\nlinks:\n  - {id: l1, related: {id: s1, held: aisle-3}}\n",
        )
        .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            set.instances.iter().any(|i| i.id == "s1"),
            "one class repeated across branches is one candidate; got: {:?}",
            set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
        );
    }

    const VESSEL_UNION_SCHEMA: &str = "\
name: Estate
default_range: string
classes:
  Root:
    tree_root: true
    attributes:
      things:
        multivalued: true
        any_of:
          - range: Shelf
          - range: Crate
  Shelf:
    attributes:
      id:
        identifier: true
      held:
        range: string
  Crate:
    attributes:
      id:
        identifier: true
      weight:
        range: integer
      label:
        range: string
";

    #[test]
    fn a_vessel_rooted_union_collection_builds_and_reports_in_every_spelling() {
        let schema: SchemaDefinition = serde_norway::from_str(VESSEL_UNION_SCHEMA).expect("schema");
        let data: serde_norway::Value =
            serde_norway::from_str("things:\n  - {id: s1, held: aisle-3}\n").expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            set.instances.iter().any(|i| i.id == "s1"),
            "no container exists to hold it, but the record is data; got: {:?}",
            set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
        );

        let data: serde_norway::Value =
            serde_norway::from_str("things:\n  s1: {held: aisle-3}\n").expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            set.instances.iter().any(|i| i.id == "s1"),
            "dict spelling loads for a vessel root too; got: {:?}",
            set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
        );

        let data: serde_norway::Value =
            serde_norway::from_str("things:\n  s1: {held: aisle-3}\n  a1: {}\n").expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            set.unusable_collection_entries
                .iter()
                .any(|u| u.key.as_deref() == Some("a1")),
            "no container record exists, and the entry is still reported; got: {:?}",
            set.unusable_collection_entries
        );
        let violations = crate::validate::validate_instances(&schema, &set);
        assert!(
            violations.iter().any(|v| v.detail.contains("a1")),
            "validation names the entry; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
    }

    const UNION_CONTAINER_SCHEMA: &str = "\
name: Estate
default_range: string
classes:
  Root:
    tree_root: true
    attributes:
      id:
        identifier: true
      things:
        multivalued: true
        any_of:
          - range: Shelf
          - range: Crate
  Shelf:
    attributes:
      id:
        identifier: true
      held:
        range: string
  Crate:
    attributes:
      id:
        identifier: true
      weight:
        range: integer
      label:
        range: string
";

    #[test]
    fn union_dict_entries_choose_their_member_or_are_reported() {
        let schema: SchemaDefinition =
            serde_norway::from_str(UNION_CONTAINER_SCHEMA).expect("schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nthings:\n  s1: {held: aisle-3}\n  c1: {weight: 5}\n")
                .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        let s1 = set
            .instances
            .iter()
            .find(|i| i.id == "s1")
            .unwrap_or_else(|| panic!("s1 loads from the dict entry; got: {set:?}"));
        assert_eq!(s1.types, vec!["Shelf"]);
        let c1 = set.instances.iter().find(|i| i.id == "c1").expect("c1");
        assert_eq!(c1.types, vec!["Crate"]);
        let root = set.instances.iter().find(|i| i.id == "est").expect("est");
        let things = root
            .slot_values
            .iter()
            .find(|sv| sv.slot == "things")
            .expect("things recorded");
        for id in ["s1", "c1"] {
            assert!(
                things.values.contains(&InstanceValue::Reference {
                    target: id.to_string(),
                    held: true,
                }),
                "the container holds `{id}` by role; got: {:?}",
                things.values
            );
        }

        let schema: SchemaDefinition =
            serde_norway::from_str(UNION_CONTAINER_SCHEMA).expect("schema");
        // Shelf has exactly one non-key slot, Crate has two — only Shelf
        // can say which slot the compact scalar fills.
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nthings:\n  s9: aisle-4\n").expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        let s9 = set
            .instances
            .iter()
            .find(|i| i.id == "s9")
            .unwrap_or_else(|| panic!("the compact entry expands as a Shelf; got: {set:?}"));
        assert_eq!(s9.types, vec!["Shelf"]);
        assert_eq!(
            s9.slot_values
                .iter()
                .find(|sv| sv.slot == "held")
                .expect("held filled")
                .values,
            [InstanceValue::Scalar(ScalarValue::String(
                "aisle-4".to_string()
            ))]
        );

        let schema: SchemaDefinition =
            serde_norway::from_str(UNION_CONTAINER_SCHEMA).expect("schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nthings:\n  s1: {held: aisle-3}\n  a1: {}\n")
                .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            set.instances.iter().any(|i| i.id == "s1"),
            "one bad entry does not forfeit the collection; got: {:?}",
            set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
        );
        assert!(
            !set.instances.iter().any(|i| i.id == "a1"),
            "no record is built by guessing a class"
        );
        assert!(
            set.unusable_collection_entries
                .iter()
                .any(|u| u.key.as_deref() == Some("a1")),
            "the unusable entry stays visible; got: {:?}",
            set.unusable_collection_entries
        );

        let schema: SchemaDefinition =
            serde_norway::from_str(UNION_CONTAINER_SCHEMA).expect("schema");
        // An un-wrapped inline record: `weight` is Crate's slot, not an id.
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nthings:\n  weight: 5\n").expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            !set.instances.iter().any(|i| i.id == "weight"),
            "no phantom record named after a field; got: {:?}",
            set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
        );
        assert!(
            set.unusable_collection_entries
                .iter()
                .any(|u| u.key.as_deref() == Some("weight") && u.reason.contains("field")),
            "the report points at the wrapping, not the value; got: {:?}",
            set.unusable_collection_entries
        );

        let schema: SchemaDefinition =
            serde_norway::from_str(UNION_CONTAINER_SCHEMA).expect("schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nthings:\n  s9: ~\n").expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            !set.instances.iter().any(|i| i.id == "s9"),
            "a null cannot choose a class; got: {:?}",
            set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
        );
        assert!(
            set.unusable_collection_entries
                .iter()
                .any(|u| u.key.as_deref() == Some("s9")),
            "got: {:?}",
            set.unusable_collection_entries
        );

        let schema: SchemaDefinition =
            serde_norway::from_str(UNION_CONTAINER_SCHEMA).expect("schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nthings:\n  5: {id: s1, held: x}\n").expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            set.instances.iter().any(|i| i.id == "s1"),
            "the record's own id carries it; got: {:?}",
            set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
        );
        assert!(
            set.unusable_collection_entries
                .iter()
                .any(|u| u.slot == "things" && u.reason.contains("non-string key")),
            "the discarded authored key is reported; got: {:?}",
            set.unusable_collection_entries
        );
    }

    const MIXED_UNION_SCHEMA: &str = "\
name: Estate
default_range: string
classes:
  Root:
    tree_root: true
    attributes:
      id:
        identifier: true
      things:
        multivalued: true
        any_of:
          - range: Shelf
          - range: Crate
          - range: string
  Shelf:
    attributes:
      id:
        identifier: true
      held:
        range: string
  Crate:
    attributes:
      id:
        identifier: true
      weight:
        range: integer
      label:
        range: string
";

    #[test]
    fn a_mixed_union_collection_keeps_string_literals_as_scalars() {
        let schema: SchemaDefinition = serde_norway::from_str(MIXED_UNION_SCHEMA).expect("schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nthings:\n  - aisle-3\n  - {id: s1, held: x}\n")
                .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        let root = set.instances.iter().find(|i| i.id == "est").expect("est");
        let things = root
            .slot_values
            .iter()
            .find(|sv| sv.slot == "things")
            .expect("things recorded");
        assert!(
            things
                .values
                .contains(&InstanceValue::Scalar(ScalarValue::String(
                    "aisle-3".to_string()
                ))),
            "a string at a class+type union stays a literal; got: {:?}",
            things.values
        );
        assert!(
            set.instances.iter().any(|i| i.id == "s1"),
            "the record entry still builds"
        );
    }

    #[test]
    fn union_list_entries_cite_build_or_are_reported() {
        let schema: SchemaDefinition =
            serde_norway::from_str(UNION_CONTAINER_SCHEMA).expect("schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nthings:\n  - s9\n  - 5\n").expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        let root = set.instances.iter().find(|i| i.id == "est").expect("est");
        assert!(
            root.references.iter().any(|r| r.target == "s9"),
            "a bare id cites; got: {:?}",
            root.references
        );
        assert!(
            !root.references.iter().any(|r| r.target == "5"),
            "a number can never cite a record; got: {:?}",
            root.references
        );
        assert!(
            set.unusable_collection_entries
                .iter()
                .any(|u| u.slot == "things" && u.reason.contains("a number")),
            "the entry is reported, not dropped; got: {:?}",
            set.unusable_collection_entries
        );

        let schema: SchemaDefinition =
            serde_norway::from_str(UNION_CONTAINER_SCHEMA).expect("schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nthings:\n  - [{id: s1, held: x}]\n").expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            set.instances.iter().any(|i| i.id == "s1"),
            "an authored arity mistake stays loadable; got: {:?}",
            set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn an_ambiguous_simple_dict_entry_names_the_entry_and_the_ambiguity() {
        const TWIN_SCHEMA: &str = "\
name: Estate
default_range: string
classes:
  Root:
    tree_root: true
    attributes:
      id:
        identifier: true
      things:
        multivalued: true
        any_of:
          - range: Shelf
          - range: Bin
  Shelf:
    attributes:
      id:
        identifier: true
      held:
        range: string
  Bin:
    attributes:
      id:
        identifier: true
      contents:
        range: string
";
        let schema: SchemaDefinition = serde_norway::from_str(TWIN_SCHEMA).expect("schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nthings:\n  s9: aisle-4\n").expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        let violations = crate::validate::validate_instances(&schema, &set);
        let about_s9: Vec<String> = violations
            .iter()
            .map(|v| v.to_string())
            .filter(|m| m.contains("s9"))
            .collect();
        assert!(
            !about_s9.is_empty(),
            "the entry is named; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
        assert!(
            about_s9.iter().any(|m| m.contains("more than one")),
            "ambiguity is the stated failure, not the value's kind; got: {about_s9:?}"
        );
    }

    #[test]
    fn a_non_string_field_key_is_reported_not_dropped() {
        let schema: SchemaDefinition = serde_norway::from_str(SHELVED_SCHEMA).expect("schema");
        // The record is restated identically (one entity, two spellings)
        // and carries both a quotable and an unquotable non-string key.
        let data: serde_norway::Value = serde_norway::from_str(
            "id: est\nshelves:\n  - {id: s1, 2024: oops, ~: nix}\n  - {id: s1, 2024: oops, ~: nix}\n",
        )
        .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        let quotable: Vec<_> = set
            .undeclared_fields
            .iter()
            .filter(|f| f.field == "2024")
            .collect();
        assert_eq!(
            quotable.len(),
            1,
            "one authored defect is one finding, however many restatements; got: {:?}",
            set.undeclared_fields
        );
        assert_eq!(quotable[0].record, "s1");
        assert_eq!(quotable[0].key_kind, Some(KeyKind::Quotable));
        let violations = crate::validate::validate_instances(&schema, &set);
        assert!(
            violations
                .iter()
                .any(|v| v.detail.contains("2024") && v.detail.contains("quote")),
            "quoting fixes a scalar key, and the report says so; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
        assert!(
            violations
                .iter()
                .any(|v| v.detail.contains("only string keys") && !v.detail.contains("quote")),
            "an unquotable key gets its own wording, not impossible advice; got: {:?}",
            violations.iter().map(|v| v.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_non_string_container_field_key_is_reported_too() {
        let schema: SchemaDefinition = serde_norway::from_str(SHELVED_SCHEMA).expect("schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\n2024: oops\nbogus: kept\nshelves: []\n")
                .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        let entry = set
            .undeclared_fields
            .iter()
            .find(|f| f.field == "2024")
            .unwrap_or_else(|| panic!("got: {:?}", set.undeclared_fields));
        assert_eq!(
            entry.record, "est",
            "the finding names the container's authored id, not its class"
        );
        assert_eq!(entry.key_kind, Some(KeyKind::Quotable));
        assert!(
            set.undeclared_fields
                .iter()
                .any(|f| f.field == "bogus" && f.key_kind.is_none()),
            "an undeclared string key on the container is kept and reported, \
             exactly as on any record; got: {:?}",
            set.undeclared_fields
        );
    }

    #[test]
    fn identical_restatements_are_one_entity_not_a_collision() {
        let schema: SchemaDefinition = serde_norway::from_str(SHELVED_SCHEMA).expect("schema");
        // s1's inline child is restated by bare id: same edge, different
        // spelling, nothing lost.
        let data: serde_norway::Value = serde_norway::from_str(
            "id: est\nshelves:\n  - {id: s1, held: [{id: w1}]}\n  - {id: s2, next: {id: s1, held: [w1]}}\n",
        )
        .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert_eq!(set.duplicate_ids, Vec::<String>::new());

        let schema: SchemaDefinition = serde_norway::from_str(SHELVED_SCHEMA).expect("schema");
        let data: serde_norway::Value = serde_norway::from_str(
            "id: est\nshelves:\n  - {id: s1, held: [{id: w1}]}\n  - {id: s2, held: [{id: w1}]}\n",
        )
        .expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert_eq!(
            set.duplicate_ids,
            Vec::<String>::new(),
            "restating a record by bare id discards nothing"
        );
    }

    #[test]
    fn an_any_of_class_ranged_container_slot_loads_its_records() {
        const ANY_OF_SCHEMA: &str = "\
name: Estate
default_range: string
classes:
  Root:
    tree_root: true
    attributes:
      id:
        identifier: true
      things:
        multivalued: true
        any_of:
          - range: Shelf
  Shelf:
    attributes:
      id:
        identifier: true
";
        let schema: SchemaDefinition = serde_norway::from_str(ANY_OF_SCHEMA).expect("schema");
        let data: serde_norway::Value =
            serde_norway::from_str("id: est\nthings:\n  - {id: s1}\n").expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            set.instances.iter().any(|i| i.id == "s1"),
            "the collection's records load through the slot's induced range; got: {:?}",
            set.instances.iter().map(|i| &i.id).collect::<Vec<_>>()
        );
        let root = set.instances.iter().find(|i| i.id == "est").expect("est");
        assert_eq!(
            root.slot_values
                .iter()
                .find(|sv| sv.slot == "things")
                .expect("things recorded")
                .values,
            [InstanceValue::Reference {
                target: "s1".to_string(),
                held: true,
            }]
        );
    }

    #[test]
    fn a_container_without_its_authored_identifier_is_not_emitted() {
        let schema: SchemaDefinition = serde_norway::from_str(SHELVED_SCHEMA).expect("schema");
        let data: serde_norway::Value =
            serde_norway::from_str("shelves:\n  - {id: s1}\n").expect("data");
        let set = InstanceSet::from_linkml_data(&schema, &data);
        assert_eq!(
            set.root_record, None,
            "a declared but never-authored identifier fabricates no container"
        );
        assert_eq!(
            set.instances
                .iter()
                .map(|i| i.id.as_str())
                .collect::<Vec<_>>(),
            vec!["s1"],
            "no phantom record with a synthesized id"
        );
        assert_eq!(set.duplicate_ids, Vec::<String>::new());
    }
}
