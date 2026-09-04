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
| **Writers** | Generate output formats from LinkML IR (`HtmlWriter`, `OwlWriter`, RDF serializers, `GraphWriter`, `RustWriter`, `PostgresWriter`, `ShaclWriter`) |
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
- Interactive schema graph visualization (`panschema-viz` wasm crate)
- E2E browser tests via playwright-rs

### v0.3.0 — Schema Package Manager + Rust Codegen + Dogfood Fixes ✅ (released 2026-08-13)

**Goal:** Make panschema usable as a versioned schema dependency in downstream Rust applications.

- **Schema package manager** ([feature 05](features/05-schema-manager.md)): `panschema init`, `add`, `release`, `fetch`, `verify`, `generate` with `panschema-publish.toml` + `panschema.toml` + `panschema.lock`. `path:` and `github:` sources. Slices 1–4.6 shipped; slice 5 (docs + dogfood + tag) remaining.
- **Rust types writer** ([feature 06](features/06-rust-codegen.md)): `panschema generate` emits a single flat Rust module per schema (structs, marker traits, `<Name>Kind` closed enums, `any_of` unions, `Box` recursion, `Eq + Hash` via recursive trait analysis, `pub fn new()` constructors). Slices 6.1–6.9 shipped; slice 6.10 (structured error surfaces) optional, not started.
- **RDF emitter correctness** ([feature 03 slice 7](features/03-reader-writer-architecture.md)): expand CURIE prefixes in TTL / JSON-LD / N-Triples / RDF/XML; emit `@prefix` / `@context` declarations; emit mixin `rdfs:subClassOf` alongside the `is_a` parent. Shipped.
- **HTML class card content** ([feature 02 slice 5](features/02-core-ontology-documentation.md)): surface direct slots + `slot_usage` overrides (including `any_of` and `required` narrowing), list mixins, and resolve `[[Name]]` xrefs in descriptions to anchor links. Shipped (β.1 mixins, β.2 xrefs, β.3 slots).
- **Responsive layout + fillable graph viz** ([feature 02 slices 6–7](features/02-core-ontology-documentation.md)): fluid `.content-area` + responsive card grid; graph viz fills the configured aspect-ratio container at all 3 viewport scales (phone / laptop / 4K) via anisotropic axial centering + √N collide-padding scaling. Shipped.
- **Layout-picker chrome** ([feature 09 slice 1](features/09-graph-layout-selection.md)): `<select>` in the graph chrome; force-directed selectable, other algorithm identifiers exposed as disabled options so the wire format stabilizes ahead of the algorithm slices. Shipped.
- **Kamada-Kawai layout** ([feature 09 slices 2–3](features/09-graph-layout-selection.md)): `petgraph-layout-kamada-kawai` from the `egraph-rs` workspace, wired end-to-end (`to_petgraph` + `kamada_kawai` helpers, wasm32 CI canary, picker exposes "Kamada-Kawai (slower init)" in 2D mode). Shipped.
- **Mode-aware picker** ([feature 09 slice 3](features/09-graph-layout-selection.md)): shipped, then retired 2026-09-04 with the 3D renderer — the picker now has a single mode, so every implemented layout is selectable and unimplemented ones stay disabled.
- **Hierarchical (Sugiyama) layout** ([feature 09 slice 6](features/09-graph-layout-selection.md)): `rust-sugiyama` over the `is_a` / `mixin` sub-DAG. Property edges (range / domain / inverse / typeof) overlay the layered output without participating in layering, so cyclic property graphs don't break the render. Orphan nodes fall back to a grid below the layered region. The literature's answer for "minimize crossings on layered DAGs." Shipped.
- **Dep bump: sophia 0.9 → 0.10**: RDF 1.2 ground-truth migration. `LiteralLanguage` becomes a 3-tuple (adds optional direction); `FastGraph::Triple` now uses `IndexedTerm` rather than `SimpleTerm`. `owl_reader` and RDF serializers migrated to use Term trait methods (`iri()`, `lexical_form()`) uniformly across any Term implementation rather than pattern-matching on `SimpleTerm` variants. `NtSerializer` renamed to `NTriplesSerializer` upstream. Shipped.
- **`cargo install --git` bootstrap**: `build.rs` runs `wasm-pack build` when the viz artifacts are missing, so consumer installs Just Work. Shipped.
- **Instance graphs end to end** ([features 33](features/33-linkml-instance-reader.md)–[38](features/38-instance-graph-shared-renderer-and-typed-encoding.md), [ADR-009](adr/009-instance-graph-publishing-and-addressing.md)): a LinkML data file (`--instances`, repeatable; manifest `instances` key; publish `[[instances]]`) renders as a first-class, navigable instance graph — typed encoding where individuals wear their class's symbol and used enum values become shared nodes, the full renderer with toolbar, hover cards, and adaptive auto-sized legends — and exports through the RDF family and `instance-graph-json` (wire format 1.2) with one shared IRI minting. Instance data is conformance-checked on the way into every output, with undeclared fields reported rather than silently minted into the schema's namespace. Shipped.
- **SHACL rule projection** ([feature 17 slice 4](features/17-class-validation-constructs.md)): `rules` → conditional `sh:or ( [sh:not pre] post )` shapes, including `value_presence` and both `any_of` forms. Shipped.
- **Versioned docs** ([feature 11](features/11-versioned-docs-publish.md)): `panschema publish` command + `[publishing]` manifest section. Orchestrates per-version HTML builds (`/schema/v0.1.0/`, `/schema/v0.2.0/`, `/schema/main/`, `/schema/current/`) and injects a version dropdown + "you're viewing X; current is Y" banner into each rendered page. `--edge-from-worktree` flag lets local dev preview reflect uncommitted edits. Default `url_pattern` is parent-relative so GitHub-Pages-style subpath deploys work out of the box. Slices 1–4, 6, 7 shipped; slice 5 (scimantic-schema dogfood + panschema release) remaining.

### v0.4.0 — Bootstrap LinkML IR + Schema Validation + Authoring Experience
*Planned. See [feature 07](features/07-schema-validation.md), [feature 08](features/08-bootstrap-linkml-ir.md), and [feature 10](features/10-authoring-experience.md).*

- **Bootstrap LinkML IR from the metaschema** ([feature 08](features/08-bootstrap-linkml-ir.md)): replace the hand-rolled `panschema/src/linkml.rs` types with types generated from the LinkML metaschema YAML via panschema's own `RustWriter`. Closes the drift between panschema's IR and the LinkML spec by construction; doubles as the most aggressive `RustWriter` dogfood (the metaschema is the hardest schema we'll feed it). Pairs naturally with feature 07 — once the IR is metaschema-derived, validation rules can be coded against canonical field names. The [LinkML coverage matrix](linkml-coverage.md) tracks today's per-metaslot, per-writer support and the prioritized gaps this would close.
- **Schema validation** ([feature 07](features/07-schema-validation.md)): `panschema validate <schema>` subcommand that checks a LinkML schema against the metaschema and surfaces actionable diagnostics. Optional CI helper: `panschema verify --strict` includes a validation pass.
- **Authoring experience** ([feature 10](features/10-authoring-experience.md)): surface idiomatic-LinkML / OBO-Foundry-aligned authoring guidance as actionable diagnostics. Slice 1 is a friction-gathering pass over a real schema (no code) to ground the rule set in observed pain rather than invented rules.

### v0.5.0+ — Future Directions
*Aspirational.*

- **Round-trip OWL ↔ LinkML conversion** (`panschema convert`).
- **JSON Schema reader + writer** (`JsonSchemaReader`, `JsonSchemaWriter`).
- **`Filter` trait** for user-customizable IR transformations.
- **Cross-graph instance identity** ([feature 41](features/41-cross-graph-instance-identity.md)): make "many datasets per schema" safe. Every individual currently mints into the *schema's* namespace regardless of dataset, so two datasets sharing an id silently merge. Filed and co-designed by three consuming repos, whose convergent finding is that instance data splits into per-dataset scoped facts and globally-shared reference vocabulary — a blanket per-dataset namespace would break already-published cross-dataset competency questions. Also covers external-IRI references (a record pointing into another graph) and cross-dataset collision detection.
- **Schema diff + migration generation** ([feature 39](features/39-schema-diff-and-migration-generation.md)): `panschema diff` reports a semantic, format-agnostic delta between two schema versions with a compatibility verdict (fulfilling the "comparison" half of the project's own description); `panschema migrate` renders the same delta as a versioned Postgres migration file. Specced 2026-07-28 — the delta layer is greenfield in the LinkML ecosystem, which has no schema-diff of any kind, while the diff→versioned-migration *pipeline* is mature prior art worth borrowing from rather than reinventing.

### v1.0.0 — Production Ready

- Comprehensive format support
- Full OWL 2 and LinkML metamodel coverage
- Stable CLI and library API
- Plugin architecture for custom formats

## Feature Specifications

| # | Feature | Description | Status |
|---|---------|-------------|--------|
| 01 | [Foundational UI Stack](features/01-foundational-ui-stack.md) | Walking skeleton: CLI, Turtle parsing, HTML output, dev server | **Released v0.1.0** |
| 02 | [Core Ontology Documentation](features/02-core-ontology-documentation.md) | Classes, properties, individuals — plus v0.3.0 class card content + responsive layout extensions | **Released v0.1.0; slices 5–7 shipped for v0.3.0** |
| 03 | [Reader/Writer Architecture](features/03-reader-writer-architecture.md) | LinkML IR + OwlReader + writers — plus v0.3.0 RDF emitter correctness | **Released v0.2.0; slice 7 shipped for v0.3.0** |
| 04 | [Schema Force Graph Visualization](features/04-schema-force-graph-visualization.md) | Schema graph viz (`panschema-viz` wasm crate); 3D/WebGPU path removed 2026-09-04 | **Released v0.2.0** |
| 05 | [Schema Package Manager](features/05-schema-manager.md) | `init` / `add` / `release` / `fetch` / `verify` / `generate` with manifest + lockfile | **In progress (v0.3.0): slices 1–4.6 shipped, slice 5 remaining** |
| 06 | [Rust Codegen + Multi-Writer Fan-Out](features/06-rust-codegen.md) | `RustWriter` producing typed Rust modules; multi-writer dispatch in `generate` | **In progress (v0.3.0): slices 6.1–6.9 shipped, slice 6.10 optional** |
| 07 | [Schema Validation](features/07-schema-validation.md) | `panschema validate` against the LinkML metaschema | **Planned (v0.4.0)** |
| 08 | [Bootstrap LinkML IR from the metaschema](features/08-bootstrap-linkml-ir.md) | Replace hand-rolled LinkML types with codegen from the metaschema | **Planned (v0.4.0)** |
| 09 | [Graph Layout Selection](features/09-graph-layout-selection.md) | Layout-algorithm picker + egraph-rs / rust-sugiyama adoption (KK, stress, SGD, Sugiyama, circular, radial) | **In progress (v0.3.0+): slices 1–3 + 6 shipped (FD, KK, Hierarchical); slices 4–5, 7–8 planned** |
| 10 | [Authoring Experience](features/10-authoring-experience.md) | Schema/ontology authoring lints + diagnostics (friction-gathered from real authoring passes) | **Planned (v0.4.0+)** |
| 11 | [Versioned Docs (`panschema publish`)](features/11-versioned-docs-publish.md) | Multi-version HTML orchestration + in-page version dropdown/banner | **In progress (v0.3.0+): slices 1–4, 6, 7 shipped; slice 5 (dogfood) remaining** |
| 12 | [LinkML IR Resolver Services](features/12-linkml-ir-resolver-services.md) | Shared `is_a`/mixin/`slot_usage` resolver + effective cardinality | **Shipped for v0.3.0** |
| 13 | [Upstream Ontology Label Cache](features/13-upstream-label-cache.md) | Cache upstream ontology labels for cross-references | **Shipped for v0.3.0** |
| 14 | [Slot Constraints](features/14-slot-constraints.md) | OWL property characteristics + `minimum_value`/`maximum_value` bounds | **Shipped for v0.3.0** |
| 15 | [Multi-file Schema Modularity](features/15-multi-file-schema-modularity.md) | Local `imports:` resolution + merge (CURIE/cross-schema imports pending, see feature 29) | **Shipped for v0.3.0 (local imports)** |
| 16 | [Lifecycle & Editorial Metadata](features/16-lifecycle-editorial-metadata.md) | `deprecated`, `aliases`, `see_also`, `examples` — render + RDF round-trip | **Shipped for v0.3.0** |
| 17 | [Class Validation Constructs](features/17-class-validation-constructs.md) | `unique_keys` + `rules` across HTML/Postgres/SHACL, incl. `value_presence` and `any_of` rule conditions in SHACL | **In progress (v0.3.0): `unique_keys` + `rules` shipped across writers; class boolean expressions deferred pending a concrete consumer** |
| 18 | [Exemplar Individuals in the Graph](features/18-exemplar-individuals-in-graph.md) | Worked-example individuals in the schema graph | **Shipped for v0.3.0 as the separate instance graph (with features 33/36–38)** |
| 19 | [Slot Defaults (`ifabsent`)](features/19-ifabsent-slot-defaults.md) | `ifabsent` → Rust field defaults + HTML "Default" row | **Shipped for v0.3.0** |
| 20 | [Dogfood Schema Regression Fixtures](features/20-dogfood-schema-regression-fixtures.md) | Downstream-schema regression fixtures + release monitoring | **Planned** |
| 21 | [mdbook → Schema Cross-Link](features/21-book-to-schema-link.md) | `mdbook-panschema install` toolbar link from a book to its schema docs | **Shipped for v0.3.0** |
| 22 | [Silently-dropped Construct Diagnostics](features/22-unsupported-construct-diagnostics.md) | Warn on LinkML constructs parsed but not IR-modeled; `--strict` fails | **Shipped for v0.3.0** |
| 23 | [Cross-writer Construct Coverage Diagnostics](features/23-cross-writer-construct-coverage-diagnostics.md) | Warn on IR-modeled constructs a writer doesn't project | **Shipped for v0.3.0** |
| 24 | [Postgres DDL Writer](features/24-postgres-ddl-writer.md) | `generate --format postgres` — tables, enums, FKs, constraints | **In progress (v0.3.0): scalar/enum/FK/constraint slices shipped; multivalued + `is_a` deferred** |
| 25 | [Rust Writer Output Verification](features/25-rust-writer-output-verification.md) | Compile-and-run V&V oracle for the Rust writer | **Planned** |
| 26 | [HTML + Graph Viz Output Verification](features/26-html-graph-viz-output-verification.md) | HTML5-conformance + browser V&V for HTML/graph output | **Shipped for v0.3.0** |
| 27 | [RDF/OWL Family Output Verification](features/27-rdf-owl-family-output-verification.md) | `oxigraph` load-and-query V&V for the RDF writers | **Shipped for v0.3.0** |
| 28 | [Postgres DDL Output Verification](features/28-postgres-ddl-writer-output-verification.md) | `pg_query` syntax + `testcontainers` apply V&V for Postgres DDL | **Shipped for v0.3.0** |
| 29 | [Shared Schema Load Pipeline + Writer Consistency](features/29-schema-load-pipeline-and-writer-consistency.md) | Unify the load path + reconcile writer projections | **Shipped (slices 1–5); writer-diagnostics surface deferred** |
| 30 | [Cross-package Schema Imports + Codegen Composition](features/30-cross-package-schema-imports-and-composition.md) | Consume a schema across the fetch/cache boundary — inline-merge or shared-crate — with exact-version pinning | **In progress: slices 1–3 (writer coverage, cross-package imports, layering) shipped** |
| 31 | [Rule Visualization in the Schema Graph](features/31-rule-visualization-in-the-schema-graph.md) | Pinned node cards, rule highlight-on-hover, persistent amber rings on rule participants | **Shipped for v0.3.0** |
| 32 | [JSON Schema Writer](features/32-json-schema-writer.md) | `generate --format json-schema` (draft 2020-12) + OpenAPI 3.1 components | **Shipped for v0.3.0** |
| 33 | [LinkML Instance Reader](features/33-linkml-instance-reader.md) | `--instances data.yaml` — a `tree_root` container A-box into the instance model | **Shipped for v0.3.0** |
| 34 | [Validate Instance Data](features/34-validate-instance-data.md) | `validate --schema --data` native conformance checks; the same checks run on the way into `generate`/`publish`; undeclared fields reported | **In progress: slices 1–6 shipped; `any_of` polymorphic ranges pending** |
| 35 | [External Groundings in the Graph](features/35-external-groundings-in-graph.md) | `subclass_of` groundings as shared muted external nodes with cached upstream labels + hover definitions | **Shipped for v0.3.0** |
| 36 | [Instance-Graph Publishing and Exports](features/36-instance-graph-publishing-and-exports.md) | A-box through RDF + `instance-graph-json`, navigable docs section, `publish [[instances]]`, manifest `instances` key | **Shipped for v0.3.0 (slices 1–5)** |
| 37 | [In-page Instance-Graph Selector](features/37-in-page-instance-graph-selector.md) | Several curated A-boxes on one schema page, switchable client-side; `exemplar` = default-selected | **Shipped for v0.3.0** |
| 38 | [Instance Graph: Shared Renderer + Typed Encoding](features/38-instance-graph-shared-renderer-and-typed-encoding.md) | Consistent graph badges, full renderer + toolbar + hover card on the A-box canvas, adaptive auto-sized legends, typed encoding (class-symbol individuals, shared enum-value nodes), graph-first section with per-dataset metadata | **Shipped for v0.3.0 (slices 1–6)** |
| 41 | [Cross-Graph Instance Identity](features/41-cross-graph-instance-identity.md) | Per-class dataset scoping (scoped facts vs global vocabulary), cross-dataset collision detection, external-IRI references, `tree_root` record emission | **Specced, not started** |
| 40 | [Agent-Facing Skill That Cannot Drift](features/40-agent-facing-skill-that-cannot-drift.md) | A shipped Claude Code skill whose examples CI executes and whose coverage the code's own enumerations assert | **In progress: slice 1 shipped** |
| 39 | [Schema Diff + Migration Generation](features/39-schema-diff-and-migration-generation.md) | `diff` reports a semantic schema delta with a compatibility verdict; `migrate` renders it as a versioned Postgres migration file (append-only, never applied by panschema) | **Specced, not started** |

## Delivery Approach

Each feature is a **vertical slice** that delivers working functionality:

1. **Incremental Refactoring** — each release preserves or improves on existing behavior.
2. **TDD Throughout** — every slice includes tests before implementation.
3. **Spec-Driven** — LinkML implementation follows the official specification.
4. **Outside-In Development** — start with user-facing behavior, work inward.
5. **Dogfood-Driven** — features and bug fixes are exercised against real downstream schemas (scimantic-schema, t2t) before tagging.
