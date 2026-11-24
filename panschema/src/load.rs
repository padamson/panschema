//! Schema loading for the commands. The model crate's loader returns the
//! warnings a load raises, on success and on failure; the commands print
//! them here, so a library consumer of the model decides for itself how to
//! report them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::import_resolve::{ImportReport, LoadFailure, LoadWarning, LoadedSchema};
use crate::io::{IoResult, ReaderLookup};
use crate::linkml::SchemaDefinition;
use crate::report::{Effect, Reporter};

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
    report(
        crate::import_resolve::load_schema_with_deps(input, registry, deps),
        "",
    )
}

/// [`load_schema_with_deps`], with the report of the files its imports
/// resolved to, for an output built from those files.
pub fn load_schema_with_report(
    input: &Path,
    registry: &dyn ReaderLookup,
    deps: &BTreeMap<String, PathBuf>,
) -> IoResult<(SchemaDefinition, ImportReport)> {
    let (loaded, imports) =
        match crate::import_resolve::load_schema_with_report(input, registry, deps) {
            Ok((loaded, imports)) => (Ok(loaded), imports),
            Err(failure) => (Err(failure), ImportReport::default()),
        };
    Ok((report(loaded, "")?, imports))
}

/// [`load_schema_with_deps`], reporting each load warning through
/// `reporter` as a finding about the schema file it came from, a warning
/// `--strict` refuses with the effect `refused`.
pub fn load_schema_reported(
    input: &Path,
    registry: &dyn ReaderLookup,
    deps: &BTreeMap<String, PathBuf>,
    refused: Effect,
    reporter: &mut Reporter,
) -> anyhow::Result<SchemaDefinition> {
    let (warnings, outcome) = split(crate::import_resolve::load_schema_with_deps(
        input, registry, deps,
    ));
    reporter.load_warnings(refused, &warnings)?;
    outcome.map_err(|e| anyhow::anyhow!("{e}"))
}

/// Print a load's warnings, each after `prefix`, and return its schema or
/// its error.
pub(crate) fn report(
    loaded: Result<LoadedSchema, LoadFailure>,
    prefix: &str,
) -> IoResult<SchemaDefinition> {
    let (warnings, outcome) = split(loaded);
    for warning in &warnings {
        eprintln!("warning: {prefix}{warning}");
    }
    outcome
}

/// A load's warnings, raised whether it succeeded or failed, and its schema
/// or its error.
fn split(
    loaded: Result<LoadedSchema, LoadFailure>,
) -> (Vec<LoadWarning>, IoResult<SchemaDefinition>) {
    match loaded {
        Ok(loaded) => (loaded.warnings, Ok(loaded.schema)),
        Err(failure) => (failure.warnings, Err(failure.error)),
    }
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

    #[test]
    fn a_reported_load_names_each_warning_by_its_kind_and_the_schema_file() {
        use crate::report::{MessageFormat, Reporter};
        let registry = FormatRegistry::with_defaults();
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("schema.yaml");
        std::fs::write(
            &path,
            "name: s\nid: https://example.org/s\nclasses:\n  Parcel:\n    frobnicate: 1\n",
        )
        .unwrap();
        let mut stdout = Vec::new();
        let schema = {
            let mut reporter =
                Reporter::to(MessageFormat::Json, true, std::io::sink(), &mut stdout);
            load_schema_reported(
                &path,
                &registry,
                &BTreeMap::new(),
                Effect::Warns,
                &mut reporter,
            )
        }
        .expect("the schema loads");
        assert!(
            schema.classes.contains_key("Parcel"),
            "the schema is returned"
        );
        let lines: Vec<serde_json::Value> = String::from_utf8(stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 1, "one warning, one finding: {lines:?}");
        let finding = &lines[0];
        assert_eq!(finding["kind"], "schema/unmodeled-construct", "{finding}");
        assert_eq!(finding["level"], "warning", "{finding}");
        assert_eq!(finding["place"], serde_json::Value::Null, "{finding}");
        assert_eq!(finding["file"], path.display().to_string(), "{finding}");
        assert!(
            finding["message"].as_str().unwrap().contains("frobnicate"),
            "{finding}"
        );
    }

    #[test]
    fn a_reported_load_that_fails_reports_the_warnings_it_raised_and_returns_the_error() {
        use crate::report::{MessageFormat, Reporter};
        let registry = FormatRegistry::with_defaults();
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        // The root overriding `Parcel` warns; two imports disagreeing on
        // `Crate` fails the load.
        for (file, body) in [
            (
                "root.yaml",
                "name: root\nid: https://example.org/root\nimports: [a, b]\nclasses:\n  Parcel:\n    description: root\n",
            ),
            (
                "a.yaml",
                "name: a\nid: https://example.org/a\nclasses:\n  Parcel:\n    description: a\n  Crate:\n    description: a\n",
            ),
            (
                "b.yaml",
                "name: b\nid: https://example.org/b\nclasses:\n  Crate:\n    description: b\n",
            ),
        ] {
            std::fs::write(dir.join(file), body).unwrap();
        }
        let mut stdout = Vec::new();
        let outcome = {
            let mut reporter =
                Reporter::to(MessageFormat::Json, false, std::io::sink(), &mut stdout);
            load_schema_reported(
                &dir.join("root.yaml"),
                &registry,
                &BTreeMap::new(),
                Effect::Warns,
                &mut reporter,
            )
        };
        let err = outcome.expect_err("two imports defining `Crate` differently do not load");
        assert!(err.to_string().contains("`Crate`"), "{err}");
        let stdout = String::from_utf8(stdout).unwrap();
        let lines: Vec<serde_json::Value> = stdout
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(
            lines
                .iter()
                .any(|line| line["kind"] == "schema/root-overrides-import"
                    && line["file"] == dir.join("a.yaml").display().to_string()
                    && line["message"]
                        .as_str()
                        .is_some_and(|m| m.contains("`Parcel`"))),
            "the override raised before the failure is still reported, about the import \
             whose definition was dropped; got:\n{stdout}"
        );
    }

    /// The reference ontology is the canonical example, and the instance
    /// graph fixture is the one the e2e tier renders: anyone running either
    /// should see output, not a warning about the example itself.
    #[test]
    fn the_example_ontologies_load_without_warnings() {
        let registry = FormatRegistry::with_defaults();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        for name in ["reference.ttl", "instance_graph.ttl"] {
            let loaded = crate::import_resolve::load_schema(&fixtures.join(name), &registry)
                .unwrap_or_else(|failure| panic!("{name} loads: {}", failure.error));
            assert!(
                loaded.warnings.is_empty(),
                "{name} loads with warnings: {:?}",
                loaded.warnings
            );
        }
    }
}
