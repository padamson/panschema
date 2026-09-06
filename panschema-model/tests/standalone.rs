//! The model crate on its own. A crate that depends on `panschema-model`
//! alone parses a LinkML YAML schema, resolves a class's effective slots,
//! loads a schema split across files through the YAML reader as its only
//! lookup, gets the load warnings back as values, and reads the
//! schema-declared slot-semantics families as data — the same answers
//! `panschema` gives, since it is the same code.

use std::path::Path;

use panschema_model::diagnostics::{SLOT_SEMANTICS_FAMILIES, schema_load_diagnostics};
use panschema_model::import_resolve::load_schema;
use panschema_model::instances::instance_iri_string;
use panschema_model::io::Reader;
use panschema_model::linkml_resolve::resolve_effective_slots;
use panschema_model::yaml_reader::YamlReader;

#[test]
fn the_sample_schema_parses_and_resolves_without_the_tool_crate() {
    let schema = YamlReader::new()
        .read(Path::new("tests/fixtures/sample_schema.yaml"))
        .expect("the sample schema parses");
    assert_eq!(schema.name, "sample_schema");

    let person = schema.classes.get("Person").expect("Person is a class");
    let resolved = resolve_effective_slots(person, &schema);
    let mut slots: Vec<&str> = resolved.keys().map(String::as_str).collect();
    slots.sort_unstable();
    assert_eq!(slots, ["age", "email", "name"], "Person's effective slots");
}

#[test]
fn a_split_schema_loads_through_a_yaml_only_lookup_with_its_imports_merged() {
    let loaded = load_schema(
        Path::new("tests/fixtures/imports/app.yaml"),
        &YamlReader::new(),
    )
    .expect("the root and its local import load");
    let schema = &loaded.schema;
    assert!(schema.classes.contains_key("Customer"), "the root's class");
    assert!(
        schema.classes.contains_key("Address"),
        "the class defined only in the imported file is merged in"
    );
    assert!(schema.enums.contains_key("Country"), "the import's enum");
    assert!(
        loaded.warnings.is_empty(),
        "a clean split schema loads without warnings; got {:?}",
        loaded.warnings
    );
}

#[test]
fn load_warnings_are_returned_as_values_and_are_the_load_diagnostics() {
    let loaded = load_schema(
        Path::new("tests/fixtures/dangling_range.yaml"),
        &YamlReader::new(),
    )
    .expect("a schema with a dangling range still loads");
    assert!(
        loaded.warnings.iter().any(|w| w.contains("Customer")),
        "the dangling range is reported by name; got {:?}",
        loaded.warnings
    );
    assert_eq!(
        loaded.warnings,
        schema_load_diagnostics(&loaded.schema),
        "the warnings a load returns are exactly the schema's load diagnostics"
    );
}

#[test]
fn the_three_slot_semantics_families_are_readable_as_data() {
    let mut names: Vec<&str> = SLOT_SEMANTICS_FAMILIES
        .iter()
        .map(|f| f.annotation)
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        ["asserts_absence", "expand_against", "records_version_of"],
        "every schema-declared slot-semantics family is enumerable"
    );
}

/// The entry a tool reads a dataset through: one call loads the schema with
/// its imports, reads the records against it, and hands back both, so each
/// record's minted IRI is one lookup away.
#[test]
fn a_consumer_reads_a_schema_and_its_dataset_through_one_entry() {
    let loaded = panschema_model::load_dataset(
        Path::new("tests/fixtures/catalog.yaml"),
        Path::new("tests/fixtures/catalog_data.yaml"),
        &YamlReader::new(),
    )
    .expect("the schema and its dataset load");

    assert_eq!(loaded.schema.name, "catalog");
    let iris: Vec<String> = loaded
        .instances
        .instances
        .iter()
        .map(|record| instance_iri_string(&loaded.schema, record))
        .collect();
    assert_eq!(
        iris,
        [
            "https://example.org/catalog/chateauMorgon",
            "https://example.org/catalog/napaCabernet",
        ],
        "each record's IRI mints under the schema's default prefix"
    );
    assert!(
        loaded.warnings.is_empty(),
        "a clean schema loads without warnings; got {:?}",
        loaded.warnings
    );
}
