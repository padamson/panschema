# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Schema package manager** (see [docs/features/05-schema-manager.md](docs/features/05-schema-manager.md)). Cargo-style dependency management for LinkML schemas. Every schema dependency is a "package": a directory containing `panschema-publish.toml` plus the main schema file it references at `[files].main`.
  - **Publishing standard**: `panschema-publish.toml` lives at the package root and declares the schema's authoritative name, version, LinkML target version, and main-file location. Schema authors publish through this file; consumers verify against it.
  - **Manifest**: `panschema.toml` in the consumer project declares `[schemas.<name>]` dependencies and per-schema `[generate.<name>]` codegen config. Cargo-style discovery (walk up from CWD).
  - **Lockfile**: `panschema.lock` records resolved version + SHA-256 checksum of each schema's main file. Committed alongside `panschema.toml`. Drift detection covers both checksum and version. (A `revision` field is reserved for future provenance; populated only when a source exposes a stable commit identifier — currently always `None`.)
  - **Source protocols**: `path:` for local packages, `github:owner/repo` for tagged GitHub commits. Both go through the same package model. Other protocols (`gitlab:`, `zenodo:`, `https:`) deferred to later releases.
  - **Github source**: anonymous tarball fetch from `codeload.github.com/<owner>/<repo>/tar.gz/refs/tags/v<version>` (no API rate limit). The tarball's top-level directory is `<repo>-<version>/`; the cache extracts to `~/.cache/panschema/github/<owner>/<repo>/<version>/<repo>-<version>/` with `fs2` file locking. Pluggable `TarballSource` trait for future protocols and for tests. Skips `pax_global_header` pseudo-entries that GitHub codeload includes. Symlink hygiene refuses paths that escape the extracted directory.
  - **Commands**:
    - `panschema init` — producer-side scaffolding. Writes a `panschema-publish.toml` in CWD. Three input modes: explicit flags (`--name X --version Y --main schema.yaml`), `--from <linkml.yaml>` (pre-fills name + version from the LinkML file's metadata), or no args (defaults to CWD basename + `0.1.0` + `schema.yaml`). Refuses to overwrite an existing publish file; `--force` opts in. After writing, prints a per-field provenance summary (explicit / from `--from` / default) so the user can see where each value came from. Post-write validation warns if the main file is missing or doesn't parse but still writes the publish file.
    - `panschema release` — producer-side version bump, modeled on `cargo release`. `--level patch|minor|major` does literal semver bumps (`0.x.y --level major` → `1.0.0`); `--version <x.y.z>` sets an exact value. Default behaviour is bump-only — prints the suggested git commands so the user can complete the release manually. `--git` runs `git add` + `git commit -m 'release: v<ver>'` + `git tag -a -m 'release v<ver>' v<ver>` (annotated tags, the only kind `git push --follow-tags` pushes); refuses on a dirty working tree or an existing tag. `--push` (requires `--git`) also runs `git push --follow-tags`. `--dry-run` prints the plan without writing or running anything. Refuses no-op bumps (`--version <current-version>`) with a clear "tag manually" hint. Refuses to release while the LinkML main file's `version:` field disagrees with publish.toml's `[schema].version`. Manifest edits go through `toml_edit` so comments survive.
    - `panschema add <spec>` — single positional spec, either `github:owner/repo@version` or a filesystem path to a package directory. Schema name is inferred from `panschema-publish.toml`; `--name <alias>` overrides. Writes only the `[schemas.<name>]` entry — the `[generate.<name>]` block is the user's to add when they want codegen output. (`generate` prints a clear "no `[generate.<name>]` block; skipping" hint for any schema without one.) Always re-runs `fetch` afterward so cache + lockfile stay consistent. Idempotent on same-shape adds; conflicting version or source raises a clear error rather than overwriting.
    - `panschema fetch` resolves all manifested schemas, populates the cache, and writes `panschema.lock`. Re-fetch is a no-op when the cached version is already extracted.
    - `panschema verify` re-checksums against the lockfile and errors with a clear diff on drift (catches both "schema edited but generate not re-run" and "publish.toml version bumped").
    - `panschema generate` (no `--input`) discovers the manifest and fans out across every populated writer key in each `[generate.<name>]` block (currently `html` and `rust`). `--input <file>` continues to work as a no-manifest shorthand for raw schema files.
  - **CLI ergonomics**: `SchemaSpec` parser (clap `FromStr`) catches malformed input at parse time — invalid version, unknown protocol, empty spec — before any side effects. Manifest edits go through `toml_edit` so user comments, key order, and whitespace survive.
- **`panschema publish` — multi-version HTML doc orchestration.** New subcommand that reads a `[publishing]` section from `panschema-publish.toml` and builds per-version HTML docs side-by-side, with an in-page version dropdown and banners that announce drift between the viewed version and the cohort's `current` (see [docs/features/11-versioned-docs-publish.md](docs/features/11-versioned-docs-publish.md)).
  - **Manifest extension**: `[publishing] versions = [...], edge = "...", current = "...", url_pattern = "...", output_dir = "..."`. Parse-time validation rejects a `current` that isn't in `versions` and doesn't equal `edge`, with an error that names the legal alternatives.
  - **Per-version build**: extracts each ref's schema main file via `git show <ref>:<path>` (no working-tree mutation), runs `HtmlWriter` against it, lands output in `<output_dir>/<tag>/`. Edge ref builds to `<output_dir>/<edge-name>/`. `<output_dir>/current/` is a byte-equal copy of the configured version's output, not a symlink (static hosts handle directories cleanly).
  - **Resolves all refs up-front**: a bad tag fails fast with a single combined error listing every unresolvable ref, before any partial build state lands on disk.
  - **In-page UX**: header gains a `<select>` populated from the cohort, default-selected to this page's version, with an inline `onchange` handler that substitutes `{version}` in `url_pattern` to navigate. Edge entries are badged `(edge)`. A non-current page shows a stale banner with a link back to `current/`; the edge page shows a distinct "edge build from HEAD" banner.
  - **CLI**: `panschema publish [--manifest <path>] [--output-dir <dir>] [--edge-from-worktree]`. `--output-dir` overrides the manifest field (relative paths resolve against the manifest's parent). `--edge-from-worktree` reads the edge ref's schema from the working tree instead of `git show <ref>:<path>`, so local dev preview reflects uncommitted edits; CI should NOT set this flag — released builds stay reproducible from committed refs. Tagged versions are unaffected by the flag.
  - **Deploy-portable defaults**: omitting `url_pattern` from the manifest produces parent-relative cross-version URLs (`../{version}/`) that resolve correctly at any deploy depth — works on GitHub Pages subpath deploys (`https://<user>.github.io/<repo>/`) and any other host without per-deploy tuning. Explicitly setting `url_pattern` in the manifest still uses the value verbatim for consumers who need an absolute form.
- **Schema graph layout picker.** The HTML viz now exposes a `<select>` next to the 2D/3D toggle for choosing which layout algorithm produces node positions (see [docs/features/09-graph-layout-selection.md](docs/features/09-graph-layout-selection.md)).
  - **Force-directed (default)** — the in-tree CPU simulation tuned for viewport filling.
  - **Kamada-Kawai** (2D only) — energy-minimization layout via [egraph-rs](https://github.com/likr/egraph-rs)'s `petgraph-layout-kamada-kawai`. Produces visibly nicer node spacing on medium graphs (≤500 nodes) than force-directed at the cost of higher init latency; labelled "(slower init)" in the picker.
  - **Hierarchical (Sugiyama)** (2D only) — layered layout via [rust-sugiyama](https://github.com/paddison/rust-sugiyama) over the `is_a` / `mixin` sub-DAG. Property edges (range / domain / inverse / typeof) deliberately don't participate in layering — they overlay the layered output afterward, so cyclic property graphs (e.g. `Person.owns: Asset`, `Asset.owner: Person`) don't break the layered render. Pathological cycles in the hierarchy spine itself fall through to rust-sugiyama's internal greedy feedback arc set. Orphan nodes (no hierarchy edges) fall back to a grid below the layered region so the connected cluster keeps the central viewport. Labelled "(best for class hierarchies)" in the picker.
  - **Mode-aware**: 3D mode greys out non-force-directed options with a "(not implemented)" suffix since the WebGPU path only runs force-directed. A 2D-only preference (KK / Hierarchical) round-trips through localStorage without being overwritten on the 3D toggle, so toggling 3D → 2D restores the original choice.
  - **Manifest plumbing**: `panschema.toml` accepts `html_default_layout = "<algorithm>"` under each `[generate.<name>]` block, validated at manifest parse time against the canonical identifier list. Unrecognized layouts produce an actionable error rather than silently falling back.
- **CommonMark markdown rendering inside LinkML `description:` fields.** Schema descriptions on schema, class, slot (per-class card + top-level property card), and individual surfaces now accept standard markdown: inline links (`[text](url)`), emphasis (`**bold**`, `*italic*`, `` `code` ``), and block constructs (paragraphs, lists, fenced code). The existing `[[Name]]` cross-reference markers continue to resolve to anchor links — markdown runs first, then xref expansion walks the rendered HTML's text nodes. HTML safety policy: **markdown only**. Raw HTML embedded in a description is escaped, not rendered — authors who need a clickable link use markdown syntax. Description containers in the HTML template moved from `<p>` to `<div>` so markdown's block output is valid HTML. New dep: `pulldown-cmark` (supply-chain exemption added). See [docs/features/02-core-ontology-documentation.md](docs/features/02-core-ontology-documentation.md) slice 9.
- **HTML class cards now surface the class's resolved slot set** under a "Slots" detail row — direct attributes, `slots:` references, and slots inherited from `is_a` and `mixins:`, each shown with its range, required/optional, and multivalued framing. Polymorphic `any_of` ranges render as `any of [A, B, C]` with each branch anchor-linked when it names a declared class. `slot_usage` refinements on the current class are flagged with a "refined here" badge so consumers can see at a glance which constraints were narrowed here versus inherited verbatim.
- **HTML class cards now surface mixins under a "Mixes in" section** with anchor links to each mixin's class card. Previously a class declaring `mixins: [A, B, C]` showed only its `is_a` parent (or nothing at all if the class had no `is_a`), so consumers couldn't see the multiple-inheritance structure from the rendered documentation. Unresolved mixin references (e.g., from un-loaded imports or typos) are skipped silently rather than emitting broken anchor links.
- **HTML namespace table now lists every prefix the schema declares** in its `prefixes:` block. Previously the table was hard-coded to RDF/OWL/RDFS/XSD, so a schema's own prefixes (BFO/CCO, DCAT, OA, NP, URREF, …) were invisible in the rendered documentation. The defaults are retained as a backstop for prefixes the schema doesn't declare; schema-declared prefixes override defaults of the same name.
- **`[[Name]]` cross-reference markers in class descriptions resolve to anchor links** (`#class-Name`, `#enum-Name`, or `#prop-Name` for slot refs), matching LinkML's documentation convention. Previously a description like "lifecycle captured by the [[ActStatus]] enum" rendered the marker as literal text. Unresolved names pass through with an HTML `<!-- WARNING -->` comment so the gap is visible in the source. The same resolution is applied to per-slot descriptions surfaced in the class card.
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
- **Rust types writer** (work in progress; see [docs/features/06-rust-codegen.md](docs/features/06-rust-codegen.md)). `[generate.<name>]` now accepts a `rust = "<path>"` key alongside `html`, and `panschema generate` fans out across every populated writer per schema. The writer emits a single flat Rust module per schema with: structs for concrete (leaf) classes; marker traits for classes used as `is_a` parents or mixins, with supertrait bounds following the LinkML inheritance chain and `impl Trait for Struct {}` blocks per concrete descendant; flattened slot inheritance (parent + mixin slots merge into the consuming struct) with `slot_usage` overrides applied per-class; LinkML enums as Rust enums with `#[serde(rename)]` on variants whose text isn't a valid Rust identifier; primitives mapped to `String` / `i64` / `bool` / `f64` / `chrono::DateTime<Utc>`; `Option<T>` / `Vec<T>` framing for optional / multivalued slots; polymorphic `any_of` ranges as per-slot `#[serde(untagged)]` union enums (branches without an explicit `range:` inherit the slot's outer range); trait-only-class slot ranges as `<Name>Kind` closed enums of concrete descendants, with `String` fallback + breadcrumb comments when a trait class has no concrete descendants; `Box` wrapping for class-typed single-valued fields to break recursive layout cycles; doc-comments from LinkML descriptions; per-field `#[serde(rename = "...")]` so the LinkML wire format round-trips. Ergonomic derives: every struct gets `PartialEq`, plus `Default` when all fields are conservatively default-able (`Option<T>` / `Vec<T>` always count; required primitives that implement `Default` qualify too). Every emitted enum carries `#[non_exhaustive]` so adding permissible values or subclasses to a schema doesn't break downstream `match` statements. Robust against malformed inputs: circular `is_a` / `mixin` chains terminate cleanly via visited-set guards rather than overflowing; unresolved global slot references emit `// WARNING:` comments rather than silently dropping fields. Generated code depends on `serde` and `chrono` in the consumer's `Cargo.toml`. Renderers are generic over `std::fmt::Write`, so `RustWriter::render_into(&mut sink, schema)` streams directly into any sink without an intermediate `String` allocation; `render(schema) -> String` remains as the convenience wrapper. Recursive `Eq + Hash` analysis: a struct, `<Name>Kind` closed enum, or `any_of` union derives `Eq + Hash` only when every transitive field type does (`f64` family disqualifies; `chrono::DateTime<Utc>`, `NaiveDate`, `NaiveTime` qualify; LinkML enums always qualify). Self-recursive class fields via `Box<T>` preserve the inner trait set; the fixpoint terminates because the support bit only flips from `true` to `false`. Each concrete struct also gets a `pub fn new(<required_fields…>) -> Self` constructor (optional fields default to `None`, multivalued to `Vec::new()`) so consumers survive schema additions of optional fields without breaking calling code; structs with no required-single fields skip the constructor since `Default::default()` already covers the empty-arg case.

### Changed
- Migrated `wgpu` 24 → 29 across `panschema-viz/webgpu.rs` and `panschema/gpu/{simulation,renderer}.rs`. Surface changes addressed: `InstanceDescriptor` no longer `Default`, `Instance::new` takes the descriptor by value, `DeviceDescriptor` requires `experimental_features` + `trace`, `Adapter::request_device` is single-arg, `PipelineLayoutDescriptor` swapped `push_constant_ranges` for `immediate_size` and now takes `&[Option<&BindGroupLayout>]`, `RenderPipelineDescriptor.multiview` → `multiview_mask`, `RenderPassColorAttachment` requires `depth_slice`, `RenderPassDescriptor` requires `multiview_mask`, `DepthStencilState.depth_write_enabled`/`depth_compare` now `Option<_>`, `wgpu::Maintain` → `wgpu::PollType`, `Surface::get_current_texture` returns `CurrentSurfaceTexture` enum instead of `Result`.
- `main.rs` and `server.rs` now use `FormatRegistry` instead of hardcoded readers/writers
- Force simulation defaults retuned for sparser graphs (stronger repulsion, weaker centering); node radii reduced for less visual crowding
- **MSRV bumped from 1.85 to 1.88** to enable let-chain syntax (`if let X = y && cond`) in source

### Fixed
- **Static graph layouts (Kamada-Kawai, Hierarchical) now survive node selection and single-node drag.** The drag handler in `panschema-viz` reheated the simulation unconditionally on every mousedown that hit a node — including click-without-drag — which lifted the simulation's `alpha` back above `alpha_min` and re-enabled the per-tick force-directed physics. The result: any click on a KK / Sugiyama page sent the layout into force-directed motion, undoing the static algorithm's work. `Visualization` now tracks `is_static_layout` (set when the constructor calls `freeze_at`) and skips the reheat for static layouts, so a single-node drag moves only that node via direct positional write and the rest of the layout stays where the static algorithm placed it. Pinned by `dragging_one_node_in_a_frozen_simulation_leaves_other_nodes_untouched` in `panschema-viz/src/simulation.rs`. See [docs/features/09-graph-layout-selection.md](docs/features/09-graph-layout-selection.md) slices 3 and 6.
- **Graph viz no longer drops slots referenced only via `class.slots:`.** The graph builder previously emitted class↔slot edges only from the slot-side `domain:` field, so slots listed by a class but missing `domain:` rendered as orphan nodes — even though the HTML class card already drew the connection correctly. The builder now also walks each class's `slots:` list and emits the equivalent edge, deduping against the slot-side pass so a slot with both `slot.domain = C` and `C.slots: [s]` produces one edge, not two. LinkML treats `domain` and `domain_of` as the same relation; the graph now matches. Visible improvement on multi-class slots like scimantic-schema's `content` (used by `Evidence` and `Conclusion`). See [docs/features/04-schema-force-graph-visualization.md](docs/features/04-schema-force-graph-visualization.md) slice 8.
- **Schema-level description in the metadata card is no longer double-escaped.** The metadata card template mounted `comment` without `|safe`, but the writer pre-renders that value as HTML (through `render_description`) — so Askama escaped the writer's `<p>…<a href="…">…</a>…` markup a second time and authors saw literal `<p>…</p>` markup as visible text. The fix is `{{ comment|safe }}`, matching the entity-card descriptions.
- **Header brand link in rendered HTML is no longer an absolute `/`.** Each page's `<a class="site-title">` previously emitted `href="/"`, which resolved to the *domain root* on subpath deploys (e.g. clicking the brand on `https://<user>.github.io/<repo>/schema/main/` sent the user to `https://<user>.github.io/`, 404 for the project). `panschema generate` now emits `./` (the page lives at the output root, so `./` is the deploy root). `panschema publish` reads the target from a new manifest field `[publishing].site_root_url`, default `"../current/"` — parent-relative to the canonical current-version page within the publish output cohort, symmetric with `url_pattern`'s `"../{version}/"` default. The default works for any standalone publish deploy; consumers whose publish dir is nested under a parent site (e.g. `<book>/schema/<version>/`) override the field (e.g. `"../../"` to escape into the book). See [docs/features/02-core-ontology-documentation.md](docs/features/02-core-ontology-documentation.md) slice 8.
- **Schema graph 2D layout now uses the full configured aspect-ratio viewport and produces legible labels at all graph sizes.** The CPU force simulation gains three composable changes: (1) anisotropic axial centering (`forceX` / `forceY` with `gravity_y / gravity_x = (w/h)³`) so isolated nodes equilibrate at a radius matching the configured aspect — landscape containers no longer leave half the horizontal space empty; (2) the largest connected component is placed at origin so anisotropic gravity doesn't have to fight an off-center initial layout; (3) `link_distance`, `charge`, and `collide_padding` scale with `√N` so the same defaults produce visibly-legible layouts from 6-node fixtures up to 100-node ontologies — the collide-padding scaling matters most, since collide is the only force that enforces minimum geometric spacing between every node pair. Validated via a multi-scale Playwright iteration test (`#[ignore]`-d for routine CI) that screenshots a synthetic graph at phone (390×844), laptop (1440×900), and 4K (3840×2160) viewports.
- **RDF serializers (TTL / JSON-LD / N-Triples / RDF/XML) now expand CURIE-shaped IRIs against the schema's `prefixes:` table** before emission. Previously a `class_uri: cco:ont00000005` produced `<cco:ont00000005>` — invalid as N-Triples (the spec requires absolute IRIs in `<...>`) and ambiguous in TTL/JSON-LD/RDF/XML (parsers read it as a relative IRI against the empty base). The TTL writer now also wires sophia's `TurtleConfig::with_prefix_map` so the schema's prefixes round-trip into `PREFIX` declarations at the top of the output. JSON-LD and RDF/XML emit fully-expanded absolute IRIs.
- **LinkML mixins now emit `rdfs:subClassOf` alongside the `is_a` parent in OWL output.** LinkML treats `mixins:` as multiple inheritance; the prior emitter only honored `is_a`, so a class declaring three mixins lost three subClassOf relations from its RDF representation.
- `YamlReader` now infers metaobject names from their dict keys (idiomatic LinkML), so explicit `name:` and permissible-value `text:` fields are optional. Applies to classes, slots, enums, types, class attributes, class slot_usage, and permissible values. Schemas produced by `linkml-runtime` and the broader LinkML toolchain (`gen-owl`, `gen-shacl`, `gen-python`) now load without modification. Explicit names still work; an explicit name that disagrees with the dict key is now a clear parse error.
- `GraphWriter` now emits range edges for inline class attributes (e.g., `Student.year` → `YearEnum`). Previously only top-level `slots:` produced domain/range edges, so most relationships in idiomatic LinkML schemas were silently dropped from the visualization. Inline attributes connect the owning class directly to the range target (no separate slot node), labeled with the attribute name.
- 3D camera `zoom()` direction was inverted relative to the 2D camera and the documented contract; `factor > 1.0` now zooms in for both.

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
