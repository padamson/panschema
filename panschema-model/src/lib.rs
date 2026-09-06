//! The LinkML model panschema reads and writes.
//!
//! Every reader in the toolchain turns a format into this model and every
//! writer turns it into a projection; a tool that reasons over schemas and
//! instance data depends on this crate and never on a projection. It
//! carries no format library, no template engine, and no CLI.

pub mod diagnostics;
pub mod import_resolve;
pub mod instances;
pub mod io;
pub mod linkml;
pub mod linkml_resolve;
pub mod primitives;
pub mod rules;
pub mod yaml_reader;
