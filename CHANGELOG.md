# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- CLI with `generate` and `serve` subcommands.
- `styleguide` subcommand for rontodoc development (requires `--features dev`).
- Turtle (.ttl) parser using sophia crate to extract ontology metadata (IRI, label, comment, version).
- HTML renderer using askama templates to generate documentation.
- Development server (`rontodoc serve`) with hot reload using axum and tower-livereload.
- File watcher (notify) that auto-regenerates documentation on input file changes.
- Reference ontology (`tests/fixtures/reference.ttl`) for testing.
- Component-driven UI development workflow with isolated component templates.
- Style guide (`rontodoc styleguide`) showing all UI components.
- Reusable components: header, footer, hero, metadata card.
- Snapshot tests using insta for component HTML output.
- Component development documentation (`docs/components.md`).
