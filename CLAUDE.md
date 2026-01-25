# CLAUDE.md

This file provides guidance for Claude Code when working on the panschema project.

## Project Overview

**panschema** is a universal CLI for schema conversion, documentation, validation, and comparison. Think of it as pandoc for data modeling — supporting OWL, LinkML, JSON Schema, and more.

**Current Status:** v0.2.0 - Reader/Writer architecture complete with OWL input and HTML output.

## Build Commands

```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo test                     # Run tests (standard)
cargo nextest run              # Run tests (recommended, faster)
cargo fmt                      # Format code
cargo fmt -- --check           # Check formatting
cargo clippy --all-targets --all-features -- -D warnings  # Lint
```

## Project Structure

```
panschema/
├── .github/workflows/         # CI (test.yml) and release (release.yml)
├── docs/
│   ├── adr/                   # Architecture Decision Records
│   ├── features/              # Feature specifications
│   └── templates/             # ADR and feature templates
├── src/
│   ├── main.rs                # CLI entry point (generate/serve/styleguide)
│   ├── components.rs          # Component rendering for isolated preview
│   ├── io.rs                  # Reader/Writer traits and FormatRegistry
│   ├── linkml.rs              # LinkML IR (SchemaDefinition, ClassDefinition, etc.)
│   ├── owl_model.rs           # OWL-specific types for OwlReader
│   ├── owl_reader.rs          # OWL/Turtle → LinkML IR
│   ├── html_writer.rs         # LinkML IR → HTML documentation
│   ├── server.rs              # Dev server with hot reload
│   └── snapshots/             # Insta snapshot files
├── templates/
│   ├── index.html             # Main documentation page
│   ├── styleguide.html        # Component showcase page
│   └── components/            # Reusable UI components
├── tests/
│   ├── fixtures/
│   │   └── reference.ttl      # Reference ontology for testing
│   ├── e2e.rs                 # Browser tests with Playwright
│   └── integration.rs         # CLI integration tests
├── Cargo.toml                 # Project manifest
├── CHANGELOG.md               # Keep updated with changes
├── README.md                  # User-facing documentation
└── WHY.md                     # Project motivation/vision
```

## Architecture

panschema uses a Reader/Writer architecture with LinkML as the internal representation:

```
Input File → Reader → LinkML IR → Writer → Output
   (TTL)    (OwlReader)  (SchemaDefinition)  (HtmlWriter)  (HTML)
```

## Reference Ontology

The reference ontology at `tests/fixtures/reference.ttl` is used for testing and serves as the canonical example.

## Development Methodology

- **TDD First:** Write tests before implementation
- **Vertical Slicing:** Deliver features end-to-end in testable slices
- **Walking Skeleton:** Start with simplest valuable feature

## Code Quality Requirements

All code must pass before merge:
- `cargo fmt --check` - no formatting errors
- `cargo clippy --all-targets --all-features -- -D warnings` - no warnings
- `cargo test` / `cargo nextest run` - all tests pass
- `cargo doc` - documentation builds cleanly

Pre-commit hooks enforce these automatically.

## Feature Development Process

1. Create feature spec in `docs/features/` using template
2. Define vertical slices with acceptance criteria
3. Implement slice-by-slice with TDD
4. Update CHANGELOG.md and README.md
5. Document architectural decisions in `docs/adr/`

## CI/CD

- **Testing:** Runs on Linux, macOS, Windows via GitHub Actions
- **Releases:** Triggered by git tags (v*.*.*), builds cross-platform binaries
- **Dependabot:** Weekly updates for Cargo and GitHub Actions

## Key Files to Know

- [WHY.md](WHY.md) - Project motivation and vision
- [docs/ROADMAP.md](docs/ROADMAP.md) - Feature roadmap and release plan
- [docs/adr/003-linkml-as-internal-representation.md](docs/adr/003-linkml-as-internal-representation.md) - IR architecture
- [docs/adr/004-reader-writer-architecture.md](docs/adr/004-reader-writer-architecture.md) - Reader/Writer design
- [docs/features/03-reader-writer-architecture.md](docs/features/03-reader-writer-architecture.md) - Feature spec
