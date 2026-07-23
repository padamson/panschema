//! RDF Serializers
//!
//! Provides multiple RDF serialization formats using sophia.
//! Builds an RDF graph from LinkML IR and serializes to JSON-LD, RDF/XML, N-Triples.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use sophia::api::graph::{Graph, MutableGraph};
use sophia::api::ns::{Namespace, rdf, rdfs};
use sophia::api::serializer::{QuadSerializer, TripleSerializer};
use sophia::inmem::graph::FastGraph;
use sophia::iri::Iri;

use crate::io::{IoError, IoResult, Writer};
use crate::linkml::{ClassDefinition, SchemaDefinition, SlotDefinition};

// Namespace constants
pub(crate) const OWL_NS: &str = "http://www.w3.org/2002/07/owl#";
const DCTERMS_NS: &str = "http://purl.org/dc/terms/";
const SKOS_NS: &str = "http://www.w3.org/2004/02/skos/core#";
pub(crate) const SH_NS: &str = "http://www.w3.org/ns/shacl#";
pub(crate) const XSD_NS: &str = "http://www.w3.org/2001/XMLSchema#";
pub(crate) const RDF_NS: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
pub(crate) const RDFS_NS: &str = "http://www.w3.org/2000/01/rdf-schema#";

/// Build a sophia Turtle prefix map from the schema's `prefixes:` block plus
/// the given per-writer builtin prefixes (e.g. `xsd:` for OWL, `sh:` for
/// SHACL) — one builder shared by every Turtle-emitting writer so their
/// declarations can't drift. A builtin whose name the schema already declares
/// is left to the schema's binding. Entries that fail sophia's prefix/IRI
/// validation are dropped with a `tracing::warn!` (they can't appear in the
/// output anyway).
pub(crate) fn build_turtle_prefix_map(
    schema: &SchemaDefinition,
    builtins: &[(&str, &str)],
) -> Vec<sophia::api::prefix::PrefixMapPair> {
    use sophia::api::prefix::Prefix;
    schema
        .prefixes
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_str()))
        .chain(
            builtins
                .iter()
                .copied()
                .filter(|(name, _)| !schema.prefixes.contains_key(*name)),
        )
        .filter_map(|(name, base)| {
            let prefix = Prefix::new(name.to_string().into_boxed_str())
                .map_err(|e| tracing::warn!(prefix = name, error = %e, "skipping invalid prefix"))
                .ok()?;
            let iri = Iri::new(base.to_string().into_boxed_str())
                .map_err(
                    |e| tracing::warn!(prefix = name, base, error = %e, "skipping bad base IRI"),
                )
                .ok()?;
            Some((prefix, iri))
        })
        .collect()
}

/// The ontology's base IRI — the schema `id`, or the shared fallback.
fn ontology_iri_string(schema: &SchemaDefinition) -> &str {
    schema
        .id
        .as_deref()
        .unwrap_or("http://example.org/ontology")
}

/// Absolute IRI for a class: its `class_uri` (CURIE-expanded) or
/// `{ontology}#{name}`. The single source of class-IRI derivation, shared
/// by the OWL graph and the SHACL shapes graph so a shape targets exactly
/// the IRI the OWL output declares.
fn class_iri_string(name: &str, class_def: &ClassDefinition, schema: &SchemaDefinition) -> String {
    class_def
        .class_uri
        .as_deref()
        .map(|c| expand_curie(c, schema))
        .unwrap_or_else(|| format!("{}#{}", ontology_iri_string(schema), name))
}

/// Absolute IRI for a slot: its `slot_uri` (CURIE-expanded) or
/// `{ontology}#{name}`. Shared by the OWL graph and the SHACL shapes graph.
fn slot_iri_string(name: &str, slot_def: &SlotDefinition, schema: &SchemaDefinition) -> String {
    slot_def
        .slot_uri
        .as_deref()
        .map(|s| expand_curie(s, schema))
        .unwrap_or_else(|| format!("{}#{}", ontology_iri_string(schema), name))
}

/// Expand a CURIE-shaped name (`prefix:local`) against `schema.prefixes`
/// into an absolute IRI. Inputs that are already absolute URLs
/// (`http://…` / `https://…` / any scheme followed by `//`) pass through
/// unchanged. Bare names (no colon) are returned as-is — callers handle
/// the `default_prefix` / `id` fallback. CURIE prefixes that don't appear
/// in `schema.prefixes` are passed through with a `tracing::warn!` so the
/// caller doesn't silently emit a relative IRI.
fn expand_curie(name: &str, schema: &SchemaDefinition) -> String {
    // Delegate the expansion decision (known prefix, absolute IRI,
    // `default_prefix` for bare names) to the one shared implementation the
    // HTML writer also uses, so the two can't diverge. That core returns
    // `None` when nothing resolves; RDF must still emit *something*, so pass
    // the input through unchanged with a warning (an undeclared prefix, or a
    // bare name with no `default_prefix`, that `build_rdf_graph` will fall
    // back on).
    crate::linkml_resolve::expand_curie(schema, name).unwrap_or_else(|| {
        tracing::warn!(
            curie = name,
            "CURIE could not be expanded against `schema.prefixes`; \
             emitting unexpanded IRI which may be invalid downstream"
        );
        name.to_string()
    })
}

/// Emit one SKOS triple per mapping value for the subject IRI,
/// CURIE-expanded against the schema's prefixes.
#[allow(clippy::too_many_arguments)]
fn emit_mappings(
    graph: &mut FastGraph,
    subject_iri: &Iri<String>,
    schema: &SchemaDefinition,
    exact: &[String],
    close: &[String],
    related: &[String],
    narrow: &[String],
    broad: &[String],
) -> IoResult<()> {
    let skos = Namespace::new_unchecked(SKOS_NS);
    for (predicate_name, values) in [
        ("exactMatch", exact),
        ("closeMatch", close),
        ("relatedMatch", related),
        ("narrowMatch", narrow),
        ("broadMatch", broad),
    ] {
        if values.is_empty() {
            continue;
        }
        let predicate = skos
            .get(predicate_name)
            .map_err(|e| IoError::Parse(e.to_string()))?;
        for value in values {
            let object_iri = make_iri(&expand_curie(value, schema))?;
            graph
                .insert(subject_iri, predicate, &object_iri)
                .map_err(|e| IoError::Write(e.to_string()))?;
        }
    }
    Ok(())
}

/// Emit the editorial cross-references for a subject IRI: one
/// `skos:altLabel` literal per alias and one `rdfs:seeAlso` IRI per
/// `see_also` reference (CURIE-expanded against the schema's prefixes).
fn emit_aliases_and_see_also(
    graph: &mut FastGraph,
    subject_iri: &Iri<String>,
    schema: &SchemaDefinition,
    aliases: &[String],
    see_also: &[String],
) -> IoResult<()> {
    let skos = Namespace::new_unchecked(SKOS_NS);
    let skos_alt_label = skos
        .get("altLabel")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    for alias in aliases {
        graph
            .insert(subject_iri, skos_alt_label, alias.as_str())
            .map_err(|e| IoError::Write(e.to_string()))?;
    }
    for reference in see_also {
        let object_iri = make_iri(&expand_curie(reference, schema))?;
        graph
            .insert(subject_iri, rdfs::seeAlso, &object_iri)
            .map_err(|e| IoError::Write(e.to_string()))?;
    }
    Ok(())
}

/// Build an RDF graph from a SchemaDefinition
pub fn build_rdf_graph(schema: &SchemaDefinition) -> IoResult<FastGraph> {
    let mut graph = FastGraph::new();

    let owl = Namespace::new_unchecked(OWL_NS);
    let dcterms = Namespace::new_unchecked(DCTERMS_NS);

    // Ontology IRI
    let ontology_iri_str = ontology_iri_string(schema);
    let ontology_iri = make_iri(ontology_iri_str)?;

    // Ontology declaration
    let owl_ontology = owl
        .get("Ontology")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    graph
        .insert(&ontology_iri, rdf::type_, owl_ontology)
        .map_err(|e| IoError::Write(e.to_string()))?;

    // rdfs:label from title
    if let Some(ref title) = schema.title {
        graph
            .insert(&ontology_iri, rdfs::label, title.as_str())
            .map_err(|e| IoError::Write(e.to_string()))?;
    }

    // rdfs:comment from description
    if let Some(ref description) = schema.description {
        graph
            .insert(&ontology_iri, rdfs::comment, description.as_str())
            .map_err(|e| IoError::Write(e.to_string()))?;
    }

    // owl:versionInfo
    let owl_version_info = owl
        .get("versionInfo")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    if let Some(ref version) = schema.version {
        graph
            .insert(&ontology_iri, owl_version_info, version.as_str())
            .map_err(|e| IoError::Write(e.to_string()))?;

        // owl:versionIRI
        if let Some(ref id) = schema.id {
            let version_iri = make_iri(&format!("{}/{}", id, version))?;
            let owl_version_iri = owl
                .get("versionIRI")
                .map_err(|e| IoError::Parse(e.to_string()))?;
            graph
                .insert(&ontology_iri, owl_version_iri, &version_iri)
                .map_err(|e| IoError::Write(e.to_string()))?;
        }
    }

    // dcterms:license
    let dcterms_license = dcterms
        .get("license")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    if let Some(ref license) = schema.license {
        let license_iri = make_iri(license)?;
        graph
            .insert(&ontology_iri, dcterms_license, &license_iri)
            .map_err(|e| IoError::Write(e.to_string()))?;
    }

    // dcterms:creator from contributors
    let dcterms_creator = dcterms
        .get("creator")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    for contributor in &schema.contributors {
        graph
            .insert(&ontology_iri, dcterms_creator, contributor.name.as_str())
            .map_err(|e| IoError::Write(e.to_string()))?;
    }

    // dcterms:created
    let dcterms_created = dcterms
        .get("created")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    if let Some(ref created) = schema.created {
        graph
            .insert(&ontology_iri, dcterms_created, created.as_str())
            .map_err(|e| IoError::Write(e.to_string()))?;
    }

    // dcterms:modified
    let dcterms_modified = dcterms
        .get("modified")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    if let Some(ref modified) = schema.modified {
        graph
            .insert(&ontology_iri, dcterms_modified, modified.as_str())
            .map_err(|e| IoError::Write(e.to_string()))?;
    }

    // Classes
    let owl_class = owl
        .get("Class")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    let owl_deprecated = owl
        .get("deprecated")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    let rdfs_subclass_of = rdfs::subClassOf;

    for (name, class_def) in &schema.classes {
        let class_iri_str = class_iri_string(name, class_def, schema);
        let class_iri = make_iri(&class_iri_str)?;

        // rdf:type owl:Class
        graph
            .insert(&class_iri, rdf::type_, owl_class)
            .map_err(|e| IoError::Write(e.to_string()))?;

        // owl:deprecated true — a Rust bool serializes as an
        // `xsd:boolean`-typed literal.
        if class_def.deprecated.is_some() {
            graph
                .insert(&class_iri, owl_deprecated, true)
                .map_err(|e| IoError::Write(e.to_string()))?;
        }

        // rdfs:label
        let label = class_def
            .annotations
            .get("panschema:label")
            .cloned()
            .unwrap_or_else(|| name.to_string());
        graph
            .insert(&class_iri, rdfs::label, label.as_str())
            .map_err(|e| IoError::Write(e.to_string()))?;

        // rdfs:comment from description
        if let Some(ref description) = class_def.description {
            graph
                .insert(&class_iri, rdfs::comment, description.as_str())
                .map_err(|e| IoError::Write(e.to_string()))?;
        }

        // rdfs:subClassOf from is_a and each mixin. LinkML treats mixins
        // as multiple inheritance; in OWL that maps to one rdfs:subClassOf
        // edge per parent, including each mixin.
        for parent in class_def.is_a.iter().chain(class_def.mixins.iter()) {
            let parent_iri_str = schema
                .classes
                .get(parent)
                .and_then(|c| c.class_uri.as_deref())
                .map(|c| expand_curie(c, schema))
                .unwrap_or_else(|| format!("{}#{}", ontology_iri_str, parent));
            let parent_iri = make_iri(&parent_iri_str)?;
            graph
                .insert(&class_iri, rdfs_subclass_of, &parent_iri)
                .map_err(|e| IoError::Write(e.to_string()))?;
        }

        // External rdfs:subClassOf grounding (`subclass_of:` in
        // LinkML) — typically an upstream ontology class (BFO, CCO,
        // IAO, …). Same predicate as `is_a`, but resolves through
        // the schema's prefix table rather than the local classes
        // map. Single-valued per the LinkML metamodel.
        if let Some(external) = class_def.subclass_of.as_deref() {
            let target_iri = make_iri(&expand_curie(external, schema))?;
            graph
                .insert(&class_iri, rdfs_subclass_of, &target_iri)
                .map_err(|e| IoError::Write(e.to_string()))?;
        }

        emit_mappings(
            &mut graph,
            &class_iri,
            schema,
            &class_def.exact_mappings,
            &class_def.close_mappings,
            &class_def.related_mappings,
            &class_def.narrow_mappings,
            &class_def.broad_mappings,
        )?;

        emit_aliases_and_see_also(
            &mut graph,
            &class_iri,
            schema,
            &class_def.aliases,
            &class_def.see_also,
        )?;
    }

    // Properties (slots)
    let owl_object_property = owl
        .get("ObjectProperty")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    let owl_datatype_property = owl
        .get("DatatypeProperty")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    let owl_inverse_of = owl
        .get("inverseOf")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    // OWL relationship-characteristic classes, in the same order as the
    // slot's bool flags below.
    let owl_characteristic_types = [
        owl.get("SymmetricProperty"),
        owl.get("AsymmetricProperty"),
        owl.get("ReflexiveProperty"),
        owl.get("IrreflexiveProperty"),
        owl.get("TransitiveProperty"),
    ];
    let owl_characteristic_types: Vec<_> = owl_characteristic_types
        .into_iter()
        .map(|t| t.map_err(|e| IoError::Parse(e.to_string())))
        .collect::<Result<_, _>>()?;

    // Assemble the properties to declare. Every top-level `schema.slots`
    // entry emits with its canonical global definition (unchanged). On top of
    // that, a class using inline `attributes:` (or a slot reached only through
    // `is_a`/mixin resolution) introduces effective slots that never appear in
    // `schema.slots`; without these the RDF output declares a class with no
    // properties, and any SHACL `sh:path` pointing at them has no OWL
    // counterpart. Fold each such slot in once (dedup by name — the same
    // name-based IRI SHACL uses), recording an owning class so it gets an
    // `rdfs:domain`.
    struct PropEmit {
        name: String,
        slot: SlotDefinition,
        domain_class: Option<String>,
    }
    let mut props: Vec<PropEmit> = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (name, slot_def) in &schema.slots {
        seen.insert(name.clone());
        props.push(PropEmit {
            name: name.clone(),
            slot: slot_def.clone(),
            domain_class: None,
        });
    }
    for (class_name, class_def) in &schema.classes {
        for (slot_name, slot) in crate::linkml_resolve::resolve_effective_slots(class_def, schema) {
            if !seen.insert(slot_name.clone()) {
                continue;
            }
            props.push(PropEmit {
                name: slot_name,
                slot,
                domain_class: Some(class_name.clone()),
            });
        }
    }

    for PropEmit {
        name,
        slot: slot_def,
        domain_class,
    } in &props
    {
        let prop_iri_str = slot_iri_string(name, slot_def, schema);
        let prop_iri = make_iri(&prop_iri_str)?;

        // Determine property type
        let is_object_property = slot_def
            .annotations
            .get("panschema:owl_property_type")
            .map(|s| s == "ObjectProperty")
            .unwrap_or_else(|| {
                slot_def
                    .range
                    .as_ref()
                    .map(|r| schema.classes.contains_key(r))
                    .unwrap_or(false)
            });

        // rdf:type
        if is_object_property {
            graph
                .insert(&prop_iri, rdf::type_, owl_object_property)
                .map_err(|e| IoError::Write(e.to_string()))?;
        } else {
            graph
                .insert(&prop_iri, rdf::type_, owl_datatype_property)
                .map_err(|e| IoError::Write(e.to_string()))?;
        }

        // owl:deprecated true — see the class emission above.
        if slot_def.deprecated.is_some() {
            graph
                .insert(&prop_iri, owl_deprecated, true)
                .map_err(|e| IoError::Write(e.to_string()))?;
        }

        // OWL relationship characteristics → `rdf:type owl:<Name>Property`.
        for (set, characteristic_type) in [
            slot_def.symmetric,
            slot_def.asymmetric,
            slot_def.reflexive,
            slot_def.irreflexive,
            slot_def.transitive,
        ]
        .into_iter()
        .zip(&owl_characteristic_types)
        {
            if set {
                graph
                    .insert(&prop_iri, rdf::type_, characteristic_type)
                    .map_err(|e| IoError::Write(e.to_string()))?;
            }
        }

        // rdfs:label
        let label = slot_def
            .annotations
            .get("panschema:label")
            .cloned()
            .unwrap_or_else(|| name.to_string());
        graph
            .insert(&prop_iri, rdfs::label, label.as_str())
            .map_err(|e| IoError::Write(e.to_string()))?;

        // rdfs:comment from description
        if let Some(ref description) = slot_def.description {
            graph
                .insert(&prop_iri, rdfs::comment, description.as_str())
                .map_err(|e| IoError::Write(e.to_string()))?;
        }

        // rdfs:domain — an explicit `domain:` wins; otherwise an
        // attribute/effective slot is domained by the class that introduced
        // it (a top-level slot without an explicit domain keeps its current
        // domain-less behavior).
        let domain_name = slot_def.domain.as_deref().or(domain_class.as_deref());
        if let Some(domain) = domain_name {
            let domain_iri_str = schema
                .classes
                .get(domain)
                .and_then(|c| c.class_uri.as_deref())
                .map(|c| expand_curie(c, schema))
                .unwrap_or_else(|| format!("{}#{}", ontology_iri_str, domain));
            let domain_iri = make_iri(&domain_iri_str)?;
            graph
                .insert(&prop_iri, rdfs::domain, &domain_iri)
                .map_err(|e| IoError::Write(e.to_string()))?;
        }

        // rdfs:range. For a datatype property the range must be a built-in
        // primitive to get an `rdfs:range` — an enum, a class the writer
        // didn't recognize as an object property, or a typo has no XSD
        // datatype, so emit none rather than fabricating a nonexistent
        // `xsd:{name}` (`xsd_datatype_iri` returns `None` for all of those).
        if let Some(ref range) = slot_def.range {
            let range_iri_str = if is_object_property {
                Some(
                    schema
                        .classes
                        .get(range)
                        .and_then(|c| c.class_uri.as_deref())
                        .map(|c| expand_curie(c, schema))
                        .unwrap_or_else(|| format!("{}#{}", ontology_iri_str, range)),
                )
            } else {
                crate::primitives::xsd_datatype_iri(range)
            };
            if let Some(range_iri_str) = range_iri_str {
                let range_iri = make_iri(&range_iri_str)?;
                graph
                    .insert(&prop_iri, rdfs::range, &range_iri)
                    .map_err(|e| IoError::Write(e.to_string()))?;
            }
        }

        // owl:inverseOf
        if let Some(ref inverse) = slot_def.inverse {
            let inverse_iri_str = schema
                .slots
                .get(inverse)
                .and_then(|s| s.slot_uri.as_deref())
                .map(|s| expand_curie(s, schema))
                .unwrap_or_else(|| format!("{}#{}", ontology_iri_str, inverse));
            let inverse_iri = make_iri(&inverse_iri_str)?;
            graph
                .insert(&prop_iri, owl_inverse_of, &inverse_iri)
                .map_err(|e| IoError::Write(e.to_string()))?;
        }

        emit_mappings(
            &mut graph,
            &prop_iri,
            schema,
            &slot_def.exact_mappings,
            &slot_def.close_mappings,
            &slot_def.related_mappings,
            &slot_def.narrow_mappings,
            &slot_def.broad_mappings,
        )?;

        emit_aliases_and_see_also(
            &mut graph,
            &prop_iri,
            schema,
            &slot_def.aliases,
            &slot_def.see_also,
        )?;
    }

    // Individuals
    if let Some(individuals_str) = schema.annotations.get("panschema:individuals") {
        let owl_named_individual = owl
            .get("NamedIndividual")
            .map_err(|e| IoError::Parse(e.to_string()))?;

        for ind_id in individuals_str.split(',') {
            let ind_id = ind_id.trim();
            if ind_id.is_empty() {
                continue;
            }

            // Get individual IRI
            let iri_key = format!("panschema:individual:{}:_iri", ind_id);
            let ind_iri_str = schema
                .annotations
                .get(&iri_key)
                .cloned()
                .unwrap_or_else(|| format!("{}#{}", ontology_iri_str, ind_id));
            let ind_iri = make_iri(&ind_iri_str)?;

            // rdf:type owl:NamedIndividual
            graph
                .insert(&ind_iri, rdf::type_, owl_named_individual)
                .map_err(|e| IoError::Write(e.to_string()))?;

            // Additional types
            let types_key = format!("panschema:individual:{}", ind_id);
            if let Some(types_str) = schema.annotations.get(&types_key) {
                for type_iri_str in types_str.split(',') {
                    let type_iri_str = type_iri_str.trim();
                    if !type_iri_str.is_empty() {
                        let type_iri = make_iri(type_iri_str)?;
                        graph
                            .insert(&ind_iri, rdf::type_, &type_iri)
                            .map_err(|e| IoError::Write(e.to_string()))?;
                    }
                }
            }

            // rdfs:label
            let label_key = format!("panschema:individual:{}:_label", ind_id);
            if let Some(label) = schema.annotations.get(&label_key) {
                graph
                    .insert(&ind_iri, rdfs::label, label.as_str())
                    .map_err(|e| IoError::Write(e.to_string()))?;
            }
        }
    }

    Ok(graph)
}

/// Helper to create an IRI
fn make_iri(s: &str) -> IoResult<Iri<String>> {
    Iri::new(s.to_string()).map_err(|e| IoError::Parse(format!("Invalid IRI '{}': {}", s, e)))
}

/// The OWL graph plus, when supplied, the A-box: every instance in `instances`
/// emitted as an `owl:NamedIndividual`. With `None` (or an empty set) the
/// graph is exactly [`build_rdf_graph`]'s.
pub fn build_rdf_graph_with_instances(
    schema: &SchemaDefinition,
    instances: Option<&crate::instances::InstanceSet>,
) -> IoResult<FastGraph> {
    let mut graph = build_rdf_graph(schema)?;
    if let Some(set) = instances {
        emit_instances(&mut graph, schema, set)?;
    }
    Ok(graph)
}

/// Absolute IRI for an instance — THE shared minting, so the RDF A-box, the
/// graph exports, and the docs agree on which individual is which. An
/// instance that already carries a resolved IRI (the OWL-sourced path) keeps
/// it; otherwise the id mints against the schema's prefixes (default prefix
/// for a bare id, any declared prefix for a CURIE id), falling back to
/// `{ontology}#{id}` when no prefix resolves.
pub fn instance_iri_string(schema: &SchemaDefinition, inst: &crate::instances::Instance) -> String {
    if let Some(iri) = &inst.iri
        && !inst.uri_unresolved
    {
        return iri.clone();
    }
    crate::linkml_resolve::expand_curie(schema, &inst.id)
        .unwrap_or_else(|| format!("{}#{}", ontology_iri_string(schema), inst.id))
}

/// Emit each instance as an `owl:NamedIndividual`: `rdf:type` per declared
/// class, `rdfs:label` from the display name, one data-property triple per
/// scalar slot value (datatype following the slot's declared range, so an
/// integer under a float-ranged slot lands as `xsd:double`), and one
/// object-property triple per id reference, resolved to the referenced
/// instance's IRI. A reference whose target id names no instance still emits
/// against the minted target IRI — RDF is open-world, and the dangling
/// diagnostic (not the writer) owns reporting the gap.
fn emit_instances(
    graph: &mut FastGraph,
    schema: &SchemaDefinition,
    set: &crate::instances::InstanceSet,
) -> IoResult<()> {
    use crate::instances::{InstanceValue, ScalarValue};

    let owl = Namespace::new_unchecked(OWL_NS);
    let owl_named_individual = owl
        .get("NamedIndividual")
        .map_err(|e| IoError::Parse(e.to_string()))?;

    // Slot IRIs and ranges resolve through each class's effective slots
    // (inherited + inline attributes), cached per class name.
    let mut slots_by_class: std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, crate::linkml::SlotDefinition>,
    > = std::collections::BTreeMap::new();
    let mut effective_slot = |class_names: &[String], slot_name: &str| {
        for class_name in class_names {
            if let Some(class_def) = schema.classes.get(class_name) {
                let slots = slots_by_class.entry(class_name.clone()).or_insert_with(|| {
                    crate::linkml_resolve::resolve_effective_slots(class_def, schema)
                });
                if let Some(def) = slots.get(slot_name) {
                    return Some(def.clone());
                }
            }
        }
        None
    };

    let iri_by_id: std::collections::BTreeMap<&str, String> = set
        .instances
        .iter()
        .map(|i| (i.id.as_str(), instance_iri_string(schema, i)))
        .collect();

    for inst in &set.instances {
        let subject = make_iri(&instance_iri_string(schema, inst))?;

        graph
            .insert(&subject, rdf::type_, owl_named_individual)
            .map_err(|e| IoError::Write(e.to_string()))?;
        for class_name in &inst.types {
            if let Some(class_def) = schema.classes.get(class_name) {
                let class_iri = make_iri(&class_iri_string(class_name, class_def, schema))?;
                graph
                    .insert(&subject, rdf::type_, &class_iri)
                    .map_err(|e| IoError::Write(e.to_string()))?;
            }
        }
        if !inst.label.is_empty() {
            graph
                .insert(&subject, rdfs::label, inst.label.as_str())
                .map_err(|e| IoError::Write(e.to_string()))?;
        }

        for sv in &inst.slot_values {
            let slot_def = effective_slot(&inst.types, &sv.slot);
            let predicate = match &slot_def {
                Some(def) => make_iri(&slot_iri_string(&sv.slot, def, schema))?,
                None => make_iri(&format!("{}#{}", ontology_iri_string(schema), sv.slot))?,
            };
            let range = slot_def.as_ref().and_then(|d| d.range.as_deref());
            let float_range = matches!(range, Some("float") | Some("double") | Some("decimal"));
            for value in &sv.values {
                let InstanceValue::Scalar(scalar) = value else {
                    // References emit below from `inst.references`; a
                    // range-kind mismatch has no faithful literal form.
                    continue;
                };
                match scalar {
                    ScalarValue::String(s) => graph.insert(&subject, &predicate, s.as_str()),
                    ScalarValue::Boolean(b) => graph.insert(&subject, &predicate, *b),
                    ScalarValue::Float(f) => graph.insert(&subject, &predicate, *f),
                    ScalarValue::Integer(i) if float_range => {
                        graph.insert(&subject, &predicate, *i as f64)
                    }
                    ScalarValue::Integer(i) => graph.insert(&subject, &predicate, *i as isize),
                }
                .map_err(|e| IoError::Write(e.to_string()))?;
            }
        }

        for reference in &inst.references {
            let predicate = match effective_slot(&inst.types, &reference.property) {
                Some(def) => make_iri(&slot_iri_string(&reference.property, &def, schema))?,
                None => make_iri(&format!(
                    "{}#{}",
                    ontology_iri_string(schema),
                    reference.property
                ))?,
            };
            let target_iri_str = iri_by_id
                .get(reference.target.as_str())
                .cloned()
                .unwrap_or_else(|| {
                    crate::linkml_resolve::expand_curie(schema, &reference.target).unwrap_or_else(
                        || format!("{}#{}", ontology_iri_string(schema), reference.target),
                    )
                });
            let target = make_iri(&target_iri_str)?;
            graph
                .insert(&subject, &predicate, &target)
                .map_err(|e| IoError::Write(e.to_string()))?;
        }
    }
    Ok(())
}

/// Build a SHACL shapes graph from the LinkML IR: one `sh:NodeShape` per
/// class (`sh:targetClass` its IRI) with a `sh:property` shape per effective
/// slot carrying that slot's value constraints. A separate artifact from the
/// OWL graph ([`build_rdf_graph`]) — a validation shapes file a SHACL engine
/// consumes — but built from the same IRI derivation, so every shape targets
/// the class/property IRIs the OWL output declares.
///
/// SHACL Core only. Slot `range` → `sh:datatype` (scalar) or `sh:class`
/// (class-valued); an enum range carries no datatype/class constraint yet
/// (`sh:in` projection is a later refinement). `required` and
/// `minimum_cardinality` reconcile to a single `sh:minCount` (explicit
/// cardinality wins); `maximum_cardinality` → `sh:maxCount`; `pattern` →
/// `sh:pattern`; `minimum_value`/`maximum_value` →
/// `sh:minInclusive`/`sh:maxInclusive`.
pub fn build_shacl_graph(schema: &SchemaDefinition) -> IoResult<FastGraph> {
    let mut graph = FastGraph::new();
    let t = ShaclTerms::new()?;

    for (name, class_def) in &schema.classes {
        let class_iri_str = class_iri_string(name, class_def, schema);
        let class_iri = make_iri(&class_iri_str)?;
        let shape_iri_str = format!("{class_iri_str}Shape");
        let shape_iri = make_iri(&shape_iri_str)?;

        graph
            .insert(&shape_iri, rdf::type_, &t.node_shape)
            .map_err(|e| IoError::Write(e.to_string()))?;
        graph
            .insert(&shape_iri, &t.target_class, &class_iri)
            .map_err(|e| IoError::Write(e.to_string()))?;

        let effective = crate::linkml_resolve::resolve_effective_slots(class_def, schema);
        for (slot_name, slot) in &effective {
            let prop_shape = make_iri(&format!("{shape_iri_str}/{slot_name}"))?;
            let path = make_iri(&slot_iri_string(slot_name, slot, schema))?;
            emit_property_shape(
                &mut graph,
                &t,
                &shape_iri,
                &prop_shape,
                &path,
                schema,
                PropertyConstraints::from_slot(slot),
            )?;
        }

        // `rules` → conditional shapes. A rule "if precondition then
        // postcondition" is SHACL Core's `sh:or ( [sh:not <pre>] <post> )`
        // — the shape analogue of the SQL `NOT (pre) OR (post)` the Postgres
        // writer emits. Pre/post are node shapes built from the same
        // `slot_conditions` field set (with `equals_string`/`equals_number`
        // → `sh:hasValue`). All sub-shapes get deterministic named IRIs
        // rather than blank nodes, so the output is stable and queryable.
        let slot_names: std::collections::BTreeSet<&str> =
            effective.keys().map(String::as_str).collect();
        for (i, rule) in class_def.rules.iter().enumerate() {
            // Skip a rule that can't be a conditional shape — one-sided, an
            // empty condition side, or a condition naming a slot the class
            // doesn't have. Never fabricate a property IRI for a missing
            // slot (that would emit a shape rejecting all valid data); the
            // omission is surfaced by `shacl_skipped_rules` on the CLI path.
            if shacl_rule_skip_reason(rule, &slot_names).is_some() {
                continue;
            }
            let pre = rule.preconditions.as_ref().unwrap();
            let post = rule.postconditions.as_ref().unwrap();

            let pre_iri = make_iri(&format!("{shape_iri_str}/rule{i}/pre"))?;
            for (slot, cond) in &pre.slot_conditions {
                let def = effective
                    .iter()
                    .find(|(n, _)| n.as_str() == slot)
                    .map(|(_, d)| d)
                    .expect("skip check guarantees the slot resolves");
                let ps = make_iri(&format!("{shape_iri_str}/rule{i}/pre/{slot}"))?;
                let path = make_iri(&slot_iri_string(slot, def, schema))?;
                emit_property_shape(
                    &mut graph,
                    &t,
                    &pre_iri,
                    &ps,
                    &path,
                    schema,
                    // A condition carries no `range` of its own; the slot's
                    // declared range is what types an `equals_number`
                    // `sh:hasValue` correctly.
                    PropertyConstraints::from_condition(cond).with_range(def.range.as_deref()),
                )?;
            }
            let post_iri = make_iri(&format!("{shape_iri_str}/rule{i}/post"))?;
            for (slot, cond) in &post.slot_conditions {
                let def = effective
                    .iter()
                    .find(|(n, _)| n.as_str() == slot)
                    .map(|(_, d)| d)
                    .expect("skip check guarantees the slot resolves");
                let ps = make_iri(&format!("{shape_iri_str}/rule{i}/post/{slot}"))?;
                let path = make_iri(&slot_iri_string(slot, def, schema))?;
                emit_property_shape(
                    &mut graph,
                    &t,
                    &post_iri,
                    &ps,
                    &path,
                    schema,
                    PropertyConstraints::from_condition(cond).with_range(def.range.as_deref()),
                )?;
            }

            // `[ sh:not <pre> ]` and the two-element list `( notpre post )`,
            // then `<classShape> sh:or ( notpre post )`.
            let notpre = make_iri(&format!("{shape_iri_str}/rule{i}/notpre"))?;
            let or0 = make_iri(&format!("{shape_iri_str}/rule{i}/or0"))?;
            let or1 = make_iri(&format!("{shape_iri_str}/rule{i}/or1"))?;
            let w = |g: &mut FastGraph, s: &Iri<String>, p, o: &Iri<String>| -> IoResult<()> {
                g.insert(s, p, o)
                    .map_err(|e| IoError::Write(e.to_string()))?;
                Ok(())
            };
            graph
                .insert(&notpre, &t.not_, &pre_iri)
                .map_err(|e| IoError::Write(e.to_string()))?;
            graph
                .insert(&shape_iri, &t.or_, &or0)
                .map_err(|e| IoError::Write(e.to_string()))?;
            w(&mut graph, &or0, rdf::first, &notpre)?;
            w(&mut graph, &or0, rdf::rest, &or1)?;
            w(&mut graph, &or1, rdf::first, &post_iri)?;
            graph
                .insert(&or1, rdf::rest, rdf::nil)
                .map_err(|e| IoError::Write(e.to_string()))?;
        }
    }

    Ok(graph)
}

/// The SHACL predicate IRIs the shapes graph uses, as owned `Iri<String>`
/// (so no `Namespace`/`NsTerm` lifetime threads through the builders).
struct ShaclTerms {
    node_shape: Iri<String>,
    target_class: Iri<String>,
    property: Iri<String>,
    path: Iri<String>,
    datatype: Iri<String>,
    class: Iri<String>,
    min_count: Iri<String>,
    max_count: Iri<String>,
    pattern: Iri<String>,
    min_inclusive: Iri<String>,
    max_inclusive: Iri<String>,
    has_value: Iri<String>,
    or_: Iri<String>,
    not_: Iri<String>,
}

impl ShaclTerms {
    fn new() -> IoResult<Self> {
        let sh = |n: &str| make_iri(&format!("{SH_NS}{n}"));
        Ok(Self {
            node_shape: sh("NodeShape")?,
            target_class: sh("targetClass")?,
            property: sh("property")?,
            path: sh("path")?,
            datatype: sh("datatype")?,
            class: sh("class")?,
            min_count: sh("minCount")?,
            max_count: sh("maxCount")?,
            pattern: sh("pattern")?,
            min_inclusive: sh("minInclusive")?,
            max_inclusive: sh("maxInclusive")?,
            has_value: sh("hasValue")?,
            or_: sh("or")?,
            not_: sh("not")?,
        })
    }
}

/// The value-constraint fields a property shape projects, drawn from either
/// a full slot ([`from_slot`]) or a rule's `slot_condition` matcher
/// ([`from_condition`], which adds the `equals_*` → `sh:hasValue` checks a
/// precondition needs). One mapping, so base slots and rule conditions
/// can't drift.
///
/// [`from_slot`]: PropertyConstraints::from_slot
/// [`from_condition`]: PropertyConstraints::from_condition
#[derive(Default)]
struct PropertyConstraints<'a> {
    range: Option<&'a str>,
    required: bool,
    pattern: Option<&'a str>,
    min_value: Option<f64>,
    max_value: Option<f64>,
    min_cardinality: Option<u32>,
    max_cardinality: Option<u32>,
    equals_string: Option<&'a str>,
    equals_number: Option<f64>,
}

impl<'a> PropertyConstraints<'a> {
    fn from_slot(slot: &'a SlotDefinition) -> Self {
        Self {
            range: slot.range.as_deref(),
            required: slot.required,
            pattern: slot.pattern.as_deref(),
            min_value: slot.minimum_value,
            max_value: slot.maximum_value,
            min_cardinality: slot.minimum_cardinality,
            max_cardinality: slot.maximum_cardinality,
            ..Default::default()
        }
    }

    fn from_condition(cond: &'a crate::linkml::SlotCondition) -> Self {
        Self {
            range: cond.range.as_deref(),
            required: cond.required,
            pattern: cond.pattern.as_deref(),
            min_value: cond.minimum_value,
            max_value: cond.maximum_value,
            min_cardinality: cond.minimum_cardinality,
            max_cardinality: cond.maximum_cardinality,
            equals_string: cond.equals_string.as_deref(),
            equals_number: cond.equals_number,
        }
    }

    /// Fill in `range` from the slot's declaration when the condition
    /// carries none of its own — a rule condition's range lives on the
    /// slot, not the condition, and it's what types `equals_number`.
    fn with_range(mut self, range: Option<&'a str>) -> Self {
        if self.range.is_none() {
            self.range = range;
        }
        self
    }
}

/// A `rules` entry [`build_shacl_graph`] can't project to a conditional
/// shape, and why — the SHACL analogue of `postgres_writer::SkippedRule`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaclSkippedRule {
    pub class: String,
    /// The rule's title, or `rule #<n>` (its position) when it has none.
    pub rule: String,
    pub reason: String,
}

/// Rules the SHACL writer can't emit as a conditional shape, with a
/// diagnostic naming each. A rule is skipped when it has only one of
/// pre/postconditions, an empty condition side, or a condition naming a
/// slot the class doesn't have (which would otherwise fabricate a property
/// IRI for a nonexistent slot). Shares [`shacl_rule_skip_reason`] with
/// `build_shacl_graph`, so it reports exactly the rules the writer drops.
pub fn shacl_skipped_rules(schema: &SchemaDefinition) -> Vec<ShaclSkippedRule> {
    let mut out = Vec::new();
    for (class_name, class) in &schema.classes {
        if class.rules.is_empty() {
            continue;
        }
        let effective = crate::linkml_resolve::resolve_effective_slots(class, schema);
        let slot_names: std::collections::BTreeSet<&str> =
            effective.keys().map(String::as_str).collect();
        for (i, rule) in class.rules.iter().enumerate() {
            if let Some(reason) = shacl_rule_skip_reason(rule, &slot_names) {
                out.push(ShaclSkippedRule {
                    class: class_name.clone(),
                    rule: rule.title.clone().unwrap_or_else(|| format!("rule #{i}")),
                    reason,
                });
            }
        }
    }
    out
}

/// Why the SHACL writer skips `rule`, or `None` if it emits a conditional
/// shape. `slot_names` is the class's effective slot set. Shared by
/// `build_shacl_graph` (skip decision) and [`shacl_skipped_rules`]
/// (diagnostic), so the two can't disagree.
fn shacl_rule_skip_reason(
    rule: &crate::linkml::ClassRule,
    slot_names: &std::collections::BTreeSet<&str>,
) -> Option<String> {
    match (&rule.preconditions, &rule.postconditions) {
        (Some(pre), Some(post)) => {
            if pre.slot_conditions.is_empty() || post.slot_conditions.is_empty() {
                return Some("a precondition or postcondition has no slot_conditions".to_string());
            }
            for slot in pre
                .slot_conditions
                .keys()
                .chain(post.slot_conditions.keys())
            {
                if !slot_names.contains(slot.as_str()) {
                    return Some(format!(
                        "references slot `{slot}`, which the class does not have"
                    ));
                }
            }
            None
        }
        _ => Some(
            "a SHACL conditional shape needs both preconditions and postconditions".to_string(),
        ),
    }
}

/// Emit one `sh:property` shape: link it to `shape`, set its `sh:path`, and
/// project each constraint the field set carries. `range` → `sh:class`
/// (class-valued) or `sh:datatype` (scalar; enum ranges stay unconstrained);
/// `required`/cardinality → `sh:minCount`/`sh:maxCount`; `pattern` →
/// `sh:pattern`; value bounds → `sh:minInclusive`/`sh:maxInclusive`;
/// `equals_*` → `sh:hasValue`.
fn emit_property_shape(
    graph: &mut FastGraph,
    t: &ShaclTerms,
    shape: &Iri<String>,
    prop_shape: &Iri<String>,
    path: &Iri<String>,
    schema: &SchemaDefinition,
    c: PropertyConstraints<'_>,
) -> IoResult<()> {
    graph
        .insert(shape, &t.property, prop_shape)
        .map_err(|e| IoError::Write(e.to_string()))?;
    graph
        .insert(prop_shape, &t.path, path)
        .map_err(|e| IoError::Write(e.to_string()))?;

    if let Some(range) = c.range {
        if let Some(target) = schema.classes.get(range) {
            let target_iri = make_iri(&class_iri_string(range, target, schema))?;
            graph
                .insert(prop_shape, &t.class, &target_iri)
                .map_err(|e| IoError::Write(e.to_string()))?;
        } else if let Some(xsd) = crate::primitives::xsd_datatype_iri(range) {
            // Only a built-in primitive gets an `sh:datatype`; an enum or a
            // typo has no XSD datatype, so emit none rather than a fabricated
            // `xsd:{name}`.
            let xsd_iri = make_iri(&xsd)?;
            graph
                .insert(prop_shape, &t.datatype, &xsd_iri)
                .map_err(|e| IoError::Write(e.to_string()))?;
        }
    }
    // `required` and `minimum_cardinality` are two spellings of the same
    // lower bound; emitting a `sh:minCount` for each would contradict itself.
    // Reconcile to one, with an explicit cardinality winning over the flag —
    // the same precedence `effective_cardinality` gives the HTML view.
    let effective_min = c.min_cardinality.unwrap_or(u32::from(c.required));
    if effective_min > 0 {
        graph
            .insert(prop_shape, &t.min_count, effective_min as i32)
            .map_err(|e| IoError::Write(e.to_string()))?;
    }
    if let Some(max) = c.max_cardinality {
        graph
            .insert(prop_shape, &t.max_count, max as i32)
            .map_err(|e| IoError::Write(e.to_string()))?;
    }
    if let Some(pattern) = c.pattern {
        graph
            .insert(prop_shape, &t.pattern, pattern)
            .map_err(|e| IoError::Write(e.to_string()))?;
    }
    if let Some(min) = c.min_value {
        graph
            .insert(prop_shape, &t.min_inclusive, min)
            .map_err(|e| IoError::Write(e.to_string()))?;
    }
    if let Some(max) = c.max_value {
        graph
            .insert(prop_shape, &t.max_inclusive, max)
            .map_err(|e| IoError::Write(e.to_string()))?;
    }
    if let Some(v) = c.equals_string {
        graph
            .insert(prop_shape, &t.has_value, v)
            .map_err(|e| IoError::Write(e.to_string()))?;
    }
    if let Some(n) = c.equals_number {
        // `sh:hasValue` is term equality (datatype-sensitive), so the
        // literal must carry the datatype the slot's range declares — an
        // integer-range slot's data is `xsd:integer`, which a default
        // `xsd:double` literal can never equal. Insert an integer-typed
        // literal (sophia types `isize` as `xsd:integer`, matching
        // `map_linkml_to_xsd`) for integer ranges; keep `xsd:double` for
        // float/double/decimal or rangeless conditions.
        let is_integer = matches!(c.range, Some("integer") | Some("int"));
        if is_integer {
            graph
                .insert(prop_shape, &t.has_value, n as isize)
                .map_err(|e| IoError::Write(e.to_string()))?;
        } else {
            graph
                .insert(prop_shape, &t.has_value, n)
                .map_err(|e| IoError::Write(e.to_string()))?;
        }
    }
    Ok(())
}

// ============================================================================
// JSON-LD Writer
// ============================================================================

/// Writer for JSON-LD format
#[derive(Default)]
pub struct JsonLdWriter {
    /// Optional A-box: when set, each instance emits as an
    /// `owl:NamedIndividual` alongside the T-box.
    instances: Option<crate::instances::InstanceSet>,
}

impl JsonLdWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach an A-box; the output becomes a self-contained knowledge
    /// graph (schema + individuals).
    pub fn with_instances(mut self, set: crate::instances::InstanceSet) -> Self {
        self.instances = Some(set);
        self
    }
}

impl Writer for JsonLdWriter {
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()> {
        let graph = build_rdf_graph_with_instances(schema, self.instances.as_ref())?;

        use sophia::jsonld::serializer::JsonLdSerializer;

        crate::io::ensure_output_parent(output)?;
        let file = File::create(output).map_err(IoError::Io)?;
        let writer = BufWriter::new(file);

        let mut serializer = JsonLdSerializer::new(writer);

        // JSON-LD serializer works with quads (datasets), so convert graph to dataset
        let dataset = graph.as_dataset();
        serializer
            .serialize_dataset(&dataset)
            .map_err(|e| IoError::Write(format!("JSON-LD serialization failed: {}", e)))?;

        Ok(())
    }

    fn format_id(&self) -> &str {
        "jsonld"
    }
}

// ============================================================================
// RDF/XML Writer
// ============================================================================

/// Writer for RDF/XML format
#[derive(Default)]
pub struct RdfXmlWriter {
    /// Optional A-box: when set, each instance emits as an
    /// `owl:NamedIndividual` alongside the T-box.
    instances: Option<crate::instances::InstanceSet>,
}

impl RdfXmlWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach an A-box; the output becomes a self-contained knowledge
    /// graph (schema + individuals).
    pub fn with_instances(mut self, set: crate::instances::InstanceSet) -> Self {
        self.instances = Some(set);
        self
    }
}

impl Writer for RdfXmlWriter {
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()> {
        let graph = build_rdf_graph_with_instances(schema, self.instances.as_ref())?;

        use sophia::xml::serializer::RdfXmlSerializer;

        crate::io::ensure_output_parent(output)?;
        let file = File::create(output).map_err(IoError::Io)?;
        let writer = BufWriter::new(file);

        let mut serializer = RdfXmlSerializer::new(writer);

        serializer
            .serialize_graph(&graph)
            .map_err(|e| IoError::Write(format!("RDF/XML serialization failed: {}", e)))?;

        Ok(())
    }

    fn format_id(&self) -> &str {
        "rdfxml"
    }
}

// ============================================================================
// N-Triples Writer
// ============================================================================

/// Writer for N-Triples format
#[derive(Default)]
pub struct NTriplesWriter {
    /// Optional A-box: when set, each instance emits as an
    /// `owl:NamedIndividual` alongside the T-box.
    instances: Option<crate::instances::InstanceSet>,
}

impl NTriplesWriter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach an A-box; the output becomes a self-contained knowledge
    /// graph (schema + individuals).
    pub fn with_instances(mut self, set: crate::instances::InstanceSet) -> Self {
        self.instances = Some(set);
        self
    }
}

impl Writer for NTriplesWriter {
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()> {
        let graph = build_rdf_graph_with_instances(schema, self.instances.as_ref())?;

        use sophia::turtle::serializer::nt::NTriplesSerializer;

        crate::io::ensure_output_parent(output)?;
        let file = File::create(output).map_err(IoError::Io)?;
        let writer = BufWriter::new(file);

        let mut serializer = NTriplesSerializer::new(writer);

        serializer
            .serialize_graph(&graph)
            .map_err(|e| IoError::Write(format!("N-Triples serialization failed: {}", e)))?;

        Ok(())
    }

    fn format_id(&self) -> &str {
        "ntriples"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linkml::{ClassDefinition, SlotDefinition};
    use std::fs;
    use tempfile::TempDir;

    // ========== A-box emission ==========

    /// Build the wine-shaped schema + one grounded instance pair inline:
    /// a container, a Bottle with an id, a float-ranged score, and a
    /// reference to a Rack.
    fn abox_fixture() -> (SchemaDefinition, crate::instances::InstanceSet) {
        let mut schema = SchemaDefinition::new("cellar");
        schema.id = Some("https://example.org/cellar".to_string());
        schema.default_prefix = Some("cellar".to_string());
        schema.prefixes.insert(
            "cellar".to_string(),
            "https://example.org/cellar/".to_string(),
        );
        let mut container = ClassDefinition::new("Cellar");
        container.tree_root = true;
        let mut bottles = SlotDefinition::new("bottles");
        bottles.range = Some("Bottle".to_string());
        bottles.multivalued = true;
        container.attributes.insert("bottles".to_string(), bottles);
        let mut racks = SlotDefinition::new("racks");
        racks.range = Some("Rack".to_string());
        racks.multivalued = true;
        container.attributes.insert("racks".to_string(), racks);
        schema.classes.insert("Cellar".to_string(), container);

        let mut bottle = ClassDefinition::new("Bottle");
        let mut id = SlotDefinition::new("id");
        id.identifier = true;
        bottle.attributes.insert("id".to_string(), id.clone());
        let mut score = SlotDefinition::new("score");
        score.range = Some("float".to_string());
        bottle.attributes.insert("score".to_string(), score);
        let mut stored_in = SlotDefinition::new("stored_in");
        stored_in.range = Some("Rack".to_string());
        bottle.attributes.insert("stored_in".to_string(), stored_in);
        schema.classes.insert("Bottle".to_string(), bottle);

        let mut rack = ClassDefinition::new("Rack");
        rack.attributes.insert("id".to_string(), id);
        schema.classes.insert("Rack".to_string(), rack);

        let data: serde_yaml::Value = serde_yaml::from_str(
            "bottles:\n  - id: b1\n    score: 4\n    stored_in: r1\nracks:\n  - id: r1\n",
        )
        .unwrap();
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        (schema, set)
    }

    #[test]
    fn instance_iri_uses_a_resolved_iri_but_never_an_unresolved_one() {
        let (schema, _) = abox_fixture();
        let mut inst = crate::instances::Instance {
            id: "b1".to_string(),
            iri: Some("https://upstream.example/b1".to_string()),
            uri_unresolved: false,
            label: "b1".to_string(),
            description: None,
            types: vec![],
            literals: vec![],
            references: vec![],
            slot_values: vec![],
        };
        assert_eq!(
            instance_iri_string(&schema, &inst),
            "https://upstream.example/b1",
            "a resolved carried IRI wins over minting"
        );
        // An unresolved IRI (a curie whose prefix never expanded) must NOT
        // be used verbatim — the id mints instead.
        inst.uri_unresolved = true;
        assert_eq!(
            instance_iri_string(&schema, &inst),
            "https://example.org/cellar/b1",
            "an unresolved IRI falls back to minting from the id"
        );
    }

    #[test]
    fn integer_value_under_a_float_range_emits_xsd_double() {
        let (schema, set) = abox_fixture();
        let graph = build_rdf_graph_with_instances(&schema, Some(&set)).expect("graph");
        // The authored `score: 4` parses as an integer, but the slot's
        // declared range is float — the literal must carry xsd:double, not
        // xsd:integer, or SPARQL numeric joins against other doubles fail.
        use sophia::api::graph::Graph;
        use sophia::api::term::Term;
        use sophia::api::triple::Triple;
        let subject = make_iri("https://example.org/cellar/b1").unwrap();
        let predicate = make_iri("https://example.org/cellar#score").unwrap();
        let mut found_double = false;
        for t in graph.triples_matching([subject], [predicate], sophia::api::term::matcher::Any) {
            let t = t.unwrap();
            let dt = t.o().datatype().map(|d| d.to_string());
            assert_eq!(
                dt.as_deref(),
                Some("http://www.w3.org/2001/XMLSchema#double"),
                "a float-range slot's integer value must emit as xsd:double"
            );
            found_double = true;
        }
        assert!(found_double, "the score literal must be present");
    }

    #[test]
    fn each_rdf_writer_with_instances_carries_the_abox() {
        // The three non-Turtle writers must route the attached A-box into
        // their output (Turtle's is covered by the oxigraph oracle).
        let (schema, set) = abox_fixture();
        let temp_dir = TempDir::new().expect("temp dir");
        let writers: [(Box<dyn Writer>, &str); 3] = [
            (
                Box::new(JsonLdWriter::new().with_instances(set.clone())),
                "out.jsonld",
            ),
            (
                Box::new(RdfXmlWriter::new().with_instances(set.clone())),
                "out.rdf",
            ),
            (
                Box::new(NTriplesWriter::new().with_instances(set.clone())),
                "out.nt",
            ),
        ];
        for (writer, name) in writers {
            let path = temp_dir.path().join(name);
            writer.write(&schema, &path).expect("write");
            let content = fs::read_to_string(&path).expect("read");
            assert!(
                content.contains("https://example.org/cellar/b1"),
                "{name} must carry the attached A-box's individual IRI; got:\n{}",
                &content[..content.len().min(400)]
            );
        }
    }

    fn create_test_schema() -> SchemaDefinition {
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some("http://example.org/test".to_string());
        schema.title = Some("Test Ontology".to_string());
        schema.description = Some("A test ontology.".to_string());
        schema.version = Some("1.0.0".to_string());

        let mut animal = ClassDefinition::new("Animal");
        animal.class_uri = Some("http://example.org/test#Animal".to_string());
        animal.description = Some("A living creature.".to_string());
        schema.classes.insert("Animal".to_string(), animal);

        let mut dog = ClassDefinition::new("Dog");
        dog.class_uri = Some("http://example.org/test#Dog".to_string());
        dog.is_a = Some("Animal".to_string());
        schema.classes.insert("Dog".to_string(), dog);

        let mut has_name = SlotDefinition::new("hasName");
        has_name.slot_uri = Some("http://example.org/test#hasName".to_string());
        has_name.range = Some("string".to_string());
        schema.slots.insert("hasName".to_string(), has_name);

        schema
    }

    #[test]
    fn build_rdf_graph_creates_valid_graph() {
        let schema = create_test_schema();
        let graph = build_rdf_graph(&schema).expect("Failed to build graph");

        // Graph should have triples
        assert!(graph.triples().count() > 0);
    }

    #[test]
    fn jsonld_writer_produces_output() {
        let schema = create_test_schema();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.jsonld");

        let writer = JsonLdWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write JSON-LD");

        assert!(output_path.exists());
        let content = fs::read_to_string(&output_path).expect("Failed to read output");
        assert!(content.contains("http://example.org/test"));
    }

    #[test]
    fn rdfxml_writer_produces_output() {
        let schema = create_test_schema();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.rdf");

        let writer = RdfXmlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write RDF/XML");

        assert!(output_path.exists());
        let content = fs::read_to_string(&output_path).expect("Failed to read output");
        assert!(content.contains("rdf:RDF"));
        assert!(content.contains("http://example.org/test"));
    }

    #[test]
    fn ntriples_writer_produces_output() {
        let schema = create_test_schema();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.nt");

        let writer = NTriplesWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write N-Triples");

        assert!(output_path.exists());
        let content = fs::read_to_string(&output_path).expect("Failed to read output");
        assert!(content.contains("<http://example.org/test>"));
    }

    #[test]
    fn jsonld_writer_format_id() {
        let writer = JsonLdWriter::new();
        assert_eq!(writer.format_id(), "jsonld");
    }

    #[test]
    fn rdfxml_writer_format_id() {
        let writer = RdfXmlWriter::new();
        assert_eq!(writer.format_id(), "rdfxml");
    }

    #[test]
    fn ntriples_writer_format_id() {
        let writer = NTriplesWriter::new();
        assert_eq!(writer.format_id(), "ntriples");
    }

    // ----- CURIE expansion --------------------------------------------

    fn schema_with_prefixes() -> SchemaDefinition {
        let mut schema = SchemaDefinition::new("s");
        schema.id = Some("http://example.org/s".to_string());
        schema.prefixes.insert(
            "cco".to_string(),
            "https://www.commoncoreontologies.org/".to_string(),
        );
        schema.prefixes.insert(
            "obo".to_string(),
            "http://purl.obolibrary.org/obo/".to_string(),
        );
        schema
    }

    #[test]
    fn expand_curie_expands_known_prefix_to_absolute_iri() {
        let schema = schema_with_prefixes();
        assert_eq!(
            expand_curie("cco:ont00000005", &schema),
            "https://www.commoncoreontologies.org/ont00000005"
        );
        assert_eq!(
            expand_curie("obo:BFO_0000015", &schema),
            "http://purl.obolibrary.org/obo/BFO_0000015"
        );
    }

    #[test]
    fn expand_curie_passes_absolute_url_through_unchanged() {
        // A class_uri that's already a full URL must not be re-expanded
        // (would corrupt the IRI by treating part of the URL as a prefix).
        let schema = schema_with_prefixes();
        let already_absolute = "http://example.org/already/absolute";
        assert_eq!(expand_curie(already_absolute, &schema), already_absolute);
    }

    #[test]
    fn expand_curie_passes_bare_name_through_unchanged() {
        // Without a `default_prefix`, a bare name has no expansion, so it
        // passes through and the caller (build_rdf_graph) applies the
        // `{ontology}#{name}` fallback.
        let schema = schema_with_prefixes();
        assert_eq!(expand_curie("BareName", &schema), "BareName");
    }

    #[test]
    fn expand_curie_uses_default_prefix_for_bare_names() {
        // With a `default_prefix`, a bare name expands against it — the same
        // decision the HTML writer's shared `linkml_resolve::expand_curie`
        // makes, so the two can't disagree.
        let mut schema = SchemaDefinition::new("s");
        schema
            .prefixes
            .insert("ex".to_string(), "http://example.org/".to_string());
        schema.default_prefix = Some("ex".to_string());
        assert_eq!(expand_curie("Thing", &schema), "http://example.org/Thing");
    }

    #[test]
    fn expand_curie_unknown_prefix_passes_through_with_warning() {
        // A CURIE whose prefix isn't in `schema.prefixes` is suspicious
        // but not necessarily wrong (e.g. user typo, or external prefix
        // not yet declared). Pass through so build_rdf_graph can still
        // produce output; the tracing::warn alerts the user. The
        // observable behaviour here is the pass-through; the warn fires
        // via tracing and is checked via integration tests if needed.
        let schema = schema_with_prefixes();
        assert_eq!(
            expand_curie("undeclared:thing", &schema),
            "undeclared:thing"
        );
    }

    #[test]
    fn build_rdf_graph_expands_class_uri_curies() {
        // End-to-end: a class with a CURIE `class_uri` produces an
        // absolute IRI in the emitted graph, NOT a relative `cco:foo`
        // term that downstream parsers would interpret as an empty-base
        // relative reference.
        use sophia::api::term::Term;
        use sophia::api::triple::Triple;

        let mut schema = schema_with_prefixes();
        let mut act = ClassDefinition::new("Act");
        act.class_uri = Some("cco:ont00000005".to_string());
        schema.classes.insert("Act".to_string(), act);
        let graph = build_rdf_graph(&schema).unwrap();

        let expected_iri = "https://www.commoncoreontologies.org/ont00000005";
        let found = graph.triples().any(|t| {
            let triple = t.unwrap();
            triple.s().iri().is_some_and(|i| i.as_str() == expected_iri)
        });
        assert!(found, "expected expanded class IRI in graph; got none");
    }

    #[test]
    fn build_rdf_graph_emits_subclass_of_per_mixin() {
        // LinkML treats mixins as multiple inheritance; each mixin must
        // produce its own rdfs:subClassOf alongside the is_a parent.
        use sophia::api::term::Term;
        use sophia::api::triple::Triple;

        let mut schema = schema_with_prefixes();
        for name in ["Parent", "MixinA", "MixinB"] {
            let mut def = ClassDefinition::new(name);
            def.class_uri = Some(format!("http://example.org/s#{name}"));
            schema.classes.insert(name.to_string(), def);
        }
        let mut child = ClassDefinition::new("Child");
        child.class_uri = Some("http://example.org/s#Child".to_string());
        child.is_a = Some("Parent".to_string());
        child.mixins = vec!["MixinA".to_string(), "MixinB".to_string()];
        schema.classes.insert("Child".to_string(), child);

        let graph = build_rdf_graph(&schema).unwrap();
        let subclass_iri = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
        let child_iri = "http://example.org/s#Child";
        let parents: std::collections::BTreeSet<String> = graph
            .triples()
            .filter_map(|t| {
                let triple = t.ok()?;
                let s = triple.s().iri()?.as_str().to_string();
                let p = triple.p().iri()?.as_str().to_string();
                let o = triple.o().iri()?.as_str().to_string();
                (s == child_iri && p == subclass_iri).then_some(o)
            })
            .collect();
        assert_eq!(
            parents,
            [
                "http://example.org/s#MixinA",
                "http://example.org/s#MixinB",
                "http://example.org/s#Parent"
            ]
            .iter()
            .map(|s| s.to_string())
            .collect()
        );
    }

    #[test]
    fn build_rdf_graph_emits_skos_mapping_triples_for_classes() {
        // Authors ground their classes in upstream ontologies via
        // exact_mappings / close_mappings / related_mappings. Each
        // mapping must surface as a triple under the matching SKOS
        // predicate — without this, the reuse story is invisible in
        // the emitted RDF and the schema looks like an isolated graph.
        use sophia::api::term::Term;
        use sophia::api::triple::Triple;

        let mut schema = schema_with_prefixes();
        schema
            .prefixes
            .insert("cito".to_string(), "http://purl.org/spar/cito/".to_string());
        let mut act = ClassDefinition::new("Act");
        act.exact_mappings = vec!["obo:BFO_0000015".into()];
        act.close_mappings = vec!["cito:supports".into()];
        schema.classes.insert("Act".to_string(), act);

        let graph = build_rdf_graph(&schema).unwrap();

        let exact_match = format!("{SKOS_NS}exactMatch");
        let close_match = format!("{SKOS_NS}closeMatch");
        let bfo_iri = "http://purl.obolibrary.org/obo/BFO_0000015";
        let cito_iri = "http://purl.org/spar/cito/supports";

        let has_exact = graph.triples().any(|t| {
            let triple = t.unwrap();
            triple.p().iri().is_some_and(|i| i.as_str() == exact_match)
                && triple.o().iri().is_some_and(|i| i.as_str() == bfo_iri)
        });
        let has_close = graph.triples().any(|t| {
            let triple = t.unwrap();
            triple.p().iri().is_some_and(|i| i.as_str() == close_match)
                && triple.o().iri().is_some_and(|i| i.as_str() == cito_iri)
        });
        assert!(has_exact, "expected skos:exactMatch triple for BFO mapping");
        assert!(
            has_close,
            "expected skos:closeMatch triple for CiTO mapping"
        );
    }

    #[test]
    fn build_rdf_graph_emits_skos_mapping_triples_for_slots() {
        // Same shape as the class test, but for slots: a property
        // with cross-ontology mappings produces SKOS triples on the
        // slot's IRI.
        use sophia::api::term::Term;
        use sophia::api::triple::Triple;

        let mut schema = schema_with_prefixes();
        schema
            .prefixes
            .insert("cito".to_string(), "http://purl.org/spar/cito/".to_string());
        let mut supports = SlotDefinition::new("supports");
        supports.exact_mappings = vec!["cito:supports".into()];
        schema.slots.insert("supports".to_string(), supports);

        let graph = build_rdf_graph(&schema).unwrap();

        let exact_match = format!("{SKOS_NS}exactMatch");
        let cito_iri = "http://purl.org/spar/cito/supports";

        let has_exact = graph.triples().any(|t| {
            let triple = t.unwrap();
            triple.p().iri().is_some_and(|i| i.as_str() == exact_match)
                && triple.o().iri().is_some_and(|i| i.as_str() == cito_iri)
        });
        assert!(
            has_exact,
            "expected skos:exactMatch triple for slot mapping"
        );
    }

    #[test]
    fn build_rdf_graph_emits_owl_characteristic_axioms_for_slots() {
        // OWL relationship characteristics are the semantic payoff: a slot
        // declared `transitive`/`symmetric` must emit the corresponding
        // `owl:<Name>Property` type axiom so a reasoner can use it.
        use sophia::api::term::Term;
        use sophia::api::triple::Triple;

        let mut schema = schema_with_prefixes();
        schema
            .classes
            .insert("Claim".to_string(), ClassDefinition::new("Claim"));
        let mut refines = SlotDefinition::new("refines");
        refines.range = Some("Claim".into()); // object property
        refines.transitive = true;
        refines.symmetric = true;
        schema.slots.insert("refines".to_string(), refines);

        let graph = build_rdf_graph(&schema).unwrap();
        let rdf_type = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
        let has_type = |obj: String| {
            graph.triples().any(|t| {
                let tr = t.unwrap();
                tr.p().iri().is_some_and(|i| i.as_str() == rdf_type)
                    && tr.o().iri().is_some_and(|i| i.as_str() == obj)
            })
        };
        assert!(
            has_type(format!("{OWL_NS}TransitiveProperty")),
            "expected owl:TransitiveProperty axiom"
        );
        assert!(
            has_type(format!("{OWL_NS}SymmetricProperty")),
            "expected owl:SymmetricProperty axiom"
        );
        assert!(
            !has_type(format!("{OWL_NS}ReflexiveProperty")),
            "unset characteristics must not be emitted"
        );
    }

    #[test]
    fn build_rdf_graph_does_not_fabricate_an_xsd_datatype_for_an_enum_range() {
        // A slot ranged on an enum is neither an object property nor an XSD
        // scalar. Falling through to the scalar mapping fabricates a
        // nonexistent `xsd:{EnumName}` as its rdfs:range; the enum must be
        // guarded (no rdfs:range yet), the way the SHACL/Postgres writers do.
        use crate::linkml::EnumDefinition;
        use sophia::api::term::Term;
        use sophia::api::triple::Triple;

        let mut schema = schema_with_prefixes();
        schema.enums.insert(
            "PriorityLevel".to_string(),
            EnumDefinition::new("PriorityLevel"),
        );
        let mut priority = SlotDefinition::new("priority");
        priority.range = Some("PriorityLevel".into());
        schema.slots.insert("priority".to_string(), priority);

        let graph = build_rdf_graph(&schema).unwrap();
        let rdfs_range = "http://www.w3.org/2000/01/rdf-schema#range";
        let fabricated = "http://www.w3.org/2001/XMLSchema#PriorityLevel";
        let has_fabricated_range = graph.triples().any(|t| {
            let tr = t.unwrap();
            tr.p().iri().is_some_and(|i| i.as_str() == rdfs_range)
                && tr.o().iri().is_some_and(|i| i.as_str() == fabricated)
        });
        assert!(
            !has_fabricated_range,
            "an enum range must not emit a fabricated xsd:{{EnumName}} rdfs:range"
        );
    }

    #[test]
    fn build_rdf_graph_emits_owl_deprecated() {
        // A class or slot marked `deprecated:` emits `owl:deprecated true`
        // on its IRI (a Rust bool serializes as an `xsd:boolean` literal),
        // so downstream consumers see the element is sunset. Undeprecated
        // elements emit no such triple.
        use sophia::api::term::Term;
        use sophia::api::triple::Triple;

        let mut schema = schema_with_prefixes();
        let mut legacy = ClassDefinition::new("LegacyClaim");
        legacy.deprecated = Some("use Claim instead".to_string());
        schema.classes.insert("LegacyClaim".to_string(), legacy);
        schema
            .classes
            .insert("Claim".to_string(), ClassDefinition::new("Claim"));
        let mut old_slot = SlotDefinition::new("old_refines");
        old_slot.deprecated = Some("use refines instead".to_string());
        schema.slots.insert("old_refines".to_string(), old_slot);
        schema
            .slots
            .insert("refines".to_string(), SlotDefinition::new("refines"));

        let graph = build_rdf_graph(&schema).unwrap();
        let owl_deprecated = format!("{OWL_NS}deprecated");

        // Collect the subjects carrying an `owl:deprecated` predicate.
        let deprecated_subjects: Vec<String> = graph
            .triples()
            .filter_map(|t| {
                let tr = t.unwrap();
                tr.p()
                    .iri()
                    .filter(|i| i.as_str() == owl_deprecated)
                    .and_then(|_| tr.s().iri().map(|i| i.as_str().to_string()))
            })
            .collect();

        assert!(
            deprecated_subjects
                .iter()
                .any(|s| s.ends_with("#LegacyClaim")),
            "expected owl:deprecated on the deprecated class; got {deprecated_subjects:?}"
        );
        assert!(
            deprecated_subjects
                .iter()
                .any(|s| s.ends_with("#old_refines")),
            "expected owl:deprecated on the deprecated slot; got {deprecated_subjects:?}"
        );
        assert!(
            !deprecated_subjects.iter().any(|s| s.ends_with("#Claim")),
            "undeprecated class must not be marked owl:deprecated; got {deprecated_subjects:?}"
        );
        assert!(
            !deprecated_subjects.iter().any(|s| s.ends_with("#refines")),
            "undeprecated slot must not be marked owl:deprecated; got {deprecated_subjects:?}"
        );

        // The object is the boolean literal `true` typed xsd:boolean.
        let has_true_object = graph.triples().any(|t| {
            let tr = t.unwrap();
            tr.p().iri().is_some_and(|i| i.as_str() == owl_deprecated)
                && tr.o().lexical_form().is_some_and(|l| l == "true")
        });
        assert!(
            has_true_object,
            "owl:deprecated object must be the literal `true`"
        );
    }

    #[test]
    fn build_rdf_graph_emits_alt_label_and_see_also() {
        // A class or slot with `aliases:` emits one `skos:altLabel`
        // literal per alias on its IRI, and `see_also:` emits one
        // `rdfs:seeAlso` IRI per reference (CURIE-expanded against the
        // schema's prefixes). Elements with neither emit no such triples.
        use sophia::api::term::Term;
        use sophia::api::triple::Triple;

        let mut schema = schema_with_prefixes();
        let mut claim = ClassDefinition::new("Claim");
        claim.aliases = vec!["Assertion".to_string(), "Statement".to_string()];
        claim.see_also = vec!["cco:ont00000005".to_string()];
        schema.classes.insert("Claim".to_string(), claim);
        schema
            .classes
            .insert("Bare".to_string(), ClassDefinition::new("Bare"));
        let mut refines = SlotDefinition::new("refines");
        refines.aliases = vec!["sharpens".to_string()];
        refines.see_also = vec!["obo:BFO_0000015".to_string()];
        schema.slots.insert("refines".to_string(), refines);
        schema
            .slots
            .insert("plain".to_string(), SlotDefinition::new("plain"));

        let graph = build_rdf_graph(&schema).unwrap();
        let alt_label = format!("{SKOS_NS}altLabel");
        let see_also_iri = "http://www.w3.org/2000/01/rdf-schema#seeAlso";

        // `(subject, object-lexical)` pairs for skos:altLabel triples.
        let alt_labels: Vec<(String, String)> = graph
            .triples()
            .filter_map(|t| {
                let tr = t.unwrap();
                if tr.p().iri().is_some_and(|i| i.as_str() == alt_label) {
                    Some((
                        tr.s().iri()?.as_str().to_string(),
                        tr.o().lexical_form()?.to_string(),
                    ))
                } else {
                    None
                }
            })
            .collect();
        assert!(
            alt_labels
                .iter()
                .any(|(s, o)| s.ends_with("#Claim") && o == "Assertion"),
            "expected skos:altLabel `Assertion` on the class; got {alt_labels:?}"
        );
        assert!(
            alt_labels
                .iter()
                .any(|(s, o)| s.ends_with("#Claim") && o == "Statement"),
            "expected both class aliases as skos:altLabel; got {alt_labels:?}"
        );
        assert!(
            alt_labels
                .iter()
                .any(|(s, o)| s.ends_with("#refines") && o == "sharpens"),
            "expected skos:altLabel `sharpens` on the slot; got {alt_labels:?}"
        );

        // `(subject, object-IRI)` pairs for rdfs:seeAlso triples.
        let see_also_links: Vec<(String, String)> = graph
            .triples()
            .filter_map(|t| {
                let tr = t.unwrap();
                if tr.p().iri().is_some_and(|i| i.as_str() == see_also_iri) {
                    Some((
                        tr.s().iri()?.as_str().to_string(),
                        tr.o().iri()?.as_str().to_string(),
                    ))
                } else {
                    None
                }
            })
            .collect();
        assert!(
            see_also_links.iter().any(|(s, o)| s.ends_with("#Claim")
                && o == "https://www.commoncoreontologies.org/ont00000005"),
            "expected rdfs:seeAlso with the expanded CURIE on the class; got {see_also_links:?}"
        );
        assert!(
            see_also_links.iter().any(|(s, o)| s.ends_with("#refines")
                && o == "http://purl.obolibrary.org/obo/BFO_0000015"),
            "expected rdfs:seeAlso with the expanded CURIE on the slot; got {see_also_links:?}"
        );

        // Elements with neither field emit no editorial cross-references.
        assert!(
            !alt_labels.iter().any(|(s, _)| s.ends_with("#Bare"))
                && !see_also_links.iter().any(|(s, _)| s.ends_with("#Bare")),
            "a class with neither field must emit no altLabel/seeAlso"
        );
        assert!(
            !alt_labels.iter().any(|(s, _)| s.ends_with("#plain"))
                && !see_also_links.iter().any(|(s, _)| s.ends_with("#plain")),
            "a slot with neither field must emit no altLabel/seeAlso"
        );
    }

    #[test]
    fn build_rdf_graph_emits_rdfs_subclass_of_for_external_subclass_of() {
        // External `subclass_of:` grounding is the LinkML mechanism for
        // declaring `rdfs:subClassOf` to an upstream ontology class
        // (BFO/CCO/IAO). Without an explicit emit step the IR field
        // is silently dropped from RDF — the schema looks like an
        // isolated graph in the upstream sense even though the author
        // declared a grounding.
        use sophia::api::term::Term;
        use sophia::api::triple::Triple;

        let mut schema = schema_with_prefixes();
        let mut act = ClassDefinition::new("Act");
        act.subclass_of = Some("cco:ont00000005".into());
        schema.classes.insert("Act".to_string(), act);

        let graph = build_rdf_graph(&schema).unwrap();

        let target_iri = "https://www.commoncoreontologies.org/ont00000005";
        let subclass_of_iri = "http://www.w3.org/2000/01/rdf-schema#subClassOf";
        let has_external_subclass = graph.triples().any(|t| {
            let triple = t.unwrap();
            triple
                .p()
                .iri()
                .is_some_and(|i| i.as_str() == subclass_of_iri)
                && triple.o().iri().is_some_and(|i| i.as_str() == target_iri)
        });
        assert!(
            has_external_subclass,
            "expected rdfs:subClassOf <cco:ont00000005> triple for external grounding"
        );
    }
}
