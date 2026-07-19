//! JSON Schema writer
//!
//! Projects the LinkML IR to a [JSON Schema](https://json-schema.org/)
//! (draft 2020-12): one closed `object` schema per class under `$defs`, with
//! each class's effective slots as typed, required-aware properties. The
//! language-agnostic structured-output contract — an LLM (via `rig`,
//! Anthropic, or OpenAI structured output) can be constrained to valid
//! instances, and any JSON validator can check instance data against it.
//!
//! A scalar range projects to its typed property (with `pattern` / numeric
//! bounds applied), an enum range to a JSON `enum` of its permissible values, a
//! class range to a `$ref` to that class's `$def`, a custom `types:` range to
//! its resolved base scalar + facets (following the `typeof` chain or the
//! type's `uri` datatype), and an `any_of` range to `anyOf` over its branches.
//! Inherited/mixed-in slots flatten onto each subclass. A range that resolves
//! to none of these is emitted permissively (`true`) so
//! `additionalProperties: false` never rejects an otherwise-valid instance.

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
    let defs = build_class_defs(schema);

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

/// Build one closed `object` schema per class, keyed by class name — the
/// `$defs` map. `$ref`s between classes point at `#/$defs/<Class>`; the OpenAPI
/// writer retargets those to `#/components/schemas/<Class>`. Deterministic
/// (`serde_json::Map` and the resolver's slot order are both sorted).
pub(crate) fn build_class_defs(schema: &SchemaDefinition) -> serde_json::Map<String, Value> {
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

    defs
}

/// The JSON Schema for a single slot: its value schema (see
/// [`slot_value_schema`]), wrapped in an `array` when the slot is multivalued.
fn slot_property(slot: &SlotDefinition, schema: &SchemaDefinition) -> Value {
    let base = slot_value_schema(slot, schema);
    if crate::linkml_resolve::effective_cardinality(slot).multivalued {
        json!({ "type": "array", "items": base })
    } else {
        base
    }
}

/// The (unwrapped) JSON Schema for a slot's value: an enum's permissible
/// values as a JSON `enum`, a class range as a `$ref` to its `$def`, or a
/// scalar type with any value constraints (`pattern`, numeric bounds) applied.
fn slot_value_schema(slot: &SlotDefinition, schema: &SchemaDefinition) -> Value {
    // A polymorphic `any_of` range: the value must satisfy at least one branch,
    // which is JSON Schema `anyOf` over each branch's value schema.
    if !slot.any_of.is_empty() {
        let branches: Vec<Value> = slot
            .any_of
            .iter()
            .map(|branch| slot_value_schema(branch, schema))
            .collect();
        return json!({ "anyOf": branches });
    }

    let range = slot
        .range
        .as_deref()
        .or(schema.default_range.as_deref())
        .unwrap_or("string");

    // Enum range → the permissible values as a JSON `enum` (BTreeMap key order,
    // so the list is deterministic).
    if let Some(enum_def) = schema.enums.get(range) {
        let values: Vec<Value> = enum_def
            .permissible_values
            .keys()
            .map(|k| json!(k))
            .collect();
        return json!({ "enum": values });
    }
    // Class range → a `$ref` to its `$def`.
    if schema.classes.contains_key(range) {
        return json!({ "$ref": format!("#/$defs/{range}") });
    }
    // A custom `types:` entry resolves to its base scalar + facets; a built-in
    // scalar maps directly. Slot-level constraints layer on top of either.
    let mut base = if schema.types.contains_key(range) {
        resolve_type_schema(schema, range, &mut Vec::new())
    } else {
        scalar_json_type(range)
    };
    apply_value_constraints(&mut base, slot);
    base
}

/// Resolve a range naming a schema `types:` entry to a JSON Schema: follow the
/// `typeof` chain (or the type's `uri` datatype) down to a base scalar, and
/// carry each type's `pattern` (a nearer type's pattern overrides). A `typeof`
/// cycle terminates at `string`.
fn resolve_type_schema(schema: &SchemaDefinition, name: &str, seen: &mut Vec<String>) -> Value {
    let Some(type_def) = schema.types.get(name) else {
        // Not a custom type — a built-in scalar name (or unrecognized → `true`).
        return scalar_json_type(name);
    };
    if seen.iter().any(|s| s == name) {
        return json!({ "type": "string" });
    }
    seen.push(name.to_string());

    let mut base = match (&type_def.typeof_, &type_def.uri) {
        (Some(parent), _) => resolve_type_schema(schema, parent, seen),
        (None, Some(uri)) => xsd_json_type(uri),
        (None, None) => json!({ "type": "string" }),
    };
    if let Some(pattern) = &type_def.pattern
        && let Some(obj) = base.as_object_mut()
    {
        obj.insert("pattern".to_string(), json!(pattern));
    }
    base
}

/// Map an XSD datatype (a type's `uri`, e.g. `xsd:integer`) to its JSON Schema
/// type/format, keyed on the local name; unknown datatypes fall back to string.
fn xsd_json_type(uri: &str) -> Value {
    let local = uri
        .rsplit([':', '#', '/'])
        .next()
        .unwrap_or(uri)
        .to_ascii_lowercase();
    match local.as_str() {
        "integer" | "int" | "long" | "short" | "nonnegativeinteger" | "positiveinteger"
        | "negativeinteger" | "nonpositiveinteger" | "unsignedint" | "unsignedlong" => {
            json!({ "type": "integer" })
        }
        "decimal" | "float" | "double" => json!({ "type": "number" }),
        "boolean" => json!({ "type": "boolean" }),
        "date" => json!({ "type": "string", "format": "date" }),
        "datetime" => json!({ "type": "string", "format": "date-time" }),
        "time" => json!({ "type": "string", "format": "time" }),
        "anyuri" => json!({ "type": "string", "format": "uri" }),
        _ => json!({ "type": "string" }),
    }
}

/// Add a slot's value constraints to its scalar schema: `pattern` for strings,
/// `minimum`/`maximum` for numbers. A permissive (`true`) base has no object to
/// attach to, so it's left as-is.
fn apply_value_constraints(base: &mut Value, slot: &SlotDefinition) {
    let Some(obj) = base.as_object_mut() else {
        return;
    };
    if let Some(pattern) = &slot.pattern {
        obj.insert("pattern".to_string(), json!(pattern));
    }
    if let Some(min) = slot.minimum_value {
        obj.insert("minimum".to_string(), json!(min));
    }
    if let Some(max) = slot.maximum_value {
        obj.insert("maximum".to_string(), json!(max));
    }
}

/// Map a LinkML built-in scalar range to its JSON Schema type/format. A range
/// that isn't a recognised scalar built-in returns `true` — the "any" schema.
/// Enum and class ranges are handled by [`slot_value_schema`] before this is
/// reached, so here that fallback covers only unrecognised custom types.
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

    /// A schema exercising an enum range, a class range, and value constraints
    /// (`pattern`, numeric bounds) — the Slice 2 surface.
    fn rich_schema() -> SchemaDefinition {
        serde_yaml::from_str(
            "\
name: cellar
classes:
  Wine:
    tree_root: true
    attributes:
      name:
        range: string
        required: true
      color:
        range: ColorEnum
      strength:
        range: float
        minimum_value: 0.0
        maximum_value: 1.0
      code:
        range: string
        pattern: '^[A-Z]{3}$'
      region:
        range: Region
      grapes:
        range: Region
        multivalued: true
  Region:
    attributes:
      id:
        range: string
      name:
        range: string
enums:
  ColorEnum:
    permissible_values:
      red:
      white:
      rose:
",
        )
        .expect("parse rich schema")
    }

    #[test]
    fn enum_class_and_constraints_project() {
        let doc = build_json_schema(&rich_schema());
        let wine = &doc["$defs"]["Wine"];

        // Enum range → the permissible values as a JSON `enum` (sorted).
        assert_eq!(
            wine["properties"]["color"],
            json!({ "enum": ["red", "rose", "white"] })
        );
        // Class range → a `$ref` to its `$def`; multivalued → array of `$ref`.
        assert_eq!(
            wine["properties"]["region"],
            json!({ "$ref": "#/$defs/Region" })
        );
        assert_eq!(
            wine["properties"]["grapes"],
            json!({ "type": "array", "items": { "$ref": "#/$defs/Region" } })
        );
        // Value constraints project onto the scalar base.
        assert_eq!(
            wine["properties"]["strength"],
            json!({ "type": "number", "minimum": 0.0, "maximum": 1.0 })
        );
        assert_eq!(
            wine["properties"]["code"],
            json!({ "type": "string", "pattern": "^[A-Z]{3}$" })
        );
    }

    // Oracle: instances exercising an enum value, a nested class ref, a
    // pattern, and a numeric bound validate as expected.
    #[test]
    fn rich_instances_validate_as_expected() {
        let doc = build_json_schema(&rich_schema());
        // Validate against the whole rooted document (Wine is the tree_root, so
        // the root `$ref`s it) — cross-class `$ref`s resolve within `$defs`,
        // which a `Wine`-fragment-in-isolation validator couldn't do.
        let v = jsonschema::validator_for(&doc).expect("the document compiles");

        assert!(
            v.is_valid(&json!({
                "name": "Chateau Morgon", "color": "red", "strength": 0.5,
                "code": "ABC", "region": { "id": "r1", "name": "Beaujolais" },
                "grapes": [{ "id": "g1", "name": "Gamay" }]
            })),
            "a well-formed instance should validate"
        );
        assert!(
            !v.is_valid(&json!({ "name": "x", "color": "teal" })),
            "an out-of-enum value should fail"
        );
        assert!(
            !v.is_valid(&json!({ "name": "x", "code": "abcd" })),
            "a pattern mismatch should fail"
        );
        assert!(
            !v.is_valid(&json!({ "name": "x", "strength": 1.5 })),
            "an out-of-bound number should fail"
        );
        assert!(
            !v.is_valid(&json!({ "name": "x", "region": "not-an-object" })),
            "a scalar where a class ref is declared should fail"
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

    #[test]
    fn inherited_slots_flatten_onto_the_subclass() {
        // Dog is_a Animal; Dog's `$def` must carry Animal's slots directly, so
        // a consumer validates a Dog without chasing `is_a`.
        let mut schema: SchemaDefinition = serde_yaml::from_str(
            "\
name: zoo
classes:
  Animal:
    attributes:
      species:
        range: string
        required: true
  Dog:
    is_a: Animal
    attributes:
      breed:
        range: string
",
        )
        .expect("parse schema");
        // The reader backfills class names from their map keys; a raw parse
        // doesn't, and the effective-slot resolver's cycle-guard keys on the
        // class name — so set them, mirroring the load path.
        for (name, class) in schema.classes.iter_mut() {
            class.name = name.clone();
        }
        let dog = &build_json_schema(&schema)["$defs"]["Dog"];
        assert_eq!(dog["properties"]["species"]["type"], "string");
        assert_eq!(dog["properties"]["breed"]["type"], "string");
        assert_eq!(dog["required"], json!(["species"]));
    }

    #[test]
    fn custom_types_resolve_to_base_scalar_and_facets() {
        let mut schema: SchemaDefinition = serde_yaml::from_str(
            "\
name: contacts
classes:
  Person:
    tree_root: true
    attributes:
      phone:
        range: PhoneNumber
      visits:
        range: Count
      home:
        range: UsPhone
types:
  PhoneNumber:
    typeof: string
    pattern: '^\\+[0-9]+$'
  Count:
    uri: xsd:integer
  UsPhone:
    typeof: PhoneNumber
",
        )
        .expect("parse schema");
        for (name, class) in schema.classes.iter_mut() {
            class.name = name.clone();
        }
        let doc = build_json_schema(&schema);
        let person = &doc["$defs"]["Person"];

        // A `typeof: string` type with a pattern → string + pattern.
        assert_eq!(
            person["properties"]["phone"],
            json!({ "type": "string", "pattern": "^\\+[0-9]+$" })
        );
        // A type carrying only `uri: xsd:integer` → integer.
        assert_eq!(person["properties"]["visits"], json!({ "type": "integer" }));
        // A `typeof` chain resolves to the base scalar and inherits the
        // ancestor's pattern.
        assert_eq!(
            person["properties"]["home"],
            json!({ "type": "string", "pattern": "^\\+[0-9]+$" })
        );

        let v = jsonschema::validator_for(&doc).expect("document compiles");
        assert!(
            v.is_valid(&json!({ "phone": "+1234", "visits": 3 })),
            "conforming values validate"
        );
        assert!(
            !v.is_valid(&json!({ "phone": "not-a-number" })),
            "a value violating the custom type's pattern fails"
        );
    }

    #[test]
    fn xsd_datatypes_map_to_expected_json_types() {
        assert_eq!(xsd_json_type("xsd:integer"), json!({ "type": "integer" }));
        assert_eq!(xsd_json_type("xsd:long"), json!({ "type": "integer" }));
        assert_eq!(
            xsd_json_type("xsd:nonNegativeInteger"),
            json!({ "type": "integer" })
        );
        assert_eq!(xsd_json_type("xsd:decimal"), json!({ "type": "number" }));
        assert_eq!(xsd_json_type("xsd:double"), json!({ "type": "number" }));
        assert_eq!(xsd_json_type("xsd:boolean"), json!({ "type": "boolean" }));
        assert_eq!(
            xsd_json_type("xsd:date"),
            json!({ "type": "string", "format": "date" })
        );
        assert_eq!(
            xsd_json_type("xsd:dateTime"),
            json!({ "type": "string", "format": "date-time" })
        );
        assert_eq!(
            xsd_json_type("xsd:time"),
            json!({ "type": "string", "format": "time" })
        );
        assert_eq!(
            xsd_json_type("xsd:anyURI"),
            json!({ "type": "string", "format": "uri" })
        );
        // A full IRI is matched on its local name.
        assert_eq!(
            xsd_json_type("http://www.w3.org/2001/XMLSchema#integer"),
            json!({ "type": "integer" })
        );
        // An unrecognized datatype falls back to string.
        assert_eq!(xsd_json_type("xsd:hexBinary"), json!({ "type": "string" }));
    }

    #[test]
    fn custom_type_edge_cases_resolve_to_string() {
        let schema: SchemaDefinition = serde_yaml::from_str(
            "\
name: t
types:
  Bare: {}
  Loopy:
    typeof: Loopy
",
        )
        .expect("parse schema");
        // A type with neither `typeof` nor `uri` defaults to string.
        assert_eq!(
            resolve_type_schema(&schema, "Bare", &mut Vec::new()),
            json!({ "type": "string" })
        );
        // A `typeof` cycle terminates at string rather than recursing forever.
        assert_eq!(
            resolve_type_schema(&schema, "Loopy", &mut Vec::new()),
            json!({ "type": "string" })
        );
    }

    #[test]
    fn any_of_range_projects_to_anyof() {
        let schema: SchemaDefinition = serde_yaml::from_str(
            "\
name: cellar
classes:
  Wine:
    tree_root: true
    attributes:
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
        let doc = build_json_schema(&schema);
        assert_eq!(
            doc["$defs"]["Wine"]["properties"]["origin"],
            json!({ "anyOf": [ { "$ref": "#/$defs/Region" }, { "type": "string" } ] })
        );

        // Oracle: either branch validates; a value matching neither fails.
        let v = jsonschema::validator_for(&doc).expect("document compiles");
        assert!(
            v.is_valid(&json!({ "origin": { "id": "beaujolais" } })),
            "the class-ref branch should validate"
        );
        assert!(
            v.is_valid(&json!({ "origin": "Beaujolais" })),
            "the string branch should validate"
        );
        assert!(
            !v.is_valid(&json!({ "origin": 42 })),
            "a value matching neither branch should fail"
        );
    }
}
