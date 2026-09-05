//! A crate that depends on `panschema-model` alone parses a LinkML YAML
//! schema and resolves a class's effective slots — the same answers
//! `panschema` gives, since it is the same code.

use std::path::Path;

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
