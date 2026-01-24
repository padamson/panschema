# CLAUDE.md

This file provides guidance for Claude Code when working on the rontodoc project.

## Project Overview

**rontodoc** is a Rust-based ontology documentation generator designed to replace heavy Java-based tools (Widoco, LODE) with a fast, single-binary alternative. The goal is to make documenting OWL/RDF ontologies as easy as documenting a Rust crate.

**Current Status:** Slice 1a complete - project scaffold with CLI, dependencies, and passing CI checks.

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
rontodoc/
├── .github/workflows/         # CI (test.yml) and release (release.yml)
├── docs/
│   ├── adr/                   # Architecture Decision Records
│   ├── features/              # Feature specifications
│   └── templates/             # ADR and feature templates
├── src/
│   └── main.rs                # CLI entry point
├── tests/
│   └── fixtures/
│       └── reference.ttl      # Reference ontology for testing
├── Cargo.toml                 # Project manifest
├── CHANGELOG.md               # Keep updated with changes
├── README.md                  # User-facing documentation
└── WHY.md                     # Project motivation/vision
```

## Reference Ontology

The reference ontology at `tests/fixtures/reference.ttl` is used for testing and serves as the canonical example of rontodoc in action.

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
- [docs/adr/001-core-architecture.md](docs/adr/001-core-architecture.md) - Pipeline architecture
- [docs/adr/002-crate-selection.md](docs/adr/002-crate-selection.md) - Dependency decisions
- [docs/features/01-foundational-ui-stack.md](docs/features/01-foundational-ui-stack.md) - First feature spec
- [docs/templates/TEMPLATE_FEATURES.md](docs/templates/TEMPLATE_FEATURES.md) - Feature template
- [docs/templates/TEMPLATE_ADR.md](docs/templates/TEMPLATE_ADR.md) - ADR template

## Next Steps (Project TODOs)

- Push to GitHub and verify CI runs green
- Tag v0.0.1 to verify release workflow
- Implement Slice 1b: Walking Skeleton (parse .ttl → generate HTML)
