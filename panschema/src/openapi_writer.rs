//! OpenAPI writer
//!
//! Projects the LinkML IR to the **`components/schemas`** of an OpenAPI 3.1
//! document. OpenAPI 3.1's schema dialect *is* JSON Schema 2020-12, so this
//! reuses the [`crate::json_schema_writer`]'s per-class object schemas verbatim
//! and only retargets the inter-class `$ref`s from `#/$defs/<Class>` (the
//! JSON-Schema location) to `#/components/schemas/<Class>` (the OpenAPI one),
//! then wraps them in a minimal `openapi`/`info`/`components` envelope.
//!
//! LinkML models the *data model*, not HTTP operations, so this emits schema
//! components only — the `paths`/operations layer is expected to come from the
//! service code (`utoipa`/`aide`) `$ref`-ing these components.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use serde_json::{Value, json};

use crate::io::{IoError, IoResult, Writer};
use crate::linkml::SchemaDefinition;

/// Writer for an OpenAPI 3.1 document (`.json`) carrying `components/schemas`.
pub struct OpenApiWriter;

impl OpenApiWriter {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OpenApiWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl Writer for OpenApiWriter {
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()> {
        let doc = build_openapi(schema);
        crate::io::ensure_output_parent(output)?;
        let file = File::create(output).map_err(IoError::Io)?;
        serde_json::to_writer_pretty(BufWriter::new(file), &doc)
            .map_err(|e| IoError::Write(format!("JSON serialization failed: {e}")))?;
        Ok(())
    }

    fn format_id(&self) -> &str {
        "openapi"
    }
}

/// Build the OpenAPI 3.1 document: the JSON-Schema class definitions under
/// `components/schemas`, with their `$ref`s retargeted, in an
/// `openapi`/`info`/`components` envelope. `info.title` is the schema's title
/// (or name); `info.version` its `version` (or `0.0.0` when unset).
pub fn build_openapi(schema: &SchemaDefinition) -> Value {
    let mut schemas = crate::json_schema_writer::build_class_defs(schema);
    for def in schemas.values_mut() {
        retarget_refs(def);
    }

    let title = schema.title.clone().unwrap_or_else(|| schema.name.clone());
    let version = schema
        .version
        .clone()
        .unwrap_or_else(|| "0.0.0".to_string());

    json!({
        "openapi": "3.1.0",
        "info": { "title": title, "version": version },
        "components": { "schemas": Value::Object(schemas) },
    })
}

/// Rewrite every `$ref` pointing at the JSON-Schema `#/$defs/<Class>` location
/// to the OpenAPI `#/components/schemas/<Class>` one, in place and recursively.
fn retarget_refs(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                if key == "$ref"
                    && let Value::String(r) = v
                    && let Some(rest) = r.strip_prefix("#/$defs/")
                {
                    *r = format!("#/components/schemas/{rest}");
                } else {
                    retarget_refs(v);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(retarget_refs),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cellar_schema() -> SchemaDefinition {
        let mut schema: SchemaDefinition = serde_yaml::from_str(
            "\
name: cellar
title: Wine Cellar
version: 2.1.0
classes:
  Wine:
    attributes:
      name:
        range: string
        required: true
      region:
        range: Region
      origin:
        any_of:
          - range: Region
          - range: string
  Region:
    attributes:
      id:
        range: string
        required: true
",
        )
        .expect("parse schema");
        for (name, class) in schema.classes.iter_mut() {
            class.name = name.clone();
        }
        schema
    }

    #[test]
    fn envelope_carries_info_and_components() {
        let doc = build_openapi(&cellar_schema());
        assert_eq!(doc["openapi"], "3.1.0");
        assert_eq!(doc["info"]["title"], "Wine Cellar");
        assert_eq!(doc["info"]["version"], "2.1.0");
        // Every class becomes a component schema.
        assert_eq!(doc["components"]["schemas"]["Wine"]["type"], "object");
        assert_eq!(doc["components"]["schemas"]["Region"]["type"], "object");
    }

    #[test]
    fn class_refs_retarget_to_components_schemas() {
        let doc = build_openapi(&cellar_schema());
        // The class-ranged slot refs the OpenAPI components location, not `$defs`.
        assert_eq!(
            doc["components"]["schemas"]["Wine"]["properties"]["region"],
            json!({ "$ref": "#/components/schemas/Region" })
        );
        // A `$ref` nested inside an `anyOf` array is retargeted too (the
        // recursion has to descend into arrays, not just objects).
        assert_eq!(
            doc["components"]["schemas"]["Wine"]["properties"]["origin"],
            json!({ "anyOf": [ { "$ref": "#/components/schemas/Region" }, { "type": "string" } ] })
        );
        // No JSON-Schema `#/$defs/` ref survives anywhere in the document.
        assert!(
            !serde_json::to_string(&doc).unwrap().contains("#/$defs/"),
            "all $refs must be retargeted to #/components/schemas/"
        );
    }

    #[test]
    fn title_and_version_fall_back_when_unset() {
        let mut schema = SchemaDefinition::new("bare");
        schema
            .classes
            .insert("C".to_string(), crate::linkml::ClassDefinition::new("C"));
        let doc = build_openapi(&schema);
        assert_eq!(doc["info"]["title"], "bare");
        assert_eq!(doc["info"]["version"], "0.0.0");
    }

    // Oracle: each component schema is a usable JSON Schema — validated by
    // rebuilding a JSON-Schema `$defs` bundle from the components (refs read
    // within the same document) and checking a conforming/nonconforming Wine.
    #[test]
    fn component_schemas_validate_instances() {
        let doc = build_openapi(&cellar_schema());
        // Rebuild a self-contained JSON-Schema doc: the OpenAPI `$ref`s already
        // point within `#/components/schemas`, so mirror that container so they
        // resolve, and point the root at Wine.
        let bundle = json!({
            "$ref": "#/components/schemas/Wine",
            "components": doc["components"].clone(),
        });
        let v = jsonschema::validator_for(&bundle).expect("component schemas compile");
        assert!(
            v.is_valid(&json!({ "name": "Morgon", "region": { "id": "beaujolais" } })),
            "a well-formed instance validates"
        );
        assert!(
            !v.is_valid(&json!({ "region": { "id": "x" } })),
            "a missing required property fails"
        );
        assert!(
            !v.is_valid(&json!({ "name": "x", "region": "not-an-object" })),
            "a scalar where a class ref is declared fails"
        );
    }
}
