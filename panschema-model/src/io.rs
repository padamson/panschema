//! The reader side of panschema's I/O: the `Reader` trait every format
//! reader implements, the error type readers and writers share, and the
//! one lookup the import resolver needs. Writers and the format registry
//! that dispatches to them live in `panschema`, since which formats exist
//! is a tool-side fact.
//!
//! Reference: [ADR-011](../../docs/adr/011-panschema-model-crate-boundary.md)

use std::path::Path;

use thiserror::Error;

use crate::linkml::SchemaDefinition;

/// Errors that can occur during reading or writing
#[derive(Error, Debug)]
pub enum IoError {
    /// The file format is not supported
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),

    /// The file extension could not be determined
    #[error("could not determine file format from path: {0}")]
    UnknownExtension(String),

    /// An I/O error occurred
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A parsing error occurred
    #[error("parse error: {0}")]
    Parse(String),

    /// A rendering/writing error occurred
    #[error("write error: {0}")]
    Write(String),
}

/// Result type for reader/writer operations
pub type IoResult<T> = Result<T, IoError>;

/// A reader parses an input format into the LinkML IR
///
/// Readers are responsible for:
/// - Parsing the input file format
/// - Mapping format-specific constructs to LinkML IR
/// - Preserving format-specific metadata in annotations
pub trait Reader {
    /// Parse the input file into a SchemaDefinition
    fn read(&self, input: &Path) -> IoResult<SchemaDefinition>;

    /// Like [`Reader::read`], additionally returning human-readable
    /// warnings about constructs the reader dropped — projections the IR
    /// cannot hold, such as an external `rdfs:subPropertyOf` parent or
    /// surplus axioms on a single-valued field. The schema is identical to
    /// what `read` returns; the warnings only add visibility, and the load
    /// path prints them alongside the schema-load diagnostics. The default
    /// wraps `read` with no warnings; a reader that projects lossily
    /// overrides this and implements `read` in terms of it.
    fn read_with_warnings(&self, input: &Path) -> IoResult<(SchemaDefinition, Vec<String>)> {
        self.read(input).map(|schema| (schema, Vec::new()))
    }

    /// File extensions this reader can handle (e.g., ["ttl", "turtle"])
    fn supported_extensions(&self) -> &[&str];

    /// Check if this reader can handle the given file extension
    fn supports_extension(&self, ext: &str) -> bool {
        self.supported_extensions()
            .iter()
            .any(|e| e.eq_ignore_ascii_case(ext))
    }
}

/// Something that can find the reader for a path — the one thing the
/// import resolver asks of a format registry. `panschema`'s registry
/// implements it; a consumer that reads only LinkML YAML can implement it
/// over a single reader.
pub trait ReaderLookup {
    /// The reader for `path`, judged by its extension.
    fn reader_for_path(&self, path: &Path) -> IoResult<&dyn Reader>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockReader {
        extensions: Vec<&'static str>,
    }

    impl Reader for MockReader {
        fn read(&self, _input: &Path) -> IoResult<SchemaDefinition> {
            Ok(SchemaDefinition::new("mock_schema"))
        }

        fn supported_extensions(&self) -> &[&str] {
            &self.extensions
        }
    }

    #[test]
    fn reader_supports_extension_case_insensitive() {
        let reader = MockReader {
            extensions: vec!["ttl", "turtle"],
        };
        assert!(reader.supports_extension("ttl"));
        assert!(reader.supports_extension("TTL"));
        assert!(reader.supports_extension("turtle"));
        assert!(!reader.supports_extension("owl"));
    }

    #[test]
    fn io_error_display() {
        let err = IoError::UnsupportedFormat("xyz".to_string());
        assert_eq!(err.to_string(), "unsupported format: xyz");

        let err = IoError::Parse("invalid syntax".to_string());
        assert_eq!(err.to_string(), "parse error: invalid syntax");
    }

    #[test]
    fn mock_reader_returns_schema() {
        let reader = MockReader {
            extensions: vec!["ttl"],
        };
        let result = reader.read(Path::new("test.ttl"));
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name, "mock_schema");
    }
}
