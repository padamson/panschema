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
- Two-column documentation layout with fixed sidebar navigation.
- Fixed header that stays visible while scrolling.
- New components: sidebar, namespace table, class card, property card, section header.
- CSS design tokens for consistent theming (colors, typography, spacing).
- Dark mode support via `prefers-color-scheme` media query.
- Responsive mobile layout with slide-out sidebar (768px breakpoint).
- End-to-end browser tests using playwright-rs.
- Cross-browser E2E testing support (chromium, firefox, webkit) via `BROWSER` env var.
- Class extraction from OWL ontologies (owl:Class with rdfs:label, rdfs:comment, rdfs:subClassOf).
- Class cards in documentation showing label, description, IRI, and class hierarchy.
- Class hierarchy display (superclass/subclass relationships with links).
