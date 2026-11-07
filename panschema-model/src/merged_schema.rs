//! A schema and everything it imports, as one LinkML YAML document.
//!
//! [`merged_schema_document`] writes the document a consumer loads
//! standalone in place of a schema split across files and packages. It
//! merges the YAML documents themselves rather than writing the IR back
//! out, so a construct the IR does not model (`comments`, `subsets`, a
//! class's `any_of`) survives into the file.
//!
//! The merge reads as LinkML reads the unmerged schema, which
//! `conformance/probe_merge_imports.py` measures: the root's definitions
//! and prefix bindings win, each import's are added beneath them (an
//! importer's before its imports', an earlier import's before a later
//! one's), and the `linkml:` modules stay imports. `gen-linkml
//! --mergeimports` moves every imported element into the root's
//! namespace; here an element an import defines without a URI is written
//! with the IRI its own file gives it, the one the loader pins.

use std::path::Path;

use serde_norway::{Mapping, Value};

use crate::import_resolve::ImportReport;
use crate::io::{IoError, ReaderLookup};
use crate::linkml::SchemaDefinition;
use crate::yaml_reader::YamlReader;

/// The sections whose named elements an import contributes.
const ELEMENT_SECTIONS: [&str; 5] = ["classes", "slots", "enums", "types", "subsets"];

/// `input` with the imports its load read merged in, as LinkML YAML that
/// loads on its own and means what `input` means. `schema` and `imports`
/// are what [`crate::import_resolve::load_schema_with_report`] returned for
/// `input`, so whatever refuses that load is refused before the merge
/// starts; the merge reads the files the report names, in the precedence it
/// gives them, and takes each imported element's URI from `schema`. Every
/// file merged must be LinkML YAML.
pub fn merged_schema_document(
    input: &Path,
    schema: &SchemaDefinition,
    imports: &ImportReport,
) -> Result<String, IoError> {
    let mut merged = read_document(input)?;
    for path in &imports.loaded_files {
        fold(&mut merged, read_document(path)?, schema);
    }
    let builtins: Vec<Value> = imports
        .builtin_imports
        .iter()
        .cloned()
        .map(Value::String)
        .collect();
    if builtins.is_empty() {
        merged.shift_remove("imports");
    } else {
        merged.insert("imports".into(), Value::Sequence(builtins));
    }
    serde_norway::to_string(&Value::Mapping(merged))
        .map_err(|e| IoError::Parse(format!("could not write the merged schema: {e}")))
}

fn read_document(path: &Path) -> Result<Mapping, IoError> {
    if YamlReader::new().reader_for_path(path).is_err() {
        return Err(IoError::Parse(format!(
            "`{}` is not LinkML YAML, and a merged LinkML schema is built from YAML documents",
            path.display()
        )));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| IoError::Parse(format!("could not read `{}`: {e}", path.display())))?;
    match serde_norway::from_str(&text) {
        Ok(Value::Mapping(document)) => Ok(document),
        Ok(_) => Err(IoError::Parse(format!(
            "`{}` is not a LinkML schema document",
            path.display()
        ))),
        Err(e) => Err(IoError::Parse(format!(
            "could not parse `{}`: {e}",
            path.display()
        ))),
    }
}

/// Add the elements and prefixes of an imported `document` that `merged`
/// does not already hold. Each class, slot, attribute, and enum carries
/// the URI the loaded `schema` gives it, which for an import's element is
/// the IRI its own file mints.
fn fold(merged: &mut Mapping, mut document: Mapping, schema: &SchemaDefinition) {
    for section in ELEMENT_SECTIONS.into_iter().chain(["prefixes"]) {
        let Some(Value::Mapping(entries)) = document.remove(section) else {
            continue;
        };
        let target = section_mut(merged, section);
        for (name, mut definition) in entries {
            if target.contains_key(&name) {
                continue;
            }
            if let Some(name) = name.as_str() {
                pin_uris(section, name, &mut definition, schema);
            }
            target.insert(name, definition);
        }
    }
}

fn section_mut<'a>(document: &'a mut Mapping, section: &str) -> &'a mut Mapping {
    let entry = document
        .entry(section.into())
        .or_insert_with(|| Value::Mapping(Mapping::new()));
    if !entry.is_mapping() {
        *entry = Value::Mapping(Mapping::new());
    }
    entry.as_mapping_mut().expect("set to a mapping above")
}

fn pin_uris(section: &str, name: &str, definition: &mut Value, schema: &SchemaDefinition) {
    match section {
        "classes" => {
            let Some(class) = schema.classes.get(name) else {
                return;
            };
            set_uri(definition, "class_uri", class.class_uri.as_deref());
            let Some(Value::Mapping(attributes)) = definition.get_mut("attributes") else {
                return;
            };
            for (attribute, attribute_definition) in attributes.iter_mut() {
                let uri = attribute
                    .as_str()
                    .and_then(|a| class.attributes.get(a))
                    .and_then(|a| a.slot_uri.as_deref());
                set_uri(attribute_definition, "slot_uri", uri);
            }
        }
        "slots" => {
            let uri = schema.slots.get(name).and_then(|s| s.slot_uri.as_deref());
            set_uri(definition, "slot_uri", uri);
        }
        "enums" => {
            let uri = schema.enums.get(name).and_then(|e| e.enum_uri.as_deref());
            set_uri(definition, "enum_uri", uri);
        }
        _ => {}
    }
}

/// Write `uri` under `key`, turning a bodyless definition (`Widget:`) into a
/// mapping to hold it.
fn set_uri(definition: &mut Value, key: &str, uri: Option<&str>) {
    let Some(uri) = uri else {
        return;
    };
    if definition.is_null() {
        *definition = Value::Mapping(Mapping::new());
    }
    if let Value::Mapping(fields) = definition {
        fields.insert(key.into(), Value::String(uri.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::import_resolve::{load_schema, load_schema_with_report};
    use crate::io::{IoResult, Reader};

    const ROOT: &str = "\
name: root
id: https://example.org/root
default_prefix: root
default_range: string
prefixes:
  root: https://example.org/root/
  lib: https://example.org/elsewhere/
imports:
  - linkml:types
  - lib
  - other
classes:
  Widget:
    description: the root's
  Plain:
";

    const LIB: &str = "\
name: lib
id: https://example.org/lib
default_prefix: lib
default_range: integer
prefixes:
  lib: https://example.org/lib/
  shared: https://example.org/shared/
imports:
  - linkml:types
  - deep
subsets:
  core:
    description: lib's
classes:
  Widget:
    description: lib's
  Gadget:
    comments:
      - a construct the IR does not model
    slots:
      - label
    slot_usage:
      label:
    attributes:
      size:
slots:
  label:
enums:
  Color:
    permissible_values:
      red:
";

    const DEEP: &str = "\
name: deep
id: https://example.org/deep
subsets:
  core:
    description: deep's
classes:
  Deep:
";

    /// An import after `lib` whose `core` subset differs from lib's, which
    /// the loader does not compare, so only merge order decides it.
    const OTHER: &str = "\
name: other
id: https://example.org/other
subsets:
  core:
    description: other's
";

    fn write_package(dir: &Path) -> PathBuf {
        std::fs::write(dir.join("lib.yaml"), LIB).unwrap();
        std::fs::write(dir.join("deep.yaml"), DEEP).unwrap();
        std::fs::write(dir.join("other.yaml"), OTHER).unwrap();
        let root = dir.join("root.yaml");
        std::fs::write(&root, ROOT).unwrap();
        root
    }

    fn merge_with(
        root: &Path,
        readers: &dyn ReaderLookup,
        deps: &BTreeMap<String, PathBuf>,
    ) -> Result<String, IoError> {
        let (loaded, imports) = load_schema_with_report(root, readers, deps).expect("load");
        merged_schema_document(root, &loaded.schema, &imports)
    }

    fn merge(root: &Path) -> Mapping {
        let text = merge_with(root, &YamlReader::new(), &BTreeMap::new()).expect("merge");
        serde_norway::from_str(&text).expect("the merged document parses")
    }

    fn field<'a>(document: &'a Mapping, path: &[&str]) -> Option<&'a Value> {
        let (first, rest) = path.split_first()?;
        rest.iter()
            .try_fold(document.get(*first)?, |value, key| value.get(*key))
    }

    /// The merged file, loaded on its own, holds the elements, ranges, IRIs,
    /// and prefixes the split schema loads to.
    #[test]
    fn a_merged_schema_loads_as_the_schema_it_merges() {
        let tmp = tempfile::tempdir().unwrap();
        let root = write_package(tmp.path());
        let unmerged = load_schema(&root, &YamlReader::new()).expect("load").schema;
        let merged_path = tmp.path().join("merged").join("root.yaml");
        std::fs::create_dir(merged_path.parent().unwrap()).unwrap();
        std::fs::write(
            &merged_path,
            merge_with(&root, &YamlReader::new(), &BTreeMap::new()).expect("merge"),
        )
        .unwrap();
        let merged = load_schema(&merged_path, &YamlReader::new())
            .expect("the merged file loads standalone")
            .schema;

        assert_eq!(merged.classes, unmerged.classes, "classes");
        assert_eq!(merged.slots, unmerged.slots, "slots");
        assert_eq!(merged.enums, unmerged.enums, "enums");
        assert_eq!(merged.types, unmerged.types, "types");
        assert_eq!(merged.prefixes, unmerged.prefixes, "prefixes");
    }

    /// The root's definitions and prefix bindings win; an import's absent
    /// ones are added, and the `linkml:` modules stay imports.
    #[test]
    fn the_root_wins_and_builtin_modules_stay_imports() {
        let tmp = tempfile::tempdir().unwrap();
        let merged = merge(&write_package(tmp.path()));
        assert_eq!(
            (
                field(&merged, &["classes", "Widget", "description"]).and_then(Value::as_str),
                field(&merged, &["prefixes", "lib"]).and_then(Value::as_str),
                field(&merged, &["prefixes", "shared"]).and_then(Value::as_str),
                merged.get("imports"),
            ),
            (
                Some("the root's"),
                Some("https://example.org/elsewhere/"),
                Some("https://example.org/shared/"),
                Some(&Value::Sequence(vec![Value::String("linkml:types".into())])),
            )
        );
    }

    /// An import's element is written with the IRI its own file mints, one
    /// whose file has no default prefix gets none, and the root's own
    /// elements are left as written.
    #[test]
    fn an_imported_element_is_written_with_its_own_files_iri() {
        let tmp = tempfile::tempdir().unwrap();
        let merged = merge(&write_package(tmp.path()));
        let uri = |path: &[&str]| field(&merged, path).and_then(Value::as_str);
        assert_eq!(
            (
                uri(&["classes", "Gadget", "class_uri"]),
                uri(&["classes", "Gadget", "attributes", "size", "slot_uri"]),
                uri(&["slots", "label", "slot_uri"]),
                uri(&["enums", "Color", "enum_uri"]),
                uri(&["classes", "Deep", "class_uri"]),
                uri(&["classes", "Widget", "class_uri"]),
            ),
            (
                Some("https://example.org/lib/Gadget"),
                Some("https://example.org/lib/size"),
                Some("https://example.org/lib/label"),
                Some("https://example.org/lib/Color"),
                None,
                None,
            )
        );
    }

    /// A construct the IR does not model survives the merge, and where two
    /// imports write it differently, the first in precedence wins: an
    /// importer's over its own import's, an earlier import's over a later.
    #[test]
    fn the_merge_keeps_what_the_ir_does_not_model() {
        let tmp = tempfile::tempdir().unwrap();
        let merged = merge(&write_package(tmp.path()));
        assert_eq!(
            field(&merged, &["subsets", "core", "description"]).and_then(Value::as_str),
            Some("lib's"),
            "lib's subset beats deep's (lib imports deep) and other's (imported after lib)"
        );
        assert_eq!(
            field(&merged, &["classes", "Gadget", "comments"]),
            Some(&Value::Sequence(vec![Value::String(
                "a construct the IR does not model".into()
            )]))
        );
    }

    /// An import naming a manifest dependency merges from that package, and
    /// a schema with no `linkml:` import is written with no `imports` and
    /// its other keys in the order the root wrote them.
    #[test]
    fn a_dependency_import_merges_from_its_package() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("base.yaml");
        std::fs::write(
            &base,
            "name: base\nid: https://example.org/base\nclasses:\n  Base:\n",
        )
        .unwrap();
        let app_dir = tmp.path().join("app");
        std::fs::create_dir(&app_dir).unwrap();
        let app = app_dir.join("app.yaml");
        std::fs::write(
            &app,
            "name: app\nid: https://example.org/app\nimports:\n  - base\n\
             description: an app\nclasses:\n  App: {}\n",
        )
        .unwrap();
        let deps = BTreeMap::from([("base".to_string(), base)]);
        let text = merge_with(&app, &YamlReader::new(), &deps).expect("merge");
        let merged: Mapping = serde_norway::from_str(&text).unwrap();
        assert_eq!(
            (
                merged.keys().filter_map(Value::as_str).collect::<Vec<_>>(),
                field(&merged, &["classes", "Base"]).is_some(),
            ),
            (vec!["name", "id", "description", "classes"], true),
            "the dependency's class merges, and dropping `imports` keeps the other keys' order"
        );
    }

    /// A remote import an import names by CURIE is written as the IRI its
    /// own file's prefix gives it, so the root binding that prefix
    /// elsewhere does not redirect it; `linkml:` modules stay CURIEs.
    #[test]
    fn a_remote_import_keeps_the_iri_its_own_file_gives_it() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("lib.yaml"),
            "name: lib\nid: https://example.org/lib\nprefixes:\n  ext: https://example.org/ext/\n  \
             linkml: https://w3id.org/linkml/\n\
             imports:\n  - linkml:types\n  - ext:thing\n",
        )
        .unwrap();
        let root = tmp.path().join("root.yaml");
        std::fs::write(
            &root,
            "name: root\nid: https://example.org/root\nprefixes:\n  ext: https://example.org/other/\n\
             imports:\n  - lib\n",
        )
        .unwrap();
        assert_eq!(
            merge(&root).get("imports"),
            Some(&Value::Sequence(vec![
                Value::String("linkml:types".into()),
                Value::String("https://example.org/ext/thing".into()),
            ]))
        );
    }

    /// A lookup that reads every path as LinkML YAML, so an import named
    /// `.ttl` loads and only the merge's own document check refuses it.
    struct EveryPathYaml(YamlReader);

    impl ReaderLookup for EveryPathYaml {
        fn reader_for_path(&self, _path: &Path) -> IoResult<&dyn Reader> {
            Ok(&self.0)
        }
    }

    #[test]
    fn an_import_that_is_not_yaml_refuses_the_merge() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(
            dir.join("owl.ttl"),
            "name: owl\nid: https://example.org/owl\n",
        )
        .unwrap();
        let root = dir.join("root.yaml");
        std::fs::write(
            &root,
            "name: root\nid: https://example.org/root\nimports:\n  - owl.ttl\n",
        )
        .unwrap();
        let error = merge_with(&root, &EveryPathYaml(YamlReader::new()), &BTreeMap::new())
            .expect_err("a non-YAML import refuses")
            .to_string();
        assert!(
            error.contains("owl.ttl"),
            "the refusal names the import: {error}"
        );
    }
}
