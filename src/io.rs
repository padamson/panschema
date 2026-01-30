//! Reader/Writer traits and format dispatch
//!
//! This module defines the core traits for reading schemas from various formats
//! and writing schemas to various output formats.
//!
//! Reference: [ADR-004: Reader/Writer Architecture](../docs/adr/004-reader-writer-architecture.md)

// Allow dead code in this module - FormatRegistry and some error variants are
// infrastructure for the reader/writer architecture. They will be used when
// adding support for additional input formats (YAML, JSON-LD) and output formats.
#![allow(dead_code)]

use std::path::Path;

use thiserror::Error;

use crate::html_writer::HtmlWriter;
use crate::linkml::SchemaDefinition;
use crate::owl_reader::OwlReader;
use crate::owl_writer::OwlWriter;
use crate::rdf_serializers::{JsonLdWriter, NTriplesWriter, RdfXmlWriter};
use crate::yaml_reader::YamlReader;

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

    /// File extensions this reader can handle (e.g., ["ttl", "turtle"])
    fn supported_extensions(&self) -> &[&str];

    /// Check if this reader can handle the given file extension
    fn supports_extension(&self, ext: &str) -> bool {
        self.supported_extensions()
            .iter()
            .any(|e| e.eq_ignore_ascii_case(ext))
    }
}

/// A writer outputs the LinkML IR to a specific format
///
/// Writers are responsible for:
/// - Converting LinkML IR to the output format
/// - Handling format-specific annotations appropriately
pub trait Writer {
    /// Write the schema to the output path
    fn write(&self, schema: &SchemaDefinition, output: &Path) -> IoResult<()>;

    /// Identifier for this output format (e.g., "html", "ttl", "yaml")
    fn format_id(&self) -> &str;
}

/// Registry of available readers and writers
pub struct FormatRegistry {
    readers: Vec<Box<dyn Reader>>,
    writers: Vec<Box<dyn Writer>>,
}

impl Default for FormatRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            readers: Vec::new(),
            writers: Vec::new(),
        }
    }

    /// Create a registry with all default readers and writers registered
    ///
    /// Currently registers:
    /// - Readers: `OwlReader` (ttl, turtle), `YamlReader` (yaml, yml)
    /// - Writers: `HtmlWriter` (html), `OwlWriter` (ttl), `JsonLdWriter` (jsonld),
    ///   `RdfXmlWriter` (rdfxml), `NTriplesWriter` (ntriples)
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register_reader(Box::new(OwlReader::new()));
        registry.register_reader(Box::new(YamlReader::new()));
        registry.register_writer(Box::new(HtmlWriter::new()));
        registry.register_writer(Box::new(OwlWriter::new()));
        registry.register_writer(Box::new(JsonLdWriter::new()));
        registry.register_writer(Box::new(RdfXmlWriter::new()));
        registry.register_writer(Box::new(NTriplesWriter::new()));
        registry
    }

    /// Register a reader
    pub fn register_reader(&mut self, reader: Box<dyn Reader>) {
        self.readers.push(reader);
    }

    /// Register a writer
    pub fn register_writer(&mut self, writer: Box<dyn Writer>) {
        self.writers.push(writer);
    }

    /// Find a reader for the given file extension
    pub fn reader_for_extension(&self, ext: &str) -> Option<&dyn Reader> {
        self.readers
            .iter()
            .find(|r| r.supports_extension(ext))
            .map(|r| r.as_ref())
    }

    /// Find a writer by format ID
    pub fn writer_for_format(&self, format_id: &str) -> Option<&dyn Writer> {
        self.writers
            .iter()
            .find(|w| w.format_id().eq_ignore_ascii_case(format_id))
            .map(|w| w.as_ref())
    }

    /// Get file extension from a path
    pub fn extension_from_path(path: &Path) -> Option<&str> {
        path.extension().and_then(|e| e.to_str())
    }

    /// Find a reader for the given path based on its extension
    pub fn reader_for_path(&self, path: &Path) -> IoResult<&dyn Reader> {
        let ext = Self::extension_from_path(path)
            .ok_or_else(|| IoError::UnknownExtension(path.display().to_string()))?;

        self.reader_for_extension(ext)
            .ok_or_else(|| IoError::UnsupportedFormat(ext.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // Mock reader for testing
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

    // Mock writer for testing
    struct MockWriter {
        format: &'static str,
    }

    impl Writer for MockWriter {
        fn write(&self, _schema: &SchemaDefinition, _output: &Path) -> IoResult<()> {
            Ok(())
        }

        fn format_id(&self) -> &str {
            self.format
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
    fn registry_finds_reader_by_extension() {
        let mut registry = FormatRegistry::new();
        registry.register_reader(Box::new(MockReader {
            extensions: vec!["ttl"],
        }));

        assert!(registry.reader_for_extension("ttl").is_some());
        assert!(registry.reader_for_extension("yaml").is_none());
    }

    #[test]
    fn registry_finds_writer_by_format() {
        let mut registry = FormatRegistry::new();
        registry.register_writer(Box::new(MockWriter { format: "html" }));

        assert!(registry.writer_for_format("html").is_some());
        assert!(registry.writer_for_format("HTML").is_some()); // case insensitive
        assert!(registry.writer_for_format("ttl").is_none());
    }

    #[test]
    fn registry_reader_for_path_extracts_extension() {
        let mut registry = FormatRegistry::new();
        registry.register_reader(Box::new(MockReader {
            extensions: vec!["ttl"],
        }));

        let path = PathBuf::from("/some/path/ontology.ttl");
        assert!(registry.reader_for_path(&path).is_ok());

        let unknown_path = PathBuf::from("/some/path/ontology.xyz");
        assert!(matches!(
            registry.reader_for_path(&unknown_path),
            Err(IoError::UnsupportedFormat(_))
        ));
    }

    #[test]
    fn extension_from_path_works() {
        assert_eq!(
            FormatRegistry::extension_from_path(Path::new("test.ttl")),
            Some("ttl")
        );
        assert_eq!(
            FormatRegistry::extension_from_path(Path::new("test.YAML")),
            Some("YAML")
        );
        assert_eq!(
            FormatRegistry::extension_from_path(Path::new("noextension")),
            None
        );
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

    #[test]
    fn mock_writer_succeeds() {
        let writer = MockWriter { format: "html" };
        let schema = SchemaDefinition::new("test");
        let result = writer.write(&schema, Path::new("output"));
        assert!(result.is_ok());
    }

    #[test]
    fn with_defaults_registers_owl_reader() {
        let registry = FormatRegistry::with_defaults();

        // Should find reader for .ttl files
        assert!(registry.reader_for_extension("ttl").is_some());
        assert!(registry.reader_for_extension("turtle").is_some());
    }

    #[test]
    fn with_defaults_registers_yaml_reader() {
        let registry = FormatRegistry::with_defaults();

        // Should find reader for .yaml files
        assert!(registry.reader_for_extension("yaml").is_some());
        assert!(registry.reader_for_extension("yml").is_some());

        // Should not find reader for unsupported formats
        assert!(registry.reader_for_extension("json").is_none());
    }

    #[test]
    fn with_defaults_registers_html_writer() {
        let registry = FormatRegistry::with_defaults();

        // Should find writer for html format
        assert!(registry.writer_for_format("html").is_some());
        assert!(registry.writer_for_format("HTML").is_some()); // case insensitive

        // Should not find writer for unsupported formats
        assert!(registry.writer_for_format("markdown").is_none());
    }

    #[test]
    fn with_defaults_reader_can_read_ttl_file() {
        let registry = FormatRegistry::with_defaults();
        let path = PathBuf::from("tests/fixtures/reference.ttl");

        let reader = registry
            .reader_for_path(&path)
            .expect("Should find TTL reader");
        let schema = reader.read(&path).expect("Should parse TTL file");

        // The schema name comes from the ontology, title has the full label
        assert_eq!(schema.name, "reference");
        assert_eq!(
            schema.title,
            Some("panschema Reference Ontology".to_string())
        );
    }

    #[test]
    fn with_defaults_registers_owl_writer() {
        let registry = FormatRegistry::with_defaults();

        // Should find writer for ttl format
        assert!(registry.writer_for_format("ttl").is_some());
        assert!(registry.writer_for_format("TTL").is_some()); // case insensitive
    }

    #[test]
    fn with_defaults_registers_jsonld_writer() {
        let registry = FormatRegistry::with_defaults();

        assert!(registry.writer_for_format("jsonld").is_some());
        assert!(registry.writer_for_format("JSONLD").is_some()); // case insensitive
    }

    #[test]
    fn with_defaults_registers_rdfxml_writer() {
        let registry = FormatRegistry::with_defaults();

        assert!(registry.writer_for_format("rdfxml").is_some());
        assert!(registry.writer_for_format("RDFXML").is_some()); // case insensitive
    }

    #[test]
    fn with_defaults_registers_ntriples_writer() {
        let registry = FormatRegistry::with_defaults();

        assert!(registry.writer_for_format("ntriples").is_some());
        assert!(registry.writer_for_format("NTRIPLES").is_some()); // case insensitive
    }
}
