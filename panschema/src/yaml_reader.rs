//! YAML Reader
//!
//! Reads native LinkML YAML schemas directly into the LinkML IR.

use std::fs;
use std::path::Path;

use crate::io::{IoError, IoResult, Reader};
use crate::linkml::SchemaDefinition;

/// Reader for native LinkML YAML schemas
pub struct YamlReader;

impl YamlReader {
    /// Create a new YAML reader
    pub fn new() -> Self {
        Self
    }
}

impl Default for YamlReader {
    fn default() -> Self {
        Self::new()
    }
}

impl Reader for YamlReader {
    fn read(&self, input: &Path) -> IoResult<SchemaDefinition> {
        let content = fs::read_to_string(input)?;
        let schema: SchemaDefinition =
            serde_yaml::from_str(&content).map_err(|e| IoError::Parse(e.to_string()))?;
        Ok(schema)
    }

    fn supported_extensions(&self) -> &[&str] {
        &["yaml", "yml"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_schema_path() -> PathBuf {
        PathBuf::from("tests/fixtures/sample_schema.yaml")
    }

    #[test]
    fn yaml_reader_supports_yaml_extensions() {
        let reader = YamlReader::new();
        assert!(reader.supports_extension("yaml"));
        assert!(reader.supports_extension("yml"));
        assert!(reader.supports_extension("YAML"));
        assert!(reader.supports_extension("YML"));
        assert!(!reader.supports_extension("ttl"));
        assert!(!reader.supports_extension("json"));
    }

    #[test]
    fn yaml_reader_parses_sample_schema() {
        let reader = YamlReader::new();
        let schema = reader
            .read(&sample_schema_path())
            .expect("Should parse YAML schema");

        assert_eq!(schema.name, "sample_schema");
        assert_eq!(schema.id, Some("https://example.org/sample".to_string()));
        assert_eq!(schema.title, Some("Sample LinkML Schema".to_string()));
        assert_eq!(schema.version, Some("1.0.0".to_string()));
    }

    #[test]
    fn yaml_reader_parses_metadata() {
        let reader = YamlReader::new();
        let schema = reader
            .read(&sample_schema_path())
            .expect("Should parse YAML schema");

        assert_eq!(
            schema.license,
            Some("https://creativecommons.org/licenses/by/4.0/".to_string())
        );
        assert_eq!(schema.created, Some("2025-01-15".to_string()));
        assert_eq!(schema.modified, Some("2026-01-29".to_string()));
    }

    #[test]
    fn yaml_reader_parses_prefixes() {
        let reader = YamlReader::new();
        let schema = reader
            .read(&sample_schema_path())
            .expect("Should parse YAML schema");

        assert_eq!(schema.prefixes.len(), 2);
        assert_eq!(
            schema.prefixes.get("linkml"),
            Some(&"https://w3id.org/linkml/".to_string())
        );
        assert_eq!(
            schema.prefixes.get("ex"),
            Some(&"https://example.org/".to_string())
        );
    }

    #[test]
    fn yaml_reader_parses_classes() {
        let reader = YamlReader::new();
        let schema = reader
            .read(&sample_schema_path())
            .expect("Should parse YAML schema");

        assert_eq!(schema.classes.len(), 2);
        assert!(schema.classes.contains_key("Person"));
        assert!(schema.classes.contains_key("Organization"));

        let person = schema.classes.get("Person").unwrap();
        assert_eq!(person.description, Some("A human being".to_string()));
        assert_eq!(person.attributes.len(), 3);
    }

    #[test]
    fn yaml_reader_parses_class_attributes() {
        let reader = YamlReader::new();
        let schema = reader
            .read(&sample_schema_path())
            .expect("Should parse YAML schema");

        let person = schema.classes.get("Person").unwrap();
        let name_attr = person.attributes.get("name").unwrap();

        assert_eq!(
            name_attr.description,
            Some("The person's full name".to_string())
        );
        assert_eq!(name_attr.range, Some("string".to_string()));
        assert!(name_attr.required);
    }

    #[test]
    fn yaml_reader_parses_slots() {
        let reader = YamlReader::new();
        let schema = reader
            .read(&sample_schema_path())
            .expect("Should parse YAML schema");

        assert_eq!(schema.slots.len(), 1);
        let identifier = schema.slots.get("identifier").unwrap();
        assert!(identifier.identifier);
        assert_eq!(identifier.range, Some("string".to_string()));
    }

    #[test]
    fn yaml_reader_parses_enums() {
        let reader = YamlReader::new();
        let schema = reader
            .read(&sample_schema_path())
            .expect("Should parse YAML schema");

        assert_eq!(schema.enums.len(), 1);
        let status_enum = schema.enums.get("StatusEnum").unwrap();
        assert_eq!(status_enum.permissible_values.len(), 3);
        assert!(status_enum.permissible_values.contains_key("active"));
        assert!(status_enum.permissible_values.contains_key("inactive"));
        assert!(status_enum.permissible_values.contains_key("pending"));
    }

    #[test]
    fn yaml_reader_parses_types() {
        let reader = YamlReader::new();
        let schema = reader
            .read(&sample_schema_path())
            .expect("Should parse YAML schema");

        assert_eq!(schema.types.len(), 1);
        let age_type = schema.types.get("age_type").unwrap();
        assert_eq!(age_type.typeof_, Some("integer".to_string()));
    }

    #[test]
    fn yaml_reader_returns_error_for_invalid_yaml() {
        let reader = YamlReader::new();
        let result = reader.read(Path::new("tests/fixtures/reference.ttl"));

        assert!(result.is_err());
        match result {
            Err(IoError::Parse(_)) => {} // Expected
            _ => panic!("Expected Parse error"),
        }
    }

    #[test]
    fn yaml_reader_returns_error_for_missing_file() {
        let reader = YamlReader::new();
        let result = reader.read(Path::new("nonexistent.yaml"));

        assert!(result.is_err());
        match result {
            Err(IoError::Io(_)) => {} // Expected
            _ => panic!("Expected Io error"),
        }
    }
}
