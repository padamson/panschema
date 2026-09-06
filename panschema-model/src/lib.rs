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
