# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/padamson/rontodoc/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/padamson/rontodoc/releases/tag/v0.1.0
