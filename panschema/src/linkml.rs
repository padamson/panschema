//! LinkML Internal Representation (IR)
//!
//! This module defines Rust structs that mirror the LinkML metamodel,
//! serving as the canonical internal representation for panschema.
//!
//! Reference: <https://linkml.io/linkml-model/latest/docs/specification/>

// Allow dead code in this module - the LinkML IR defines many optional fields
// that may not be populated by all readers or consumed by all writers. This is
// by design to support the full LinkML metamodel across different formats.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A prefix mapping for CURIE expansion
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prefix {
    /// The prefix name (e.g., "schema", "rdfs")
    pub prefix_prefix: String,
    /// The IRI expansion (e.g., "http://schema.org/")
    pub prefix_reference: String,
}

/// A contributor to the schema (author, editor, etc.)
///
/// Used to capture Dublin Core-style contributor metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contributor {
    /// The contributor's name
    pub name: String,
    /// ORCID identifier URL (e.g., "https://orcid.org/0000-0002-1825-0097")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orcid: Option<String>,
    /// Role in the project (e.g., "author", "editor", "contributor")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

impl Contributor {
    /// Create a new contributor with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            orcid: None,
            role: None,
        }
    }

    /// Create a contributor with name and role
    pub fn with_role(name: impl Into<String>, role: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            orcid: None,
            role: Some(role.into()),
        }
    }
}

/// Root container for a LinkML schema
///
/// Corresponds to LinkML SchemaDefinition.
/// Reference: <https://linkml.io/linkml-model/latest/docs/SchemaDefinition/>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaDefinition {
    /// A unique, machine-readable identifier for the schema
    pub name: String,
    /// The official URI for this schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Human-readable title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Schema description/documentation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Schema version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// License for the schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Contributors to the schema (authors, editors, etc.)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contributors: Vec<Contributor>,
    /// Creation date (ISO 8601 format, e.g., "2025-01-15")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    /// Last modification date (ISO 8601 format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
    /// Imported schemas/ontologies (URIs)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<String>,
    /// Prefix mappings for CURIE expansion
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub prefixes: BTreeMap<String, String>,
    /// Default prefix for unprefixed names
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_prefix: Option<String>,
    /// Default range for slots without explicit range
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_range: Option<String>,
    /// Class definitions in this schema
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub classes: BTreeMap<String, ClassDefinition>,
    /// Slot definitions in this schema
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub slots: BTreeMap<String, SlotDefinition>,
    /// Enum definitions in this schema
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub enums: BTreeMap<String, EnumDefinition>,
    /// Type definitions in this schema
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub types: BTreeMap<String, TypeDefinition>,
    /// Format-specific annotations (e.g., OWL-specific metadata)
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
}

impl SchemaDefinition {
    /// Create a new schema with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            id: None,
            title: None,
            description: None,
            version: None,
            license: None,
            contributors: Vec::new(),
            created: None,
            modified: None,
            imports: Vec::new(),
            prefixes: BTreeMap::new(),
            default_prefix: None,
            default_range: None,
            classes: BTreeMap::new(),
            slots: BTreeMap::new(),
            enums: BTreeMap::new(),
            types: BTreeMap::new(),
            annotations: BTreeMap::new(),
        }
    }

    /// Returns the display title (title if available, otherwise name)
    pub fn display_title(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }
}

/// A class definition in a LinkML schema
///
/// Corresponds to LinkML ClassDefinition.
/// Reference: <https://linkml.io/linkml-model/latest/docs/ClassDefinition/>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClassDefinition {
    /// The unique name of this class within the schema
    pub name: String,
    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Primary parent class (single inheritance)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_a: Option<String>,
    /// Secondary parent classes (mixins)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mixins: Vec<String>,
    /// Whether this class is abstract (cannot be instantiated directly)
    #[serde(default, skip_serializing_if = "is_false")]
    pub r#abstract: bool,
    /// Slots that apply to this class
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<String>,
    /// Inline slot definitions specific to this class
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, SlotDefinition>,
    /// Slot refinements in the context of this class
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub slot_usage: BTreeMap<String, SlotDefinition>,
    /// URI for semantic interpretation (e.g., owl:Class IRI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_uri: Option<String>,
    /// Format-specific annotations
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
}

impl ClassDefinition {
    /// Create a new class with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            is_a: None,
            mixins: Vec::new(),
            r#abstract: false,
            slots: Vec::new(),
            attributes: BTreeMap::new(),
            slot_usage: BTreeMap::new(),
            class_uri: None,
            annotations: BTreeMap::new(),
        }
    }

    /// Returns the display label (name is always available in LinkML)
    pub fn display_label(&self) -> &str {
        &self.name
    }
}

/// A slot (property) definition in a LinkML schema
///
/// Corresponds to LinkML SlotDefinition.
/// Reference: <https://linkml.io/linkml-model/latest/docs/SlotDefinition/>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlotDefinition {
    /// The unique name of this slot within the schema
    pub name: String,
    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The type of values this slot holds (class name, type name, or enum name)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    /// The class that owns this slot (domain)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    /// Whether this slot must be present
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
    /// Whether this slot can hold multiple values
    #[serde(default, skip_serializing_if = "is_false")]
    pub multivalued: bool,
    /// Minimum number of values (for multivalued slots)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_cardinality: Option<u32>,
    /// Maximum number of values (for multivalued slots)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_cardinality: Option<u32>,
    /// Regular expression pattern for string values
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// Whether this slot uniquely identifies instances
    #[serde(default, skip_serializing_if = "is_false")]
    pub identifier: bool,
    /// URI for semantic interpretation (e.g., owl:ObjectProperty IRI)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_uri: Option<String>,
    /// Inverse slot (for bidirectional relationships)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inverse: Option<String>,
    /// Format-specific annotations
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
}

impl SlotDefinition {
    /// Create a new slot with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            range: None,
            domain: None,
            required: false,
            multivalued: false,
            minimum_cardinality: None,
            maximum_cardinality: None,
            pattern: None,
            identifier: false,
            slot_uri: None,
            inverse: None,
            annotations: BTreeMap::new(),
        }
    }

    /// Returns the display label (name is always available in LinkML)
    pub fn display_label(&self) -> &str {
        &self.name
    }
}

/// An enumeration definition in a LinkML schema
///
/// Corresponds to LinkML EnumDefinition.
/// Reference: <https://linkml.io/linkml-model/latest/docs/EnumDefinition/>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumDefinition {
    /// The unique name of this enum within the schema
    pub name: String,
    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The allowed values for this enum
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub permissible_values: BTreeMap<String, PermissibleValue>,
    /// Format-specific annotations
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
}

impl EnumDefinition {
    /// Create a new enum with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            permissible_values: BTreeMap::new(),
            annotations: BTreeMap::new(),
        }
    }
}

/// A permissible value within an enumeration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PermissibleValue {
    /// The value text
    pub text: String,
    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// URI for semantic interpretation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meaning: Option<String>,
}

impl PermissibleValue {
    /// Create a new permissible value with the given text
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            description: None,
            meaning: None,
        }
    }
}

/// A type definition in a LinkML schema
///
/// Corresponds to LinkML TypeDefinition.
/// Reference: <https://linkml.io/linkml-model/latest/docs/TypeDefinition/>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeDefinition {
    /// The unique name of this type within the schema
    pub name: String,
    /// Human-readable description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Parent type (for type inheritance)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub typeof_: Option<String>,
    /// URI for the underlying datatype (e.g., xsd:string)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    /// Regular expression pattern for validation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// Format-specific annotations
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
}

impl TypeDefinition {
    /// Create a new type with the given name
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            typeof_: None,
            uri: None,
            pattern: None,
            annotations: BTreeMap::new(),
        }
    }
}

/// Helper function for serde skip_serializing_if
fn is_false(b: &bool) -> bool {
    !(*b)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== SchemaDefinition Tests ==========

    #[test]
    fn schema_definition_new_creates_minimal_schema() {
        let schema = SchemaDefinition::new("my_schema");
        assert_eq!(schema.name, "my_schema");
        assert!(schema.id.is_none());
        assert!(schema.classes.is_empty());
        assert!(schema.slots.is_empty());
    }

    #[test]
    fn schema_definition_display_title_uses_title_when_present() {
        let mut schema = SchemaDefinition::new("test");
        schema.title = Some("My Schema".to_string());
        assert_eq!(schema.display_title(), "My Schema");
    }

    #[test]
    fn schema_definition_display_title_falls_back_to_name() {
        let schema = SchemaDefinition::new("my_schema");
        assert_eq!(schema.display_title(), "my_schema");
    }

    #[test]
    fn schema_definition_serializes_to_yaml() {
        let mut schema = SchemaDefinition::new("example");
        schema.id = Some("https://example.org/schema".to_string());
        schema.description = Some("An example schema".to_string());

        let yaml = serde_yaml::to_string(&schema).unwrap();
        assert!(yaml.contains("name: example"));
        assert!(yaml.contains("id: https://example.org/schema"));
        assert!(yaml.contains("description: An example schema"));
    }

    #[test]
    fn schema_definition_deserializes_from_yaml() {
        let yaml = r#"
name: test_schema
id: https://example.org/test
description: A test schema
"#;
        let schema: SchemaDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(schema.name, "test_schema");
        assert_eq!(schema.id, Some("https://example.org/test".to_string()));
        assert_eq!(schema.description, Some("A test schema".to_string()));
    }

    #[test]
    fn schema_definition_with_classes() {
        let mut schema = SchemaDefinition::new("animals");
        schema
            .classes
            .insert("Animal".to_string(), ClassDefinition::new("Animal"));
        schema.classes.insert("Dog".to_string(), {
            let mut dog = ClassDefinition::new("Dog");
            dog.is_a = Some("Animal".to_string());
            dog
        });

        assert_eq!(schema.classes.len(), 2);
        assert!(schema.classes.contains_key("Animal"));
        assert_eq!(
            schema.classes.get("Dog").unwrap().is_a,
            Some("Animal".to_string())
        );
    }

    // ========== Contributor Tests ==========

    #[test]
    fn contributor_new_creates_minimal_contributor() {
        let contributor = Contributor::new("Jane Doe");
        assert_eq!(contributor.name, "Jane Doe");
        assert!(contributor.orcid.is_none());
        assert!(contributor.role.is_none());
    }

    #[test]
    fn contributor_with_role_sets_name_and_role() {
        let contributor = Contributor::with_role("John Smith", "author");
        assert_eq!(contributor.name, "John Smith");
        assert_eq!(contributor.role, Some("author".to_string()));
        assert!(contributor.orcid.is_none());
    }

    #[test]
    fn contributor_with_all_fields() {
        let mut contributor = Contributor::new("Jane Doe");
        contributor.orcid = Some("https://orcid.org/0000-0002-1825-0097".to_string());
        contributor.role = Some("editor".to_string());

        assert_eq!(contributor.name, "Jane Doe");
        assert_eq!(
            contributor.orcid,
            Some("https://orcid.org/0000-0002-1825-0097".to_string())
        );
        assert_eq!(contributor.role, Some("editor".to_string()));
    }

    #[test]
    fn contributor_serializes_to_yaml() {
        let mut contributor = Contributor::new("Jane Doe");
        contributor.role = Some("author".to_string());

        let yaml = serde_yaml::to_string(&contributor).unwrap();
        assert!(yaml.contains("name: Jane Doe"));
        assert!(yaml.contains("role: author"));
        // orcid should be omitted when None
        assert!(!yaml.contains("orcid"));
    }

    // ========== SchemaDefinition Metadata Tests ==========

    #[test]
    fn schema_definition_new_initializes_metadata_fields() {
        let schema = SchemaDefinition::new("test");
        assert!(schema.contributors.is_empty());
        assert!(schema.created.is_none());
        assert!(schema.modified.is_none());
        assert!(schema.imports.is_empty());
    }

    #[test]
    fn schema_definition_with_contributors() {
        let mut schema = SchemaDefinition::new("test");
        schema
            .contributors
            .push(Contributor::with_role("Alice", "author"));
        schema
            .contributors
            .push(Contributor::with_role("Bob", "contributor"));

        assert_eq!(schema.contributors.len(), 2);
        assert_eq!(schema.contributors[0].name, "Alice");
        assert_eq!(schema.contributors[1].name, "Bob");
    }

    #[test]
    fn schema_definition_with_dates() {
        let mut schema = SchemaDefinition::new("test");
        schema.created = Some("2025-01-15".to_string());
        schema.modified = Some("2026-01-29".to_string());

        assert_eq!(schema.created, Some("2025-01-15".to_string()));
        assert_eq!(schema.modified, Some("2026-01-29".to_string()));
    }

    #[test]
    fn schema_definition_with_imports() {
        let mut schema = SchemaDefinition::new("test");
        schema
            .imports
            .push("http://purl.obolibrary.org/obo/bfo.owl".to_string());
        schema.imports.push("http://purl.org/dc/terms/".to_string());

        assert_eq!(schema.imports.len(), 2);
        assert!(
            schema
                .imports
                .contains(&"http://purl.obolibrary.org/obo/bfo.owl".to_string())
        );
    }

    #[test]
    fn schema_definition_metadata_serializes_to_yaml() {
        let mut schema = SchemaDefinition::new("example");
        schema.created = Some("2025-01-15".to_string());
        schema
            .contributors
            .push(Contributor::with_role("Jane Doe", "author"));

        let yaml = serde_yaml::to_string(&schema).unwrap();
        assert!(yaml.contains("created: '2025-01-15'") || yaml.contains("created: 2025-01-15"));
        assert!(yaml.contains("name: Jane Doe"));
    }

    #[test]
    fn schema_definition_metadata_deserializes_from_yaml() {
        let yaml = r#"
name: test_schema
created: "2025-01-15"
modified: "2026-01-29"
contributors:
  - name: Jane Doe
    role: author
imports:
  - http://purl.obolibrary.org/obo/bfo.owl
"#;
        let schema: SchemaDefinition = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(schema.created, Some("2025-01-15".to_string()));
        assert_eq!(schema.modified, Some("2026-01-29".to_string()));
        assert_eq!(schema.contributors.len(), 1);
        assert_eq!(schema.contributors[0].name, "Jane Doe");
        assert_eq!(schema.imports.len(), 1);
    }

    // ========== ClassDefinition Tests ==========

    #[test]
    fn class_definition_new_creates_minimal_class() {
        let class = ClassDefinition::new("Person");
        assert_eq!(class.name, "Person");
        assert!(class.is_a.is_none());
        assert!(class.mixins.is_empty());
        assert!(!class.r#abstract);
    }

    #[test]
    fn class_definition_with_inheritance() {
        let mut class = ClassDefinition::new("Dog");
        class.is_a = Some("Animal".to_string());
        class.description = Some("A domesticated canine".to_string());

        assert_eq!(class.is_a, Some("Animal".to_string()));
        assert_eq!(class.display_label(), "Dog");
    }

    #[test]
    fn class_definition_with_mixins() {
        let mut class = ClassDefinition::new("Person");
        class.mixins = vec!["Named".to_string(), "Aged".to_string()];

        assert_eq!(class.mixins.len(), 2);
        assert!(class.mixins.contains(&"Named".to_string()));
    }

    #[test]
    fn class_definition_serializes_correctly() {
        let mut class = ClassDefinition::new("Animal");
        class.description = Some("A living creature".to_string());
        class.r#abstract = true;

        let yaml = serde_yaml::to_string(&class).unwrap();
        assert!(yaml.contains("name: Animal"));
        assert!(yaml.contains("abstract: true"));
    }

    // ========== SlotDefinition Tests ==========

    #[test]
    fn slot_definition_new_creates_minimal_slot() {
        let slot = SlotDefinition::new("name");
        assert_eq!(slot.name, "name");
        assert!(slot.range.is_none());
        assert!(!slot.required);
        assert!(!slot.multivalued);
    }

    #[test]
    fn slot_definition_with_range_and_constraints() {
        let mut slot = SlotDefinition::new("age");
        slot.range = Some("integer".to_string());
        slot.required = true;
        slot.description = Some("The age in years".to_string());

        assert_eq!(slot.range, Some("integer".to_string()));
        assert!(slot.required);
        assert_eq!(slot.display_label(), "age");
    }

    #[test]
    fn slot_definition_with_cardinality() {
        let mut slot = SlotDefinition::new("friends");
        slot.multivalued = true;
        slot.minimum_cardinality = Some(0);
        slot.maximum_cardinality = Some(10);

        assert!(slot.multivalued);
        assert_eq!(slot.minimum_cardinality, Some(0));
        assert_eq!(slot.maximum_cardinality, Some(10));
    }

    #[test]
    fn slot_definition_with_inverse() {
        let mut slot = SlotDefinition::new("has_owner");
        slot.range = Some("Person".to_string());
        slot.inverse = Some("owns".to_string());

        assert_eq!(slot.inverse, Some("owns".to_string()));
    }

    // ========== EnumDefinition Tests ==========

    #[test]
    fn enum_definition_new_creates_minimal_enum() {
        let enum_def = EnumDefinition::new("Color");
        assert_eq!(enum_def.name, "Color");
        assert!(enum_def.permissible_values.is_empty());
    }

    #[test]
    fn enum_definition_with_values() {
        let mut enum_def = EnumDefinition::new("Status");
        enum_def
            .permissible_values
            .insert("active".to_string(), PermissibleValue::new("active"));
        enum_def.permissible_values.insert("inactive".to_string(), {
            let mut pv = PermissibleValue::new("inactive");
            pv.description = Some("No longer active".to_string());
            pv
        });

        assert_eq!(enum_def.permissible_values.len(), 2);
        assert!(enum_def.permissible_values.contains_key("active"));
    }

    // ========== TypeDefinition Tests ==========

    #[test]
    fn type_definition_new_creates_minimal_type() {
        let type_def = TypeDefinition::new("age_type");
        assert_eq!(type_def.name, "age_type");
        assert!(type_def.uri.is_none());
    }

    #[test]
    fn type_definition_with_uri() {
        let mut type_def = TypeDefinition::new("string");
        type_def.uri = Some("xsd:string".to_string());
        type_def.description = Some("A character string".to_string());

        assert_eq!(type_def.uri, Some("xsd:string".to_string()));
    }

    // ========== Annotation Tests ==========

    #[test]
    fn schema_preserves_source_format_annotation() {
        let mut schema = SchemaDefinition::new("test");
        schema
            .annotations
            .insert("panschema:source_format".to_string(), "owl".to_string());

        assert_eq!(
            schema.annotations.get("panschema:source_format"),
            Some(&"owl".to_string())
        );
    }

    #[test]
    fn class_preserves_owl_specific_annotations() {
        let mut class = ClassDefinition::new("Person");
        class.annotations.insert(
            "panschema:owl_class_iri".to_string(),
            "http://example.org/Person".to_string(),
        );

        let yaml = serde_yaml::to_string(&class).unwrap();
        assert!(yaml.contains("panschema:owl_class_iri"));
    }

    // ========== Round-trip Tests ==========

    #[test]
    fn schema_roundtrip_yaml() {
        let mut schema = SchemaDefinition::new("roundtrip_test");
        schema.id = Some("https://example.org/roundtrip".to_string());
        schema
            .prefixes
            .insert("ex".to_string(), "https://example.org/".to_string());

        let mut animal = ClassDefinition::new("Animal");
        animal.description = Some("A living thing".to_string());
        schema.classes.insert("Animal".to_string(), animal);

        let mut name_slot = SlotDefinition::new("name");
        name_slot.range = Some("string".to_string());
        name_slot.required = true;
        schema.slots.insert("name".to_string(), name_slot);

        // Serialize
        let yaml = serde_yaml::to_string(&schema).unwrap();

        // Deserialize
        let restored: SchemaDefinition = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(schema, restored);
    }
}
