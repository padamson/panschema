# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Schema package manager** (work in progress toward v0.3.0; see [docs/features/05-schema-manager.md](docs/features/05-schema-manager.md)). Cargo-style dependency management for LinkML schemas. Every schema dependency is a "package": a directory containing `panschema-publish.toml` plus the main schema file it references at `[files].main`.
  - **Publishing standard**: `panschema-publish.toml` lives at the package root and declares the schema's authoritative name, version, LinkML target version, and main-file location. Schema authors publish through this file; consumers verify against it.
  - **Manifest**: `panschema.toml` in the consumer project declares `[schemas.<name>]` dependencies and per-schema `[generate.<name>]` codegen config. Cargo-style discovery (walk up from CWD).
  - **Lockfile**: `panschema.lock` records resolved version + SHA-256 checksum of each schema's main file. Committed alongside `panschema.toml`. Drift detection covers both checksum and version. (A `revision` field is reserved for future provenance; populated only when a source exposes a stable commit identifier — currently always `None`.)
  - **Source protocols** (v0.3): `path:` for local packages, `github:owner/repo` for tagged GitHub commits. Both go through the same package model. Other protocols (`gitlab:`, `zenodo:`, `https:`) deferred to later releases.
  - **Github source**: anonymous tarball fetch from `codeload.github.com/<owner>/<repo>/tar.gz/refs/tags/v<version>` (no API rate limit). The tarball's top-level directory is `<repo>-<version>/`; the cache extracts to `~/.cache/panschema/github/<owner>/<repo>/<version>/<repo>-<version>/` with `fs2` file locking. Pluggable `TarballSource` trait for future protocols and for tests. Skips `pax_global_header` pseudo-entries that GitHub codeload includes. Symlink hygiene refuses paths that escape the extracted directory.
  - **Commands**:
    - `panschema init` — producer-side scaffolding. Writes a `panschema-publish.toml` in CWD. Three input modes: explicit flags (`--name X --version Y --main schema.yaml`), `--from <linkml.yaml>` (pre-fills name + version from the LinkML file's metadata), or no args (defaults to CWD basename + `0.1.0` + `schema.yaml`). Refuses to overwrite an existing publish file; `--force` opts in. After writing, prints a per-field provenance summary (explicit / from `--from` / default) so the user can see where each value came from. Post-write validation warns if the main file is missing or doesn't parse but still writes the publish file.
    - `panschema release` — producer-side version bump, modeled on `cargo release`. `--level patch|minor|major` does literal semver bumps (`0.x.y --level major` → `1.0.0`); `--version <x.y.z>` sets an exact value. Default behaviour is bump-only — prints the suggested git commands so the user can complete the release manually. `--git` runs `git add` + `git commit -m 'release: v<ver>'` + `git tag -a -m 'release v<ver>' v<ver>` (annotated tags, the only kind `git push --follow-tags` pushes); refuses on a dirty working tree or an existing tag. `--push` (requires `--git`) also runs `git push --follow-tags`. `--dry-run` prints the plan without writing or running anything. Refuses no-op bumps (`--version <current-version>`) with a clear "tag manually" hint. Refuses to release while the LinkML main file's `version:` field disagrees with publish.toml's `[schema].version`. Manifest edits go through `toml_edit` so comments survive.
    - `panschema add <spec>` — single positional spec, either `github:owner/repo@version` or a filesystem path to a package directory. Schema name is inferred from `panschema-publish.toml`; `--name <alias>` overrides. Writes only the `[schemas.<name>]` entry — the `[generate.<name>]` block is the user's to add when they want codegen output. (`generate` prints a clear "no `[generate.<name>]` block; skipping" hint for any schema without one.) Always re-runs `fetch` afterward so cache + lockfile stay consistent. Idempotent on same-shape adds; conflicting version or source raises a clear error rather than overwriting.
    - `panschema fetch` resolves all manifested schemas, populates the cache, and writes `panschema.lock`. Re-fetch is a no-op when the cached version is already extracted.
    - `panschema verify` re-checksums against the lockfile and errors with a clear diff on drift (catches both "schema edited but generate not re-run" and "publish.toml version bumped").
    - `panschema generate` (no `--input`) discovers the manifest and fans out across every populated writer key in each `[generate.<name>]` block (currently `html` and `rust`). `--input <file>` continues to work as a no-manifest shorthand for raw schema files.
  - **CLI ergonomics**: `SchemaSpec` parser (clap `FromStr`) catches malformed input at parse time — invalid version, unknown protocol, empty spec — before any side effects. Manifest edits go through `toml_edit` so user comments, key order, and whitespace survive.
- `Contributor` struct for Dublin Core-style contributor metadata (name, ORCID, role)
- `SchemaDefinition` metadata fields: `contributors`, `created`, `modified`, `imports`
- `FormatRegistry::with_defaults()` for dynamic reader/writer dispatch
- `YamlReader` for native LinkML YAML schemas (yaml, yml)
- `OwlWriter` for writing LinkML IR to OWL/Turtle format (ttl)
- Library crate (`lib.rs`) exposing public API for integration testing
- **GPU Force Graph Visualization** (optional `gpu` feature):
  - `GpuSimulation` for GPU-accelerated force-directed graph layout
  - `GpuRenderer` for 3D rendering with instanced spheres (nodes) and lines (edges)
  - `Camera3D` with orbit, zoom, and pan controls
  - WGSL compute shaders: link force, many-body force, center force, velocity integration
  - WGSL render shaders with Blinn-Phong lighting
  - Icosphere mesh generation for smooth node spheres
- `GraphWriter` for exporting schema as graph JSON (`graph-json` format)
- University schema example in `examples/university/`
- **Interactive Schema Graph Visualization** in HTML output:
  - WebGPU 3D visualization with orbit/zoom/pan controls (Chrome 113+, Firefox 121+, Safari 18+)
  - 2D Canvas fallback for browsers without WebGPU
  - Static graph fallback when WASM unavailable
  - Embedded WASM bundle for offline capability
  - Smooth fit-to-bounds animation after simulation settles
  - Sidebar "Schema Graph" link with node/edge count badge
  - Browser support message when using 2D fallback
  - **Label controls**: Toggle all labels, node labels, or edge labels independently
  - **Hover-to-reveal**: Show individual label on hover even when labels are toggled off
  - **Persistent preferences**: Label visibility settings saved to localStorage
  - **3D HTML overlay labels**: Projected node/edge labels via HTML overlay for crisp text
- **Interactive node manipulation** in graph visualization (Slice 6, in progress):
  - Click to select a node; details panel shows label, type, IRI, fixed state, and connection count
  - Drag to reposition any node while the simulation continues
  - Shift+click to toggle pin (node holds its position); shift+drag-release pins at the new position
  - Keyboard shortcuts: `R` reset view, `F` focus selected, `Esc` deselect, `Delete` unpin selected
  - Cursor feedback (grab/grabbing) on hover and drag
  - Hit testing via 3D ray-cast and 2D point-in-circle
- Force simulation collide pass (geometric overlap resolution) prevents node overlap regardless of graph topology
- `panschema completions <shell>` subcommand to generate shell completion scripts (bash, zsh, fish, powershell, elvish)
- **Rust types writer** (work in progress; see [docs/features/06-rust-codegen.md](docs/features/06-rust-codegen.md)). `[generate.<name>]` now accepts a `rust = "<path>"` key alongside `html`, and `panschema generate` fans out across every populated writer per schema. The writer currently emits Rust structs for each class (with direct slots — inheritance and mixins follow later), Rust enums for each LinkML enum, primitives mapped to `String`/`i64`/`bool`/`f64`/`chrono::DateTime<Utc>`, `Option<T>` / `Vec<T>` framing for optional / multivalued slots, doc-comments from LinkML descriptions, and per-field `#[serde(rename = "...")]` so the LinkML wire format round-trips. Generated code depends on `serde` and `chrono` in the consumer's `Cargo.toml`. Inheritance traits, mixin flattening, `slot_usage` overrides, and polymorphic `any_of` ranges land in later slices before v0.4.0.

### Changed
- Migrated `wgpu` 24 → 29 across `panschema-viz/webgpu.rs` and `panschema/gpu/{simulation,renderer}.rs`. Surface changes addressed: `InstanceDescriptor` no longer `Default`, `Instance::new` takes the descriptor by value, `DeviceDescriptor` requires `experimental_features` + `trace`, `Adapter::request_device` is single-arg, `PipelineLayoutDescriptor` swapped `push_constant_ranges` for `immediate_size` and now takes `&[Option<&BindGroupLayout>]`, `RenderPipelineDescriptor.multiview` → `multiview_mask`, `RenderPassColorAttachment` requires `depth_slice`, `RenderPassDescriptor` requires `multiview_mask`, `DepthStencilState.depth_write_enabled`/`depth_compare` now `Option<_>`, `wgpu::Maintain` → `wgpu::PollType`, `Surface::get_current_texture` returns `CurrentSurfaceTexture` enum instead of `Result`.
- `main.rs` and `server.rs` now use `FormatRegistry` instead of hardcoded readers/writers
- Force simulation defaults retuned for sparser graphs (stronger repulsion, weaker centering); node radii reduced for less visual crowding
- **MSRV bumped from 1.85 to 1.88** to enable let-chain syntax (`if let X = y && cond`) in source

### Fixed
- 3D camera `zoom()` direction was inverted relative to the 2D camera and the documented contract; `factor > 1.0` now zooms in for both
- `YamlReader` now infers metaobject names from their dict keys (idiomatic LinkML), so explicit `name:` and permissible-value `text:` fields are optional. Applies to classes, slots, enums, types, class attributes, class slot_usage, and permissible values. Schemas produced by `linkml-runtime` and the broader LinkML toolchain (`gen-owl`, `gen-shacl`, `gen-python`) now load without modification. Explicit names still work; an explicit name that disagrees with the dict key is now a clear parse error.
- `GraphWriter` now emits range edges for inline class attributes (e.g., `Student.year` → `YearEnum`). Previously only top-level `slots:` produced domain/range edges, so most relationships in idiomatic LinkML schemas were silently dropped from the visualization. Inline attributes connect the owning class directly to the range target (no separate slot node), labeled with the attribute name.

## [0.2.0] - 2026-01-25

Project renamed from **rontodoc** to **panschema** to reflect broader schema support.

### Added
- **LinkML Internal Representation (IR)**: Canonical data model based on LinkML metamodel
- **Reader/Writer Architecture**: Extensible pipeline for multi-format support
- `OwlReader`: Parses OWL/Turtle to LinkML IR
- `HtmlWriter`: Generates HTML documentation from LinkML IR
- Support for OWL individuals with type links and property values

### Changed
- **BREAKING**: Binary renamed from `rontodoc` to `panschema`
- **BREAKING**: Crate renamed from `rontodoc` to `panschema`
- Internal architecture refactored to use Reader → IR → Writer pipeline
- Classes map to LinkML `ClassDefinition` with hierarchy preserved
- Properties map to LinkML `SlotDefinition` with domain/range
- XSD datatypes mapped to LinkML built-in types

### Removed
- Old monolithic parser and renderer (replaced by Reader/Writer architecture)

## [0.1.0] - 2026-01-24

Initial release of rontodoc — a fast, single-binary ontology documentation generator.

### Added
- CLI with `generate` and `serve` subcommands.
- Turtle (.ttl) parser for OWL ontologies: classes, properties, individuals, and metadata.
- Class cards with labels, descriptions, IRIs, and class hierarchy (superclass/subclass links).
- Property cards with type badges, domain/range, and inverse-of relationships.
- Individual cards with type links and property values.
- Sidebar navigation with section links and count badges.
- Development server with hot reload for live documentation preview.
- Responsive two-column layout with dark mode support.
- Component-driven UI with style guide (`--features dev`).

[Unreleased]: https://github.com/padamson/panschema/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/padamson/panschema/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/padamson/panschema/releases/tag/v0.1.0
