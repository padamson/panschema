//! OWL/Turtle Writer
//!
//! Writes LinkML IR to OWL ontologies in Turtle format using sophia.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use sophia::api::prefix::{Prefix, PrefixMapPair};
use sophia::api::serializer::TripleSerializer;
use sophia::iri::Iri;
use sophia::turtle::serializer::turtle::{TurtleConfig, TurtleSerializer};

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

        // Wire the schema's `prefixes:` block into sophia's TurtleConfig so
        // the serializer emits `@prefix foo: <https://...>` declarations and
        // can fold absolute IRIs back into compact CURIE form. `with_pretty`
        // is required for prefix-aware output (sophia ignores the prefix map
        // in streaming mode). Prefixes that aren't valid sophia `Prefix`
        // values are silently dropped — they can't be used in the output
        // anyway.
        let prefix_map = build_prefix_map(schema);
        let config = TurtleConfig::new()
            .with_pretty(true)
            .with_own_prefix_map(prefix_map);
        let mut serializer = TurtleSerializer::new_with_config(writer, config);

        serializer
            .serialize_graph(&graph)
            .map_err(|e| IoError::Write(format!("Turtle serialization failed: {}", e)))?;

        Ok(())
    }

    fn format_id(&self) -> &str {
        "ttl"
    }
}

/// Construct a sophia `PrefixMap` from the schema's `prefixes:` block.
/// Each `(name, base)` pair becomes a `(Prefix, Iri)` entry; entries that
/// fail sophia's prefix/IRI validation are skipped with a `tracing::warn!`.
fn build_prefix_map(schema: &SchemaDefinition) -> Vec<PrefixMapPair> {
    schema
        .prefixes
        .iter()
        .filter_map(|(name, base)| {
            let prefix = Prefix::new(name.clone().into_boxed_str())
                .map_err(|e| {
                    tracing::warn!(
                        prefix = name,
                        error = %e,
                        "skipping invalid prefix declaration"
                    );
                })
                .ok()?;
            let iri = Iri::new(base.clone().into_boxed_str())
                .map_err(|e| {
                    tracing::warn!(
                        prefix = name,
                        base = base,
                        error = %e,
                        "skipping prefix with invalid base IRI"
                    );
                })
                .ok()?;
            Some((prefix, iri))
        })
        .collect()
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
    fn ttl_output_declares_prefixes_from_schema_prefixes_block() {
        // The schema's prefixes block must round-trip into TTL `PREFIX`
        // declarations. Without these, sophia's pretty serializer
        // can't compact absolute IRIs back into CURIE form and would
        // emit verbose absolute IRIs everywhere.
        let mut schema = create_test_schema();
        schema.prefixes.insert(
            "cco".to_string(),
            "https://www.commoncoreontologies.org/".to_string(),
        );
        schema.prefixes.insert(
            "obo".to_string(),
            "http://purl.obolibrary.org/obo/".to_string(),
        );
        let mut act = ClassDefinition::new("Act");
        act.class_uri = Some("cco:ont00000005".to_string());
        schema.classes.insert("Act".to_string(), act);

        let temp_dir = TempDir::new().expect("tempdir");
        let output_path = temp_dir.path().join("out.ttl");
        OwlWriter::new().write(&schema, &output_path).unwrap();
        let content = fs::read_to_string(&output_path).unwrap();

        assert!(
            content.contains("cco:") && content.contains("https://www.commoncoreontologies.org/"),
            "expected cco prefix declaration; got:\n{content}"
        );
        assert!(
            content.contains("obo:") && content.contains("http://purl.obolibrary.org/obo/"),
            "expected obo prefix declaration; got:\n{content}"
        );
    }

    #[test]
    fn ttl_output_uses_compact_curie_for_expanded_class_uri() {
        // When a class_uri is a CURIE against a declared prefix, the
        // TTL output should use the compact `cco:ont00000005` form,
        // never the invalid `<cco:ont00000005>` form (CURIE wrapped in
        // angle brackets, which would parse as an absolute IRI).
        let mut schema = create_test_schema();
        schema.prefixes.insert(
            "cco".to_string(),
            "https://www.commoncoreontologies.org/".to_string(),
        );
        let mut act = ClassDefinition::new("Act");
        act.class_uri = Some("cco:ont00000005".to_string());
        schema.classes.insert("Act".to_string(), act);

        let temp_dir = TempDir::new().expect("tempdir");
        let output_path = temp_dir.path().join("out.ttl");
        OwlWriter::new().write(&schema, &output_path).unwrap();
        let content = fs::read_to_string(&output_path).unwrap();

        assert!(
            !content.contains("<cco:ont00000005>"),
            "invalid CURIE-in-brackets form leaked into TTL output:\n{content}"
        );
        assert!(
            content.contains("cco:ont00000005"),
            "expected compact CURIE form; got:\n{content}"
        );
    }

    #[test]
    fn ttl_output_emits_subclass_of_for_each_mixin() {
        // Multiple-inheritance via LinkML mixins must surface as
        // multiple rdfs:subClassOf relations in the OWL output.
        let mut schema = create_test_schema();
        for name in ["Parent", "MixinA"] {
            let mut def = ClassDefinition::new(name);
            def.class_uri = Some(format!("http://example.org/test#{name}"));
            schema.classes.insert(name.to_string(), def);
        }
        let mut child = ClassDefinition::new("Child");
        child.class_uri = Some("http://example.org/test#Child".to_string());
        child.is_a = Some("Parent".to_string());
        child.mixins = vec!["MixinA".to_string()];
        schema.classes.insert("Child".to_string(), child);

        let temp_dir = TempDir::new().expect("tempdir");
        let output_path = temp_dir.path().join("out.ttl");
        OwlWriter::new().write(&schema, &output_path).unwrap();
        let content = fs::read_to_string(&output_path).unwrap();

        // TTL's compact form folds multiple objects of the same
        // predicate into one `subClassOf <a>, <b>.` chain, so the
        // assertion is "both parent IRIs appear under Child's
        // subClassOf relation," not a literal count of the predicate
        // keyword. Strip whitespace + newlines so the assertion is
        // robust to sophia's indentation.
        let flat: String = content.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            flat.contains("Child> a <http://www.w3.org/2002/07/owl#Class>")
                || flat.contains("test#Child>"),
            "Child class not declared in TTL output:\n{content}"
        );
        let parent_iri = "<http://example.org/test#Parent>";
        let mixin_iri = "<http://example.org/test#MixinA>";
        assert!(
            content.contains(parent_iri),
            "expected is_a parent IRI {parent_iri} in TTL output:\n{content}"
        );
        assert!(
            content.contains(mixin_iri),
            "expected mixin IRI {mixin_iri} in TTL output:\n{content}"
        );
        // Both parents share the subClassOf predicate — confirm the
        // mixin IRI follows the subClassOf keyword somewhere
        // (compact form lists them with `,` separators).
        let subclass_pos = content.find("subClassOf").expect("subClassOf keyword");
        let after = &content[subclass_pos..];
        assert!(
            after.contains(parent_iri) && after.contains(mixin_iri),
            "expected both is_a parent and mixin under subClassOf; got:\n{content}"
        );
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
