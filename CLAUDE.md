# CLAUDE.md

This file provides guidance for Claude Code when working on the panschema project.

## Project Overview

**panschema** is a universal CLI for schema conversion, documentation, validation, and comparison. Think of it as pandoc for data modeling — supporting OWL, LinkML, JSON Schema, and more.

**Current Status:** v0.2.0 — Reader/Writer architecture with OWL/Turtle and LinkML-YAML input and multiple output writers: HTML docs, the RDF/OWL family (Turtle, JSON-LD, RDF/XML, N-Triples), graph JSON, Rust, Postgres DDL, and SHACL shapes.

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

This is a Cargo workspace with three members (`panschema`, `panschema-viz`,
`mdbook-panschema`). The layout below groups modules by role rather than
listing every file — see `panschema/src/` for the full set.

```
panschema/                        # workspace root
├── .github/workflows/            # CI (test.yml) and release (release.yml)
├── panschema/                    # main crate — CLI + readers + writers
│   ├── src/
│   │   ├── main.rs               # CLI entry (generate, serve, styleguide, init, release, publish, verify, completions)
│   │   ├── io.rs                 # Reader/Writer traits + FormatRegistry
│   │   ├── linkml.rs             # LinkML IR (SchemaDefinition, ClassDefinition, ...)
│   │   ├── linkml_resolve.rs     # is_a / mixin / slot_usage resolution + effective cardinality
│   │   ├── owl_reader.rs         # OWL/Turtle → IR (owl_model.rs: reader types)
│   │   ├── yaml_reader.rs        # LinkML YAML → IR
│   │   ├── html_writer.rs        # IR → HTML documentation
│   │   ├── rdf_serializers.rs    # IR → OWL/TTL/JSON-LD/RDF-XML/N-Triples + SHACL graph (owl_writer.rs, shacl_writer.rs)
│   │   ├── graph_writer.rs       # IR → graph JSON (consumed by panschema-viz)
│   │   ├── rust_writer.rs        # IR → Rust structs/enums
│   │   ├── postgres_writer.rs    # IR → Postgres DDL
│   │   ├── diagnostics.rs        # silent-drop / unprojected-construct warnings
│   │   ├── import_resolve.rs     # local `imports:` resolution + merge
│   │   ├── source.rs, cache.rs, lockfile.rs, manifest.rs, publish.rs  # release + publish pipeline
│   │   └── server.rs, components.rs, labels.rs                        # dev server + component preview
│   ├── templates/                # Askama HTML templates + components/
│   └── tests/
│       ├── fixtures/reference.ttl  # reference ontology for testing
│       ├── e2e.rs                # browser tests with Playwright
│       └── integration.rs        # CLI integration tests
├── panschema-viz/                # WASM force-graph visualization (embedded in HTML output)
├── mdbook-panschema/             # mdBook preprocessor embedding rendered schema components
├── docs/                         # adr/, features/, templates/, ROADMAP.md, linkml-coverage.md
├── scripts/                      # mutants.sh, dev-install.sh, ...
├── CHANGELOG.md                  # Keep updated with changes
├── README.md                     # User-facing documentation
└── WHY.md                        # Project motivation/vision
```

## Architecture

panschema uses a Reader/Writer architecture with LinkML as the internal representation:

```
Input File → Reader → LinkML IR → Writer → Output
```

`FormatRegistry` (io.rs) holds every reader and writer. Readers cover
OWL/Turtle and LinkML YAML; writers cover HTML, the RDF/OWL family (Turtle,
JSON-LD, RDF/XML, N-Triples), graph JSON, Rust, Postgres DDL, and SHACL. One
concrete path — OWL/Turtle in, HTML out — is `OwlReader → SchemaDefinition →
HtmlWriter`; any reader pairs with any writer.

## Reference Ontology

The reference ontology at `panschema/tests/fixtures/reference.ttl` is used for testing and serves as the canonical example.

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

## Optional Features

- **`gpu`**: GPU-accelerated force simulation for graph visualization (requires wgpu)
  - Build: `cargo build --features gpu`
  - Test: `cargo test --features gpu --lib`
  - See: [docs/features/04-schema-force-graph-visualization.md](docs/features/04-schema-force-graph-visualization.md)
