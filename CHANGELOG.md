# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- `Contributor` struct for Dublin Core-style contributor metadata (name, ORCID, role)
- `SchemaDefinition` metadata fields: `contributors`, `created`, `modified`, `imports`
- `FormatRegistry::with_defaults()` for dynamic reader/writer dispatch
- `YamlReader` for native LinkML YAML schemas (yaml, yml)
- `OwlWriter` for writing LinkML IR to OWL/Turtle format (ttl)
- Library crate (`lib.rs`) exposing public API for integration testing

### Changed
- `main.rs` and `server.rs` now use `FormatRegistry` instead of hardcoded readers/writers

## [0.2.0] - 2026-01-25

Project renamed from **rontodoc** to **panschema** to reflect broader schema support.

### Added
- **LinkML Internal Representation (IR)**: Canonical data model based on LinkML metamodel
- **Reader/Writer Architecture**: Extensible pipeline for multi-format support
- `OwlReader`: Parses OWL/Turtle to LinkML IR
- `HtmlWriter`: Generates HTML documentation from LinkML IR
- Support for OWL individuals with type links and property values

### Changed
- **BREAKING**: Binary renamed from `rontodoc` to `panschema`
- **BREAKING**: Crate renamed from `rontodoc` to `panschema`
- Internal architecture refactored to use Reader → IR → Writer pipeline
- Classes map to LinkML `ClassDefinition` with hierarchy preserved
- Properties map to LinkML `SlotDefinition` with domain/range
- XSD datatypes mapped to LinkML built-in types

### Removed
- Old monolithic parser and renderer (replaced by Reader/Writer architecture)

## [0.1.0] - 2026-01-24

Initial release of rontodoc — a fast, single-binary ontology documentation generator.

### Added
- CLI with `generate` and `serve` subcommands.
- Turtle (.ttl) parser for OWL ontologies: classes, properties, individuals, and metadata.
- Class cards with labels, descriptions, IRIs, and class hierarchy (superclass/subclass links).
- Property cards with type badges, domain/range, and inverse-of relationships.
- Individual cards with type links and property values.
- Sidebar navigation with section links and count badges.
- Development server with hot reload for live documentation preview.
- Responsive two-column layout with dark mode support.
- Component-driven UI with style guide (`--features dev`).

[Unreleased]: https://github.com/padamson/panschema/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/padamson/panschema/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/padamson/panschema/releases/tag/v0.1.0
