# Panschema Roadmap

> **Note:** This project evolved from `rontodoc` (OWL documentation generator) to `panschema` (a "pandoc for data modeling" tool). The rename happened at v0.2.0.

## Vision

**Panschema** aims to be the universal tool for data modeling workflows:
- Convert between schema languages (LinkML, OWL/TTL, JSON Schema, SHACL, SQL DDL)
- Generate documentation, language-native types (Rust, …), and machine-readable schemas from any supported format
- Manage schemas as versioned, pinned packages (Cargo-style)
- Validate schemas and check compatibility
- Compare schemas and track changes

Like pandoc for documents, panschema provides a single binary that bridges the data modeling ecosystem.

## Architecture

See ADRs for architectural decisions:
- [ADR-003: LinkML as Internal Representation](adr/003-linkml-as-internal-representation.md)
- [ADR-004: Reader/Writer Architecture](adr/004-reader-writer-architecture.md)

### Core Pipeline

```
Input → Reader → LinkML IR → [Filters] → Writer → Output
```

| Component | Description |
|-----------|-------------|
| **Readers** | Parse input formats into LinkML IR (`OwlReader`, `YamlReader`) |
| **Writers** | Generate output formats from LinkML IR (`HtmlWriter`, `OwlWriter`, RDF serializers, `GraphWriter`, `RustWriter`) |
| **Filters** | Transform IR (optional, user-customizable; not yet implemented) |

## Release Strategy

### v0.1.0 — OWL Documentation MVP ✅
*Released as `rontodoc`*

- Turtle (.ttl) parser for OWL ontologies
- Documentation generation: classes, properties, individuals
- Development server with hot reload
- Cross-platform binaries (Linux, macOS, Windows)

### v0.2.0 — Reader/Writer Architecture ✅
*Renamed to `panschema`*

- LinkML internal representation (`SchemaDefinition`, `ClassDefinition`, …)
- Reader/Writer traits + `FormatRegistry`
- `OwlReader` (.ttl) + `YamlReader` (.yaml LinkML)
- `HtmlWriter`, `OwlWriter`, RDF serializers (TTL, JSON-LD, N-Triples, RDF/XML), `GraphWriter`
- Interactive WebGPU schema graph visualization (`panschema-viz` wasm crate)
- E2E browser tests via playwright-rs

### v0.3.0 — Schema Package Manager + Rust Codegen + Dogfood Fixes (current)

**Goal:** Make panschema usable as a versioned schema dependency in downstream Rust applications.

- **Schema package manager** ([feature 05](features/05-schema-manager.md)): `panschema init`, `add`, `release`, `fetch`, `verify`, `generate` with `panschema-publish.toml` + `panschema.toml` + `panschema.lock`. `path:` and `github:` sources.
- **Rust types writer** ([feature 06](features/06-rust-codegen.md)): `panschema generate` emits a single flat Rust module per schema (structs, marker traits, `<Name>Kind` closed enums, `any_of` unions, `Box` recursion, `Eq + Hash` via recursive trait analysis, `pub fn new()` constructors).
- **RDF emitter correctness** ([feature 03 slice 7](features/03-reader-writer-architecture.md)): expand CURIE prefixes in TTL / JSON-LD / N-Triples / RDF/XML; emit `@prefix` / `@context` declarations; emit mixin `rdfs:subClassOf` alongside the `is_a` parent.
- **HTML class card content** ([feature 02 slice 5](features/02-core-ontology-documentation.md)): surface direct slots + `slot_usage` overrides (including `any_of` and `required` narrowing), list mixins, and resolve `[[Name]]` xrefs in descriptions to anchor links.
- **`cargo install --git` bootstrap**: `build.rs` runs `wasm-pack build --features webgpu` when the viz artifacts are missing, so consumer installs Just Work.

### v0.4.0 — Bootstrap LinkML IR + Schema Validation
*Planned. See [feature 07](features/07-schema-validation.md) and [feature 08](features/08-bootstrap-linkml-ir.md).*

- **Bootstrap LinkML IR from the metaschema** ([feature 08](features/08-bootstrap-linkml-ir.md)): replace the hand-rolled `panschema/src/linkml.rs` types with types generated from the LinkML metaschema YAML via panschema's own `RustWriter`. Closes the drift between panschema's IR and the LinkML spec by construction; doubles as the most aggressive `RustWriter` dogfood (the metaschema is the hardest schema we'll feed it). Pairs naturally with feature 07 — once the IR is metaschema-derived, validation rules can be coded against canonical field names.
- **Schema validation** ([feature 07](features/07-schema-validation.md)): `panschema validate <schema>` subcommand that checks a LinkML schema against the metaschema and surfaces actionable diagnostics. Optional CI helper: `panschema verify --strict` includes a validation pass.

### v0.5.0+ — Future Directions
*Aspirational.*

- **Round-trip OWL ↔ LinkML conversion** (`panschema convert`).
- **JSON Schema reader + writer** (`JsonSchemaReader`, `JsonSchemaWriter`).
- **SHACL writer** as a third writer in the `[generate.<name>]` fan-out.
- **`Filter` trait** for user-customizable IR transformations.
- **Schema diff / compatibility checks** (`panschema diff`).

### v1.0.0 — Production Ready

- Comprehensive format support
- Full OWL 2 and LinkML metamodel coverage
- Stable CLI and library API
- Plugin architecture for custom formats

## Feature Specifications

| # | Feature | Description | Status |
|---|---------|-------------|--------|
| 01 | [Foundational UI Stack](features/01-foundational-ui-stack.md) | Walking skeleton: CLI, Turtle parsing, HTML output, dev server | **Released v0.1.0** |
| 02 | [Core Ontology Documentation](features/02-core-ontology-documentation.md) | Classes, properties, individuals — plus v0.3.0 class card content extensions | **Released v0.1.0; slice 5 in progress for v0.3.0** |
| 03 | [Reader/Writer Architecture](features/03-reader-writer-architecture.md) | LinkML IR + OwlReader + writers — plus v0.3.0 RDF emitter correctness | **Released v0.2.0; slice 7 in progress for v0.3.0** |
| 04 | [Schema Force Graph Visualization](features/04-schema-force-graph-visualization.md) | WebGPU schema graph viz (`panschema-viz` wasm crate) | **Released v0.2.0** |
| 05 | [Schema Package Manager](features/05-schema-manager.md) | `init` / `add` / `release` / `fetch` / `verify` / `generate` with manifest + lockfile | **In progress (v0.3.0)** |
| 06 | [Rust Codegen + Multi-Writer Fan-Out](features/06-rust-codegen.md) | `RustWriter` producing typed Rust modules; multi-writer dispatch in `generate` | **In progress (v0.3.0)** |
| 07 | [Schema Validation](features/07-schema-validation.md) | `panschema validate` against the LinkML metaschema | **Planned (v0.4.0)** |
| 08 | [Bootstrap LinkML IR from the metaschema](features/08-bootstrap-linkml-ir.md) | Replace hand-rolled LinkML types with codegen from the metaschema | **Planned (v0.4.0)** |

## Delivery Approach

Each feature is a **vertical slice** that delivers working functionality:

1. **Incremental Refactoring** — each release preserves or improves on existing behavior.
2. **TDD Throughout** — every slice includes tests before implementation.
3. **Spec-Driven** — LinkML implementation follows the official specification.
4. **Outside-In Development** — start with user-facing behavior, work inward.
5. **Dogfood-Driven** — features and bug fixes are exercised against real downstream schemas (scimantic-schema, t2t) before tagging.
