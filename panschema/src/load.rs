//! Schema loading for the commands. The model crate's loader returns the
//! warnings a load raises, on success and on failure; the commands print
//! them here, so a library consumer of the model decides for itself how to
//! report them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::io::{IoResult, ReaderLookup};
use crate::linkml::SchemaDefinition;

/// Load a schema with its local imports resolved, printing each load
/// warning to stderr the way every command reports them.
pub fn load_schema(input: &Path, registry: &dyn ReaderLookup) -> IoResult<SchemaDefinition> {
    load_schema_with_deps(input, registry, &BTreeMap::new())
}

/// [`load_schema`] resolving `imports:` entries that name manifest
/// dependencies through `deps` as well.
pub fn load_schema_with_deps(
    input: &Path,
    registry: &dyn ReaderLookup,
    deps: &BTreeMap<String, PathBuf>,
) -> IoResult<SchemaDefinition> {
    let (warnings, outcome) =
        match crate::import_resolve::load_schema_with_deps(input, registry, deps) {
            Ok(loaded) => (loaded.warnings, Ok(loaded.schema)),
            Err(failure) => (failure.warnings, Err(failure.error)),
        };
    for warning in &warnings {
        eprintln!("warning: {warning}");
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::FormatRegistry;

    /// A rangeless property in an imported OWL/Turtle file stays genuinely
    /// rangeless: its file carries no default, so the deferred fill never
    /// touches it, and the untyped-slot diagnostic still sees it — a mixed
    /// YAML-root/Turtle-import schema reports what the Turtle file reports
    /// standalone.
    #[test]
    fn an_imported_turtle_files_rangeless_property_stays_untyped() {
        let registry = FormatRegistry::with_defaults();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(
            dir.join("vocab.ttl"),
            "@prefix owl: <http://www.w3.org/2002/07/owl#> .\n\
             @prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .\n\
             @prefix ex: <https://example.org/vocab#> .\n\
             ex: a owl:Ontology .\n\
             ex:Widget a owl:Class .\n\
             ex:label a owl:DatatypeProperty ; rdfs:domain ex:Widget .\n",
        )
        .unwrap();
        let root_path = dir.join("root.yaml");
        std::fs::write(
            &root_path,
            "name: root\nid: https://example.org/root\nimports:\n  - vocab\n",
        )
        .unwrap();

        let schema = load_schema(&root_path, &registry).expect("load");
        let label = schema
            .classes
            .get("Widget")
            .and_then(|c| c.attributes.get("label"))
            .or_else(|| schema.slots.get("label"))
            .expect("the imported property is in the merged schema");
        assert_eq!(
            label.range, None,
            "no default reaches a property whose own file has none"
        );
        assert!(
            crate::diagnostics::untyped_slots(&schema)
                .iter()
                .any(|u| u.name == "label"),
            "and the untyped-slot diagnostic still reports it"
        );
    }
}
