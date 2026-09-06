//! The LinkML model panschema reads and writes.
//!
//! Every reader in the toolchain turns a format into this model and every
//! writer turns it into a projection; a tool that reasons over schemas and
//! instance data depends on this crate and never on a projection. It
//! carries no format library, no template engine, and no CLI.
//!
//! # Reading a schema and its dataset
//!
//! [`load_dataset`] is the entry: it loads a schema with its `imports:`
//! resolved, reads a dataset's records against it, and hands back both,
//! so each record's minted IRI — the same string panschema's RDF and graph
//! outputs name it by — is one call away.
//!
//! ```
//! use std::fs;
//!
//! use panschema_model::instances::instance_iri_string;
//! use panschema_model::yaml_reader::YamlReader;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let dir = tempfile::tempdir()?;
//! let schema_path = dir.path().join("catalog.yaml");
//! fs::write(
//!     &schema_path,
//!     "\
//! name: catalog
//! id: https://example.org/catalog
//! default_prefix: cat
//! prefixes:
//!   cat: https://example.org/catalog/
//! default_range: string
//! classes:
//!   Catalog:
//!     tree_root: true
//!     attributes:
//!       wines:
//!         range: Wine
//!         multivalued: true
//!   Wine:
//!     attributes:
//!       id:
//!         identifier: true
//!       name: {}
//! ",
//! )?;
//! let data_path = dir.path().join("catalog-data.yaml");
//! fs::write(
//!     &data_path,
//!     "wines:\n  - id: chateauMorgon\n    name: Château Morgon\n",
//! )?;
//!
//! // A consumer that reads only LinkML YAML is its own reader lookup.
//! let loaded = panschema_model::load_dataset(&schema_path, &data_path, &YamlReader::new())?;
//! for warning in &loaded.warnings {
//!     eprintln!("warning: {warning}");
//! }
//! for record in &loaded.instances.instances {
//!     println!("{}", instance_iri_string(&loaded.schema, record));
//! }
//! # let iris: Vec<String> = loaded
//! #     .instances
//! #     .instances
//! #     .iter()
//! #     .map(|record| instance_iri_string(&loaded.schema, record))
//! #     .collect();
//! # assert_eq!(iris, ["https://example.org/catalog/chateauMorgon"]);
//! # Ok(())
//! # }
//! ```
//!
//! Prints:
//!
//! ```text
//! https://example.org/catalog/chateauMorgon
//! ```

use std::path::Path;

pub mod diagnostics;
pub mod import_resolve;
pub mod instances;
pub mod io;
pub mod linkml;
pub mod linkml_resolve;
pub mod primitives;
pub mod rules;
pub mod yaml_reader;

use import_resolve::{LoadFailure, LoadedSchema};
use instances::InstanceSet;
use io::{IoError, IoResult, ReaderLookup};
use linkml::SchemaDefinition;

/// A schema, the dataset read against it, and the warnings the load raised.
#[derive(Debug)]
pub struct LoadedDataset {
    pub schema: SchemaDefinition,
    pub instances: InstanceSet,
    /// What the load reported, in the order a command prints it. Returning
    /// them is the library's whole job here; deciding where they go is the
    /// caller's.
    pub warnings: Vec<String>,
}

/// Load a schema with its `imports:` resolved and read a LinkML dataset —
/// a `tree_root` container of records — against it, expanding anchors and
/// resolving every name the way panschema's own commands do.
///
/// `readers` supplies the reader for each schema file by extension; a
/// consumer that reads only LinkML YAML passes a [`yaml_reader::YamlReader`],
/// which is its own lookup. The dataset itself is LinkML YAML.
///
/// A failure carries the warnings raised before it, so nothing a reader
/// reported is lost to the error, and a load that reads no records warns
/// rather than handing back an empty set as if it had succeeded. See the
/// crate documentation for a worked example.
pub fn load_dataset(
    schema: &Path,
    data: &Path,
    readers: &dyn ReaderLookup,
) -> Result<LoadedDataset, LoadFailure> {
    let LoadedSchema { schema, warnings } = import_resolve::load_schema(schema, readers)?;
    let read_records = || -> IoResult<InstanceSet> {
        let text = std::fs::read_to_string(data)
            .map_err(|e| IoError::Parse(format!("could not read `{}`: {e}", data.display())))?;
        let value: serde_norway::Value = serde_norway::from_str(&text)
            .map_err(|e| IoError::Parse(format!("could not parse `{}`: {e}", data.display())))?;
        Ok(InstanceSet::from_linkml_data(&schema, &value))
    };
    let instances = match read_records() {
        Ok(instances) => instances,
        Err(error) => return Err(LoadFailure { warnings, error }),
    };
    // Records anchor on the schema's `tree_root` container, so a schema
    // without one reads nothing from any dataset, and a dataset shaped for
    // some other schema reads nothing from this one. Both come back as an
    // empty set, which is indistinguishable from success unless the load
    // says so.
    let mut warnings = warnings;
    if !schema.classes.values().any(|class| class.tree_root) {
        warnings.push(format!(
            "the schema declares no `tree_root` container, so no records can be read from `{}`",
            data.display()
        ));
    } else if instances.instances.is_empty() {
        warnings.push(format!(
            "no records read from `{}`; its shape does not match the schema's `tree_root` \
             container",
            data.display()
        ));
    }
    Ok(LoadedDataset {
        schema,
        instances,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml_reader::YamlReader;

    /// A dataset file that is not there fails the load, naming the file.
    #[test]
    fn a_dataset_that_is_not_there_names_the_file() {
        let failure = load_dataset(
            Path::new("tests/fixtures/catalog.yaml"),
            Path::new("tests/fixtures/no_such_dataset.yaml"),
            &YamlReader::new(),
        )
        .expect_err("a missing dataset fails the load");
        let message = failure.error.to_string();
        assert!(
            message.contains("could not read") && message.contains("no_such_dataset.yaml"),
            "the error names the file it could not read: {message}"
        );
    }

    /// A dataset that is not YAML fails the load naming that file — not the
    /// schema, the other YAML path in play — and the warnings the schema's own
    /// load raised come back with the error rather than vanishing.
    #[test]
    fn a_dataset_that_does_not_parse_names_it_and_keeps_the_schema_warnings() {
        let dir = tempfile::tempdir().expect("tempdir");
        let data = dir.path().join("catalog_data.yaml");
        std::fs::write(
            &data,
            "wines:\n  - id: chateauMorgon\n   name: bad indent\n",
        )
        .expect("write the dataset");

        let failure = load_dataset(
            Path::new("tests/fixtures/dangling_range.yaml"),
            &data,
            &YamlReader::new(),
        )
        .expect_err("a dataset that is not YAML fails the load");
        let message = failure.error.to_string();
        assert!(
            message.contains("could not parse") && message.contains("catalog_data.yaml"),
            "the error names the dataset it could not parse: {message}"
        );
        assert!(
            failure.warnings.iter().any(|w| w.contains("Customer")),
            "the schema's own load warnings survive the failure; got {:?}",
            failure.warnings
        );
    }

    /// Records anchor on a `tree_root` container, so a schema without one and a
    /// dataset shaped for another schema both read nothing. Either way the load
    /// says which it was, rather than returning an empty set as a success.
    #[test]
    fn a_load_that_reads_no_records_says_why() {
        let no_container = load_dataset(
            Path::new("tests/fixtures/dangling_range.yaml"),
            Path::new("tests/fixtures/catalog_data.yaml"),
            &YamlReader::new(),
        )
        .expect("a schema without a container still loads");
        assert!(no_container.instances.instances.is_empty());
        assert!(
            no_container
                .warnings
                .iter()
                .any(|w| w.contains("declares no `tree_root` container")),
            "the load names the missing container; got {:?}",
            no_container.warnings
        );

        let dir = tempfile::tempdir().expect("tempdir");
        let data = dir.path().join("grapes.yaml");
        std::fs::write(&data, "grapes:\n  - id: zinfandel\n").expect("write the dataset");
        let mismatched = load_dataset(
            Path::new("tests/fixtures/catalog.yaml"),
            &data,
            &YamlReader::new(),
        )
        .expect("a mismatched dataset still loads");
        assert!(mismatched.instances.instances.is_empty());
        assert!(
            mismatched
                .warnings
                .iter()
                .any(|w| w.contains("no records read from") && w.contains("grapes.yaml")),
            "the load names the dataset that matched nothing; got {:?}",
            mismatched.warnings
        );
    }
}
