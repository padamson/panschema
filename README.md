# panschema

> A universal CLI for schema conversion, documentation, validation, and comparison.

**Status:** Active Development

## Vision

**panschema** aims to be a universal tool for data modeling workflows:

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
- **mdbook integration**: `mdbook-panschema install` adds a maintained toolbar link from an mdbook book to its schema docs
- **Loud about gaps**: warns on LinkML constructs it parses but doesn't model (so nothing is silently dropped); `generate --strict` fails the build instead
- **Postgres DDL**: `generate --format postgres` emits `CREATE TABLE`/`CREATE TYPE` DDL from the same LinkML schema your Rust structs come from — no hand-written SQL to keep in sync
- **Versioned migrations**: `migrate --schema schema.yaml --migrations db/migrations/` writes that DDL as a migration file a checksumming runner can apply — deterministic bytes, append-only, and no database connection
- **SHACL shapes**: `generate --format shacl` emits a SHACL shapes graph so a schema's value constraints are machine-checkable by any SHACL engine, not just visible in the docs
- **JSON Schema / OpenAPI**: `generate --format json-schema` (draft 2020-12) and `--format openapi` (3.1 `components/schemas`) emit a structured-output/API contract from the same LinkML source — an LLM's structured output or a generated TS/Swift client shares the model the Rust types come from
- **Instance-data validation**: `validate --schema schema.yaml --data data.yaml` checks a LinkML instance-data file against the schema and exits non-zero on any violation — a conformance gate for CI or an LLM authoring loop

See [CHANGELOG.md](CHANGELOG.md) for detailed version history.

## Installation

Download a pre-built binary from
[GitHub Releases](https://github.com/padamson/panschema/releases), or build
from source (requires [wasm-pack](https://rustwasm.github.io/wasm-pack/),
which builds the embedded visualization bundle):

```bash
cargo install wasm-pack
cargo install --git https://github.com/padamson/panschema --tag v0.3.0 panschema
```

### Working with an AI coding agent

panschema ships a Claude Code skill that teaches an agent the CLI, the
manifest format, and the traps that have actually bitten consumers. Install
it as a plugin so it updates with the tool instead of rotting as a copy:

```
/plugin marketplace add padamson/panschema
/plugin install panschema@panschema
```

`/plugin update panschema` picks up later releases. The plugin's version
tracks the crate's, enforced by a test, so the skill an agent reads
describes the panschema you have installed.

## Quick Start

Generate documentation from an OWL ontology:

```bash
panschema generate --schema ontology.ttl --output docs/
```

Start a development server with hot reload:

```bash
panschema serve --schema ontology.ttl
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
| Graph JSON (schema / T-box) | Full support |
| Instance graph JSON (A-box) | Full support |
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
panschema generate --schema schema.yaml --output docs/

# Disable graph visualization
panschema generate --schema schema.yaml --output docs/ --no-graph

# Force specific visualization mode
panschema generate --schema schema.yaml --output docs/ --viz-mode 2d
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
panschema generate --schema schema.yaml --instances data.yaml --output docs/

# Several curated graphs on one page — a small teaching preview and the full
# worked example — with a selector to switch between them in place
panschema generate --schema schema.yaml \
  --instances data/preview.yaml --instances data/worked-example.yaml \
  --output docs/
```

#### Page composition

A page built around its data can lead with the instance graph, or drop the
schema reference sections entirely. In a manifest's `[generate.<name>]`
table (and, for versioned sites, under `[publishing]` in
`panschema-publish.toml` as `layout` / `schema_sections`):

```toml
[generate.myschema]
html = "docs/"
html_page_layout = "instances-first"   # default: "schema-first"
html_schema_sections = false           # default: true — keep the class/slot/enum cards
```

The defaults reproduce today's page exactly; the sidebar follows whatever
order and sections the page renders. A page without the schema sections
keeps its metadata card and the namespace table — the instance cards'
CURIEs expand through it — and warns if it would otherwise be empty.
(`panschema serve` does not yet apply composition keys; preview composed
pages with `generate`.)

#### Dependency pages

A versioned publish can also document a *dependency* schema alongside your
own — the contract-plus-local-records page. An `[[instances]]` entry that
names a dependency from the repo's `panschema.toml` moves its dataset onto
a second published page rendering that dependency's schema; entries naming
the same dependency share the page:

```toml
[[instances]]
name = "assessments"
data = "data/assessments.yaml"
schema = "cqa"                   # a [schemas.cqa] dependency

[publishing.pages.cqa]           # optional per-page settings
dir = "contracts"                # default: the dependency's name
layout = "instances-first"
schema_sections = false
```

The page lives in its own directory inside the publish output tree, with
the same per-version + `current/` layout as the main page. It is built
only for refs where both the dependency and some of its data exist, and
its version dropdown offers exactly those refs; when the configured
`current` version isn't among them, the page publishes without a
`current/` alias (noted on stderr). Each ref renders that ref's data
against the dependency version the ref's own manifest pins, resolved from
the local cache — publish never fetches over the network, so run
`panschema fetch` first for `github:` sources — and checked against the
ref's committed `panschema.lock`: cached content failing its checksum
(covering the schema's main file, the content `panschema fetch` locks)
skips the page at that ref with the mismatch named. (`path:` dependencies
carry no pin and always resolve from the working tree.) Naming a
dependency the manifest doesn't declare fails the publish.

One sizing note: dependency pages sit one directory deeper than the
main page. The parent-relative defaults for `url_pattern` and
`site_root_url` resolve correctly on every page, and a relative
`site_root_url` override (like `../../`, escaping into a containing
book) is re-based per page — a dependency page adds the extra `../`
its depth needs — while root-relative and scheme-carrying values pass
through unchanged. A relative override must climb (begin with `../`);
anything else resolves somewhere different on every page and is
refused at parse. An overridden `url_pattern`, by contrast, still
targets the main page's depth only — a dependency page's version
dropdown navigates that page's own version tree — so keep the
`url_pattern` default when publishing dependency pages; per-page URL
overrides can follow if a site needs them.

When a parent site fronts the pages, `site_title` names it: the header
brand link on every page — the main page and each dependency page —
carries that one site identity instead of each page's schema title,
while each page's heading and browser title keep the schema. Pair it
with a `site_root_url` override so the brand's text and target agree:

```toml
[publishing]
site_title = "Building wine"     # default: each page's schema title
site_root_url = "../../"         # the containing site's root
```

Once a site publishes more than one page, every page's header gains a
small nav listing the site's pages by schema name, with the page you
are on marked rather than linked. Links target each sibling's
`current/` alias — or, for a page publishing without one, the version
standing as its current (the first released ref present, in the
manifest's version order) — so they never point at a directory that
was not built. A single-page site keeps today's header untouched.

Repeat `--instances` to carry more than one curated graph. Each is labelled by
its file stem and gets its own cards, provenance line, and node/edge counts;
the first is shown until the reader picks another, and switching happens in the
page with no navigation. Curated graphs are teaching artifacts, so keep them
small — panschema warns per graph past a few hundred nodes. Formats that emit a
single A-box (`ttl`, `jsonld`, `rdfxml`, `ntriples`, `instance-graph-json`)
take exactly one file.

Each record becomes a typed node keyed by its identifier; a class-valued slot
becomes an edge to the referenced record, and scalar values ride along as node
metadata — so the JSON an LLM emits against a class's JSON Schema (see
`generate --format json-schema`) is a LinkML instance you can read straight
back and visualize, no OWL detour.

Whether the container itself becomes an individual is your call, made by
giving it an identifier. A `tree_root` class that declares an `identifier`
slot emits as a record like any other — RDF individual, graph node, card —
with references to what it contains; one that declares none emits nothing,
and its scalars surface as dataset metadata instead. A vessel that exists
only because a file needs a root stays silent; a domain root — an enterprise,
a study, a tenant — becomes the anchor another graph can point at.

A class-valued slot may also name something *outside* this data file: write
an absolute IRI or a CURIE against a prefix the schema declares, and it is
read as a cross-graph reference — an IRI object in RDF, exempt from the
dangling-reference check, drawn as a muted node, and listed in a summary of
what leaves the dataset. A bare identifier always means a record in this
file, and is still checked as one.

The same A-box also exports as machine-readable artifacts, one invocation per
artifact: fold it into the RDF outputs as `owl:NamedIndividual`s (a
self-contained knowledge graph a triple store loads directly), or render it as
a typed, traversable graph document:

```bash
# Schema + individuals as one Turtle knowledge graph
panschema generate --schema schema.yaml --instances data.yaml --format ttl --output kg.ttl

# The instance graph as its own graph-JSON document
panschema generate --schema schema.yaml --instances data.yaml \
  --format instance-graph-json --output instances.json
```

Individual IRIs mint identically across the docs, the RDF, and the graph
JSON, so a SPARQL query and a graph-JSON traversal agree on which node is
which.

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

Then, from the book directory (the one containing `book.toml`) — the `mdbook-panschema` binary ships with panschema, so `cargo install panschema` provides it:

```bash
mdbook-panschema install          # or: mdbook-panschema install <book-dir>
```

This writes `schema-link.js` / `schema-link.css` into the book and wires them into `book.toml`'s `additional-js` / `additional-css`, idempotently — re-run after upgrading to refresh the asset. With `[book_link]` absent or `enabled = false`, `install` does nothing.

A book fronting a site with several published pages — the main schema plus dependency pages — writes one `[[book_link]]` entry per page instead; the button becomes a menu listing each entry by its label:

```toml
[[book_link]]
schema_path = "schema/current/"
label = "Catalog schema"

[[book_link]]
schema_path = "schema/cqa/current/"
label = "CQA contract"
```

Writing an entry is itself the opt-in, so the list form has no `enabled` switch; an empty list means off.

## Generating a Postgres schema

If your application is backed by Postgres, `generate --format postgres` emits the `CREATE TABLE` / `CREATE TYPE` DDL for the same LinkML schema your Rust structs come from, so the two never drift apart by hand:

```bash
panschema generate --schema schema.yaml --output schema.sql --format postgres
```

Coverage today is concrete classes with scalar/enum/single-valued-class-reference slots; a class using `is_a`, a multivalued slot, or `any_of` is skipped with a warning naming why, rather than emitting broken DDL. See [docs/features/24-postgres-ddl-writer.md](docs/features/24-postgres-ddl-writer.md) for the full design and what's still to come.

`schema.sql` describes the *current* desired schema, not a diff, so it is useful exactly once — on an empty database. For a database that already has tables, `panschema migrate` writes the DDL as a versioned migration file instead:

```bash
panschema migrate --schema schema.yaml --migrations db/migrations/
# writes db/migrations/V1__my_schema.sql
```

The file lands in the layout a checksumming versioned runner (`refinery` and its family) discovers, and the SQL is byte-identical across runs and machines — no timestamp, no tool version — because such a runner hashes the raw text and aborts a deploy when the hash changes. Re-running against an unchanged schema is a no-op, and a directory that already holds other migrations is refused rather than guessed at. panschema writes migration files and never connects to a database; applying them is the runner's job, and the generated SQL is a draft to review, not an authoritative artifact.

Today `migrate` emits the *initial* migration. Incremental migrations, and a `diff` command that reports a schema delta with a compatibility verdict, are specced in [docs/features/39-schema-diff-and-migration-generation.md](docs/features/39-schema-diff-and-migration-generation.md). Until those land, a tool that introspects your live database covers the incremental case:

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
wasm-pack build panschema-viz --target web --dev
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
panschema serve --schema panschema/tests/fixtures/reference.ttl
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
