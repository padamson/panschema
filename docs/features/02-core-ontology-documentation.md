# Feature: Core Ontology Documentation

**Feature:** MVP Ontology Content Extraction & Display

**User Story:** As an ontology developer, I want to generate documentation that shows my classes, properties, and individuals, so that others can understand and use my ontology.

**Related ADR:** None yet

**Approach:** Vertical Slicing with Outside-In TDD

---

## Implementation Strategy

This implementation follows **vertical slicing** - each slice delivers end-to-end user value and can be tested/released independently.

Building on Feature 01 (Foundational UI Stack), this feature adds actual ontology content extraction and display using the existing component infrastructure.

*Documentation updates required:*
- [Main README](../../README.md) - Update once content is rendered
- [CHANGELOG](../../CHANGELOG.md) - Document each slice

---

## Vertical Slices

### Slice 1: Class Extraction & Display

**Status:** Completed

**User Value:** Users see their ontology's classes with labels, descriptions, and class hierarchy displayed in the documentation.

**Acceptance Criteria:**
- [x] Parser extracts owl:Class entities from Turtle files
- [x] Parser extracts rdfs:label and rdfs:comment for each class
- [x] Parser extracts rdfs:subClassOf relationships
- [x] Classes section displays all classes (not "0" count)
- [x] Class cards show label, description, parent class, and subclasses
- [x] E2E test verifies classes are rendered from reference.ttl

**Notes:**
- Uses existing class_card component
- Class hierarchy is displayed as links (parent/subclass)
- Reference ontology has: Animal → Mammal → Dog, Cat; Person

---

### Slice 2: Property Extraction & Display

**Status:** Completed

**User Value:** Users see their ontology's properties with types, domains, ranges, and descriptions.

**Acceptance Criteria:**
- [x] Parser extracts owl:ObjectProperty entities
- [x] Parser extracts owl:DatatypeProperty entities
- [x] Parser extracts rdfs:domain and rdfs:range for properties
- [x] Parser extracts owl:inverseOf relationships
- [x] Properties section displays all properties (not "0" count)
- [x] Property cards show label, description, type, domain, range
- [x] E2E test verifies properties are rendered from reference.ttl

**Notes:**
- Uses existing property_card component
- Domain/range resolved to class links (EntityRef) when the IRI matches a known class, otherwise displayed as datatype text (e.g., xsd:integer)
- Inverse-of relationships displayed as characteristics on property cards (e.g., "Inverse of: has owner")
- Reference ontology has: hasOwner, owns (object); hasName, hasAge (datatype)
- Sidebar simplified from individual entity listings to section-level links (Overview, Namespaces, Classes, Properties) with count badges
- Namespace count badge and section header with count added to main layout
- namespace_table.html refactored to remove its own heading/section wrapper (now wrapped by section_header in index.html)
- Metadata card heading renamed from "Ontology Metadata" to "Overview" for consistency with sidebar
- "Namespaces" link added to header navigation

---

### Slice 3: Individual Extraction & Display

**Status:** Completed

**User Value:** Users see example individuals with their types and property values.

**Acceptance Criteria:**
- [x] Parser extracts named individuals (entities with rdf:type pointing to a class)
- [x] Parser extracts property values for individuals
- [x] Individuals section displays all individuals
- [x] Individual cards show label, type(s), and property values
- [x] E2E test verifies individuals are rendered from reference.ttl

**Notes:**
- New `individual_card` component with snapshot tests (full and minimal variants)
- Individuals identified by `rdf:type owl:NamedIndividual` in reference ontology
- Type(s) displayed as links to class cards; property values displayed with links to property cards
- Sidebar updated with Individuals link and count badge
- "Overview" renamed to "Metadata" in sidebar and metadata card for consistency
- Reference ontology has: fido (a Dog with hasName="Fido" and hasAge=5)

---

### Slice 4: Release v0.1.0

**Status:** Completed

**User Value:** Users can install rontodoc from crates.io and generate useful documentation.

**Acceptance Criteria:**
- [x] crates.io account configured with API token
- [x] GitHub secret for CARGO_REGISTRY_TOKEN
- [x] Release workflow publishes to crates.io on tag
- [x] Tag v0.1.0 and verify CD pipeline
- [x] `cargo install rontodoc` works from crates.io
- [x] Remove "(Note: Not yet published to crates.io)" from README

**Notes:**
- Moved from Feature 01 Slice 6
- Only release once slices 1-3 provide real value
- Cargo.toml version bumped from 0.0.1 to 0.1.0
- CHANGELOG streamlined for initial release
- Release workflow uses 3-job pipeline: test (fmt, clippy, nextest with Playwright) → build-release (4 platforms) → publish-crate

---

### Slice 5: Class card content (v0.3.0 dogfood follow-up)

**Status:** Completed (β.1 mixins, β.2 xrefs, β.3 slots all shipped)

**User Value:** A reader of generated HTML can see every constraint the schema actually declares about a class — its direct slots, every `slot_usage` refinement (range, `any_of`, required narrowing), every mixin, and links to entities referenced via `[[Name]]` in descriptions — without falling back to the raw YAML.

**Context:** Surfaced by the scimantic-schema v0.2.0 dogfood (see [docs/mutation-debt.md] sibling notes). The pre-existing class card showed only header + description + `Subclass of <is_a>`; everything else the schema declared was invisible to a reader of the generated documentation.

**Acceptance Criteria:**
- [x] The class card lists every resolved slot (direct attributes + slots referenced via `slots:` + inherited slots from `is_a` and mixins), with each slot's range, required/optional, and multivalued framing visible.
- [x] `slot_usage` refinements are rendered alongside the inherited slot, with the narrowed range (`any_of: [A, B, C]`), narrowed `required: true`, or other overrides clearly distinguished from the inherited definition via a "refined here" tag.
- [x] The card lists each mixin under a "Mixes in" section, with anchor links to the mixin's class card.
- [x] `[[Name]]` markers in class descriptions and per-slot descriptions are resolved to `<a href="#class-Name">Name</a>` (or `#enum-Name` / `#prop-Name`), matching LinkML's documentation cross-reference convention. Unresolved names fall back to literal text with an HTML `<!-- WARNING -->` comment in the generated source.
- [x] Integration test `class_card_surfaces_mixins_slots_and_resolved_xrefs` runs against a YAML fixture exercising all three constructs (mixins, slot_usage override, `[[Name]]` xref to an enum); existing snapshot tests for the class_card component updated to include the new sections.

**Notes:**
- The graph-json already carries the mixin edges (`edge_type: "mixin"`); this slice just surfaces them in the per-class HTML.
- The `slot_usage` refinement rendering is the bulk of the work; class-card-as-template needs a section that distinguishes "inherited slot" from "refined slot at this class."
- Defer SHACL output of the same constraints — that's gated on the SHACL writer existing at all (future slice). For now, HTML is the only writer that needs to surface these facts.

---

### Slice 6: Responsive full-width layout + configurable graph aspect ratio

**Status:** Completed

**User Value:** Documentation pages use the available browser-window width fluidly. On a large display, entity cards tile into a multi-column grid; on narrow viewports they collapse to a single column. The schema graph visualization expands to fill the full content area at a configurable aspect ratio (default 16:8) so consumers can explore large graphs on big screens. Per-schema override via `panschema.toml` lets producers tune the ratio for their target audience (laptop default vs. desktop 16:9 vs. ultrawide 21:9, etc.).

**Context:** The pre-existing layout capped `.content-area` at `--content-max-width: 900px`, so on a 4K monitor the documentation sat in a narrow column with most of the screen blank. The schema graph was fixed at `height: 500px` regardless of viewport. Both bite hardest on the dogfood case (scimantic v0.2.0's 49-class BFO/CCO graph), where a larger graph viewport would meaningfully improve exploration. The 16:8 default (vs. the more familiar 16:9) fits a typical laptop screen alongside browser chrome and an OS task bar without the page needing to scroll.

**Acceptance Criteria:**
- [x] `.content-area` expands fluidly with no hard `max-width` cap; on viewports ≥769px the sidebar holds its fixed share and the rest of the row is content area.
- [x] Class, property, and individual card sections render as a responsive CSS grid (`repeat(auto-fill, minmax(~380px, 1fr))`). On viewports too narrow for two columns the cards stack to a single column.
- [x] `.graph-container` uses `aspect-ratio: W/H` (height derived from width) instead of the fixed `height: 500px`. Width remains `100%` of the content area. Default ratio is 16:8.
- [x] At widths ≤768px the existing single-column / collapsed-sidebar behavior is preserved unchanged.
- [x] `panschema.toml` accepts an optional `html_graph_aspect = "W:H"` field under each `[generate.<name>]` block, overriding the default per schema. Parser validates that both components are positive integers ≤9999 and rejects malformed input with an actionable error.
- [x] e2e test (Playwright) at 1280×720 asserts: at least two class cards share a row (within a small Y tolerance) AND the graph container's bounding box satisfies `width / height ≈ 16/8 ± 5%`. A narrow-viewport companion (375×667) asserts cards are strictly stacked.
- [x] Unit tests cover: `aspect-ratio: 16 / 8` in the default-rendered CSS; `aspect-ratio: 4 / 3` (or any override) when constructed via `HtmlWriter::with_graph_aspect`; `parse_graph_aspect` accepts well-formed `"W:H"` and rejects malformed input; the manifest field round-trips through `Manifest::from_str`.

**Notes:**
- Line-length-readability for descriptions is bounded by each card's individual width (~380–500px under the grid layout), not by the outer content frame, so wide viewports don't produce unreadable prose lines.
- The graph already uses `width: 100%`; the change is to replace `height: 500px` with `aspect-ratio: W/H` + `height: auto`. The wasm-side renderer queries `canvas.getBoundingClientRect()` at init, so it picks up the new dimensions automatically.
- Aspect ratio is stored internally as `(u32, u32)` because CSS's `aspect-ratio: W / H` declaration takes integer components. A single-`f64` representation would simplify the API but lose authorial intent (`16:9` is more readable than `1.778`) and introduce floating-point fuzz on clean ratios like `4/3`.

---

### Slice 7: Improve force-directed default so the graph fills its viewport

**Status:** Completed

**User Value:** The schema graph fills the available width and height of its container. Connected clusters spread far enough that node labels read individually instead of stacking into an unreadable blob. Isolated property/slot nodes distribute around the connected cluster rather than piling up on one side. The same default parameters work across phone-sized, laptop-sized, and 4K viewports.

**Context:** After slice 6 made the graph container fluidly fill the content area at a configurable aspect ratio, the underlying simulation still settled into a roughly-circular equilibrium because its forces were isotropic and its parameters didn't scale with node count — a 100-node cluster packed into the same ~200-unit radius as a 10-node one, with severely overlapping labels.

**Approach:** Three composable changes to the existing CPU force simulation:

1. **Anisotropic axial centering (`forceX` / `forceY`).** Each tick, every node feels a weak harmonic pull toward the origin with anisotropic stiffness:

   ```
   vx -= gravity_x · x
   vy -= gravity_y · y
   ```

   For an isolated node feeling cluster repulsion `R/d²` and centering pull `g·d` along axis `a`, the equilibrium distance is `d_a = (R/g_a)^(1/3)`. Choosing `gravity_y / gravity_x = (w/h)³` gives `d_x / d_y = w/h` — bbox aspect matches the configured container aspect for isolated nodes. Spring-bound cluster nodes are dominated by their links and barely move, which is the intent.

2. **Largest-component-at-origin initial layout.** `layout_by_component_2d` places the biggest connected component at the origin and rings smaller components around it. Placing the dominant component off-origin would interact poorly with the anisotropic gravity — gravity can only partially correct an off-origin start within the alpha schedule, so any offset bakes a layout asymmetry into the equilibrium.

3. **`√N` scaling of force parameters.** `link_distance`, `charge`, and especially `collide_padding` all scale with `√N` inside `from_graph_data`. The collide-padding scaling matters most: it enforces a minimum geometric distance between every node pair, which is the only force in the system that breaks up "siblings stacked at angle 2π/B around a high-branching tree node." Without it, the natural layout for any tree-structured ontology has labels stacking at the trunk regardless of repulsion strength.

**Validation harness:** a multi-scale Playwright iteration test (`#[ignore]`-d for routine CI, run explicitly) generates a synthetic ontology + screenshot at three viewport / graph-size combinations:

| Scale | Viewport | Synthetic graph |
|---|---|---|
| Phone | 390 × 844 | 6 connected + 2 isolated |
| Laptop | 1440 × 900 | 30 connected + 8 isolated |
| 4K | 3840 × 2160 | 80 connected + 20 isolated |

Each run writes `target/graph-2d-{phone,laptop,4k}.png` and dumps a JSON pixel-bbox summary so parameter changes can be compared numerically across scales without manual rebuilds.

**Deferred to [Feature 09 (Graph layout selection)](09-graph-layout-selection.md):** the user-selectable picker for alternate layout algorithms (Sugiyama for tree-structured ontologies, circular for cycles, etc.). Force-directed inherently produces a dense core for high-branching trees — the structural fix is a different algorithm, not parameter tuning.

**Edge-crossing minimization** is deferred to [Feature 09 (Graph layout selection)](09-graph-layout-selection.md), where adopting a maintained algorithm crate (`rust-sugiyama` for hierarchical, `egraph-rs` for stress / KK / SGD) addresses crossings as part of the broader algorithm-picker effort. An earlier attempt at multi-seed best-of-K within this slice was empirically a no-op — for the synthetic test graphs all rotations land in basins with identical crossing counts — so the right path is to adopt algorithms that target crossings as an objective, not to layer rotation-sampling on top of FR.

**Acceptance Criteria:**
- [x] `SimulationConfig` gains `gravity_x_strength: f32` and `gravity_y_strength: f32`, default `0.0` (no-op, preserving existing single-render behavior in callers that don't opt in).
- [x] `CpuSimulation::with_aspect_ratio(w: u32, h: u32) -> Self` builder sets `gravity_y_strength / gravity_x_strength = (w/h)³`, with absolute magnitudes split asymmetrically (`base · (h/w)^1.5` and `base · (w/h)^1.5`) so both axes converge in similar tick counts regardless of which aspect is configured.
- [x] `tick_with_fixed` applies the centering forces after repulsion and link forces, before velocity integration. Fixed (pinned) nodes are not affected.
- [x] `from_graph_data` scales `link_distance` (`× (1 + √N · 0.10)`), `charge` (`× (1 + √N · 0.10)`), and `collide_padding` (`4 + √N · 4`) with the node count, so the same defaults produce legible layouts from 6-node up to 100-node graphs.
- [x] `layout_by_component_2d` places the largest connected component at origin; smaller components ring around it on a `big_radius = 150` circle.
- [x] Native unit test: with `with_aspect_ratio(16, 8)` on a lopsided graph (20 connected + 8 isolated), the post-settle bbox is biased wider than tall (`w > h · 1.3`). Symmetric `(8, 16)` test passes the same tolerance taller-than-wide.
- [x] Native unit test: with `with_aspect_ratio(16, 8)` on a lopsided graph (30 connected + 5 isolated), the layout has (a) no node more than 3× the median distance from centroid, (b) the angular distribution of isolated nodes spans ≥ π radians (largest open arc < π), (c) bbox width ≥ 200 world units (catches "collapsed to origin").
- [x] Default (no `with_aspect_ratio` call): `gravity_*_strength` stays at `0.0`, preserving the historical circular equilibrium for callers that don't opt in.
- [x] 2D JS init in `graph_viz.html` reads the container's `--graph-aspect` custom property and calls `with_aspect_ratio` accordingly. (3D path: API parity but no-op — ellipsoid centering is a follow-up.)
- [x] Multi-scale Playwright screenshot harness (`e2e_2d_graph_screenshots`, `#[ignore]`-d) writes three PNGs and dumps pixel-bbox stats plus a post-settle edge-crossing count, used as the iteration feedback loop while tuning force parameters. Supporting infrastructure: `sim_common::count_edge_crossings_2d` (CCW orientation predicates, shared-endpoints excluded) with unit tests, `Visualization::edge_crossings()` exposing the count over the wasm boundary, and a `chord` enrichment in the synthetic-TTL generator so non-isomorphic basin structures appear in graphs with ≥10 connected nodes (a precondition for any future crossing-aware algorithm to meaningfully reduce crossings).

**Notes:**
- `gravity_y / gravity_x = (w/h)³`, not `(w/h)²` — derived from `equilibrium_d³ = repulsion / gravity` (centering linear in `d`, repulsion quadratic). The cube exponent is what gives the right ratio for the 2D case.
- Aggressive `√N` scaling of `link_distance` / `charge` (factor ≥ 0.20) backfires: it blows the world bbox out past the viewport and `fit_to_bounds` zooms back to fit, shrinking the rendered cluster to a tiny patch. The `0.10` factor keeps the bbox roughly canvas-sized; collide-padding does the legibility work without needing larger world dimensions.
- Strength tuning is empirical: `GRAVITY_BASE = 0.003` and the `√N` factors above produce visibly-good layouts at all three test scales. The single-source-of-truth is the screenshot harness; future parameter changes should re-run it and compare PNGs across scales.

---

### Slice 8: Parent-relative header brand link + absolute-URL audit

**Status:** Completed

**User Value:** The "back to root" navigation affordance in every rendered page (the site-title brand link in the header) resolves correctly regardless of whether the site is deployed at a domain root or at a subpath. Today's template emits `<a href="/" class="site-title">` — absolute path against the domain root, so on standard GitHub Pages (`https://<user>.github.io/<repo>/`) clicking the brand sends the reader to `https://<user>.github.io/` (404 for the project), not the project's landing page. This is the same bug class as feature 11 slice 7's `url_pattern` default; slice 7 fixed the cross-version dropdown links but didn't audit the rest of the template.

**Design — why configurable, not hardcoded:** A naive depth-based fix would emit `./` for `panschema generate` (page at output root) and `../../` for `panschema publish` (page two levels down). `../../` works for scimantic-schema's specific layout (publish dir nested under a book at `<book>/schema/<version>/`) but bakes in that assumption — a standalone publish deploy at `https://<user>.github.io/<repo>/<version>/` would emit `../../` and land at `https://<user>.github.io/` (the user's gh-pages root), same bug class, one level up. So the brand link target is a manifest-configurable field, symmetric with slice 7's `url_pattern`: `[publishing].site_root_url`, default `"../current/"` (parent-relative to the canonical current-version page within the publish output cohort — works for any standalone deploy). Consumers whose publish output is nested under a parent site override to e.g. `"../../"`.

**Acceptance Criteria:**
- [x] `panschema generate` (single-version) emits `<a href="./" class="site-title">` — page lives at the output root, `./` is the deploy root.
- [x] `panschema publish` (versioned) emits the brand link from a manifest field `[publishing].site_root_url`. Default `"../current/"`; the value is forwarded verbatim into each per-version page.
- [x] Audit `panschema/templates/**` for any other `href="/..."` or `src="/..."` absolute-URL emissions. Each finding is either (a) routed through a configurable / depth-aware mechanism or (b) explicitly justified in a comment.
- [x] Unit tests assert: (i) the default `site_root_url` (`../current/`) renders on each per-version page; (ii) a user-supplied override (`../../`) survives verbatim to the rendered HTML; (iii) a single-version page emits `./`.

**Notes:**
- The depth computation must thread through the same render context as the existing `VersionContext` (slice 4 of feature 11): the writer knows whether it's rendering single-version or per-version output, and per-version output knows it sits two directory levels deep below the deploy root.
- This slice is small but unblocks the scimantic-schema dogfood's "back to book" navigation immediately — the header brand becomes the back-link affordance once it resolves correctly. No schema-side workaround is acceptable; the fix must land in panschema.

---

### Slice 9: Markdown rendering in description fields (preserve `[[Name]]` xrefs)

**Status:** Completed

**User Value:** Schema authors can use standard markdown in `description:` fields — `[link text](url)`, `**bold**`, `*italic*`, `` `code` `` — and have it render as HTML in the documentation. Today the description processor handles `[[Name]]` cross-reference markers (slice 5 work) but escapes every other markup, so authors can't put inline links or emphasis in schema descriptions. The `[[Name]]` xref handling is the strong precedent: descriptions are *already* processed text, not raw text — extending that processor to also handle markdown is a natural increment.

**Acceptance Criteria:**
- [x] Description fields (schema-level + per-class + per-slot + per-individual) accept standard CommonMark markdown and render the canonical HTML output for at minimum:
  - Inline links: `[text](url)` → `<a href="url">text</a>`
  - Emphasis: `**bold**`, `*italic*`, `` `code` ``
  - Stretch (call separately): paragraphs, lists, fenced code blocks
- [x] Existing `[[Name]]` cross-reference resolution continues to work and produces anchor links as today.
- [x] HTML safety: the markdown processor's output is HTML-safe by construction (e.g. `pulldown-cmark` with safe-mode HTML disallowed); raw HTML embedded in descriptions is either rendered safely or escaped — pick one explicitly and document the choice in source.
- [x] All existing description-rendering tests continue to pass; new tests cover the new markup forms and the no-regression case for `[[Name]]` xrefs.
- [x] One markdown-aware library lands as a new dep (likely `pulldown-cmark`; supply-chain exemption added).

**Notes:**
- The processing order matters: `[[Name]]` xref expansion should run *before* markdown so that `[[ClassName]]` doesn't get parsed as a markdown reference (`[ClassName]` would otherwise be a markdown link reference syntax). Or run xref resolution against the post-markdown HTML; pick whichever produces cleaner edge-case behavior.
- HTML-safety policy: the cleanest default is "markdown only — raw HTML escaped." Authors who genuinely need raw HTML (`<a href="...">` linking to external resources) can use the equivalent markdown form. If that's too restrictive in practice, revisit; until then, prefer the safer default.
- The `[[Name]]` xref handling itself shipped under [slice 5](#slice-5-class-card-content-v030-dogfood-follow-up); this slice extends the same processor.

---

### Slice 10: Class card consumes the shared slot resolver

**Status:** ✅ Complete

**Priority:** Should Have

**User Value:** The class card now reads its effective slot set directly from `panschema::linkml_resolve` instead of through a re-export chain. The HTML output, Rust output, and graph output all observe the same effective slot data — fixes for `slot_usage`, mixin flattening, or cycle handling that land in the shared resolver light up in every writer simultaneously.

**Acceptance Criteria:**
- [x] `html_writer`'s class-card slot resolution imports `linkml_resolve::resolve_effective_slots` directly. (Previously imported `resolve_slots` from `rust_writer`; after feature 12 slice 12.1 that was already a re-export of the shared resolver, so byte-identity for the rendered HTML is guaranteed.)
- [x] Class-card "refined here" tags continue to render exactly as today for `slot_usage` overrides — driven by `class_def.slot_usage.contains_key(slot_name)`, which is independent of which resolver returned the effective slot.
- [x] Existing class-card integration tests (including `class_data_flags_slot_usage_refinements_with_refined_here`) pass unchanged.

**Notes:**
- This shipped as a one-line import change because feature 12 slice 12.1's `rust_writer::resolve_slots` re-export had already pointed html_writer at the shared implementation transitively. The direct import is for clarity and to make the dependency visible to anyone reading the file. Output remains byte-identical.
- Two follow-up items — surfacing slot provenance ("from `<class>`" tags) and a cross-writer consistency test — were originally drafted as part of this slice but blocked on other work (provenance needs feature 12 slice 12.4's `ResolvedSlot` wrapper; the consistency test needs feature 04 slice 12's deferred per-class slot views). They moved to slice 11 below.

---

### Slice 11: Class card surfaces slot provenance + cross-writer consistency test

**Status:** ✅ Complete

**Priority:** Nice to Have

**User Value:** Once the shared resolver carries provenance metadata, the class card can surface "inherited from `<Parent>`" / "from `<Mixin>` (mixin)" tags on flattened slots — authors building intuition for inheritance get the answer without manually walking the hierarchy. A consistency test then pins that the class card and the graph hover card agree on the effective shape of a `slot_usage`-refined slot.

**Acceptance Criteria:**
- [x] Inherited slots in the class card carry a small "from `<class>`" tag sourced from `Provenance::Inherited` (shipped with feature 12 slice 12.4). Direct attributes get no tag; `slot_usage`-refined slots get the existing "refined here" tag in addition.
- [x] One new integration test builds a fixture where `Question` (extending `Activity`) refines `wasGeneratedBy` via `slot_usage`, generates one page, and asserts the class card and the embedded graph hover payload surface the same effective range (`QuestionFormation`, not `Activity`). The graph side carries the refined view via feature 04 slice 14's structured slot payload.

**Notes:**
- Blocked on feature 12 slice 12.4 and feature 04 slice 12's deferred slot-side completion. Both are tracked as dependencies; this slice can ship as soon as either's payload is available.

---

### Slice 12: `*_mappings` round-trip — IR + HTML + RDF

**Status:** ✅ Complete

**Priority:** Must Have

**User Value:** Schema authors who ground their classes and slots in upstream ontologies (BFO, CCO, IAO, CiTO, …) via `exact_mappings:` / `close_mappings:` / `related_mappings:` / `narrow_mappings:` / `broad_mappings:` currently get **silent data loss**: the YAML parses (no `deny_unknown_fields` on the relevant types), the values vanish into the void, and nothing surfaces in the rendered HTML or the emitted RDF. The whole reuse story is invisible. After this slice, mappings are first-class IR fields, render on class and property cards, and emit as `skos:exactMatch` / `closeMatch` / `relatedMatch` / `narrowMatch` / `broadMatch` triples in the RDF writers.

**Acceptance Criteria:**
- [x] `ClassDefinition` and `SlotDefinition` in `linkml.rs` gain optional `exact_mappings: Vec<String>`, `close_mappings: Vec<String>`, `related_mappings: Vec<String>`, `narrow_mappings: Vec<String>`, `broad_mappings: Vec<String>` fields (all `#[serde(default)]` for back-compat against schemas that don't use them).
- [x] `yaml_reader.rs` parses each of the five fields when present. (No code change needed — `serde_yaml::from_str` handles them automatically via the `#[serde(default)]` annotations.)
- [x] HTML class card gains a "Mappings" row when any of the five fields is non-empty. Each mapping is rendered with its kind and the value as a CURIE-expanded hyperlink via `linkml_resolve::expand_curie`; unresolved prefixes render as a muted `<span>` with a tooltip explaining the gap.
- [x] HTML property card gains the same "Mappings" row with the same shape.
- [x] RDF writers (TTL, JSON-LD, N-Triples, RDF/XML) emit one triple per mapping using the SKOS predicates: `skos:exactMatch`, `closeMatch`, `relatedMatch`, `narrowMatch`, `broadMatch`. Built via `build_rdf_graph` which is the shared entry point for all four serializers.
- [x] Integration tests cover: class-side mappings (BFO + CiTO mix) producing SKOS triples in the RDF graph; slot-side mappings producing the same; HTML rendering surfacing `Mapping { kind, display, href }` view-models with `href = Some(expanded)` for known prefixes, `Some(passthrough)` for absolute URLs, and `None` for unknown prefixes.

**Notes:**
- Source: friction `[2026-06-06] exact_mappings / close_mappings silently dropped from all output` (severity: silent-correctness-bug).
- Skipped from initial scope: emit a top-level `@prefix skos:` declaration in TTL output when at least one mapping triple lands. The serializer already accepts unknown-prefix IRIs as absolute URLs, so output is valid TTL today; adding the prefix declaration is a cosmetic follow-up.

**Notes:**
- Source: friction `[2026-06-06] exact_mappings / close_mappings silently dropped from all output` (severity: silent-correctness-bug).
- The five fields are LinkML's full mapping vocabulary; ship them together so authors don't hit "exact_mappings works but close_mappings doesn't".
- Out of scope: rendering mappings in the graph hover card. The hover card is for hot iteration; mappings are a "reuse provenance" affordance better served by the persistent panel.

---

### Slice 13: Hyperlink + CURIE-expand `class_uri` / `slot_uri` in HTML

**Status:** ✅ Complete

**Priority:** Should Have

**User Value:** Class and property cards currently render `class_uri: cco:ont00000958` as plain `<code>` text. Authors grounding their schema in upstream ontologies want the IRI to be a clickable link to the upstream PURL so they can verify the grounding without leaving the docs. After this slice, every `class_uri` / `slot_uri` becomes a hyperlinked, CURIE-expanded label.

**Acceptance Criteria:**
- [x] HTML class card's IRI display passes the `class_uri` value through `linkml_resolve::expand_curie` (slice 12.2 of feature 12). When `expand_curie` returns `Some(full_iri)`, the displayed CURIE is wrapped in `<a href="...">` pointing at the expanded IRI; the copy-button payload also switches to the expanded IRI so a click yields a directly-resolvable URL.
- [x] HTML property card's IRI display gets the same treatment for `slot_uri`.
- [ ] Permissible-value `meaning:` values (when present) get the same treatment so enum-value cards link out to their upstream definition. **Deferred** — enum cards are a separate template; carved into a follow-up slice once we touch that surface.
- [x] Integration test: a class with `class_uri: cco:ont00000005` and `prefixes: { cco: http://example.org/cco/ }` populates `ClassData.iri_href` with the expanded IRI; a class with `class_uri: unknown:Foo` leaves `iri_href = None`; a class with no `class_uri` leaves `iri_href = None`. Symmetric test for `PropertyData.iri_href`.
- [x] CSS rule for `.entity-iri-link` keeps the card visually quiet at rest (no underline by default), with hover state revealing the link. Author-supplied prefixes that don't expand fall back to today's plain `<code>` rendering verbatim.

**Notes:**
- Tested with the existing class-card snapshots — they update mechanically. New unit tests in `html_writer.rs::tests` cover the IR → view-model expansion.

**Notes:**
- Source: friction `[2026-06-06] class_uri / slot_uri shown as plain text, never hyperlinked or CURIE-expanded in HTML` (severity: annoyance).
- Depends on feature 12 slice 12.2 (`expand_curie`) being available — that's complete.

---

### Slice 14: Abstract-class badge on class cards

**Status:** ✅ Complete

**Priority:** Should Have

**User Value:** Authors marking foundational classes `abstract: true` (e.g. BFO/CCO bases that exist for inheritance but aren't meant to be instantiated) currently get no visual hint in the HTML doc body — `is_abstract` reaches only the graph-viz JSON, not the cards. A reader can't tell at a glance which classes are foundation vs. instantiable. After this slice, abstract classes carry a clear badge on their card heading.

**Acceptance Criteria:**
- [x] `ClassData` (the template view model in `html_writer.rs`) gains an `is_abstract: bool` field threaded from `ClassDefinition.r#abstract`.
- [x] The `class_card.html` template renders a small `abstract` badge in the card heading when `is_abstract` is true — uppercase, muted color, sits inline next to the class title. The badge style stays subtle (it's a hint, not an alarm).
- [x] Snapshot test `snapshot_class_card_abstract_variant.snap` captures the badged rendering; assertion in the test body pins the `<span class="abstract-badge"` presence.
- [x] Unit test `class_data_threads_is_abstract_from_class_definition` builds a schema with one abstract + one concrete class and verifies only the abstract `ClassData` carries `is_abstract = true`.

**Notes:**
- Source: friction `[2026-06-06] abstract classes have no indicator in the HTML doc body` (severity: annoyance).
- The graph hover card already surfaces the badge (slice 9 of feature 04, `(abstract)` suffix). This slice closes the loop on the persistent HTML view.

---

### Slice 15: Hierarchy view in the Classes section

**Status:** ✅ Complete

**Priority:** Should Have

**User Value:** The Classes section currently renders as an alphabetical flat list of cards. Authors building an `is_a`-rooted hierarchy can't see the subclass/superclass structure at a glance — they have to read each "Subclass of" line on each card, or switch to the graph view. After this slice, classes are grouped or indented by `is_a` so the inheritance tree is visible in the doc body itself.

**Acceptance Criteria:**
- [x] HTML Classes section is rendered as a hierarchical structure rooted at classes with no `is_a` parent. Subclasses nest under their parents via CSS indentation (the rendered HTML uses semantic nesting — `<ul>` / `<li>` or equivalent — not just visual indentation).
- [x] Classes that participate in multiple inheritance chains (via mixins or pathological `is_a` overrides) appear once under their `is_a` parent; mixin relationships continue to be surfaced via the "Mixes in" section per slice 5.
- [x] Disconnected roots (classes with no `is_a` and no descendants) appear as flat top-level entries alongside the rooted trees.
- [x] Anchor links from elsewhere in the page (e.g. `#class-Foo`) continue to scroll to the right card; the nesting change is purely structural.
- [x] A user-facing toggle ("Flat" / "Tree") in the section header lets readers switch between hierarchical and the existing alphabetical view; the preference persists in `localStorage` like the existing label-visibility prefs.
- [x] Integration test: a fixture with a 3-level `is_a` chain (`Animal → Mammal → Dog`) renders Dog nested under Mammal nested under Animal in the tree view, and as a flat alphabetical list in the flat view.

**Notes:**
- Source: friction `[2026-06-06] Classes section is a flat list; no hierarchy view` (severity: annoyance / feature request).
- The tree view is the natural default once it ships; the toggle exists for readers who want the alphabetical view back for searching.
- Out of scope here: the equivalent treatment for slots or enums. Slots don't have an `is_a` hierarchy in panschema's IR today; enums don't either. If those grow it, file follow-ups.

---

### Slice 16: External `subclass_of` grounding — IR + HTML + RDF

**Status:** ✅ Complete

**Priority:** Must Have

**User Value:** Schema authors who ground their classes in upstream ontologies via `subclass_of: cco:ont…` (the LinkML mechanism for declaring `rdfs:subClassOf` to an external term, distinct from intra-schema `is_a`) previously got **silent data loss**: the YAML key vanished into the void, the HTML showed nothing, and the emitted RDF was missing the grounding axioms the schema declared. The whole "this class is a subclass of BFO:Continuant" story was invisible in every output. After this slice, `subclass_of` is a first-class IR field; the HTML class card renders a "Subclass of (external)" row with CURIE-expanded hyperlinks, and the RDF writers emit one `rdfs:subClassOf <expanded>` triple per entry.

**Acceptance Criteria:**
- [x] `ClassDefinition` in `linkml.rs` gains `subclass_of: Option<String>` — scalar, mirroring the LinkML metamodel (`subclass_of` is `multivalued: false`). Authors needing multiple groundings use mixins.
- [x] `yaml_reader.rs` parses the field when present. (No code change needed — serde handles it via the standard `Option` deserialization; a regression test `subclass_of_deserializes_as_scalar_per_linkml_metamodel` pins the scalar contract.)
- [x] HTML class card gains a "Subclass of (external)" row when `subclass_of` is set. The value is CURIE-expanded via `linkml_resolve::expand_curie` and rendered as `<a href="...">` (with `target="_blank"` since the link points outside the schema); unresolved prefixes fall back to plain text with a tooltip flagging the missing declaration.
- [x] RDF writers (TTL, JSON-LD, N-Triples, RDF/XML) emit `rdfs:subClassOf <expanded>` when `subclass_of` is set. Sits alongside the existing `is_a` / mixin subClassOf emissions — the OWL output now carries every grounding axiom the LinkML source declared.
- [x] Integration tests cover: class-side `subclass_of` produces an `rdfs:subClassOf` triple in the RDF graph (`build_rdf_graph_emits_rdfs_subclass_of_for_external_subclass_of`); HTML rendering surfaces `external_superclasses` view-models with `href = Some(expanded)` for known prefixes and `None` for unknown (`class_data_threads_external_subclass_of_with_expanded_iri`).

**Notes:**
- Source: friction `[2026-06-06] subclass_of (external grounding) dropped from HTML and RDF` (severity: silent-correctness-bug — the second one surfaced by the scimantic-schema dogfood).
- The symmetric pattern to slice 12 (mappings). RDF emission reuses the existing `expand_curie` helper from the rdf_serializers module rather than the linkml_resolve one; consolidating the two is filed under feature 12.

---

### Slice 17: Unify on "slot" terminology + slot-card parity

**Status:** ✅ Complete

**Priority:** Should Have

**User Value:** The rendered doc and the schema graph called the same entity two different things — the graph said "slot" (matching LinkML and the YAML the author writes), while the HTML doc said "property" (a "Properties" section, sidebar entry, and `Object Property` / `Datatype Property` badge, inherited from the OWL/RDF side). A reader moving between the graph and the doc body had to translate between two vocabularies for one concept. After this slice the doc says **"slot" everywhere**, and the per-relation card carries everything the graph hover used to show on its own, so the two views can't drift. See [ADR-006](../adr/006-slot-terminology.md).

**Acceptance Criteria:**
- [x] The HTML "Properties" section, sidebar entry, and heading become **"Slots"** (`id="slots"`, `#slots` anchor); each card id becomes `#slot-<name>` and the `[[Name]]` xref output for a slot resolves to `#slot-<name>` (superseding slice 5's `#prop-<name>`). The per-card badge reads a single **"Slot"** instead of `Object Property` / `Datatype Property` — the object-vs-datatype distinction is carried by the card's Range row (a class link vs a datatype name), so no information is lost.
- [x] The rename holds regardless of source format: an OWL-imported schema reads as "slot" because the reader normalizes to the LinkML IR before rendering. The OWL reader/writer/RDF layer keeps "property" internally (OWL spec terms: `owl:ObjectProperty`, `owl:DatatypeProperty`).
- [x] Internal identifiers are renamed so the code carries one vocabulary: `PropertyData` → `SlotData`, `property_card.html` → `slot_card.html`, `PropertyCardComponent` → `SlotCardComponent`, `property_type` → `slot_type`, the `.property-badge` / `.prop-ref` CSS classes → `.slot-badge` / `.slot-ref`.
- [x] The slot card is brought up to parity with the graph hover: it lists every class the slot is a domain of (a slot can belong to several — resolved via `linkml_resolve::resolve_slot_domains`), its validation `pattern`, an `identifier` flag, and explicit `minimum_cardinality` / `maximum_cardinality` bounds (`min..max`), alongside required / multivalued / inverse and mappings.
- [x] A polymorphic `any_of` range renders on the slot card as `any of [A, B, C]` with each branch anchor-linked when it names a declared class (previously the Range row was blank for union-ranged slots).
- [x] Snapshot tests for the renamed `slot_card` component and the `render_xref` slot branch pin the new anchors and badge; the `#slots`/`#slot-<name>` anchors are exercised by the e2e happy-path test.

**Notes:**
- Source: friction surfaced by the scimantic-schema dogfood — the graph↔doc vocabulary split, plus `any_of` ranges silently dropping from the slot card.
- The `#properties` → `#slots` and `#prop-<name>` → `#slot-<name>` anchor changes break external deep links; acceptable pre-1.0 and recorded in ADR-006.
- The `any of [A, B, C]` row shows the slot's *global* union. A class that narrows it via `slot_usage` renders the same global union here; the per-class induced range is slice 19 (pending), built on feature 12 slice 12.5.

---

### Slice 18: Enumerations and Types HTML card sections

**Status:** ✅ Complete

**Priority:** Should Have

**User Value:** The schema graph renders enum nodes (diamonds) and type nodes (rectangles), but the HTML doc has only Classes / Slots / Individuals sections — so enums and types are graph-only. Enums especially carry information worth reading: permissible values, each value's description, and a `meaning` IRI grounding the value in an upstream vocabulary. An author inspecting the rendered docs can't review an enum's allowed values without opening the YAML or the graph. This slice adds **Enumerations** and **Types** sections for parity with every node kind the graph draws, and lets the graph hover reuse those cards (closing the enum/type gap left by feature 04 slice 21).

**Acceptance Criteria:**
- [x] `EnumData` and `TypeData` view-models built from the IR's `EnumDefinition` (permissible values + per-value description + CURIE-expanded `meaning`) and `TypeDefinition` (base type, `uri`, pattern where present).
- [x] An **Enumerations** section (`id="enums"`, `#enum-<name>` card ids) renders one card per enum: its description, and a list of permissible values each showing its text, description, and a hyperlinked `meaning` IRI (reusing the upstream-label cache so the link reads as a label when cached).
- [x] A **Types** section (`id="types"`, `#type-<name>` card ids) renders one card per declared type: parent type (`typeof`, linked to its own card when declared here), `uri`, and `pattern`.
- [x] Sidebar gains "Enumerations" and "Types" entries with count badges; the `render_xref` `#enum-<name>` branch now resolves to a real card, and a new `#type-<name>` branch links type references. Both sections are omitted when the schema declares none.
- [x] The graph hover (feature 04 slice 21) reuses the rendered `#enum-<name>` / `#type-<name>` card for enum and type nodes — `nodeCardElement` now maps every node kind to its card.
- [x] Snapshot tests for the new `enum_card` / `type_card` components; an integration test renders the full sections from an in-memory schema, and an e2e (`e2e_renders_enum_and_type_sections`) asserts both sections, cards, and the sidebar entries render in a browser from a LinkML fixture (the OWL reference fixture carries no enums/types, so a dedicated `enum_type.yaml` fixture is used).

**Notes:**
- Source: friction `[2026-06-14] enums (and types) render in the graph but have no HTML card section` (severity: annoyance / completeness gap). The IR data already exists; only the HTML surface was missing.
- Graph↔HTML parity check: every node kind the graph renders now has a corresponding HTML card section.
- New `--color-enum` / `--color-type` theme vars match the graph's purple-diamond / orange-rectangle node palette.

---

### Slice 19: Render the induced per-class slot range on class and slot cards

**Status:** ✅ Complete

**Priority:** Should Have

**User Value:** A class that narrows an inherited slot via `slot_usage` — a single `range`, a smaller `any_of`, or `maximum_cardinality: 0` — should show its *narrowed* I/O on the card, not the wide inherited union. Today the card shows the global range even where a subclass refined it, so the per-class story (e.g. `Analysis` takes a `Dataset` and produces a `Result`; `EvidenceAssessment` produces no artifact) is invisible. This slice consumes the induced effective-slot range (feature 12 slice 12.5) so each class card reflects what that class actually declares.

**Acceptance Criteria:**
- [x] The class card's slot row shows the induced effective range for the current class: a `slot_usage` `range` narrowing renders the narrowed single range; a narrowed `any_of` renders the smaller union; the `range ∩ any_of` intersection is reflected (no lingering base union masking a single-range narrowing). A single-member induced union collapses to a single range row.
- [x] A slot suppressed for the class via `maximum_cardinality: 0` renders as "produces no value" rather than showing the inherited range, and keeps its "refined here" badge.
- [x] The standalone slot card continues to show the slot's global range/union (slice 17); the per-class narrowing appears on the *class* card where the refinement is declared (the slot card reads the global `schema.slots`, the class card the resolved per-class view).
- [x] Integration test (`class_card_renders_induced_per_class_slot_range`) against a fixture with a scalar narrowing, a smaller-`any_of` replacement, and a `maximum_cardinality: 0`, asserting the class card shows the narrowed range, the replaced union, and the suppressed slot. Dogfood-verified end-to-end render.

**Notes:**
- Source: same friction as feature 12 slice 12.5 (`[2026-06-14] slot_usage / per-class facets not rendered`). This is the HTML-card half; feature 04 slice 22 is the graph-edge half; feature 12 slice 12.5 is the IR foundation both consume.
- `SlotInClass` gained a `suppressed` flag; the build maps `InducedRange.ranges` to a single `range` (one member) or `any_of` (several), keeping the template's existing range/union rendering.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: Classes | Must Have | Feature 01 | Completed |
| Slice 2: Properties | Must Have | Slice 1 | Completed |
| Slice 3: Individuals | Should Have | Slice 2 | Completed |
| Slice 4: Release | Must Have | Slice 1-3 | Completed |
| Slice 5: Class card content | Should Have (v0.3.0) | Slice 1, Feature 03 | Completed |
| Slice 6: Responsive layout + configurable graph aspect | Should Have (v0.3.0) | Slice 1, Feature 04 | Completed |
| Slice 7: Improved force-directed default (fill viewport) | Should Have (v0.3.0) | Slice 6, Feature 04 | Completed |
| Slice 8: Parent-relative header brand link + absolute-URL audit | Must Have | Slice 4, Feature 11 slice 4 | Completed |
| Slice 9: Markdown rendering in description fields | Should Have | Slice 5 | Completed |
| Slice 10: Class card consumes the shared slot resolver | Should Have | Slice 5, Feature 12 slice 12.1 | ✅ Complete |
| Slice 11: Class card slot provenance + cross-writer consistency test | Nice to Have | Slice 10, Feature 12 slice 12.4, Feature 04 slice 12 | ✅ Complete |
| Slice 12: `*_mappings` round-trip — IR + HTML + RDF | Must Have | Feature 12 slice 12.2 | ✅ Complete |
| Slice 13: Hyperlink + CURIE-expand `class_uri` / `slot_uri` in HTML | Should Have | Feature 12 slice 12.2 | ✅ Complete |
| Slice 14: Abstract-class badge on class cards | Should Have | None | ✅ Complete |
| Slice 15: Hierarchy view in the Classes section | Should Have | None | ✅ Complete |
| Slice 16: External `subclass_of` grounding — IR + HTML + RDF | Must Have | Feature 12 slice 12.2 | ✅ Complete |
| Slice 17: Unify on "slot" terminology + slot-card parity | Should Have | Slice 5, ADR-006, Feature 04 | ✅ Complete |
| Slice 18: Enumerations and Types HTML card sections | Should Have | Slice 1, Feature 13, Feature 04 slice 21 | ✅ Complete |
| Slice 19: Induced per-class slot range on cards | Should Have | Feature 12 slice 12.5 | ✅ Complete |
