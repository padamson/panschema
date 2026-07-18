//! JSON Schema writer
//!
//! Projects the LinkML IR to a [JSON Schema](https://json-schema.org/)
//! (draft 2020-12): one closed `object` schema per class under `$defs`, with
//! each class's effective slots as typed, required-aware properties. The
//! language-agnostic structured-output contract — an LLM (via `rig`,
//! Anthropic, or OpenAI structured output) can be constrained to valid
//! instances, and any JSON validator can check instance data against it.
//!
//! Scalar slots project to typed properties; a non-scalar range (class /
//! enum / custom type) is emitted permissively (`true`) so
//! `additionalProperties: false` never rejects an otherwise-valid instance.
//! Enum/class `$ref`s and value-constraint keywords are not projected yet.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use serde_json::{Value, json};

use crate::io::{IoError, IoResult, Writer};
use crate::linkml::{SchemaDefinition, SlotDefinition};

/// The JSON Schema dialect the emitted documents declare.
const DIALECT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";

/// Writer for a JSON Schema document (`.json`).
pub struct JsonSchemaWriter;

impl JsonSchemaWriter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for JsonSchemaWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer for JsonSchemaWriter {
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()> {
        let doc = build_json_schema(schema);
        crate::io::ensure_output_parent(output)?;
        let file = File::create(output).map_err(IoError::Io)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &doc)
            .map_err(|e| IoError::Write(format!("JSON serialization failed: {e}")))?;
        Ok(())
    }

    fn format_id(&self) -> &str {
        "json-schema"
    }
}

/// Build the JSON Schema document: `$schema` + `$defs` with one `object`
/// schema per class. Deterministic — `$defs` and each class's `properties`
/// come out in `serde_json::Map` (sorted) order, and `required` follows the
/// resolver's alphabetical slot order, so the output is byte-stable.
pub fn build_json_schema(schema: &SchemaDefinition) -> Value {
    let mut defs = serde_json::Map::new();

    for (class_name, class_def) in &schema.classes {
        let mut properties = serde_json::Map::new();
        let mut required: Vec<Value> = Vec::new();

        // Effective slots: the same resolver the HTML/Rust/Postgres writers
        // use, so JSON Schema describes the same shape (inherited, mixed-in,
        // and refined slots included).
        for (slot_name, resolved) in
            crate::linkml_resolve::resolve_effective_slots_with_provenance(class_def, schema)
        {
            let cardinality = crate::linkml_resolve::effective_cardinality(&resolved.definition);
            properties.insert(
                slot_name.clone(),
                slot_property(&resolved.definition, schema),
            );
            if cardinality.required {
                required.push(Value::String(slot_name));
            }
        }

        let mut obj = serde_json::Map::new();
        obj.insert("type".to_string(), json!("object"));
        if let Some(desc) = &class_def.description {
            obj.insert("description".to_string(), json!(desc));
        }
        obj.insert("properties".to_string(), Value::Object(properties));
        if !required.is_empty() {
            obj.insert("required".to_string(), Value::Array(required));
        }
        // Closed object: a stray property is a bug, not silently accepted —
        // what strict LLM structured output and instance validation want.
        obj.insert("additionalProperties".to_string(), json!(false));

        defs.insert(class_name.clone(), Value::Object(obj));
    }

    let mut root = serde_json::Map::new();
    root.insert("$schema".to_string(), json!(DIALECT_2020_12));
    // When the schema declares a `tree_root` container class, the document
    // roots at it (a conforming instance is an instance of that class);
    // otherwise it is `$defs`-only and a consumer refs the class it wants.
    if let Some((name, _)) = schema.classes.iter().find(|(_, c)| c.tree_root) {
        root.insert("$ref".to_string(), json!(format!("#/$defs/{name}")));
    }
    root.insert("$defs".to_string(), Value::Object(defs));
    Value::Object(root)
}

/// The JSON Schema for a single slot: its range's scalar type, wrapped in an
/// `array` when the slot is multivalued. A non-scalar range (class / enum /
/// custom type) is emitted permissively (`true`).
fn slot_property(slot: &SlotDefinition, schema: &SchemaDefinition) -> Value {
    let range = slot
        .range
        .as_deref()
        .or(schema.default_range.as_deref())
        .unwrap_or("string");
    let base = scalar_json_type(range);

    if crate::linkml_resolve::effective_cardinality(slot).multivalued {
        json!({ "type": "array", "items": base })
    } else {
        base
    }
}

/// Map a LinkML built-in scalar range to its JSON Schema type/format. A range
/// that isn't a recognised scalar built-in (a class, enum, or custom type)
/// returns `true` — the "any" schema (not yet tightened to a `$ref` / `enum`).
fn scalar_json_type(range: &str) -> Value {
    match range {
        "integer" | "int" => json!({ "type": "integer" }),
        "float" | "double" | "decimal" => json!({ "type": "number" }),
        "boolean" | "bool" => json!({ "type": "boolean" }),
        "date" => json!({ "type": "string", "format": "date" }),
        "datetime" => json!({ "type": "string", "format": "date-time" }),
        "time" => json!({ "type": "string", "format": "time" }),
        "string" | "str" | "uri" | "uriorcurie" | "curie" | "ncname" | "objectidentifier"
        | "nodeidentifier" | "jsonpointer" | "jsonpath" | "sparqlpath" => {
            json!({ "type": "string" })
        }
        // Class / enum / custom-type / unknown — permissive for now.
        _ => Value::Bool(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linkml::{ClassDefinition, SlotDefinition};

    /// A `Wine` class with a required string, an optional integer, and a
    /// multivalued string — enough to exercise scalar typing, `required`,
    /// and array-wrapping. Self-contained (no fixture file).
    fn wine_schema() -> SchemaDefinition {
        let mut schema = SchemaDefinition::new("wine");
        let mut wine = ClassDefinition::new("Wine");

        let mut name = SlotDefinition::new("name");
        name.range = Some("string".to_string());
        name.required = true;
        wine.attributes.insert("name".to_string(), name);

        let mut vintage = SlotDefinition::new("vintage");
        vintage.range = Some("integer".to_string());
        wine.attributes.insert("vintage".to_string(), vintage);

        let mut tags = SlotDefinition::new("tags");
        tags.range = Some("string".to_string());
        tags.multivalued = true;
        wine.attributes.insert("tags".to_string(), tags);

        schema.classes.insert("Wine".to_string(), wine);
        schema
    }

    #[test]
    fn scalar_ranges_map_to_expected_json_types() {
        // Numbers, temporals (string + format), and strings each map
        // distinctly; a non-scalar range (class / enum) is permissive.
        assert_eq!(scalar_json_type("integer"), json!({ "type": "integer" }));
        assert_eq!(scalar_json_type("float"), json!({ "type": "number" }));
        assert_eq!(scalar_json_type("double"), json!({ "type": "number" }));
        assert_eq!(scalar_json_type("decimal"), json!({ "type": "number" }));
        assert_eq!(scalar_json_type("boolean"), json!({ "type": "boolean" }));
        assert_eq!(
            scalar_json_type("date"),
            json!({ "type": "string", "format": "date" })
        );
        assert_eq!(
            scalar_json_type("datetime"),
            json!({ "type": "string", "format": "date-time" })
        );
        assert_eq!(
            scalar_json_type("time"),
            json!({ "type": "string", "format": "time" })
        );
        assert_eq!(scalar_json_type("string"), json!({ "type": "string" }));
        assert_eq!(scalar_json_type("uri"), json!({ "type": "string" }));
        assert_eq!(scalar_json_type("Wine"), Value::Bool(true));
    }

    #[test]
    fn document_roots_at_the_tree_root_class() {
        // Without a tree_root, the document is $defs-only (no root ref).
        assert!(build_json_schema(&wine_schema()).get("$ref").is_none());

        // With one, the document root refs it — and stays a valid schema.
        let mut schema = wine_schema();
        schema.classes.get_mut("Wine").unwrap().tree_root = true;
        let doc = build_json_schema(&schema);
        assert_eq!(doc["$ref"], "#/$defs/Wine");
        assert!(
            jsonschema::validator_for(&doc).is_ok(),
            "a rooted document is still a valid schema"
        );
    }

    #[test]
    fn class_becomes_a_closed_typed_object() {
        let doc = build_json_schema(&wine_schema());
        let wine = &doc["$defs"]["Wine"];

        assert_eq!(wine["type"], "object");
        assert_eq!(wine["additionalProperties"], serde_json::json!(false));
        assert_eq!(wine["properties"]["name"]["type"], "string");
        assert_eq!(wine["properties"]["vintage"]["type"], "integer");
        // Multivalued → array of the scalar type.
        assert_eq!(wine["properties"]["tags"]["type"], "array");
        assert_eq!(wine["properties"]["tags"]["items"]["type"], "string");
        // Only the required slot is listed as required.
        assert_eq!(wine["required"], serde_json::json!(["name"]));
    }

    // Oracle 1: the emitted document is a usable JSON Schema — it compiles in
    // an independent validator (`jsonschema`), which rejects a structurally
    // invalid schema. Proves we emit a real schema, not just a JSON shape
    // this codebase expects.
    #[test]
    fn emitted_document_compiles_as_a_valid_json_schema() {
        let doc = build_json_schema(&wine_schema());
        assert!(
            jsonschema::validator_for(&doc).is_ok(),
            "the emitted document must be a valid, compilable JSON Schema"
        );
    }

    // Oracle 2: instances validate as expected against the class schema —
    // valid data passes; missing-required, wrong-typed, and extra-property
    // data all fail (the last thanks to `additionalProperties: false`).
    #[test]
    fn accepts_valid_and_rejects_invalid_scalar_instances() {
        let doc = build_json_schema(&wine_schema());
        let validator =
            jsonschema::validator_for(&doc["$defs"]["Wine"]).expect("Wine schema should compile");

        assert!(
            validator.is_valid(&serde_json::json!({
                "name": "Chateau Morgon", "vintage": 2017, "tags": ["red", "dry"]
            })),
            "a well-formed instance should validate"
        );
        assert!(
            !validator.is_valid(&serde_json::json!({ "vintage": 2017 })),
            "a missing required property should fail"
        );
        assert!(
            !validator.is_valid(&serde_json::json!({ "name": "x", "vintage": "not-an-int" })),
            "a wrong-typed property should fail"
        );
        assert!(
            !validator.is_valid(&serde_json::json!({ "name": "x", "color": "red" })),
            "an extra property should fail under additionalProperties:false"
        );
    }
}
