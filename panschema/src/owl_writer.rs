//! OWL/Turtle Writer
//!
//! Writes LinkML IR to OWL ontologies in Turtle format using sophia.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use sophia::api::serializer::TripleSerializer;
use sophia::turtle::serializer::turtle::{TurtleConfig, TurtleSerializer};

use crate::io::{IoError, IoResult, Writer};
use crate::linkml::SchemaDefinition;
use crate::rdf_serializers::{
    OWL_NS, RDF_NS, RDFS_NS, XSD_NS, build_rdf_graph_with_instances, build_turtle_prefix_map,
};

/// Writer for OWL ontologies in Turtle (.ttl) format
#[derive(Default)]
pub struct OwlWriter {
    /// Optional A-box: when set, each instance emits as an
    /// `owl:NamedIndividual` alongside the T-box.
    instances: Option<crate::instances::InstanceSet>,
}

impl OwlWriter {
    /// Create a new OWL writer
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

impl Writer for OwlWriter {
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()> {
        let graph = build_rdf_graph_with_instances(schema, self.instances.as_ref())?;

        crate::io::ensure_output_parent(output)?;
        let file = File::create(output).map_err(IoError::Io)?;
        let writer = BufWriter::new(file);

        // Wire the schema's `prefixes:` block plus the standard namespaces the
        // OWL graph emits (`xsd:`/`rdf:`/`rdfs:`/`owl:`) into sophia's
        // TurtleConfig, so terms like `rdfs:range xsd:integer` serialize in
        // compact CURIE form instead of verbose absolute IRIs. `with_pretty`
        // is required for prefix-aware output (sophia ignores the prefix map
        // in streaming mode).
        let prefix_map = build_turtle_prefix_map(
            schema,
            &[
                ("xsd", XSD_NS),
                ("rdf", RDF_NS),
                ("rdfs", RDFS_NS),
                ("owl", OWL_NS),
            ],
        );
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
        let writer = OwlWriter::default();
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
    fn roundtrip_preserves_inline_attribute_properties() {
        use crate::owl_reader::OwlReader;
        // The reference fixture is TTL-sourced, so its slots land top-level and
        // never exercise the inline `attributes:` path. An attribute-defined
        // property must survive an OWL write -> read: OWL has no `attributes:`
        // concept, so it reads back as a top-level slot with its range — but it
        // must not vanish, as it did when the writer ignored attributes.
        let mut schema = create_test_schema();
        let mut order = ClassDefinition::new("Order");
        order.class_uri = Some("http://example.org/test#Order".to_string());
        let mut amount = SlotDefinition::new("amount");
        amount.range = Some("integer".to_string());
        order.attributes.insert("amount".to_string(), amount);
        schema.classes.insert("Order".to_string(), order);

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");
        OwlWriter::new()
            .write(&schema, &output_path)
            .expect("Failed to write");
        let schema2 = OwlReader::new()
            .read(&output_path)
            .expect("Failed to read back");

        let amount2 = schema2
            .slots
            .get("amount")
            .expect("the inline attribute must survive the OWL round-trip as a property");
        assert_eq!(amount2.range.as_deref(), Some("integer"));
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
    fn ttl_output_declares_the_xsd_prefix_for_xsd_typed_terms() {
        // A datatype-property range like `integer` emits `rdfs:range
        // xsd:integer`. The OWL prefix map must include `xsd:` so that term
        // serializes in compact form, not as a verbose absolute IRI — the
        // SHACL writer already declares `xsd:`, the OWL writer did not.
        let mut schema = create_test_schema();
        let mut thing = ClassDefinition::new("Thing");
        thing.class_uri = Some("http://example.org/test#Thing".to_string());
        let mut count = SlotDefinition::new("count");
        count.range = Some("integer".to_string());
        thing.attributes.insert("count".to_string(), count);
        schema.classes.insert("Thing".to_string(), thing);

        let temp_dir = TempDir::new().expect("tempdir");
        let output_path = temp_dir.path().join("out.ttl");
        OwlWriter::new().write(&schema, &output_path).unwrap();
        let content = fs::read_to_string(&output_path).unwrap();

        assert!(
            content.contains("xsd:integer"),
            "OWL output must declare and use the `xsd:` prefix for xsd-typed terms; got:\n{content}"
        );
    }

    #[test]
    fn int_alias_range_maps_to_xsd_integer_not_a_fabricated_iri() {
        // A range alias like `int` must resolve to `xsd:integer`, never a
        // nonexistent `xsd:int` fabricated by the old alias fallback.
        let mut schema = create_test_schema();
        let mut thing = ClassDefinition::new("Thing");
        thing.class_uri = Some("http://example.org/test#Thing".to_string());
        let mut n = SlotDefinition::new("n");
        n.range = Some("int".to_string());
        thing.attributes.insert("n".to_string(), n);
        schema.classes.insert("Thing".to_string(), thing);

        let store = render_to_store(&schema);
        let rdfs_range = "http://www.w3.org/2000/01/rdf-schema#range";
        assert!(
            ask(
                &store,
                &format!(
                    "ASK {{ <http://example.org/test#n> <{rdfs_range}> <http://www.w3.org/2001/XMLSchema#integer> }}"
                )
            ),
            "the `int` alias must map to xsd:integer"
        );
        assert!(
            !ask(
                &store,
                &format!(
                    "ASK {{ <http://example.org/test#n> <{rdfs_range}> <http://www.w3.org/2001/XMLSchema#int> }}"
                )
            ),
            "must not fabricate a nonexistent xsd:int"
        );
    }

    #[test]
    fn unknown_datatype_range_emits_no_rdfs_range_instead_of_fabricating() {
        // A range that names no class/enum/type/primitive (a typo, already
        // flagged by the dangling-reference diagnostic) must not emit a
        // fabricated `rdfs:range xsd:Bogus`.
        let mut schema = create_test_schema();
        let mut thing = ClassDefinition::new("Thing");
        thing.class_uri = Some("http://example.org/test#Thing".to_string());
        let mut m = SlotDefinition::new("m");
        m.range = Some("Bogus".to_string());
        thing.attributes.insert("m".to_string(), m);
        schema.classes.insert("Thing".to_string(), thing);

        let store = render_to_store(&schema);
        assert!(
            !ask(
                &store,
                "ASK { <http://example.org/test#m> <http://www.w3.org/2000/01/rdf-schema#range> ?r }"
            ),
            "an unknown datatype range must emit no rdfs:range, not a fabricated xsd IRI"
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

    // ========== Independent RDF oracle (oxigraph) ==========
    //
    // `sophia` (the serializer) guarantees syntactic well-formedness by
    // construction, but nothing so far checks the generated graph against
    // an *independent* RDF engine. These tests load the rendered TTL into
    // a separate, real triple store (oxigraph) and query it — catching
    // graph-level mistakes sophia's own serializer wouldn't self-report.

    fn render_to_store(schema: &SchemaDefinition) -> oxigraph::store::Store {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let output_path = temp_dir.path().join("output.ttl");
        OwlWriter::new()
            .write(schema, &output_path)
            .expect("Failed to write");
        let ttl = fs::read_to_string(&output_path).expect("Failed to read");

        let store = oxigraph::store::Store::new().expect("Failed to create oxigraph store");
        store
            .load_from_slice(oxigraph::io::RdfFormat::Turtle, &ttl)
            .unwrap_or_else(|e| panic!("oxigraph rejected generated TTL: {e}\n\nTTL:\n{ttl}"));
        store
    }

    fn ask(store: &oxigraph::store::Store, query: &str) -> bool {
        use oxigraph::sparql::{QueryResults, SparqlEvaluator};
        match SparqlEvaluator::new()
            .parse_query(query)
            .unwrap_or_else(|e| panic!("invalid SPARQL query: {e}\n\n{query}"))
            .on_store(store)
            .execute()
            .expect("query execution failed")
        {
            QueryResults::Boolean(b) => b,
            QueryResults::Solutions(_) => panic!("expected an ASK query result, got Solutions"),
            QueryResults::Graph(_) => panic!("expected an ASK query result, got Graph"),
        }
    }

    #[test]
    fn oxigraph_rejects_malformed_turtle() {
        // The oracle must have teeth: if oxigraph accepted garbage, the
        // "loads cleanly" checks below would be vacuous.
        let store = oxigraph::store::Store::new().unwrap();
        let result =
            store.load_from_slice(oxigraph::io::RdfFormat::Turtle, "this is not turtle {{{");
        assert!(
            result.is_err(),
            "oxigraph should reject syntactically invalid Turtle"
        );
    }

    #[test]
    fn generated_ttl_loads_into_an_independent_triple_store() {
        let mut schema = create_test_schema();
        let mut animal = ClassDefinition::new("Animal");
        animal.class_uri = Some("http://example.org/test#Animal".to_string());
        schema.classes.insert("Animal".to_string(), animal);

        // `render_to_store` itself panics on a load failure — reaching
        // this line is the assertion.
        render_to_store(&schema);
    }

    #[test]
    fn every_class_has_an_owl_class_type_triple_in_the_independent_store() {
        let mut schema = create_test_schema();
        let mut animal = ClassDefinition::new("Animal");
        animal.class_uri = Some("http://example.org/test#Animal".to_string());
        schema.classes.insert("Animal".to_string(), animal);
        let mut dog = ClassDefinition::new("Dog");
        dog.class_uri = Some("http://example.org/test#Dog".to_string());
        dog.is_a = Some("Animal".to_string());
        schema.classes.insert("Dog".to_string(), dog);

        let store = render_to_store(&schema);

        for uri in [
            "http://example.org/test#Animal",
            "http://example.org/test#Dog",
        ] {
            assert!(
                ask(
                    &store,
                    &format!("ASK {{ <{uri}> a <http://www.w3.org/2002/07/owl#Class> }}")
                ),
                "expected {uri} to have an owl:Class type triple, independently queryable via oxigraph"
            );
        }

        // And the inheritance edge is queryable too.
        assert!(
            ask(
                &store,
                "ASK { <http://example.org/test#Dog> <http://www.w3.org/2000/01/rdf-schema#subClassOf> <http://example.org/test#Animal> }"
            ),
            "expected Dog rdfs:subClassOf Animal to be independently queryable via oxigraph"
        );
    }

    #[test]
    fn inline_attributes_are_declared_as_properties_in_the_independent_store() {
        // A class using the idiomatic inline `attributes:` form must still
        // emit its properties. The RDF emitter walked only top-level
        // `schema.slots`, so an attribute-defined property vanished from the
        // OWL output entirely — the class was declared with no properties,
        // and any SHACL `sh:path` pointing at it had no OWL counterpart.
        let mut schema = create_test_schema();
        let mut order = ClassDefinition::new("Order");
        order.class_uri = Some("http://example.org/test#Order".to_string());
        let mut amount = SlotDefinition::new("amount");
        amount.range = Some("integer".to_string());
        order.attributes.insert("amount".to_string(), amount);
        schema.classes.insert("Order".to_string(), order);

        let store = render_to_store(&schema);
        assert!(
            ask(
                &store,
                "ASK { \
                    <http://example.org/test#amount> \
                        a <http://www.w3.org/2002/07/owl#DatatypeProperty> ; \
                        <http://www.w3.org/2000/01/rdf-schema#domain> <http://example.org/test#Order> ; \
                        <http://www.w3.org/2000/01/rdf-schema#range> <http://www.w3.org/2001/XMLSchema#integer> \
                }"
            ),
            "an inline attribute must be declared as a datatype property with its domain and range"
        );
    }

    // ========== A-box emission (feature 36 slice 1) ==========

    /// Read the checked-in wine catalog schema + instance data — the real
    /// consumer shape (a `tree_root` container, id-keyed records, a
    /// cross-class reference) — and render TTL with the A-box attached.
    fn wine_store() -> oxigraph::store::Store {
        use crate::yaml_reader::YamlReader;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let schema = YamlReader::new()
            .read(&root.join("wine_catalog.yaml"))
            .expect("read wine schema");
        let data: serde_yaml::Value = serde_yaml::from_str(
            &fs::read_to_string(root.join("wine_instances.yaml")).expect("read instances"),
        )
        .expect("parse instances");
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);
        assert!(
            !set.instances.is_empty(),
            "fixture data must load instances"
        );

        let temp_dir = TempDir::new().expect("temp dir");
        let output_path = temp_dir.path().join("output.ttl");
        OwlWriter::new()
            .with_instances(set)
            .write(&schema, &output_path)
            .expect("write ttl with instances");
        let ttl = fs::read_to_string(&output_path).expect("read ttl");

        let store = oxigraph::store::Store::new().expect("store");
        store
            .load_from_slice(oxigraph::io::RdfFormat::Turtle, &ttl)
            .unwrap_or_else(|e| panic!("oxigraph rejected generated TTL: {e}\n\nTTL:\n{ttl}"));
        store
    }

    #[test]
    fn instances_emit_typed_labelled_named_individuals() {
        let store = wine_store();
        // The instance id minted against the schema's default prefix, typed
        // as both owl:NamedIndividual and its class IRI, with its display
        // name as rdfs:label.
        assert!(
            ask(
                &store,
                "PREFIX owl: <http://www.w3.org/2002/07/owl#>\n\
                 PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\n\
                 ASK { <https://example.org/wine/chateauMorgon> a owl:NamedIndividual ,\n\
                       <https://example.org/wine#Wine> ;\n\
                       rdfs:label \"Château Morgon\" }",
            ),
            "the wine individual must be typed and labelled"
        );
    }

    #[test]
    fn instances_emit_data_and_object_properties() {
        let store = wine_store();
        assert!(
            ask(
                &store,
                "ASK { <https://example.org/wine/chateauMorgon>\n\
                       <https://example.org/wine#color> \"red\" }",
            ),
            "a scalar slot value must emit as a data-property assertion"
        );
        // An id reference resolves to the referenced individual's IRI — the
        // SPARQL join a retrieval loop actually performs.
        assert!(
            ask(
                &store,
                "PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\n\
                 ASK { <https://example.org/wine/chateauMorgon>\n\
                       <https://example.org/wine#produced_by> ?w .\n\
                       ?w rdfs:label \"Morgon Estate\" }",
            ),
            "an id reference must emit as an object property to the referenced individual"
        );
    }

    #[test]
    fn integer_range_slot_values_carry_xsd_integer() {
        let mut schema = SchemaDefinition::new("cellar");
        schema.id = Some("https://example.org/cellar".to_string());
        schema.default_prefix = Some("cellar".to_string());
        schema.prefixes.insert(
            "cellar".to_string(),
            "https://example.org/cellar/".to_string(),
        );
        let mut container = ClassDefinition::new("Cellar");
        container.tree_root = true;
        let mut racks = SlotDefinition::new("bottles");
        racks.range = Some("Bottle".to_string());
        racks.multivalued = true;
        container.attributes.insert("bottles".to_string(), racks);
        schema.classes.insert("Cellar".to_string(), container);
        let mut bottle = ClassDefinition::new("Bottle");
        let mut id = SlotDefinition::new("id");
        id.identifier = true;
        bottle.attributes.insert("id".to_string(), id);
        let mut rating = SlotDefinition::new("rating");
        rating.range = Some("integer".to_string());
        bottle.attributes.insert("rating".to_string(), rating);
        schema.classes.insert("Bottle".to_string(), bottle);

        let data: serde_yaml::Value =
            serde_yaml::from_str("bottles:\n  - id: b1\n    rating: 4\n").unwrap();
        let set = crate::instances::InstanceSet::from_linkml_data(&schema, &data);

        let temp_dir = TempDir::new().expect("temp dir");
        let output_path = temp_dir.path().join("output.ttl");
        OwlWriter::new()
            .with_instances(set)
            .write(&schema, &output_path)
            .expect("write");
        let ttl = fs::read_to_string(&output_path).expect("read");
        let store = oxigraph::store::Store::new().unwrap();
        store
            .load_from_slice(oxigraph::io::RdfFormat::Turtle, &ttl)
            .unwrap_or_else(|e| panic!("oxigraph rejected TTL: {e}\n{ttl}"));
        // A bare `4` in SPARQL is an xsd:integer literal; the match fails if
        // the value was emitted as a string or double.
        assert!(
            ask(
                &store,
                "ASK { <https://example.org/cellar/b1> <https://example.org/cellar#rating> 4 }",
            ),
            "an integer-range slot value must carry xsd:integer"
        );
    }

    #[test]
    fn writer_without_instances_is_unchanged() {
        // The A-box attachment must not perturb the T-box-only output.
        let schema = create_test_schema();
        let temp_dir = TempDir::new().expect("temp dir");
        let plain = temp_dir.path().join("plain.ttl");
        let attached = temp_dir.path().join("attached.ttl");
        OwlWriter::new().write(&schema, &plain).expect("plain");
        OwlWriter::new()
            .with_instances(crate::instances::InstanceSet::default())
            .write(&schema, &attached)
            .expect("attached");
        assert_eq!(
            fs::read_to_string(&plain).unwrap(),
            fs::read_to_string(&attached).unwrap(),
            "an empty instance set must produce byte-identical output"
        );
    }
}
