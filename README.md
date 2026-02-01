# panschema

> A universal CLI for schema conversion, documentation, validation, and comparison.

**Status:** Active Development

## Vision

**panschema** aims to be the universal tool for data modeling workflows:

- **Convert** between schema languages (LinkML, OWL/TTL, JSON Schema, SHACL, SQL DDL)
- **Generate documentation** from any supported format
- **Validate** schemas and check compatibility
- **Compare** schemas and track changes

Think of it as **pandoc for data modeling** — a single tool that speaks all schema languages.

## Current Features

- **Multi-format input/output** via Reader/Writer architecture
- **LinkML IR** as canonical internal representation
- **Fast**: Generate complete documentation in milliseconds
- **CI-native**: Single binary, no JVM or complex dependencies
- **Hot reload**: Development server with live preview
- **GPU visualization** (optional `gpu` feature): 3D force-directed graph for schema exploration

See [CHANGELOG.md](CHANGELOG.md) for detailed version history.

## Installation

```bash
cargo install panschema
```

Or download pre-built binaries from [GitHub Releases](https://github.com/padamson/panschema/releases).

## Quick Start

Generate documentation from an OWL ontology:

```bash
panschema generate --input ontology.ttl --output docs/
```

Start a development server with hot reload:

```bash
panschema serve --input ontology.ttl
```

Open http://localhost:3000 to view the documentation.

## Supported Formats

### Input Formats
| Format | Status | Extension |
|--------|--------|-----------|
| OWL/Turtle | Full support | `.ttl` |
| LinkML YAML | Full support | `.yaml`, `.yml` |
| JSON Schema | Planned | `.json` |
| SHACL | Planned | `.ttl` |

### Output Formats
| Format | Status |
|--------|--------|
| HTML Documentation | Full support |
| OWL/Turtle | Full support |
| LinkML YAML | Planned |
| Markdown | Planned |
| JSON Schema | Planned |

## Architecture

panschema uses a Reader/Writer architecture with LinkML as the internal representation:

```
Input File → Reader → LinkML IR → Writer → Output
   (TTL)    (OwlReader)  (SchemaDefinition)  (HtmlWriter)  (HTML)
```

This design enables:
- Adding new input formats by implementing the `Reader` trait
- Adding new output formats by implementing the `Writer` trait
- Format-agnostic documentation and conversion

## Graph Visualization

panschema includes an interactive force-directed graph visualization for exploring schema relationships directly in the browser.

### Browser Visualization (2D Canvas)

The generated HTML documentation includes an animated graph visualization:

```bash
# Generate documentation with graph (default)
panschema generate --input schema.yaml --output docs/

# Disable graph visualization
panschema generate --input schema.yaml --output docs/ --no-graph

# Force specific visualization mode
panschema generate --input schema.yaml --output docs/ --viz-mode 2d
```

The visualization features:
- **Animated force layout**: Nodes organize themselves based on connections
- **Pan and zoom**: Mouse drag to pan, scroll wheel to zoom
- **Labels**: Node and edge labels with automatic positioning
- **Touch support**: Works on mobile devices

### GPU Visualization (Native - Optional)

For native GPU-accelerated visualization during development:

```bash
# Build with GPU feature
cargo build --features gpu

# Run tests
cargo test --features gpu --lib
```

See [examples/university/](examples/university/) for a sample schema and [docs/features/04-schema-force-graph-visualization.md](docs/features/04-schema-force-graph-visualization.md) for the full feature plan.

## Why panschema?

Read our [WHY.md](WHY.md) to understand the full vision.

**TL;DR:** Data modeling is fragmented across many schema languages. panschema provides a unified interface — fast, CI-native, and extensible.

## Development

### Prerequisites

- Rust 1.85+ (edition 2024)
- `cargo-nextest` (recommended for testing)
- Node.js 20+ and Playwright browsers (for E2E tests)

```bash
# Install Playwright browsers
npx playwright@1.56.1 install
```

### Building & Testing

```bash
cargo build
cargo nextest run --features dev
```

### Building WASM Visualization

The browser visualization is built with wasm-pack. The WASM files are embedded in the panschema binary.

```bash
# Install wasm-pack (one time)
cargo install wasm-pack

# Build WASM (from repository root)
cd panschema-viz && wasm-pack build --target web

# Rebuild panschema to embed updated WASM
cargo build
```

After building, generated HTML will include the animated graph visualization automatically.

### Manual Verification

```bash
panschema serve --input tests/fixtures/reference.ttl
```

### UI Component Style Guide

```bash
cargo watch -w src -w templates -x 'run --features dev -- styleguide --serve'
```

## Contributing

Contributions are welcome! Please follow our standards:
- **TDD First**: Write tests before implementation
- **Strict Linting**: Pass `cargo fmt` and `cargo clippy`
- **Pre-commit**: Use our pre-commit hooks

## License

Apache-2.0
