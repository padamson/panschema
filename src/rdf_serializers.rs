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
use crate::linkml::SchemaDefinition;

// Namespace constants
const OWL_NS: &str = "http://www.w3.org/2002/07/owl#";
const DCTERMS_NS: &str = "http://purl.org/dc/terms/";

/// Build an RDF graph from a SchemaDefinition
pub fn build_rdf_graph(schema: &SchemaDefinition) -> IoResult<FastGraph> {
    let mut graph = FastGraph::new();

    let owl = Namespace::new_unchecked(OWL_NS);
    let dcterms = Namespace::new_unchecked(DCTERMS_NS);

    // Ontology IRI
    let ontology_iri_str = schema
        .id
        .as_deref()
        .unwrap_or("http://example.org/ontology");
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
    let rdfs_subclass_of = rdfs::subClassOf;

    for (name, class_def) in &schema.classes {
        let class_iri_str = class_def
            .class_uri
            .clone()
            .unwrap_or_else(|| format!("{}#{}", ontology_iri_str, name));
        let class_iri = make_iri(&class_iri_str)?;

        // rdf:type owl:Class
        graph
            .insert(&class_iri, rdf::type_, owl_class)
            .map_err(|e| IoError::Write(e.to_string()))?;

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

        // rdfs:subClassOf from is_a
        if let Some(ref parent) = class_def.is_a {
            let parent_iri_str = schema
                .classes
                .get(parent)
                .and_then(|c| c.class_uri.clone())
                .unwrap_or_else(|| format!("{}#{}", ontology_iri_str, parent));
            let parent_iri = make_iri(&parent_iri_str)?;
            graph
                .insert(&class_iri, rdfs_subclass_of, &parent_iri)
                .map_err(|e| IoError::Write(e.to_string()))?;
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

    for (name, slot_def) in &schema.slots {
        let prop_iri_str = slot_def
            .slot_uri
            .clone()
            .unwrap_or_else(|| format!("{}#{}", ontology_iri_str, name));
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

        // rdfs:domain
        if let Some(ref domain) = slot_def.domain {
            let domain_iri_str = schema
                .classes
                .get(domain)
                .and_then(|c| c.class_uri.clone())
                .unwrap_or_else(|| format!("{}#{}", ontology_iri_str, domain));
            let domain_iri = make_iri(&domain_iri_str)?;
            graph
                .insert(&prop_iri, rdfs::domain, &domain_iri)
                .map_err(|e| IoError::Write(e.to_string()))?;
        }

        // rdfs:range
        if let Some(ref range) = slot_def.range {
            let range_iri_str = if is_object_property {
                schema
                    .classes
                    .get(range)
                    .and_then(|c| c.class_uri.clone())
                    .unwrap_or_else(|| format!("{}#{}", ontology_iri_str, range))
            } else {
                map_linkml_to_xsd(range)
            };
            let range_iri = make_iri(&range_iri_str)?;
            graph
                .insert(&prop_iri, rdfs::range, &range_iri)
                .map_err(|e| IoError::Write(e.to_string()))?;
        }

        // owl:inverseOf
        if let Some(ref inverse) = slot_def.inverse {
            let inverse_iri_str = schema
                .slots
                .get(inverse)
                .and_then(|s| s.slot_uri.clone())
                .unwrap_or_else(|| format!("{}#{}", ontology_iri_str, inverse));
            let inverse_iri = make_iri(&inverse_iri_str)?;
            graph
                .insert(&prop_iri, owl_inverse_of, &inverse_iri)
                .map_err(|e| IoError::Write(e.to_string()))?;
        }
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

/// Map LinkML types to XSD IRIs
fn map_linkml_to_xsd(linkml_type: &str) -> String {
    let xsd_ns = "http://www.w3.org/2001/XMLSchema#";
    match linkml_type {
        "string" => format!("{}string", xsd_ns),
        "integer" => format!("{}integer", xsd_ns),
        "float" => format!("{}float", xsd_ns),
        "double" => format!("{}double", xsd_ns),
        "boolean" => format!("{}boolean", xsd_ns),
        "date" => format!("{}date", xsd_ns),
        "datetime" => format!("{}dateTime", xsd_ns),
        "time" => format!("{}time", xsd_ns),
        "uri" => format!("{}anyURI", xsd_ns),
        _ => format!("{}{}", xsd_ns, linkml_type),
    }
}

// ============================================================================
// JSON-LD Writer
// ============================================================================

/// Writer for JSON-LD format
pub struct JsonLdWriter;

impl JsonLdWriter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for JsonLdWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer for JsonLdWriter {
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()> {
        let graph = build_rdf_graph(schema)?;

        use sophia::jsonld::serializer::JsonLdSerializer;

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
pub struct RdfXmlWriter;

impl RdfXmlWriter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RdfXmlWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer for RdfXmlWriter {
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()> {
        let graph = build_rdf_graph(schema)?;

        use sophia::xml::serializer::RdfXmlSerializer;

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
pub struct NTriplesWriter;

impl NTriplesWriter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NTriplesWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer for NTriplesWriter {
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()> {
        let graph = build_rdf_graph(schema)?;

        use sophia::turtle::serializer::nt::NtSerializer;

        let file = File::create(output).map_err(IoError::Io)?;
        let writer = BufWriter::new(file);

        let mut serializer = NtSerializer::new(writer);

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
}
