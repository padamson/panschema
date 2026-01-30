//! OWL/Turtle Writer
//!
//! Writes LinkML IR to OWL ontologies in Turtle format using sophia.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use sophia::api::serializer::TripleSerializer;
use sophia::turtle::serializer::turtle::TurtleSerializer;

use crate::io::{IoError, IoResult, Writer};
use crate::linkml::SchemaDefinition;
use crate::rdf_serializers::build_rdf_graph;

/// Writer for OWL ontologies in Turtle (.ttl) format
pub struct OwlWriter;

impl OwlWriter {
    /// Create a new OWL writer
    pub fn new() -> Self {
        Self
    }
}

impl Default for OwlWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer for OwlWriter {
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()> {
        let graph = build_rdf_graph(schema)?;

        let file = File::create(output).map_err(IoError::Io)?;
        let writer = BufWriter::new(file);

        let mut serializer = TurtleSerializer::new(writer);

        serializer
            .serialize_graph(&graph)
            .map_err(|e| IoError::Write(format!("Turtle serialization failed: {}", e)))?;

        Ok(())
    }

    fn format_id(&self) -> &str {
        "ttl"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::Reader;
    use crate::linkml::{ClassDefinition, Contributor, SlotDefinition};
    use std::fs;
    use tempfile::TempDir;

    fn create_test_schema() -> SchemaDefinition {
        let mut schema = SchemaDefinition::new("test");
        schema.id = Some("http://example.org/test".to_string());
        schema.title = Some("Test Ontology".to_string());
        schema.description = Some("A test ontology.".to_string());
        schema.version = Some("1.0.0".to_string());
        schema
    }

    // ========== Basic Writer Tests ==========

    #[test]
    fn owl_writer_format_id_is_ttl() {
        let writer = OwlWriter::new();
        assert_eq!(writer.format_id(), "ttl");
    }

    #[test]
    fn owl_writer_has_default() {
        let writer = OwlWriter;
        assert_eq!(writer.format_id(), "ttl");
    }

    #[test]
    fn owl_writer_creates_file() {
        let schema = create_test_schema();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");

        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        assert!(output_path.exists());
    }

    // ========== Content Tests (semantic, not exact string matching) ==========

    #[test]
    fn output_contains_ontology_declaration() {
        let schema = create_test_schema();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");

        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        let content = fs::read_to_string(&output_path).expect("Failed to read");
        assert!(content.contains("http://example.org/test"));
        assert!(
            content.contains("owl:Ontology")
                || content.contains("http://www.w3.org/2002/07/owl#Ontology")
        );
    }

    #[test]
    fn output_contains_title_as_label() {
        let schema = create_test_schema();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");

        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        let content = fs::read_to_string(&output_path).expect("Failed to read");
        assert!(content.contains("Test Ontology"));
    }

    #[test]
    fn output_contains_description_as_comment() {
        let schema = create_test_schema();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");

        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        let content = fs::read_to_string(&output_path).expect("Failed to read");
        assert!(content.contains("A test ontology."));
    }

    #[test]
    fn output_contains_version() {
        let schema = create_test_schema();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");

        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        let content = fs::read_to_string(&output_path).expect("Failed to read");
        assert!(content.contains("1.0.0"));
    }

    #[test]
    fn output_contains_version_iri() {
        let schema = create_test_schema();
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");

        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        let content = fs::read_to_string(&output_path).expect("Failed to read");
        assert!(content.contains("http://example.org/test/1.0.0"));
    }

    #[test]
    fn output_contains_license() {
        let mut schema = create_test_schema();
        schema.license = Some("https://creativecommons.org/licenses/by/4.0/".to_string());

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");

        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        let content = fs::read_to_string(&output_path).expect("Failed to read");
        assert!(content.contains("https://creativecommons.org/licenses/by/4.0/"));
    }

    #[test]
    fn output_contains_creators() {
        let mut schema = create_test_schema();
        schema.contributors.push(Contributor::new("Jane Doe"));
        schema.contributors.push(Contributor::new("John Smith"));

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");

        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        let content = fs::read_to_string(&output_path).expect("Failed to read");
        assert!(content.contains("Jane Doe"));
        assert!(content.contains("John Smith"));
    }

    #[test]
    fn output_contains_created_date() {
        let mut schema = create_test_schema();
        schema.created = Some("2025-01-15".to_string());

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");

        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        let content = fs::read_to_string(&output_path).expect("Failed to read");
        assert!(content.contains("2025-01-15"));
    }

    #[test]
    fn output_contains_modified_date() {
        let mut schema = create_test_schema();
        schema.modified = Some("2026-01-29".to_string());

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");

        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        let content = fs::read_to_string(&output_path).expect("Failed to read");
        assert!(content.contains("2026-01-29"));
    }

    // ========== Class Tests ==========

    #[test]
    fn output_contains_class_declaration() {
        let mut schema = create_test_schema();
        let mut animal = ClassDefinition::new("Animal");
        animal.class_uri = Some("http://example.org/test#Animal".to_string());
        schema.classes.insert("Animal".to_string(), animal);

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");

        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        let content = fs::read_to_string(&output_path).expect("Failed to read");
        assert!(content.contains("http://example.org/test#Animal"));
        assert!(
            content.contains("owl:Class")
                || content.contains("http://www.w3.org/2002/07/owl#Class")
        );
    }

    #[test]
    fn output_contains_subclass_relationship() {
        let mut schema = create_test_schema();

        let mut animal = ClassDefinition::new("Animal");
        animal.class_uri = Some("http://example.org/test#Animal".to_string());
        schema.classes.insert("Animal".to_string(), animal);

        let mut dog = ClassDefinition::new("Dog");
        dog.class_uri = Some("http://example.org/test#Dog".to_string());
        dog.is_a = Some("Animal".to_string());
        schema.classes.insert("Dog".to_string(), dog);

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");

        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        let content = fs::read_to_string(&output_path).expect("Failed to read");
        assert!(content.contains("http://example.org/test#Dog"));
        assert!(content.contains("http://example.org/test#Animal"));
        assert!(
            content.contains("subClassOf")
                || content.contains("http://www.w3.org/2000/01/rdf-schema#subClassOf")
        );
    }

    // ========== Property Tests ==========

    #[test]
    fn output_contains_object_property() {
        let mut schema = create_test_schema();

        let mut person = ClassDefinition::new("Person");
        person.class_uri = Some("http://example.org/test#Person".to_string());
        schema.classes.insert("Person".to_string(), person);

        let mut has_owner = SlotDefinition::new("hasOwner");
        has_owner.slot_uri = Some("http://example.org/test#hasOwner".to_string());
        has_owner.range = Some("Person".to_string());
        has_owner.annotations.insert(
            "panschema:owl_property_type".to_string(),
            "ObjectProperty".to_string(),
        );
        schema.slots.insert("hasOwner".to_string(), has_owner);

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");

        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        let content = fs::read_to_string(&output_path).expect("Failed to read");
        assert!(content.contains("http://example.org/test#hasOwner"));
        assert!(
            content.contains("ObjectProperty")
                || content.contains("http://www.w3.org/2002/07/owl#ObjectProperty")
        );
    }

    #[test]
    fn output_contains_datatype_property() {
        let mut schema = create_test_schema();

        let mut has_age = SlotDefinition::new("hasAge");
        has_age.slot_uri = Some("http://example.org/test#hasAge".to_string());
        has_age.range = Some("integer".to_string());
        schema.slots.insert("hasAge".to_string(), has_age);

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");

        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        let content = fs::read_to_string(&output_path).expect("Failed to read");
        assert!(content.contains("http://example.org/test#hasAge"));
        assert!(
            content.contains("DatatypeProperty")
                || content.contains("http://www.w3.org/2002/07/owl#DatatypeProperty")
        );
    }

    #[test]
    fn output_contains_inverse_relationship() {
        let mut schema = create_test_schema();

        let mut owns = SlotDefinition::new("owns");
        owns.slot_uri = Some("http://example.org/test#owns".to_string());
        owns.inverse = Some("hasOwner".to_string());
        owns.annotations.insert(
            "panschema:owl_property_type".to_string(),
            "ObjectProperty".to_string(),
        );
        schema.slots.insert("owns".to_string(), owns);

        let mut has_owner = SlotDefinition::new("hasOwner");
        has_owner.slot_uri = Some("http://example.org/test#hasOwner".to_string());
        has_owner.annotations.insert(
            "panschema:owl_property_type".to_string(),
            "ObjectProperty".to_string(),
        );
        schema.slots.insert("hasOwner".to_string(), has_owner);

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");

        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        let content = fs::read_to_string(&output_path).expect("Failed to read");
        assert!(
            content.contains("inverseOf")
                || content.contains("http://www.w3.org/2002/07/owl#inverseOf")
        );
    }

    // ========== Round-trip Tests ==========

    #[test]
    fn roundtrip_with_owl_reader() {
        use crate::owl_reader::OwlReader;
        use std::path::PathBuf;

        // Read reference ontology
        let input_path = PathBuf::from("tests/fixtures/reference.ttl");
        let reader = OwlReader::new();
        let schema = reader
            .read(&input_path)
            .expect("Failed to read reference.ttl");

        // Write to temporary file
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");
        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write output.ttl");

        // Read back the written file
        let schema2 = reader
            .read(&output_path)
            .expect("Failed to read written output.ttl");

        // Verify key data is preserved
        assert_eq!(schema.name, schema2.name);
        assert_eq!(schema.title, schema2.title);
        assert_eq!(schema.version, schema2.version);
        assert_eq!(schema.classes.len(), schema2.classes.len());
        assert_eq!(schema.slots.len(), schema2.slots.len());
    }

    #[test]
    fn roundtrip_preserves_class_hierarchy() {
        use crate::owl_reader::OwlReader;
        use std::path::PathBuf;

        let input_path = PathBuf::from("tests/fixtures/reference.ttl");
        let reader = OwlReader::new();
        let schema = reader
            .read(&input_path)
            .expect("Failed to read reference.ttl");

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");
        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        let schema2 = reader.read(&output_path).expect("Failed to read back");

        // Check Dog's parent is preserved
        let dog = schema.classes.get("Dog").unwrap();
        let dog2 = schema2.classes.get("Dog").unwrap();
        assert_eq!(dog.is_a, dog2.is_a);
    }

    #[test]
    fn roundtrip_preserves_inverse_relationships() {
        use crate::owl_reader::OwlReader;
        use std::path::PathBuf;

        let input_path = PathBuf::from("tests/fixtures/reference.ttl");
        let reader = OwlReader::new();
        let schema = reader
            .read(&input_path)
            .expect("Failed to read reference.ttl");

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");
        let writer = OwlWriter::new();
        writer
            .write(&schema, &output_path)
            .expect("Failed to write");

        let schema2 = reader.read(&output_path).expect("Failed to read back");

        // Check owns has inverse relationship
        let owns = schema.slots.get("owns").unwrap();
        let owns2 = schema2.slots.get("owns").unwrap();
        assert_eq!(owns.inverse, owns2.inverse);
    }
}
