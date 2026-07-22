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
- **mdbook integration**: `mdbook-panschema install` adds a maintained toolbar link from an mdbook book to its schema docs
- **Loud about gaps**: warns on LinkML constructs it parses but doesn't model (so nothing is silently dropped); `generate --strict` fails the build instead
- **Postgres DDL**: `generate --format postgres` emits `CREATE TABLE`/`CREATE TYPE` DDL from the same LinkML schema your Rust structs come from — no hand-written SQL to keep in sync
- **SHACL shapes**: `generate --format shacl` emits a SHACL shapes graph so a schema's value constraints are machine-checkable by any SHACL engine, not just visible in the docs
- **JSON Schema / OpenAPI**: `generate --format json-schema` (draft 2020-12) and `--format openapi` (3.1 `components/schemas`) emit a structured-output/API contract from the same LinkML source — an LLM's structured output or a generated TS/Swift client shares the model the Rust types come from
- **Instance-data validation**: `validate --schema schema.yaml --data data.yaml` checks a LinkML instance-data file against the schema and exits non-zero on any violation — a conformance gate for CI or an LLM authoring loop

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
| OWL/Turtle, JSON-LD, RDF/XML, N-Triples | Full support |
| Rust types | Full support |
| Graph JSON | Full support |
| SHACL shapes | Full support |
| Postgres DDL | Partial support (concrete classes, scalars, enums, single-valued class references, and `unique_keys`/`pattern`/value-bound/`rules` constraints) |
| JSON Schema (draft 2020-12) | Full support |
| OpenAPI 3.1 (`components/schemas`) | Full support |
| LinkML YAML | Planned |
| Markdown | Planned |

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
- **Node selection**: Click a node to view its details (label, type, IRI, connections)
- **External groundings**: a class's external `subclass_of` grounding draws an edge to a muted, dashed upstream-category node (labelled by its cached upstream `rdfs:label`, CURIE fallback); classes sharing a grounding share one node
- **Drag to reposition**: Drag any node; release to rejoin the simulation, or shift+release to pin in place
- **Shift+click to toggle pin**: Pin/unpin nodes so they hold their position
- **Keyboard shortcuts**: `R` reset view · `F` focus · `Esc` deselect · `Delete` unpin
- **Touch support**: Pan, orbit, and pinch-zoom on mobile devices

### Instance graph (A-box)

Beneath the schema graph, the docs can also draw an **instance graph** — the
records that populate the schema (its A-box), as a distinct force-directed viz.
It comes from either the schema's embedded OWL individuals, or a separate
**LinkML instance-data file** passed with `--instances`:

```bash
# Render a LinkML data file (a tree_root container of records) as the instance graph
panschema generate --input schema.yaml --instances data.yaml --output docs/
```

Each record becomes a typed node keyed by its identifier; a class-valued slot
becomes an edge to the referenced record, and scalar values ride along as node
metadata — so the JSON an LLM emits against a class's JSON Schema (see
`generate --format json-schema`) is a LinkML instance you can read straight
back and visualize, no OWL detour.

### GPU Visualization (Native - Optional)

For native GPU-accelerated visualization during development:

```bash
# Build with GPU feature
cargo build --features gpu

# Run tests
cargo test --features gpu --lib
```

See [examples/university/](examples/university/) for a sample schema and [docs/features/04-schema-force-graph-visualization.md](docs/features/04-schema-force-graph-visualization.md) for the full feature plan.

## Linking an mdbook book to the schema docs

If you publish both an mdbook book and panschema-generated schema docs on one site, the `mdbook-panschema` plugin installs a maintained toolbar button linking the book to the schema docs — the way `mdbook-admonish install` drops its assets, so you don't hand-roll (and re-fix on every mdbook release) per-book JavaScript.

Declare the link in `panschema-publish.toml`:

```toml
[book_link]
enabled = true
schema_path = "schema/current/"   # book-relative path to the schema docs
label = "Schema reference"         # button tooltip / aria-label
```

Then, from the book directory (the one containing `book.toml`):

```bash
cargo install mdbook-panschema
mdbook-panschema install          # or: mdbook-panschema install <book-dir>
```

This writes `schema-link.js` / `schema-link.css` into the book and wires them into `book.toml`'s `additional-js` / `additional-css`, idempotently — re-run after upgrading to refresh the asset. With `[book_link]` absent or `enabled = false`, `install` does nothing.

## Generating a Postgres schema

If your application is backed by Postgres, `generate --format postgres` emits the `CREATE TABLE` / `CREATE TYPE` DDL for the same LinkML schema your Rust structs come from, so the two never drift apart by hand:

```bash
panschema generate --input schema.yaml --output schema.sql --format postgres
```

Coverage today is concrete classes with scalar/enum/single-valued-class-reference slots; a class using `is_a`, a multivalued slot, or `any_of` is skipped with a warning naming why, rather than emitting broken DDL. See [docs/features/24-postgres-ddl-writer.md](docs/features/24-postgres-ddl-writer.md) for the full design and what's still to come.

panschema doesn't do migrations — `schema.sql` describes the *current* desired schema, not a diff. Pair it with a dedicated schema-diff tool that introspects your live database and applies the delta:

```bash
# Declarative, idempotent apply (no migration-file history)
psqldef mydb < schema.sql

# Or: generate a discrete, reviewable migration file (closer to alembic)
atlas migrate diff --to file://schema.sql --dev-url "docker://postgres/16"
```

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
npx playwright@1.60.0 install
```

### Building & Testing

```bash
cargo install wasm-pack    # one-time prerequisite
cargo build
cargo nextest run --features dev
```

On a fresh checkout, `cargo build` invokes wasm-pack via `panschema/build.rs` to produce the WASM visualization bundle. Subsequent builds reuse that bundle — see the workflow below.

### Refreshing the WASM bundle after viz edits

The WASM bundle in `panschema-viz/pkg/` is cached across builds. If you edit `panschema-viz/src/`, rebuild it explicitly:

```bash
wasm-pack build panschema-viz --target web --dev --features webgpu
```

(Use `--release` instead of `--dev` for size-optimized bundles in CI / publish.) Schema authors who don't touch `panschema-viz/` can ignore this — `cargo build` keeps using the previously-built bundle.

### Faster builds (optional)

If the link time on the debug `panschema` binary becomes a bottleneck, uncomment the relevant block in `.cargo/config.toml` to point cargo at `lld` / `mold` / `sold`. Install instructions are in that file.

### Vendoring a dogfood schema release

panschema regression-tests itself against every released version of the real
dogfood schemas (`scimantic-schema`, `scidatica-schema`). Each
release is checked in as a frozen snapshot under
`panschema/tests/fixtures/dogfood/<repo>/<tag>.yaml` so the test suite runs
offline. When one of those schemas cuts a new release, vendor it and commit:

```bash
scripts/vendor-dogfood-schemas.sh scimantic-schema v0.2.0   # one tag
scripts/vendor-dogfood-schemas.sh scimantic-schema all      # every tag (needs gh)
```

The script is the only network path; it fetches the release via `panschema add`
and writes the snapshot. The weekly Dogfood Release Monitor workflow opens a
tracking issue when a release hasn't been vendored yet. A new release may use a
LinkML construct panschema doesn't support yet — do any needed panschema work
first, then vendor and commit.

### Manual Verification

```bash
panschema serve --input panschema/tests/fixtures/reference.ttl
```

### UI Component Style Guide

```bash
cargo watch -w panschema/src -w panschema/templates -x 'run -p panschema --features dev -- styleguide --serve'
```

## Contributing

Contributions are welcome! Please follow our standards:
- **TDD First**: Write tests before implementation
- **Strict Linting**: Pass `cargo fmt` and `cargo clippy`
- **Pre-commit**: Use our pre-commit hooks

## License

Apache-2.0
