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
        .unwrap_or_else(|| fallback_element_iri(name, schema))
}

/// How an authored class designation matches a candidate set.
pub(crate) enum ClassMatch<'a> {
    One(&'a str),
    Several,
    None,
}

/// The candidate `authored` names — by exact class name, or by IRI/CURIE
/// equality with the class's minted IRI ([`class_iri_by_name`]); the name
/// arm is load-bearing for a class whose declared `class_uri` differs
/// from its default-prefix mint. Several matches (duplicate `class_uri`
/// declarations) are no match: a designation must name one thing. Type
/// designators and the absence check's `via` narrowing both resolve
/// through here, so the two rules cannot drift apart.
pub(crate) fn class_named_by<'a>(
    schema: &crate::linkml::SchemaDefinition,
    candidates: &[&'a String],
    authored: &str,
) -> ClassMatch<'a> {
    class_named_by_expanded(schema, schema, candidates, authored)
}

/// Every spelling [`class_named_by`] resolves to `class`: its name, its
/// minted IRI, each CURIE the schema's prefixes can form for that IRI,
/// and the bare local name the default prefix expands to it. This is
/// the enumeration inverse of the matcher — the Rust writer compiles it
/// into generated dispatch tables — and the equivalence test in this
/// module keeps the two from drifting apart.
pub(crate) fn class_spellings(
    schema: &crate::linkml::SchemaDefinition,
    class: &str,
) -> Vec<String> {
    let iri = class_iri_by_name(class, schema);
    let mut spellings = vec![class.to_string()];
    for (prefix, base) in &schema.prefixes {
        if let Some(rest) = iri.strip_prefix(base.as_str()) {
            spellings.push(format!("{prefix}:{rest}"));
            // A bare word expands through the default prefix, so the
            // IRI's local name under it is a spelling of its own.
            if schema.default_prefix.as_deref() == Some(prefix) && rest != class {
                spellings.push(rest.to_string());
            }
        }
    }
    spellings.push(iri);
    spellings
}

/// [`class_named_by`] with the two schema roles split: an IRI or CURIE
/// spelling of `authored` expands against `expansion_schema` — the
/// schema whose document authored the value — while the candidates'
/// IRIs mint from their own `schema`. A type designator's record and
/// classes share one schema; an absence claim's `via` is authored in
/// the claiming schema and names a sibling's class.
pub(crate) fn class_named_by_expanded<'a>(
    expansion_schema: &crate::linkml::SchemaDefinition,
    schema: &crate::linkml::SchemaDefinition,
    candidates: &[&'a String],
    authored: &str,
) -> ClassMatch<'a> {
    let mut hit: Option<&'a str> = None;
    let note = |name: &'a str, hit: &mut Option<&'a str>| -> bool {
        if hit.is_some_and(|h| h != name) {
            return true;
        }
        *hit = Some(name);
        false
    };
    for candidate in candidates {
        if candidate.as_str() == authored && note(candidate, &mut hit) {
            return ClassMatch::Several;
        }
    }
    if hit.is_none() {
        // The IRI derivation only runs when no bare name matched — the
        // dominant authoring style never pays for it.
        let authored_iri = resolve_reference_iri(expansion_schema, authored);
        for candidate in candidates {
            if class_iri_by_name(candidate, schema) == authored_iri && note(candidate, &mut hit) {
                return ClassMatch::Several;
            }
        }
    }
    match hit {
        Some(name) => ClassMatch::One(name),
        None => ClassMatch::None,
    }
}

/// Absolute IRI for a class referenced by name — its declaration's
/// `class_uri` when it has one, else the shared fallback. The one derivation
/// for every site that holds a class *name* rather than its definition
/// (parents, domains, ranges, union members, inverses), so none of them can
/// drift from what the class's own declaration emits.
pub(crate) fn class_iri_by_name(name: &str, schema: &SchemaDefinition) -> String {
    match schema.classes.get(name) {
        Some(class_def) => class_iri_string(name, class_def, schema),
        None => fallback_element_iri(name, schema),
    }
}

/// Absolute IRI for a slot referenced by *name* — its declaration's
/// `slot_uri` when it has one, else the shared fallback. Resolved through
/// the shared by-name lookup, so a reference to an attribute-declared
/// slot (`inverse`, slot-level `is_a`) gets the IRI that attribute's own
/// emission uses.
fn slot_iri_by_name(name: &str, schema: &SchemaDefinition) -> String {
    schema
        .find_slot(name)
        .map(|def| slot_iri_string(name, def, schema))
        .unwrap_or_else(|| fallback_element_iri(name, schema))
}

/// LinkML's default URI for an element that declares none:
/// `{default_prefix}:{name}`, expanded — the same rule linkml-runtime
/// applies, so the two tools mint identical IRIs for the same schema. A
/// schema without a usable `default_prefix` falls back to `{id}#{name}`,
/// since LinkML has nothing to expand against there either.
fn fallback_element_iri(name: &str, schema: &SchemaDefinition) -> String {
    crate::linkml_resolve::expand_curie(schema, name)
        .unwrap_or_else(|| format!("{}#{}", ontology_iri_string(schema), name))
}

/// The IRI of an enum's permissible value, matched against either the value
/// key or its `text`, or `None` when the enum does not permit it. Mirrors the
/// derivation used when the enum's individuals are emitted, so the A-box and
/// the T-box name the same thing.
fn enum_value_iri(
    enum_name: &str,
    enum_def: &crate::linkml::EnumDefinition,
    authored: &str,
    schema: &SchemaDefinition,
) -> Option<String> {
    let key = crate::rules::permitted_value_key(enum_def, authored)?;
    let pv = &enum_def.permissible_values[key];
    Some(
        pv.meaning
            .as_deref()
            .map(|m| expand_curie(m, schema))
            .unwrap_or_else(|| format!("{}/{}", enum_iri_string(enum_name, schema), key)),
    )
}

/// Absolute IRI for an enum, mirroring how classes and slots without an
/// explicit URI are addressed. The IR carries no `enum_uri`, so there is
/// nothing to prefer over the derived form.
fn enum_iri_string(name: &str, schema: &SchemaDefinition) -> String {
    fallback_element_iri(name, schema)
}

/// Absolute IRI for a slot: its `slot_uri` (CURIE-expanded) or
/// `{ontology}#{name}`. Shared by the OWL graph and the SHACL shapes graph.
fn slot_iri_string(name: &str, slot_def: &SlotDefinition, schema: &SchemaDefinition) -> String {
    slot_def
        .slot_uri
        .as_deref()
        .map(|s| expand_curie(s, schema))
        .unwrap_or_else(|| fallback_element_iri(name, schema))
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
            triple(graph, subject_iri, predicate, &object_iri)?;
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
        triple(graph, subject_iri, skos_alt_label, alias.as_str())?;
    }
    for reference in see_also {
        let object_iri = make_iri(&expand_curie(reference, schema))?;
        triple(graph, subject_iri, rdfs::seeAlso, &object_iri)?;
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
    triple(&mut graph, &ontology_iri, rdf::type_, owl_ontology)?;

    // rdfs:label from title
    if let Some(ref title) = schema.title {
        triple(&mut graph, &ontology_iri, rdfs::label, title.as_str())?;
    }

    // rdfs:comment from description
    if let Some(ref description) = schema.description {
        triple(
            &mut graph,
            &ontology_iri,
            rdfs::comment,
            description.as_str(),
        )?;
    }

    // owl:versionInfo
    let owl_version_info = owl
        .get("versionInfo")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    if let Some(ref version) = schema.version {
        triple(
            &mut graph,
            &ontology_iri,
            owl_version_info,
            version.as_str(),
        )?;

        // owl:versionIRI — one separator whichever way the `id` is spelled
        // (a slash-ended and a bare `id:` are both legal LinkML, and an
        // empty path segment would make it a different resource).
        if let Some(ref id) = schema.id {
            let version_iri = make_iri(&format!("{}/{}", id.trim_end_matches('/'), version))?;
            let owl_version_iri = owl
                .get("versionIRI")
                .map_err(|e| IoError::Parse(e.to_string()))?;
            triple(&mut graph, &ontology_iri, owl_version_iri, &version_iri)?;
        }
    }

    // dcterms:license
    let dcterms_license = dcterms
        .get("license")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    if let Some(ref license) = schema.license {
        let license_iri = make_iri(license)?;
        triple(&mut graph, &ontology_iri, dcterms_license, &license_iri)?;
    }

    // dcterms:creator from contributors
    let dcterms_creator = dcterms
        .get("creator")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    for contributor in &schema.contributors {
        triple(
            &mut graph,
            &ontology_iri,
            dcterms_creator,
            contributor.name.as_str(),
        )?;
    }

    // dcterms:created
    let dcterms_created = dcterms
        .get("created")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    if let Some(ref created) = schema.created {
        triple(&mut graph, &ontology_iri, dcterms_created, created.as_str())?;
    }

    // dcterms:modified
    let dcterms_modified = dcterms
        .get("modified")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    if let Some(ref modified) = schema.modified {
        triple(
            &mut graph,
            &ontology_iri,
            dcterms_modified,
            modified.as_str(),
        )?;
    }

    // Classes
    let owl_class = owl
        .get("Class")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    let owl_deprecated = owl
        .get("deprecated")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    let owl_union_of = owl
        .get("unionOf")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    let rdfs_subclass_of = rdfs::subClassOf;

    for (name, class_def) in &schema.classes {
        let class_iri_str = class_iri_string(name, class_def, schema);
        let class_iri = make_iri(&class_iri_str)?;

        // rdf:type owl:Class
        triple(&mut graph, &class_iri, rdf::type_, owl_class)?;

        // owl:deprecated true — a Rust bool serializes as an
        // `xsd:boolean`-typed literal.
        if class_def.deprecated.is_some() {
            triple(&mut graph, &class_iri, owl_deprecated, true)?;
        }

        // rdfs:label
        let label = class_def
            .annotations
            .get("panschema:label")
            .cloned()
            .unwrap_or_else(|| name.to_string());
        triple(&mut graph, &class_iri, rdfs::label, label.as_str())?;

        // rdfs:comment from description
        if let Some(ref description) = class_def.description {
            triple(&mut graph, &class_iri, rdfs::comment, description.as_str())?;
        }

        // rdfs:subClassOf from is_a and each mixin. LinkML treats mixins
        // as multiple inheritance; in OWL that maps to one rdfs:subClassOf
        // edge per parent, including each mixin.
        for parent in class_def.is_a.iter().chain(class_def.mixins.iter()) {
            let parent_iri_str = class_iri_by_name(parent, schema);
            let parent_iri = make_iri(&parent_iri_str)?;
            triple(&mut graph, &class_iri, rdfs_subclass_of, &parent_iri)?;
        }

        // External rdfs:subClassOf grounding (`subclass_of:` in
        // LinkML) — typically an upstream ontology class (BFO, CCO,
        // IAO, …). Same predicate as `is_a`, but resolves through
        // the schema's prefix table rather than the local classes
        // map. Single-valued per the LinkML metamodel.
        if let Some(external) = class_def.subclass_of.as_deref() {
            let target_iri = make_iri(&expand_curie(external, schema))?;
            triple(&mut graph, &class_iri, rdfs_subclass_of, &target_iri)?;
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

    // Enums. A permissible value set is a class whose members are the named
    // individuals it enumerates, so a consumer can read the allowed values
    // off the ontology instead of only out of the docs. `owl:oneOf` closes
    // the set; a value's `meaning:` CURIE, when given, is its IRI.
    let owl_named_individual_t = owl
        .get("NamedIndividual")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    let owl_one_of = owl
        .get("oneOf")
        .map_err(|e| IoError::Parse(e.to_string()))?;
    for (enum_name, enum_def) in &schema.enums {
        let enum_iri_str = enum_iri_string(enum_name, schema);
        let enum_iri = make_iri(&enum_iri_str)?;
        triple(&mut graph, &enum_iri, rdf::type_, owl_class)?;
        triple(&mut graph, &enum_iri, rdfs::label, enum_name.as_str())?;
        if let Some(description) = &enum_def.description {
            triple(&mut graph, &enum_iri, rdfs::comment, description.as_str())?;
        }
        if enum_def.deprecated.is_some() {
            triple(&mut graph, &enum_iri, owl_deprecated, true)?;
        }

        let mut value_iris = Vec::new();
        for (key, pv) in &enum_def.permissible_values {
            let value_iri_str = pv
                .meaning
                .as_deref()
                .map(|m| expand_curie(m, schema))
                .unwrap_or_else(|| format!("{enum_iri_str}/{key}"));
            let value_iri = make_iri(&value_iri_str)?;
            triple(&mut graph, &value_iri, rdf::type_, owl_named_individual_t)?;
            triple(&mut graph, &value_iri, rdf::type_, &enum_iri)?;
            let label = if pv.text.is_empty() {
                key.as_str()
            } else {
                pv.text.as_str()
            };
            triple(&mut graph, &value_iri, rdfs::label, label)?;
            if let Some(description) = &pv.description {
                triple(&mut graph, &value_iri, rdfs::comment, description.as_str())?;
            }
            value_iris.push(value_iri);
        }
        if !value_iris.is_empty() {
            emit_rdf_list(
                &mut graph,
                &enum_iri,
                owl_one_of,
                &enum_iri_str,
                "valuecell",
                value_iris,
            )?;
        }
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

        // An `any_of` union whose every member names a class. Such a slot has
        // no scalar `range:`, so without this it would fall through as a
        // datatype property while its instances assert IRI objects.
        let union_classes: Vec<&String> = slot_def
            .any_of
            .iter()
            .filter_map(|branch| branch.range.as_ref())
            .filter(|r| schema.classes.contains_key(*r))
            .collect();
        let all_union_classes =
            !slot_def.any_of.is_empty() && union_classes.len() == slot_def.any_of.len();

        // Determine property type
        let is_object_property = slot_def
            .annotations
            .get("panschema:owl_property_type")
            .map(|s| s == "ObjectProperty")
            .unwrap_or_else(|| {
                slot_def
                    .range
                    .as_ref()
                    .map(|r| schema.classes.contains_key(r) || schema.enums.contains_key(r))
                    .unwrap_or(all_union_classes)
            });

        // rdf:type
        if is_object_property {
            triple(&mut graph, &prop_iri, rdf::type_, owl_object_property)?;
        } else {
            triple(&mut graph, &prop_iri, rdf::type_, owl_datatype_property)?;
        }

        // owl:deprecated true — see the class emission above.
        if slot_def.deprecated.is_some() {
            triple(&mut graph, &prop_iri, owl_deprecated, true)?;
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
                triple(&mut graph, &prop_iri, rdf::type_, characteristic_type)?;
            }
        }

        // rdfs:label
        let label = slot_def
            .annotations
            .get("panschema:label")
            .cloned()
            .unwrap_or_else(|| name.to_string());
        triple(&mut graph, &prop_iri, rdfs::label, label.as_str())?;

        // rdfs:comment from description
        if let Some(ref description) = slot_def.description {
            triple(&mut graph, &prop_iri, rdfs::comment, description.as_str())?;
        }

        // rdfs:domain — an explicit `domain:` wins; otherwise an
        // attribute/effective slot is domained by the class that introduced
        // it (a top-level slot without an explicit domain keeps its current
        // domain-less behavior).
        let domain_name = slot_def.domain.as_deref().or(domain_class.as_deref());
        if let Some(domain) = domain_name {
            let domain_iri_str = class_iri_by_name(domain, schema);
            let domain_iri = make_iri(&domain_iri_str)?;
            triple(&mut graph, &prop_iri, rdfs::domain, &domain_iri)?;
        }

        // rdfs:range. For a datatype property the range must be a built-in
        // primitive to get an `rdfs:range` — an enum, a class the writer
        // didn't recognize as an object property, or a typo has no XSD
        // datatype, so emit none rather than fabricating a nonexistent
        // `xsd:{name}` (`xsd_datatype_iri` returns `None` for all of those).
        if let Some(ref range) = slot_def.range {
            let range_iri_str = if schema.enums.contains_key(range) {
                // An enum range names the enum's class, now that enums are
                // emitted; previously this fell through with no range at all.
                Some(enum_iri_string(range, schema))
            } else if is_object_property {
                Some(class_iri_by_name(range, schema))
            } else {
                crate::primitives::xsd_datatype_iri(range)
            };
            if let Some(range_iri_str) = range_iri_str {
                let range_iri = make_iri(&range_iri_str)?;
                triple(&mut graph, &prop_iri, rdfs::range, &range_iri)?;
            }
        } else if all_union_classes {
            // No single range to name: the slot accepts any member of the
            // union, which OWL states as a class expression over `owl:unionOf`.
            let members = union_classes
                .iter()
                .map(|member| make_iri(&class_iri_by_name(member, schema)))
                .collect::<Result<Vec<_>, _>>()?;
            let union_node = make_iri(&format!("{prop_iri_str}/range"))?;
            triple(&mut graph, &prop_iri, rdfs::range, &union_node)?;
            triple(&mut graph, &union_node, rdf::type_, owl_class)?;
            emit_rdf_list(
                &mut graph,
                &union_node,
                owl_union_of,
                &prop_iri_str,
                "unioncell",
                members,
            )?;
        }

        // owl:inverseOf — the inverse names another slot, so it derives the
        // way that slot's own declaration would.
        if let Some(ref inverse) = slot_def.inverse {
            let inverse_iri = make_iri(&slot_iri_by_name(inverse, schema))?;
            triple(&mut graph, &prop_iri, owl_inverse_of, &inverse_iri)?;
        }

        // rdfs:subPropertyOf — slot-level `is_a` names another slot; its
        // IRI derives the way that slot's own declaration would.
        if let Some(ref parent) = slot_def.is_a {
            let parent_iri = make_iri(&slot_iri_by_name(parent, schema))?;
            triple(&mut graph, &prop_iri, rdfs::subPropertyOf, &parent_iri)?;
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
                .unwrap_or_else(|| {
                    // Same rule as instance ids: default_prefix expansion,
                    // fragment only when the schema gives nothing to expand
                    // against.
                    crate::linkml_resolve::expand_curie(schema, ind_id)
                        .unwrap_or_else(|| format!("{}#{}", ontology_iri_str, ind_id))
                });
            let ind_iri = make_iri(&ind_iri_str)?;

            // rdf:type owl:NamedIndividual
            triple(&mut graph, &ind_iri, rdf::type_, owl_named_individual)?;

            // Additional types
            let types_key = format!("panschema:individual:{}", ind_id);
            if let Some(types_str) = schema.annotations.get(&types_key) {
                for type_iri_str in types_str.split(',') {
                    let type_iri_str = type_iri_str.trim();
                    if !type_iri_str.is_empty() {
                        let type_iri = make_iri(type_iri_str)?;
                        triple(&mut graph, &ind_iri, rdf::type_, &type_iri)?;
                    }
                }
            }

            // rdfs:label
            let label_key = format!("panschema:individual:{}:_label", ind_id);
            if let Some(label) = schema.annotations.get(&label_key) {
                triple(&mut graph, &ind_iri, rdfs::label, label.as_str())?;
            }
        }
    }

    Ok(graph)
}

/// A typed literal carrying one of [`crate::primitives`]' static XSD
/// datatype IRIs — table-validated, so no per-value IRI parse.
fn typed_literal<'a>(
    lexical: &'a str,
    datatype: &'static str,
) -> sophia::api::term::SimpleTerm<'a> {
    sophia::api::term::SimpleTerm::LiteralDatatype(
        sophia::api::MownStr::from(lexical),
        sophia::iri::IriRef::new_unchecked(sophia::api::MownStr::from(datatype)),
    )
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
    // A record named by CURIE or absolute IRI carries its own namespace — a
    // shared-vocabulary record, or one belonging to another graph — so its
    // dataset's scope does not apply to it. That asymmetry is what lets a
    // scoped dataset and a shared one live under the same mechanism.
    let names_its_own_namespace = inst.id.contains("://")
        || inst.id.starts_with("urn:")
        || inst
            .id
            .split_once(':')
            .is_some_and(|(prefix, _)| schema.prefixes.contains_key(prefix));
    if !names_its_own_namespace && let Some(scope) = &inst.scope {
        return format!("{scope}/{}", inst.id);
    }
    resolve_reference_iri(schema, &inst.id)
}

/// The IRI a reference target (or bare record id) denotes: prefix or
/// absolute-IRI expansion against the schema, falling back to the
/// ontology-fragment mint for a target nothing expands. One derivation
/// shared by instance minting, reference emission, and the cross-graph
/// resolution check, so they cannot disagree on what a name points at.
pub fn resolve_reference_iri(schema: &SchemaDefinition, target: &str) -> String {
    crate::linkml_resolve::expand_curie(schema, target)
        .unwrap_or_else(|| format!("{}#{}", ontology_iri_string(schema), target))
}

/// Every record's minted IRI keyed by its id, across `sets` — the map a
/// reference target resolves through before falling back to
/// [`resolve_reference_iri`], shared by the RDF emission and the
/// cross-graph absence check so both prefer a record's real minted IRI
/// (scoping included) over a fabricated expansion.
pub(crate) fn instance_iris_by_id<'a>(
    schema: &SchemaDefinition,
    sets: &'a [crate::instances::InstanceSet],
) -> std::collections::BTreeMap<&'a str, String> {
    sets.iter()
        .flat_map(|set| &set.instances)
        .map(|i| (i.id.as_str(), instance_iri_string(schema, i)))
        .collect()
}

/// The namespace a schema's instance minting expands bare ids under —
/// the default prefix's expansion, or the ontology IRI's fragment base.
/// Scoped records start with it too (their scope is itself minted under
/// it), so this is the ownership test a cross-graph resolution check
/// scopes references by. A record whose id names its own namespace (an
/// absolute-IRI id) can mint outside it.
pub fn instance_namespace(schema: &SchemaDefinition) -> String {
    schema
        .default_prefix
        .as_deref()
        .and_then(|p| schema.prefixes.get(p))
        .cloned()
        .unwrap_or_else(|| format!("{}#", ontology_iri_string(schema)))
}

/// Emit each instance as an `owl:NamedIndividual`: `rdf:type` per declared
/// class, `rdfs:label` from the display name, one data-property triple per
/// scalar slot value, and one object-property triple per id reference,
/// resolved to the referenced instance's IRI. A conforming scalar's literal
/// carries the datatype the slot's range implies — the same derivation the
/// SHACL writer uses for `sh:datatype`, so an integer under a float-ranged
/// slot lands as `xsd:float` and agrees with the shapes emitted from the
/// same schema. A value the range cannot faithfully type (wrong kind,
/// malformed date, `NaN` under `decimal`) emits as authored with its own
/// value-kind typing: nothing vanishes, and the mismatch stays visible to
/// the shapes and to the conformance check instead of being silently
/// converted. A reference whose target id names no instance still emits
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

    let iri_by_id = instance_iris_by_id(schema, std::slice::from_ref(set));

    for inst in &set.instances {
        let subject = make_iri(&instance_iri_string(schema, inst))?;

        triple(graph, &subject, rdf::type_, owl_named_individual)?;
        for class_name in &inst.types {
            if let Some(class_def) = schema.classes.get(class_name) {
                let class_iri = make_iri(&class_iri_string(class_name, class_def, schema))?;
                triple(graph, &subject, rdf::type_, &class_iri)?;
            }
        }
        if !inst.label.is_empty() {
            triple(graph, &subject, rdfs::label, inst.label.as_str())?;
        }

        for sv in &inst.slot_values {
            let slot_def = effective_slot(&inst.types, &sv.slot);
            let predicate = match &slot_def {
                Some(def) => make_iri(&slot_iri_string(&sv.slot, def, schema))?,
                None => make_iri(&format!("{}#{}", ontology_iri_string(schema), sv.slot))?,
            };
            let range = slot_def.as_ref().and_then(|d| d.range.as_deref());
            // Guard order mirrors the shapes writer: a class or enum range is
            // an object-property assertion, never a typed literal — so an
            // enum or class whose name collides with a primitive (an enum
            // named `date`) can never take the datatype path the shapes
            // don't take.
            let range_is_class = range.is_some_and(|r| schema.classes.contains_key(r));
            // An enum-ranged slot is an object property over the enum's value
            // individuals, so its values assert as IRIs rather than literals.
            let range_enum = range.and_then(|r| schema.enums.get(r).map(|e| (r, e)));
            for value in &sv.values {
                let InstanceValue::Scalar(scalar) = value else {
                    // References emit below from `inst.references`; a
                    // range-kind mismatch has no faithful literal form.
                    continue;
                };
                if let Some((enum_name, enum_def)) = range_enum {
                    let authored = crate::instances::scalar_to_display(scalar);
                    if let Some(value_iri_str) =
                        enum_value_iri(enum_name, enum_def, &authored, schema)
                    {
                        let object = make_iri(&value_iri_str)?;
                        triple(graph, &subject, &predicate, &object)?;
                        continue;
                    }
                    // A value the enum doesn't permit: keep it as authored so
                    // nothing is invented, and let validation report it.
                }
                if !range_is_class
                    && range_enum.is_none()
                    && let Some((lexical, datatype)) =
                        range.and_then(|r| crate::primitives::range_typed_literal(r, scalar))
                {
                    // A conforming value under a primitive range: the literal
                    // carries the datatype the shapes constrain the property
                    // to, derived from the same table (`xsd_datatype`).
                    triple(
                        graph,
                        &subject,
                        &predicate,
                        typed_literal(&lexical, datatype),
                    )?;
                    continue;
                }
                // The authored value-kind form: rangeless and custom-typed
                // slots (which the shapes leave unconstrained), and values a
                // primitive range cannot faithfully type — present in the
                // output for the shapes and the conformance check to report,
                // with nothing invented and nothing dropped.
                match scalar {
                    ScalarValue::String(s) => graph.insert(&subject, &predicate, s.as_str()),
                    ScalarValue::Boolean(b) => graph.insert(&subject, &predicate, *b),
                    ScalarValue::Float(f) => graph.insert(&subject, &predicate, *f),
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
                .unwrap_or_else(|| resolve_reference_iri(schema, &reference.target));
            let target = make_iri(&target_iri_str)?;
            triple(graph, &subject, &predicate, &target)?;
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
/// SHACL Core only. Slot `range` → `sh:datatype` (scalar), `sh:class`
/// (class-valued), or `sh:in` over the value IRIs (enum-valued — the same
/// IRIs the A-box asserts, so an unpermitted value fails validation). A
/// rule condition's range is *not* closed this way: there it only types
/// the condition's literals, and a second constraint beside the
/// condition's own `sh:hasValue` would leave a precondition nothing can
/// satisfy. `required` and
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

        triple(&mut graph, &shape_iri, rdf::type_, &t.node_shape)?;
        triple(&mut graph, &shape_iri, &t.target_class, &class_iri)?;

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
            emit_condition_shape(
                &mut graph,
                &t,
                &pre_iri,
                &format!("{shape_iri_str}/rule{i}/pre"),
                pre,
                &effective,
                schema,
            )?;
            let post_iri = make_iri(&format!("{shape_iri_str}/rule{i}/post"))?;
            emit_condition_shape(
                &mut graph,
                &t,
                &post_iri,
                &format!("{shape_iri_str}/rule{i}/post"),
                post,
                &effective,
                schema,
            )?;

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
            triple(&mut graph, &notpre, &t.not_, &pre_iri)?;
            triple(&mut graph, &shape_iri, &t.or_, &or0)?;
            w(&mut graph, &or0, rdf::first, &notpre)?;
            w(&mut graph, &or0, rdf::rest, &or1)?;
            w(&mut graph, &or1, rdf::first, &post_iri)?;
            triple(&mut graph, &or1, rdf::rest, rdf::nil)?;
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
    in_: Iri<String>,
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
            in_: sh("in")?,
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
    /// `value_presence` — PRESENT is "at least one value" (`sh:minCount 1`),
    /// ABSENT is "no values" (`sh:maxCount 0`).
    value_presence: Option<crate::linkml::ValuePresence>,
    /// Slot-level `any_of`: the value satisfies at least one alternative —
    /// emitted as `sh:or` over per-alternative constraint shapes.
    any_of: Vec<PropertyConstraints<'a>>,
    /// Whether an enum `range` should close the value set with `sh:in`.
    ///
    /// True for a slot's own property shape, which is where the range is a
    /// constraint. False inside a rule-condition shape: there the range is
    /// carried only to *type* `equals_number` (see [`with_range`]), and
    /// adding `sh:in` would contradict the condition's own
    /// `sh:hasValue` — leaving a precondition nothing can satisfy, which
    /// makes the whole rule vacuously true instead of enforced.
    ///
    /// [`with_range`]: PropertyConstraints::with_range
    close_enum_range: bool,
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
            close_enum_range: true,
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
            value_presence: cond.value_presence,
            any_of: cond.any_of.iter().map(Self::from_condition).collect(),
            // A condition's range types its literals; it does not re-assert
            // the slot's value set. The class-level property shape already
            // carries that, and duplicating it here would contradict the
            // condition's own `sh:hasValue`.
            close_enum_range: false,
        }
    }

    /// Fill in `range` from the slot's declaration when the condition
    /// carries none of its own — a rule condition's range lives on the
    /// slot, not the condition, and it's what types `equals_number`.
    fn with_range(mut self, range: Option<&'a str>) -> Self {
        if self.range.is_none() {
            self.range = range;
        }
        self.any_of = self
            .any_of
            .into_iter()
            .map(|alt| alt.with_range(range))
            .collect();
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
                    rule: crate::rules::rule_label(rule, i),
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
/// Emit one side of a rule (pre or post) as a node shape at `cond_iri`:
/// a property shape per `slot_conditions` entry, plus — when the condition
/// carries `any_of` alternatives — an `sh:or` over recursively-emitted
/// alternative condition-set shapes at `{base}/alt{k}`. A shape conforms
/// when all its property shapes hold AND at least one alternative holds,
/// which is exactly LinkML's conjunction of `slot_conditions` with `any_of`.
#[allow(clippy::too_many_arguments)]
fn emit_condition_shape(
    graph: &mut FastGraph,
    t: &ShaclTerms,
    cond_iri: &Iri<String>,
    base: &str,
    conditions: &crate::linkml::RuleConditions,
    effective: &std::collections::BTreeMap<String, SlotDefinition>,
    schema: &SchemaDefinition,
) -> IoResult<()> {
    for (slot, cond) in &conditions.slot_conditions {
        let def = effective
            .get(slot)
            .expect("skip check guarantees the slot resolves");
        let ps = make_iri(&format!("{base}/{slot}"))?;
        let path = make_iri(&slot_iri_string(slot, def, schema))?;
        emit_property_shape(
            graph,
            t,
            cond_iri,
            &ps,
            &path,
            schema,
            // A condition carries no `range` of its own; the slot's
            // declared range is what types an `equals_number`
            // `sh:hasValue` correctly.
            PropertyConstraints::from_condition(cond).with_range(def.range.as_deref()),
        )?;
    }
    if !conditions.any_of.is_empty() {
        let mut alts = Vec::new();
        for (k, alt) in conditions.any_of.iter().enumerate() {
            let alt_iri = make_iri(&format!("{base}/alt{k}"))?;
            emit_condition_shape(
                graph,
                t,
                &alt_iri,
                &format!("{base}/alt{k}"),
                alt,
                effective,
                schema,
            )?;
            alts.push(alt_iri);
        }
        emit_or_list(graph, t, cond_iri, base, alts)?;
    }
    Ok(())
}

fn shacl_rule_skip_reason(
    rule: &crate::linkml::ClassRule,
    slot_names: &std::collections::BTreeSet<&str>,
) -> Option<String> {
    fn side_reason(
        conditions: &crate::linkml::RuleConditions,
        slot_names: &std::collections::BTreeSet<&str>,
    ) -> Option<String> {
        if conditions.slot_conditions.is_empty() && conditions.any_of.is_empty() {
            return Some(
                "a precondition or postcondition has neither slot_conditions nor any_of"
                    .to_string(),
            );
        }
        for slot in conditions.slot_conditions.keys() {
            if !slot_names.contains(slot.as_str()) {
                return Some(format!(
                    "references slot `{slot}`, which the class does not have"
                ));
            }
        }
        conditions
            .any_of
            .iter()
            .find_map(|alt| side_reason(alt, slot_names))
    }

    match (&rule.preconditions, &rule.postconditions) {
        (Some(pre), Some(post)) => {
            side_reason(pre, slot_names).or_else(|| side_reason(post, slot_names))
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
/// Insert one triple, mapping the graph's mutation error into [`IoError`].
///
/// The builders below assert around seventy triples. Spelling the error
/// mapping at each site buried the triple being asserted under plumbing —
/// three lines to say one fact — and the file had already grown two ad-hoc
/// local closures to escape it.
fn triple<S, P, O>(graph: &mut FastGraph, s: S, p: P, o: O) -> IoResult<()>
where
    S: sophia::api::term::Term,
    P: sophia::api::term::Term,
    O: sophia::api::term::Term,
{
    graph
        .insert(s, p, o)
        .map_err(|e| IoError::Write(e.to_string()))?;
    Ok(())
}

/// Insert `items` as an RDF collection, linked from `subject` by
/// `predicate`. A no-op for an empty list, since there is no useful
/// `sh:in ()`.
///
/// Cells are named by `cell` rather than being blank nodes — the same
/// choice the conditional rule shapes make, and for the same reason:
/// deterministic IRIs keep the output byte-stable across runs and let a
/// consumer query into the list.
fn insert_rdf_list(
    graph: &mut FastGraph,
    subject: &Iri<String>,
    predicate: &Iri<String>,
    items: &[String],
    cell: impl Fn(usize) -> String,
) -> IoResult<()> {
    let Some(last) = items.len().checked_sub(1) else {
        return Ok(());
    };
    let write = |e: <FastGraph as sophia::api::graph::MutableGraph>::MutationError| {
        IoError::Write(e.to_string())
    };

    graph
        .insert(subject, predicate, &make_iri(&cell(0))?)
        .map_err(write)?;
    for (i, item) in items.iter().enumerate() {
        let here = make_iri(&cell(i))?;
        graph
            .insert(&here, rdf::first, &make_iri(item)?)
            .map_err(write)?;
        if i == last {
            graph.insert(&here, rdf::rest, rdf::nil).map_err(write)?;
        } else {
            graph
                .insert(&here, rdf::rest, &make_iri(&cell(i + 1))?)
                .map_err(write)?;
        }
    }
    Ok(())
}

fn emit_property_shape(
    graph: &mut FastGraph,
    t: &ShaclTerms,
    shape: &Iri<String>,
    prop_shape: &Iri<String>,
    path: &Iri<String>,
    schema: &SchemaDefinition,
    c: PropertyConstraints<'_>,
) -> IoResult<()> {
    triple(graph, shape, &t.property, prop_shape)?;
    triple(graph, prop_shape, &t.path, path)?;
    emit_constraint_fields(graph, t, prop_shape, schema, c)
}

/// Emit `c`'s value constraints onto `node` — the body shared by a full
/// property shape and each `sh:or` alternative (which applies to the same
/// value nodes and so carries no `sh:path` of its own). Slot-level
/// `any_of` recurses here: alternatives become an `sh:or` list of
/// constraint shapes at `{node}/or{j}`.
fn emit_constraint_fields(
    graph: &mut FastGraph,
    t: &ShaclTerms,
    prop_shape: &Iri<String>,
    schema: &SchemaDefinition,
    c: PropertyConstraints<'_>,
) -> IoResult<()> {
    if let Some(range) = c.range {
        if let Some(target) = schema.classes.get(range) {
            let target_iri = make_iri(&class_iri_string(range, target, schema))?;
            triple(graph, prop_shape, &t.class, &target_iri)?;
        } else if c.close_enum_range
            && let Some(enum_def) = schema.enums.get(range)
        {
            // An enum range closes the value set over the IRIs the A-box
            // actually asserts, so an unlisted value fails validation
            // instead of passing unconstrained. No `sh:datatype` rides
            // along: the values are IRIs, and there is no XSD datatype for
            // an enum to fabricate.
            let values: Vec<String> = enum_def
                .permissible_values
                .keys()
                .filter_map(|key| enum_value_iri(range, enum_def, key, schema))
                .collect();
            insert_rdf_list(graph, prop_shape, &t.in_, &values, |i| {
                format!("{prop_shape}/in{i}")
            })?;
        } else if let Some(xsd) = crate::primitives::xsd_datatype_iri(range) {
            // Only a built-in primitive gets an `sh:datatype`; a typo has no
            // XSD datatype, so emit none rather than a fabricated
            // `xsd:{name}`.
            let xsd_iri = make_iri(&xsd)?;
            triple(graph, prop_shape, &t.datatype, &xsd_iri)?;
        }
    }
    // `required` and `minimum_cardinality` are two spellings of the same
    // lower bound; emitting a `sh:minCount` for each would contradict itself.
    // Reconcile to one, with an explicit cardinality winning over the flag —
    // the same precedence `effective_cardinality` gives the HTML view.
    // `value_presence` folds into the same bounds: PRESENT is a lower
    // bound of 1 ("at least one value"), ABSENT an upper bound of 0.
    let presence_min = u32::from(matches!(
        c.value_presence,
        Some(crate::linkml::ValuePresence::Present)
    ));
    let effective_min = c
        .min_cardinality
        .unwrap_or(u32::from(c.required))
        .max(presence_min);
    if effective_min > 0 {
        triple(graph, prop_shape, &t.min_count, effective_min as i32)?;
    }
    let effective_max = if matches!(c.value_presence, Some(crate::linkml::ValuePresence::Absent)) {
        Some(0)
    } else {
        c.max_cardinality
    };
    if let Some(max) = effective_max {
        triple(graph, prop_shape, &t.max_count, max as i32)?;
    }
    if let Some(pattern) = c.pattern {
        triple(graph, prop_shape, &t.pattern, pattern)?;
    }
    if let Some(min) = c.min_value {
        triple(graph, prop_shape, &t.min_inclusive, min)?;
    }
    if let Some(max) = c.max_value {
        triple(graph, prop_shape, &t.max_inclusive, max)?;
    }
    // `sh:hasValue` is term equality (datatype-sensitive), so the literal
    // must carry the exact datatype the A-box derives from the slot's range
    // — via the same `range_typed_literal` derivation — or conforming data
    // could never equal it. A rangeless condition, or a constant the range
    // cannot faithfully type, keeps the value-kind default the A-box falls
    // back to for the same case.
    if let Some(v) = c.equals_string {
        let scalar = crate::instances::ScalarValue::String(v.to_string());
        match c
            .range
            .and_then(|r| crate::primitives::range_typed_literal(r, &scalar))
        {
            Some((lexical, datatype)) => {
                triple(
                    graph,
                    prop_shape,
                    &t.has_value,
                    typed_literal(&lexical, datatype),
                )?;
            }
            None => triple(graph, prop_shape, &t.has_value, v)?,
        }
    }
    if let Some(n) = c.equals_number {
        let scalar = crate::instances::ScalarValue::Float(n);
        match c
            .range
            .and_then(|r| crate::primitives::range_typed_literal(r, &scalar))
        {
            Some((lexical, datatype)) => {
                triple(
                    graph,
                    prop_shape,
                    &t.has_value,
                    typed_literal(&lexical, datatype),
                )?;
            }
            None => triple(graph, prop_shape, &t.has_value, n)?,
        }
    }
    if !c.any_of.is_empty() {
        let mut alts = Vec::new();
        for (j, alt) in c.any_of.into_iter().enumerate() {
            let member = make_iri(&format!("{prop_shape}/or{j}"))?;
            emit_constraint_fields(graph, t, &member, schema, alt)?;
            alts.push(member);
        }
        emit_or_list(graph, t, prop_shape, &format!("{prop_shape}"), alts)?;
    }
    Ok(())
}

/// Wire `subject sh:or ( members… )` with deterministic named list cells
/// (`{base}/orcell{j}`) rather than blank nodes, matching the rule shapes'
/// stable-IRI convention.
/// Attach an RDF list of `members` to `subject` under `predicate`. List cells
/// are minted from `base` rather than blank nodes, matching how this module
/// already writes SHACL's `sh:or` lists, so the output stays addressable.
fn emit_rdf_list<P>(
    graph: &mut FastGraph,
    subject: &Iri<String>,
    predicate: P,
    base: &str,
    cell_name: &str,
    members: Vec<Iri<String>>,
) -> IoResult<()>
where
    P: sophia::api::term::Term + Copy,
{
    let mut cells = Vec::new();
    for j in 0..members.len() {
        cells.push(make_iri(&format!("{base}/{cell_name}{j}"))?);
    }
    if let Some(first) = cells.first() {
        triple(graph, subject, predicate, first)?;
    }
    for (j, (cell, member)) in cells.iter().zip(&members).enumerate() {
        triple(graph, cell, rdf::first, member)?;
        match cells.get(j + 1) {
            Some(next) => graph
                .insert(cell, rdf::rest, next)
                .map_err(|e| IoError::Write(e.to_string()))?,
            None => graph
                .insert(cell, rdf::rest, rdf::nil)
                .map_err(|e| IoError::Write(e.to_string()))?,
        };
    }
    Ok(())
}

fn emit_or_list(
    graph: &mut FastGraph,
    t: &ShaclTerms,
    subject: &Iri<String>,
    base: &str,
    members: Vec<Iri<String>>,
) -> IoResult<()> {
    let mut cells = Vec::new();
    for j in 0..members.len() {
        cells.push(make_iri(&format!("{base}/orcell{j}"))?);
    }
    if let Some(first) = cells.first() {
        triple(graph, subject, &t.or_, first)?;
    }
    for (j, (cell, member)) in cells.iter().zip(&members).enumerate() {
        triple(graph, cell, rdf::first, member)?;
        match cells.get(j + 1) {
            Some(next) => graph
                .insert(cell, rdf::rest, next)
                .map_err(|e| IoError::Write(e.to_string()))?,
            None => graph
                .insert(cell, rdf::rest, rdf::nil)
                .map_err(|e| IoError::Write(e.to_string()))?,
        };
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

    /// `class_spellings` is the enumeration inverse of `class_named_by`:
    /// every spelling it emits must resolve to its class through the
    /// matcher, and the matcher-accepted spellings — the bare local name
    /// the default prefix expands included — must all be emitted.
    #[test]
    fn class_spellings_and_the_matcher_agree() {
        let mut schema = crate::linkml::SchemaDefinition::new("s");
        schema.default_prefix = Some("ex".to_string());
        schema
            .prefixes
            .insert("ex".to_string(), "https://example.org/x/".to_string());
        schema
            .classes
            .insert("Lamp".to_string(), ClassDefinition::new("Lamp"));
        // A class whose `class_uri` local name differs from its class
        // name answers to both.
        let mut bar = ClassDefinition::new("Bar");
        bar.class_uri = Some("ex:Foo".to_string());
        schema.classes.insert("Bar".to_string(), bar);

        for class in ["Lamp", "Bar"] {
            let spellings = class_spellings(&schema, class);
            let class_string = class.to_string();
            let candidates = [&class_string];
            for spelling in &spellings {
                assert!(
                    matches!(
                        class_named_by(&schema, &candidates, spelling),
                        ClassMatch::One(_)
                    ),
                    "every emitted spelling resolves through the matcher: `{spelling}`"
                );
            }
        }
        let bar_spellings = class_spellings(&schema, "Bar");
        for accepted in ["Bar", "Foo", "ex:Foo", "https://example.org/x/Foo"] {
            assert!(
                bar_spellings.iter().any(|s| s == accepted),
                "the matcher accepts `{accepted}`, so the enumeration must emit it; got: \
                 {bar_spellings:?}"
            );
        }

        // Without a default prefix there is no bare-word expansion, so
        // the bare local name must not be emitted — the matcher would
        // resolve it to nothing.
        schema.default_prefix = None;
        let mut bar = ClassDefinition::new("Bar");
        bar.class_uri = Some("ex:Foo".to_string());
        schema.classes.insert("Bar".to_string(), bar);
        let spellings = class_spellings(&schema, "Bar");
        let bar_string = "Bar".to_string();
        for spelling in &spellings {
            assert!(
                matches!(
                    class_named_by(&schema, &[&bar_string], spelling),
                    ClassMatch::One(_)
                ),
                "every emitted spelling must still resolve without a default prefix: \
                 `{spelling}`; got: {spellings:?}"
            );
        }
    }

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

        let data: serde_norway::Value = serde_norway::from_str(
            "bottles:\n  - id: b1\n    score: 4\n    stored_in: r1\nracks:\n  - id: r1\n",
        )
        .unwrap();
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        (schema, set)
    }

    #[test]
    fn an_undeclared_class_uri_mints_via_default_prefix_like_linkml_does() {
        // LinkML's rule: an element without an explicit URI gets
        // `{default_prefix}:{Name}`. Minting anything else means the same
        // schema yields different IRIs here than through linkml-runtime, and
        // cross-tool joins fail silently.
        let (schema, _) = abox_fixture();
        let bottle = schema.classes.get("Bottle").expect("Bottle");
        assert_eq!(
            class_iri_string("Bottle", bottle, &schema),
            "https://example.org/cellar/Bottle",
            "class fallback expands against default_prefix, not id#fragment"
        );
        let score = bottle.attributes.get("score").expect("score");
        assert_eq!(
            slot_iri_string("score", score, &schema),
            "https://example.org/cellar/score",
            "slot fallback follows the same rule"
        );
        assert_eq!(
            enum_iri_string("Color", &schema),
            "https://example.org/cellar/Color",
            "enum fallback follows the same rule"
        );
    }

    #[test]
    fn without_a_default_prefix_the_fragment_fallback_remains() {
        // A schema that declares no default_prefix gives LinkML nothing to
        // expand against either; the fragment form is the stable fallback.
        let mut schema = SchemaDefinition::new("bare");
        schema.id = Some("https://example.org/bare".to_string());
        let class = ClassDefinition::new("Thing");
        assert_eq!(
            class_iri_string("Thing", &class, &schema),
            "https://example.org/bare#Thing"
        );
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
            scope: None,
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
    fn a_cross_graph_reference_emits_as_an_iri_object_not_a_literal() {
        // The whole point of a cross-graph edge is that another graph can
        // join on it. A literal `"catalog:aws"` joins with nothing.
        let (mut schema, _) = abox_fixture();
        schema.prefixes.insert(
            "catalog".to_string(),
            "https://example.org/catalog/".to_string(),
        );
        let data: serde_norway::Value =
            serde_norway::from_str("bottles:\n  - id: b1\n    stored_in: 'catalog:vault'\n")
                .unwrap();
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let graph = build_rdf_graph_with_instances(&schema, Some(&set)).expect("graph");

        use sophia::api::graph::Graph;
        use sophia::api::term::Term;
        use sophia::api::triple::Triple;
        let subject = make_iri("https://example.org/cellar/b1").unwrap();
        let predicate = make_iri("https://example.org/cellar/stored_in").unwrap();
        let objects: Vec<String> = graph
            .triples_matching([subject], [predicate], sophia::api::term::matcher::Any)
            .map(|t| {
                let t = t.unwrap();
                assert!(
                    t.o().is_iri(),
                    "a cross-graph target must be an IRI object, not a literal"
                );
                t.o().iri().expect("iri").to_string()
            })
            .collect();
        assert_eq!(
            objects,
            vec!["https://example.org/catalog/vault".to_string()],
            "and the CURIE expands against the prefix the schema declares"
        );
    }

    #[test]
    fn instance_namespace_is_the_minting_base() {
        // The ownership test cross-graph resolution scopes by must be the
        // base bare-id minting expands under — the default prefix's
        // expansion, or the ontology fragment base without one.
        let (schema, _) = abox_fixture();
        assert_eq!(instance_namespace(&schema), "https://example.org/cellar/");
        let mut bare = SchemaDefinition::new("bare");
        bare.id = Some("https://example.org/bare".to_string());
        assert_eq!(
            instance_namespace(&bare),
            "https://example.org/bare#",
            "no default prefix falls back to the ontology fragment base"
        );
    }

    #[test]
    fn integer_value_under_a_float_range_emits_xsd_float() {
        let (schema, set) = abox_fixture();
        let graph = build_rdf_graph_with_instances(&schema, Some(&set)).expect("graph");
        // The authored `score: 4` parses as an integer, but the slot's
        // declared range is float — the literal carries the datatype the
        // schema's own SHACL shapes constrain it to (`sh:datatype
        // xsd:float`), never the value's parse kind. SPARQL numeric
        // promotion still joins xsd:float against other numeric types.
        assert_eq!(
            literal_parts(
                &graph,
                "https://example.org/cellar/b1",
                "https://example.org/cellar/score"
            ),
            vec![(
                "4".to_string(),
                "http://www.w3.org/2001/XMLSchema#float".to_string()
            )],
            "a float-range slot's integer value must emit as xsd:float"
        );
    }

    /// Every literal object at (subject IRI, predicate IRI) as its
    /// `(lexical form, datatype IRI)` pair, for asserting A-box typing in
    /// any fixture namespace.
    fn literal_parts(graph: &FastGraph, subject: &str, predicate: &str) -> Vec<(String, String)> {
        use sophia::api::graph::Graph;
        use sophia::api::term::Term;
        use sophia::api::triple::Triple;
        let subject = make_iri(subject).unwrap();
        let predicate = make_iri(predicate).unwrap();
        graph
            .triples_matching([subject], [predicate], sophia::api::term::matcher::Any)
            .map(|t| {
                let t = t.unwrap();
                let o = t.o();
                (
                    o.lexical_form().expect("a literal object").to_string(),
                    o.datatype().expect("a literal object").to_string(),
                )
            })
            .collect()
    }

    /// A schema exercising every range-vs-value-kind combination the A-box
    /// typing contract covers, with data authored against it. Record `e1`
    /// conforms throughout (a date under `date`, an integral float under
    /// `integer`, an integer under `decimal`, a float under `float`, an
    /// integer at a rangeless slot); record `e2` carries the values a
    /// primitive range cannot faithfully type (a malformed date, `NaN`
    /// under `decimal`, an f32-overflowing value under `float`) alongside
    /// `e1`'s wrong-kinded integer under `string`.
    fn typed_abox_fixture() -> (SchemaDefinition, crate::instances::InstanceSet) {
        let mut schema = SchemaDefinition::new("cellar");
        schema.id = Some("https://example.org/cellar".to_string());
        schema.default_prefix = Some("cellar".to_string());
        schema.prefixes.insert(
            "cellar".to_string(),
            "https://example.org/cellar/".to_string(),
        );
        let mut container = ClassDefinition::new("Ledger");
        container.tree_root = true;
        let mut events = SlotDefinition::new("events");
        events.range = Some("Event".to_string());
        events.multivalued = true;
        container.attributes.insert("events".to_string(), events);
        schema.classes.insert("Ledger".to_string(), container);

        let mut event = ClassDefinition::new("Event");
        let mut id = SlotDefinition::new("id");
        id.identifier = true;
        event.attributes.insert("id".to_string(), id);
        for (name, range) in [
            ("on", Some("date")),
            ("label", Some("string")),
            ("count", Some("integer")),
            ("weight", Some("decimal")),
            ("ratio", Some("float")),
            ("note", None),
        ] {
            let mut slot = SlotDefinition::new(name);
            slot.range = range.map(str::to_string);
            event.attributes.insert(name.to_string(), slot);
        }
        schema.classes.insert("Event".to_string(), event);

        let data: serde_norway::Value = serde_norway::from_str(
            "events:\n  - id: e1\n    on: 2024-06-01\n    label: 42\n    count: 5.0\n    weight: 4\n    ratio: 2.5\n    note: 7\n  - id: e2\n    on: tomorrow\n    weight: .nan\n    ratio: 1.0e300\n",
        )
        .unwrap();
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        (schema, set)
    }

    /// The typed fixture's predicate IRI for `slot`.
    fn cellar(slot: &str) -> String {
        format!("https://example.org/cellar/{slot}")
    }

    #[test]
    fn a_date_ranged_slots_value_carries_the_shapes_datatype() {
        // A YAML date is string-kinded, but the slot's range is `date` and
        // the same run's shapes say `sh:datatype xsd:date` — an xsd:string
        // literal would fail the shapes the schema itself emitted.
        let (schema, set) = typed_abox_fixture();
        let graph = build_rdf_graph_with_instances(&schema, Some(&set)).expect("graph");
        assert_eq!(
            literal_parts(&graph, &cellar("e1"), &cellar("on")),
            vec![(
                "2024-06-01".to_string(),
                "http://www.w3.org/2001/XMLSchema#date".to_string()
            )],
            "a conforming value is typed by the slot's range, not its parse kind"
        );
    }

    #[test]
    fn a_wrong_kinded_value_emits_as_authored_for_the_shapes_to_reject() {
        // `label: 42` at a string range is a conformance violation. The
        // triple still emits, in the value's own kind — the shapes'
        // `sh:datatype xsd:string` then reports it, nothing vanishes from
        // the output, and a required slot's `sh:minCount` stays satisfied.
        let (schema, set) = typed_abox_fixture();
        let graph = build_rdf_graph_with_instances(&schema, Some(&set)).expect("graph");
        assert_eq!(
            literal_parts(&graph, &cellar("e1"), &cellar("label")),
            vec![(
                "42".to_string(),
                "http://www.w3.org/2001/XMLSchema#integer".to_string()
            )],
            "a nonconforming value stays present, typed as authored, for the shapes to flag"
        );
    }

    #[test]
    fn a_malformed_date_emits_as_a_well_formed_string_for_the_shapes_to_reject() {
        // `on: tomorrow` is string-kinded but outside xsd:date's lexical
        // space: stamping the range's datatype would mint an ill-formed
        // literal some stores reject at load. As authored xsd:string it is
        // well-formed RDF that visibly fails the shapes' sh:datatype.
        let (schema, set) = typed_abox_fixture();
        let graph = build_rdf_graph_with_instances(&schema, Some(&set)).expect("graph");
        assert_eq!(
            literal_parts(&graph, &cellar("e2"), &cellar("on")),
            vec![(
                "tomorrow".to_string(),
                "http://www.w3.org/2001/XMLSchema#string".to_string()
            )],
        );
    }

    #[test]
    fn a_non_finite_decimal_value_stays_present_as_its_authored_kind() {
        // xsd:decimal has no lexical form for NaN, and the kind check
        // cannot flag the value (a float is a valid decimal kind) — so
        // dropping the triple would be silent data loss on every path.
        // As authored xsd:double the value is present and the shapes'
        // sh:datatype xsd:decimal reports the mismatch.
        let (schema, set) = typed_abox_fixture();
        let graph = build_rdf_graph_with_instances(&schema, Some(&set)).expect("graph");
        assert_eq!(
            literal_parts(&graph, &cellar("e2"), &cellar("weight")),
            vec![(
                "NaN".to_string(),
                "http://www.w3.org/2001/XMLSchema#double".to_string()
            )],
        );
    }

    #[test]
    fn an_f32_overflowing_float_value_keeps_its_authored_double_typing() {
        // 1e300 fits xsd:double but overflows xsd:float's single-precision
        // value space — a conforming processor would read it back as INF.
        // The authored double typing preserves the value; the shapes'
        // sh:datatype xsd:float reports that the slot can't hold it.
        let (schema, set) = typed_abox_fixture();
        let graph = build_rdf_graph_with_instances(&schema, Some(&set)).expect("graph");
        let parts = literal_parts(&graph, &cellar("e2"), &cellar("ratio"));
        let [(lexical, datatype)] = parts.as_slice() else {
            panic!("the overflowing value must still emit exactly one literal; got: {parts:?}");
        };
        assert_eq!(datatype, "http://www.w3.org/2001/XMLSchema#double");
        assert_eq!(
            lexical.parse::<f64>().expect("a numeric lexical"),
            1e300,
            "the value survives, exactly"
        );
    }

    #[test]
    fn a_conforming_float_emits_the_shapes_xsd_float() {
        let (schema, set) = typed_abox_fixture();
        let graph = build_rdf_graph_with_instances(&schema, Some(&set)).expect("graph");
        assert_eq!(
            literal_parts(&graph, &cellar("e1"), &cellar("ratio")),
            vec![(
                "2.5".to_string(),
                "http://www.w3.org/2001/XMLSchema#float".to_string()
            )],
        );
    }

    #[test]
    fn an_integral_float_under_an_integer_range_emits_canonical_xsd_integer() {
        // `count: 5.0` conforms to `integer` under number semantics, but
        // `"5.0"^^xsd:integer` is an ill-formed literal — the lexical form
        // must be canonical for the emitted datatype.
        let (schema, set) = typed_abox_fixture();
        let graph = build_rdf_graph_with_instances(&schema, Some(&set)).expect("graph");
        assert_eq!(
            literal_parts(&graph, &cellar("e1"), &cellar("count")),
            vec![(
                "5".to_string(),
                "http://www.w3.org/2001/XMLSchema#integer".to_string()
            )],
            "the integral float takes integer's lexical space"
        );
    }

    #[test]
    fn a_decimal_ranged_integer_emits_xsd_decimal() {
        // The shapes for a decimal range say `sh:datatype xsd:decimal`;
        // collapsing to xsd:double contradicts them.
        let (schema, set) = typed_abox_fixture();
        let graph = build_rdf_graph_with_instances(&schema, Some(&set)).expect("graph");
        assert_eq!(
            literal_parts(&graph, &cellar("e1"), &cellar("weight")),
            vec![(
                "4".to_string(),
                "http://www.w3.org/2001/XMLSchema#decimal".to_string()
            )],
        );
    }

    #[test]
    fn a_rangeless_slots_value_keeps_its_value_kind_typing() {
        // With no range and no `default_range` there is no shape constraint
        // to agree with, so the value's own kind is the only typing there
        // is: the fallback neither vanishes nor fabricates a datatype.
        let (schema, set) = typed_abox_fixture();
        let graph = build_rdf_graph_with_instances(&schema, Some(&set)).expect("graph");
        assert_eq!(
            literal_parts(&graph, &cellar("e1"), &cellar("note")),
            vec![(
                "7".to_string(),
                "http://www.w3.org/2001/XMLSchema#integer".to_string()
            )],
        );
    }

    #[test]
    fn an_enum_named_like_a_primitive_never_takes_the_datatype_path() {
        // The shapes writer checks enum ranges before datatypes, so an enum
        // named `date` gets `sh:in` over value IRIs and no `sh:datatype`.
        // The A-box takes the same branch order: a non-permitted value
        // emits as the authored string, never as an ill-formed xsd:date.
        let mut schema = SchemaDefinition::new("cellar");
        schema.id = Some("https://example.org/cellar".to_string());
        schema.default_prefix = Some("cellar".to_string());
        schema.prefixes.insert(
            "cellar".to_string(),
            "https://example.org/cellar/".to_string(),
        );
        let mut phases = crate::linkml::EnumDefinition::new("date");
        phases.permissible_values.insert(
            "start".to_string(),
            crate::linkml::PermissibleValue::new("start"),
        );
        schema.enums.insert("date".to_string(), phases);
        let mut container = ClassDefinition::new("Ledger");
        container.tree_root = true;
        let mut events = SlotDefinition::new("events");
        events.range = Some("Event".to_string());
        events.multivalued = true;
        container.attributes.insert("events".to_string(), events);
        schema.classes.insert("Ledger".to_string(), container);
        let mut event = ClassDefinition::new("Event");
        let mut id = SlotDefinition::new("id");
        id.identifier = true;
        event.attributes.insert("id".to_string(), id);
        let mut phase = SlotDefinition::new("phase");
        phase.range = Some("date".to_string());
        event.attributes.insert("phase".to_string(), phase);
        schema.classes.insert("Event".to_string(), event);

        let data: serde_norway::Value =
            serde_norway::from_str("events:\n  - id: e1\n    phase: someday\n").unwrap();
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let graph = build_rdf_graph_with_instances(&schema, Some(&set)).expect("graph");
        assert_eq!(
            literal_parts(&graph, &cellar("e1"), &cellar("phase")),
            vec![(
                "someday".to_string(),
                "http://www.w3.org/2001/XMLSchema#string".to_string()
            )],
            "the enum branch wins the name collision, as it does in the shapes"
        );
    }

    #[test]
    fn abox_literal_datatype_and_sh_datatype_agree_for_the_same_slot() {
        // The contract behind all of the above: the A-box literal's datatype
        // and the shapes' `sh:datatype` for one slot come from the same
        // derivation, so `generate` cannot contradict itself in one run.
        use sophia::api::graph::Graph;
        use sophia::api::term::Term;
        use sophia::api::triple::Triple;
        let (schema, set) = typed_abox_fixture();
        let abox = build_rdf_graph_with_instances(&schema, Some(&set)).expect("graph");
        let shapes = build_shacl_graph(&schema).expect("shapes");
        let sh_datatype = make_iri("http://www.w3.org/ns/shacl#datatype").unwrap();
        let shape_dt: Vec<String> = shapes
            .triples_matching(
                sophia::api::term::matcher::Any,
                [sh_datatype],
                sophia::api::term::matcher::Any,
            )
            .filter_map(|t| {
                let t = t.unwrap();
                // Property shapes carry their path IRI in the shape IRI; the
                // `on` slot's shape is the one whose subject ends in /on.
                let subj = t.s().iri()?.to_string();
                subj.ends_with("/on")
                    .then(|| t.o().iri().expect("iri").to_string())
            })
            .collect();
        let abox_dt: Vec<String> = literal_parts(&abox, &cellar("e1"), &cellar("on"))
            .into_iter()
            .map(|(_, datatype)| datatype)
            .collect();
        assert_eq!(
            shape_dt, abox_dt,
            "the shape's sh:datatype and the emitted literal's datatype must be one value"
        );
    }

    /// A schema whose `qualifies` slot ranges over an `any_of` union of two
    /// classes — the T-box and A-box shapes a union carries.
    fn union_tbox_fixture() -> SchemaDefinition {
        let mut schema = SchemaDefinition::new("prov");
        schema.id = Some("https://example.org/prov".to_string());
        schema.default_prefix = Some("prov".to_string());
        schema
            .prefixes
            .insert("prov".to_string(), "https://example.org/prov/".to_string());
        schema.default_range = Some("string".to_string());

        let mut id = SlotDefinition::new("id");
        id.identifier = true;
        let mut qualifies = SlotDefinition::new("qualifies");
        let mut claim_branch = SlotDefinition::new("");
        claim_branch.range = Some("Claim".to_string());
        let mut method_branch = SlotDefinition::new("");
        method_branch.range = Some("Method".to_string());
        qualifies.any_of = vec![claim_branch, method_branch];

        let mut root = ClassDefinition::new("Root");
        root.tree_root = true;
        for (name, range) in [("states", "State"), ("claims", "Claim")] {
            let mut s = SlotDefinition::new(name);
            s.range = Some(range.to_string());
            s.multivalued = true;
            root.attributes.insert(name.to_string(), s);
        }
        schema.classes.insert("Root".to_string(), root);

        let mut state = ClassDefinition::new("State");
        state.attributes.insert("id".to_string(), id.clone());
        state.attributes.insert("qualifies".to_string(), qualifies);
        schema.classes.insert("State".to_string(), state);

        let mut claim = ClassDefinition::new("Claim");
        claim.attributes.insert("id".to_string(), id.clone());
        schema.classes.insert("Claim".to_string(), claim);

        // Both union members must be declared classes; an undeclared member
        // would make this a mixed union, whose strings stay literals.
        let mut method = ClassDefinition::new("Method");
        method.attributes.insert("id".to_string(), id);
        schema.classes.insert("Method".to_string(), method);

        schema
    }

    /// Whether `graph` holds a triple with this subject/predicate/object IRI.
    fn has_iri_triple(graph: &FastGraph, subject: &str, predicate: &str, object: &str) -> bool {
        use sophia::api::graph::Graph;
        use sophia::api::term::Term;
        use sophia::api::triple::Triple;
        graph.triples().filter_map(Result::ok).any(|t| {
            t.s().iri().is_some_and(|i| i.as_str() == subject)
                && t.p().iri().is_some_and(|i| i.as_str() == predicate)
                && t.o().iri().is_some_and(|i| i.as_str() == object)
        })
    }

    /// A slot specializing another (slot-level `is_a`) emits
    /// `rdfs:subPropertyOf`, so RDF consumers see the subset relation the
    /// schema states — every value of the child is also a value of the
    /// parent.
    #[test]
    fn a_slot_specializing_another_emits_sub_property_of() {
        let mut schema = SchemaDefinition::new("s");
        schema.id = Some("https://example.org/s".to_string());
        let mut anchors = SlotDefinition::new("expected_anchors");
        anchors.range = Some("string".to_string());
        schema.slots.insert("expected_anchors".to_string(), anchors);
        let mut citations = SlotDefinition::new("expected_citations");
        citations.range = Some("string".to_string());
        citations.is_a = Some("expected_anchors".to_string());
        schema
            .slots
            .insert("expected_citations".to_string(), citations);

        let graph = build_rdf_graph(&schema).expect("build graph");
        assert!(
            has_iri_triple(
                &graph,
                "https://example.org/s#expected_citations",
                "http://www.w3.org/2000/01/rdf-schema#subPropertyOf",
                "https://example.org/s#expected_anchors",
            ),
            "the specializing slot must state rdfs:subPropertyOf its parent"
        );
    }

    /// A parent declared as a class attribute resolves to the IRI that
    /// attribute's own emission uses — its `slot_uri` — never a minted
    /// fallback, so the `rdfs:subPropertyOf` edge lands on a property the
    /// graph actually declares.
    #[test]
    fn a_specialization_of_an_attribute_parent_targets_its_declared_iri() {
        let mut schema = SchemaDefinition::new("s");
        schema.id = Some("https://example.org/s".to_string());
        let mut item = ClassDefinition::new("Item");
        let mut parent = SlotDefinition::new("relation");
        parent.slot_uri = Some("http://purl.org/dc/terms/relation".to_string());
        item.attributes.insert("relation".to_string(), parent);
        schema.classes.insert("Item".to_string(), item);
        let mut child = SlotDefinition::new("cites");
        child.is_a = Some("relation".to_string());
        schema.slots.insert("cites".to_string(), child);

        let graph = build_rdf_graph(&schema).expect("build graph");
        assert!(
            has_iri_triple(
                &graph,
                "https://example.org/s#cites",
                "http://www.w3.org/2000/01/rdf-schema#subPropertyOf",
                "http://purl.org/dc/terms/relation",
            ),
            "the parent IRI must be the attribute's declared slot_uri"
        );
    }

    /// The `owl:versionIRI` joins the schema `id` and version with exactly
    /// one slash, whichever way the `id` is spelled — an empty path segment
    /// is significant in a URI, so `…/wine//0.2.0` and `…/wine/0.2.0` are
    /// different resources and anything resolving or joining on the version
    /// IRI would miss.
    #[test]
    fn version_iri_joins_with_a_single_slash_whatever_the_id_spelling() {
        let version_iri_for = |id: &str| {
            let mut schema = SchemaDefinition::new("wine");
            schema.id = Some(id.to_string());
            schema.version = Some("0.2.0".to_string());
            let graph = build_rdf_graph(&schema).expect("build graph");
            objects_of(&graph, id, "http://www.w3.org/2002/07/owl#versionIRI")
        };
        assert_eq!(
            version_iri_for("https://example.org/wine/"),
            vec!["https://example.org/wine/0.2.0".to_string()],
            "a slash-ended id must not yield a doubled slash"
        );
        assert_eq!(
            version_iri_for("https://example.org/wine"),
            vec!["https://example.org/wine/0.2.0".to_string()],
            "a bare id gains the one separator"
        );
    }

    /// A top-level slot typed only by `default_range` still emits its
    /// `rdfs:range`: loading materializes the default into the slot
    /// definition, and the writer emits what the definition carries —
    /// whichever path (top-level `slots:` or class attributes) a property
    /// arrives by.
    #[test]
    fn a_default_ranged_top_level_slot_emits_its_range() {
        let mut schema = SchemaDefinition::new("s");
        schema.id = Some("https://example.org/s".to_string());
        schema.default_range = Some("string".to_string());
        schema
            .slots
            .insert("question".to_string(), SlotDefinition::new("question"));
        let mut item = ClassDefinition::new("Item");
        item.slots.push("question".to_string());
        schema.classes.insert("Item".to_string(), item);
        crate::linkml_resolve::materialize_default_range(&mut schema);

        let graph = build_rdf_graph(&schema).expect("build graph");
        let ranges = objects_of(
            &graph,
            "https://example.org/s#question",
            "http://www.w3.org/2000/01/rdf-schema#range",
        );
        assert_eq!(
            ranges,
            vec!["http://www.w3.org/2001/XMLSchema#string".to_string()],
            "the defaulted slot should carry xsd:string"
        );
    }

    /// The object IRIs of every `subject predicate ?o` triple.
    fn objects_of(graph: &FastGraph, subject: &str, predicate: &str) -> Vec<String> {
        use sophia::api::graph::Graph;
        use sophia::api::term::Term;
        use sophia::api::triple::Triple;
        graph
            .triples()
            .filter_map(Result::ok)
            .filter(|t| {
                t.s().iri().is_some_and(|i| i.as_str() == subject)
                    && t.p().iri().is_some_and(|i| i.as_str() == predicate)
            })
            .filter_map(|t| t.o().iri().map(|i| i.to_string()))
            .collect()
    }

    /// A schema with one enum and a slot ranged on it.
    fn enum_fixture() -> SchemaDefinition {
        use crate::linkml::{EnumDefinition, PermissibleValue};
        let mut schema = SchemaDefinition::new("orders");
        schema.id = Some("https://example.org/orders".to_string());
        schema.default_prefix = Some("orders".to_string());
        schema.prefixes.insert(
            "orders".to_string(),
            "https://example.org/orders/".to_string(),
        );
        let mut status = EnumDefinition::new("OrderStatus");
        status.description = Some("How far along an order is".to_string());
        for (key, desc) in [("open", "Not yet shipped"), ("shipped", "On its way")] {
            let mut pv = PermissibleValue::new(key);
            pv.description = Some(desc.to_string());
            status.permissible_values.insert(key.to_string(), pv);
        }
        // A value whose display text differs from its map key, so the two
        // ways of naming a permissible value are distinguishable.
        let mut on_hold = PermissibleValue::new("On hold");
        on_hold.description = Some("Paused by the customer".to_string());
        status
            .permissible_values
            .insert("on_hold".to_string(), on_hold);
        schema.enums.insert("OrderStatus".to_string(), status);

        let mut order = ClassDefinition::new("Order");
        let mut id = SlotDefinition::new("id");
        id.identifier = true;
        order.attributes.insert("id".to_string(), id);
        let mut slot = SlotDefinition::new("status");
        slot.range = Some("OrderStatus".to_string());
        order.attributes.insert("status".to_string(), slot);
        schema.classes.insert("Order".to_string(), order);

        let mut root = ClassDefinition::new("Book");
        root.tree_root = true;
        let mut orders = SlotDefinition::new("orders");
        orders.range = Some("Order".to_string());
        orders.multivalued = true;
        root.attributes.insert("orders".to_string(), orders);
        schema.classes.insert("Book".to_string(), root);
        schema
    }

    #[test]
    fn an_enum_becomes_a_class_whose_permissible_values_are_individuals() {
        // Enums were absent from RDF entirely: a consumer loading the
        // ontology could not see what values a status may take.
        let graph = build_rdf_graph(&enum_fixture()).expect("graph");
        let enum_iri = "https://example.org/orders/OrderStatus";
        let rdf_type = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
        assert!(
            has_iri_triple(
                &graph,
                enum_iri,
                rdf_type,
                "http://www.w3.org/2002/07/owl#Class"
            ),
            "the enum is a class"
        );
        for value in ["open", "shipped"] {
            let value_iri = format!("https://example.org/orders/OrderStatus/{value}");
            assert!(
                has_iri_triple(
                    &graph,
                    &value_iri,
                    rdf_type,
                    "http://www.w3.org/2002/07/owl#NamedIndividual"
                ) && has_iri_triple(&graph, &value_iri, rdf_type, enum_iri),
                "`{value}` is a named individual of the enum"
            );
        }
        let one_of = objects_of(&graph, enum_iri, "http://www.w3.org/2002/07/owl#oneOf");
        assert_eq!(
            one_of.len(),
            1,
            "the enum enumerates its values; got: {one_of:?}"
        );
    }

    #[test]
    fn an_enum_ranged_slot_declares_an_object_property() {
        // The enum is a class whose values are individuals, so a slot ranged
        // on it relates individuals — a datatype property with a class range
        // is a contradiction.
        let graph = build_rdf_graph(&enum_fixture()).expect("graph");
        let status = "https://example.org/orders/status";
        let rdf_type = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";
        assert!(
            has_iri_triple(
                &graph,
                status,
                rdf_type,
                "http://www.w3.org/2002/07/owl#ObjectProperty"
            ),
            "an enum-ranged slot is an object property"
        );
        assert!(
            !has_iri_triple(
                &graph,
                status,
                rdf_type,
                "http://www.w3.org/2002/07/owl#DatatypeProperty"
            ),
            "and not also a datatype property"
        );
    }

    #[test]
    fn an_individuals_enum_value_asserts_the_value_individual() {
        // The A-box must point at the enum value's individual, not repeat the
        // value as a literal — otherwise it contradicts the object property.
        let schema = enum_fixture();
        let data: serde_norway::Value =
            serde_norway::from_str("orders:\n  - {id: o1, status: shipped}\n").unwrap();
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let graph = build_rdf_graph_with_instances(&schema, Some(&set)).expect("graph");
        assert!(
            has_iri_triple(
                &graph,
                "https://example.org/orders/o1",
                "https://example.org/orders/status",
                "https://example.org/orders/OrderStatus/shipped"
            ),
            "the assertion names the value's individual"
        );
        use sophia::api::graph::Graph;
        use sophia::api::term::Term;
        use sophia::api::triple::Triple;
        let literal_kept = graph.triples().filter_map(Result::ok).any(|t| {
            t.p()
                .iri()
                .is_some_and(|i| i.as_str() == "https://example.org/orders/status")
                && t.o().lexical_form().is_some_and(|l| l == "shipped")
        });
        assert!(!literal_kept, "and does not also assert the bare literal");
    }

    #[test]
    fn an_enum_value_resolves_by_either_its_key_or_its_text() {
        // A permissible value can be named in data by its map key or by its
        // display text; both must reach the same individual.
        let schema = enum_fixture();
        for authored in ["on_hold", "On hold"] {
            let data: serde_norway::Value =
                serde_norway::from_str(&format!("orders:\n  - {{id: o1, status: {authored}}}\n"))
                    .unwrap();
            let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
            let graph = build_rdf_graph_with_instances(&schema, Some(&set)).expect("graph");
            assert!(
                has_iri_triple(
                    &graph,
                    "https://example.org/orders/o1",
                    "https://example.org/orders/status",
                    "https://example.org/orders/OrderStatus/on_hold"
                ),
                "`{authored}` must resolve to the value's individual"
            );
        }
    }

    #[test]
    fn enumerated_value_list_cells_are_not_named_as_unions() {
        // The list cells are addressable IRIs, so their names are read by
        // people; an `owl:oneOf` list is not a union.
        let graph = build_rdf_graph(&enum_fixture()).expect("graph");
        let cells = objects_of(
            &graph,
            "https://example.org/orders/OrderStatus",
            "http://www.w3.org/2002/07/owl#oneOf",
        );
        assert_eq!(cells.len(), 1, "got: {cells:?}");
        assert!(
            !cells[0].contains("union"),
            "an enumeration's cells must not read as a union; got: {}",
            cells[0]
        );
    }

    #[test]
    fn an_enum_ranged_slot_ranges_over_the_enum_class() {
        let graph = build_rdf_graph(&enum_fixture()).expect("graph");
        assert!(
            has_iri_triple(
                &graph,
                "https://example.org/orders/status",
                "http://www.w3.org/2000/01/rdf-schema#range",
                "https://example.org/orders/OrderStatus"
            ),
            "an enum-ranged slot ranges over the enum's class"
        );
    }

    #[test]
    fn a_union_of_classes_declares_an_object_property() {
        // The A-box asserts IRI objects for this slot, so the T-box must
        // declare it an object property — a datatype declaration alongside
        // IRI objects is a contradiction an OWL reasoner will reject.
        let graph = build_rdf_graph(&union_tbox_fixture()).expect("graph");
        let qualifies = "https://example.org/prov/qualifies";
        assert!(
            has_iri_triple(
                &graph,
                qualifies,
                "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
                "http://www.w3.org/2002/07/owl#ObjectProperty"
            ),
            "a union of classes is an object property"
        );
        assert!(
            !has_iri_triple(
                &graph,
                qualifies,
                "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
                "http://www.w3.org/2002/07/owl#DatatypeProperty"
            ),
            "and must not also be declared a datatype property"
        );
    }

    #[test]
    fn a_union_of_classes_ranges_over_an_owl_union() {
        // `rdfs:range` names a class expression whose `owl:unionOf` list
        // holds every member, so a consumer can see what the slot accepts.
        let graph = build_rdf_graph(&union_tbox_fixture()).expect("graph");
        let ranges = objects_of(
            &graph,
            "https://example.org/prov/qualifies",
            "http://www.w3.org/2000/01/rdf-schema#range",
        );
        assert_eq!(
            ranges.len(),
            1,
            "one range, a union expression; got: {ranges:?}"
        );
        let union_node = &ranges[0];
        assert!(
            has_iri_triple(
                &graph,
                union_node,
                "http://www.w3.org/1999/02/22-rdf-syntax-ns#type",
                "http://www.w3.org/2002/07/owl#Class"
            ),
            "the range is a class expression"
        );
        let list_head = objects_of(&graph, union_node, "http://www.w3.org/2002/07/owl#unionOf");
        assert_eq!(list_head.len(), 1, "one union list; got: {list_head:?}");

        // Walk the RDF list and collect its members.
        let mut members = Vec::new();
        let mut cell = list_head[0].clone();
        let nil = "http://www.w3.org/1999/02/22-rdf-syntax-ns#nil";
        // Bounded so a malformed list fails the assertion rather than hanging.
        for _ in 0..8 {
            if cell == nil {
                break;
            }
            members.extend(objects_of(
                &graph,
                &cell,
                "http://www.w3.org/1999/02/22-rdf-syntax-ns#first",
            ));
            let rest = objects_of(
                &graph,
                &cell,
                "http://www.w3.org/1999/02/22-rdf-syntax-ns#rest",
            );
            assert_eq!(rest.len(), 1, "a well-formed list cell has one rest");
            cell = rest[0].clone();
        }
        assert_eq!(cell, nil, "the list terminates");
        members.sort();
        assert_eq!(
            members,
            vec![
                "https://example.org/prov/Claim".to_string(),
                "https://example.org/prov/Method".to_string()
            ],
            "the union lists both member classes"
        );
    }

    #[test]
    fn a_union_ranged_slot_emits_an_object_property_assertion() {
        // A slot whose range is an `any_of` class union carries references,
        // not literals — so the A-box must contain an object-property
        // assertion between the two individuals, not a string.
        let schema = union_tbox_fixture();
        let data: serde_norway::Value =
            serde_norway::from_str("states:\n  - {id: s1, qualifies: c1}\nclaims:\n  - {id: c1}\n")
                .unwrap();
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        let graph = build_rdf_graph_with_instances(&schema, Some(&set)).expect("graph");

        use sophia::api::graph::Graph;
        use sophia::api::triple::Triple;
        let subject = make_iri("https://example.org/prov/s1").unwrap();
        let predicate = make_iri("https://example.org/prov/qualifies").unwrap();
        let linked = graph
            .triples_matching([subject], [predicate], sophia::api::term::matcher::Any)
            .filter_map(Result::ok)
            .any(|t| {
                use sophia::api::term::Term;
                t.o()
                    .iri()
                    .is_some_and(|i| i.to_string().contains("prov/c1"))
            });
        assert!(
            linked,
            "a union-ranged slot must assert an object property to the referenced individual"
        );
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
