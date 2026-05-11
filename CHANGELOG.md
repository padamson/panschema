# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- Migrated `wgpu` 24 → 29 across `panschema-viz/webgpu.rs` and `panschema/gpu/{simulation,renderer}.rs`. Surface changes addressed: `InstanceDescriptor` no longer `Default`, `Instance::new` takes the descriptor by value, `DeviceDescriptor` requires `experimental_features` + `trace`, `Adapter::request_device` is single-arg, `PipelineLayoutDescriptor` swapped `push_constant_ranges` for `immediate_size` and now takes `&[Option<&BindGroupLayout>]`, `RenderPipelineDescriptor.multiview` → `multiview_mask`, `RenderPassColorAttachment` requires `depth_slice`, `RenderPassDescriptor` requires `multiview_mask`, `DepthStencilState.depth_write_enabled`/`depth_compare` now `Option<_>`, `wgpu::Maintain` → `wgpu::PollType`, `Surface::get_current_texture` returns `CurrentSurfaceTexture` enum instead of `Result`.

### Added
- **Schema package manager** (work in progress toward v0.3.0; see [docs/features/05-schema-manager.md](docs/features/05-schema-manager.md)):
  - `panschema-publish.toml` parser — the schema-side publishing standard
  - `panschema.toml` parser — consumer-side dependency manifest
  - `panschema.lock` lockfile with SHA-256 checksums for reproducible builds
  - Cargo-style manifest discovery (walk up from CWD)
  - `panschema generate` with no `--input` discovers the manifest and runs HtmlWriter for each `[generate.<name>]` block
  - `panschema fetch` resolves all manifested schemas, computes checksums, and writes `panschema.lock`
  - `panschema verify` re-checksums against the lockfile and errors with a clear diff on drift (catches "schema edited but generate not re-run")
  - Clear errors when a schema's `path:` target is missing
  - `--input <file>` continues to work as a no-manifest shorthand
  - `github:owner/repo` source protocol with shared cargo-style cache at `~/.cache/panschema/github/<owner>/<repo>/<version>/`
  - Anonymous tarball fetch from `codeload.github.com` — no GitHub API rate limit
  - Commit SHA recorded in `panschema.lock` for `github:` sources (read from the tarball's top-level directory name)
  - `panschema-publish.toml` read from the tagged commit; declared `version` verified against the manifest at fetch time
  - File locking on the cache (`fs2`) so concurrent fetches don't race
  - Symlink hygiene: refuses to follow paths that escape the cache directory
  - Re-fetch is a no-op when the cached version is already extracted (no network call)
  - Pluggable `TarballSource` trait for future protocols (`gitlab:`, `https:`, etc.) and for tests
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

### Changed
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
