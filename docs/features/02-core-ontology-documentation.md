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

**Status:** In Progress

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

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: Classes | Must Have | Feature 01 | Completed |
| Slice 2: Properties | Must Have | Slice 1 | Completed |
| Slice 3: Individuals | Should Have | Slice 2 | Completed |
| Slice 4: Release | Must Have | Slice 1-3 | Completed |
| Slice 5: Class card content | Should Have (v0.3.0) | Slice 1, Feature 03 | Completed |
| Slice 6: Responsive layout + configurable graph aspect | Should Have (v0.3.0) | Slice 1, Feature 04 | In Progress |
