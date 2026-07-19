//! YAML Reader
//!
//! Reads native LinkML YAML schemas directly into the LinkML IR.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::io::{IoError, IoResult, Reader};
use crate::linkml::{SchemaDefinition, SlotDefinition};

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
        let mut schema: SchemaDefinition =
            serde_yaml::from_str(&content).map_err(|e| IoError::Parse(e.to_string()))?;
        backfill_names(&mut schema)?;
        Ok(schema)
    }

    fn supported_extensions(&self) -> &[&str] {
        &["yaml", "yml"]
    }
}

/// Apply LinkML's "dict key is the canonical name" rule to all metaobjects.
///
/// For each `name` field in classes, slots, enums, types, and inline
/// attributes/slot_usage inside classes:
///
/// - If `name` is empty, fill it from the dict key.
/// - If `name` is non-empty and agrees with the key, leave it.
/// - If `name` is non-empty and disagrees with the key, return a Parse error.
fn backfill_names(schema: &mut SchemaDefinition) -> IoResult<()> {
    for (key, class) in schema.classes.iter_mut() {
        backfill_one("class", key, &mut class.name)?;
        backfill_slot_map(&format!("class '{key}' attribute"), &mut class.attributes)?;
        backfill_slot_map(&format!("class '{key}' slot_usage"), &mut class.slot_usage)?;
    }
    backfill_slot_map("slot", &mut schema.slots)?;
    for (key, enum_def) in schema.enums.iter_mut() {
        backfill_one("enum", key, &mut enum_def.name)?;
        for (pv_key, pv) in enum_def.permissible_values.iter_mut() {
            backfill_one(&format!("enum '{key}' value"), pv_key, &mut pv.text)?;
        }
    }
    for (key, type_def) in schema.types.iter_mut() {
        backfill_one("type", key, &mut type_def.name)?;
    }
    Ok(())
}

fn backfill_slot_map(kind: &str, slots: &mut BTreeMap<String, SlotDefinition>) -> IoResult<()> {
    for (key, slot) in slots {
        backfill_one(kind, key, &mut slot.name)?;
    }
    Ok(())
}

fn backfill_one(kind: &str, key: &str, name: &mut String) -> IoResult<()> {
    if name.is_empty() {
        *name = key.to_string();
        Ok(())
    } else if name == key {
        Ok(())
    } else {
        Err(IoError::Parse(format!(
            "{kind} '{key}': explicit `name: {name}` disagrees with dict key '{key}'. \
             Either remove the explicit name (it's inferred from the key) or fix the mismatch."
        )))
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
    fn class_parses_tree_root_flag() {
        use crate::linkml::ClassDefinition;
        // `tree_root: true` is now a modeled field (the reader deserializes
        // classes with the same serde the schema read uses), and absent
        // defaults to false.
        let root: ClassDefinition =
            serde_yaml::from_str("name: Catalog\ntree_root: true\n").expect("parse");
        assert!(root.tree_root, "tree_root: true parses onto the IR");
        let plain: ClassDefinition = serde_yaml::from_str("name: Wine\n").expect("parse");
        assert!(!plain.tree_root, "absent tree_root defaults to false");
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

    // -------------------------------------------------------------------
    // Name inference from dict keys (idiomatic LinkML).
    //
    // LinkML treats dict-keyed sub-objects as having their `name` field
    // implicitly set to the key. panschema must accept this — without it,
    // schemas produced by `linkml-runtime`, `gen-owl`, etc. fail to load.
    // -------------------------------------------------------------------

    /// Test helper: write a YAML schema to a temp file and parse it.
    fn parse_yaml(yaml: &str) -> IoResult<SchemaDefinition> {
        use std::io::Write;
        let mut tmp = tempfile::Builder::new()
            .suffix(".yaml")
            .tempfile()
            .expect("Should create temp file");
        tmp.write_all(yaml.as_bytes())
            .expect("Should write yaml to temp file");
        YamlReader::new().read(tmp.path())
    }

    // ---- Classes ----

    #[test]
    fn dict_keyed_class_without_name_inherits_key() {
        let schema = parse_yaml(
            r#"
id: https://example.org/x
name: x
classes:
  Entity:
    description: A thing
"#,
        )
        .expect("Should parse without explicit class name");

        let entity = schema
            .classes
            .get("Entity")
            .expect("Class 'Entity' should be present");
        assert_eq!(entity.name, "Entity");
    }

    #[test]
    fn dict_keyed_class_with_agreeing_name_succeeds() {
        let schema = parse_yaml(
            r#"
id: https://example.org/x
name: x
classes:
  Entity:
    name: Entity
    description: A thing
"#,
        )
        .expect("Agreeing explicit name should succeed");
        assert_eq!(schema.classes.get("Entity").unwrap().name, "Entity");
    }

    #[test]
    fn dict_keyed_class_with_disagreeing_name_errors() {
        let result = parse_yaml(
            r#"
id: https://example.org/x
name: x
classes:
  Entity:
    name: Person
"#,
        );
        match result {
            Err(IoError::Parse(msg)) => {
                assert!(
                    msg.contains("Entity") && msg.contains("Person"),
                    "Error message should name both the dict key and the disagreeing name; got: {msg}"
                );
            }
            other => panic!("Expected Parse error for disagreeing class name, got: {other:?}"),
        }
    }

    // ---- Top-level slots ----

    #[test]
    fn dict_keyed_slot_without_name_inherits_key() {
        let schema = parse_yaml(
            r#"
id: https://example.org/x
name: x
slots:
  identifier:
    range: string
"#,
        )
        .expect("Should parse without explicit slot name");
        assert_eq!(schema.slots.get("identifier").unwrap().name, "identifier");
    }

    #[test]
    fn dict_keyed_slot_with_disagreeing_name_errors() {
        let result = parse_yaml(
            r#"
id: https://example.org/x
name: x
slots:
  identifier:
    name: id
"#,
        );
        assert!(matches!(result, Err(IoError::Parse(_))));
    }

    // ---- Class attributes (inline slots inside classes) ----

    #[test]
    fn dict_keyed_attribute_without_name_inherits_key() {
        let schema = parse_yaml(
            r#"
id: https://example.org/x
name: x
classes:
  Person:
    attributes:
      first_name:
        range: string
"#,
        )
        .expect("Should parse attribute without explicit name");
        let person = schema.classes.get("Person").unwrap();
        let attr = person
            .attributes
            .get("first_name")
            .expect("Attribute 'first_name' should be present");
        assert_eq!(attr.name, "first_name");
    }

    #[test]
    fn dict_keyed_attribute_with_disagreeing_name_errors() {
        let result = parse_yaml(
            r#"
id: https://example.org/x
name: x
classes:
  Person:
    attributes:
      first_name:
        name: surname
"#,
        );
        assert!(matches!(result, Err(IoError::Parse(_))));
    }

    // ---- Enums ----

    #[test]
    fn dict_keyed_enum_without_name_inherits_key() {
        let schema = parse_yaml(
            r#"
id: https://example.org/x
name: x
enums:
  RankEnum:
    permissible_values:
      gold:
        text: gold
"#,
        )
        .expect("Should parse enum without explicit name");
        assert_eq!(schema.enums.get("RankEnum").unwrap().name, "RankEnum");
    }

    // ---- Permissible values (text field inferred from dict key) ----

    #[test]
    fn dict_keyed_permissible_value_without_text_inherits_key() {
        let schema = parse_yaml(
            r#"
id: https://example.org/x
name: x
enums:
  Color:
    permissible_values:
      red:
        description: The color red
      blue: {}
"#,
        )
        .expect("Should parse permissible values without explicit text");
        let color = schema.enums.get("Color").unwrap();
        assert_eq!(color.permissible_values.get("red").unwrap().text, "red");
        assert_eq!(color.permissible_values.get("blue").unwrap().text, "blue");
    }

    // ---- Types ----

    #[test]
    fn dict_keyed_type_without_name_inherits_key() {
        let schema = parse_yaml(
            r#"
id: https://example.org/x
name: x
types:
  age_t:
    typeof: integer
"#,
        )
        .expect("Should parse type without explicit name");
        assert_eq!(schema.types.get("age_t").unwrap().name, "age_t");
    }
}
