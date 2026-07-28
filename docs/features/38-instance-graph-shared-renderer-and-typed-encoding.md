# Feature 38: Instance Graph — Shared Renderer, Typed Encoding, Consistent Badges

**Feature:** Make the rendered instance graph a first-class view rather than a
reduced one: give it the same interaction and legend the schema graph has, and
serialize the A-box with a **typed** encoding in which each schema node type
expands into its concrete instances, drawn with that type's own symbol.

**User Story:** As a reader of a schema's docs, I want the instance graph to
behave like the schema graph — fills its canvas, drags, zooms, has a legend I
can read it by — and to *look* like the schema it realizes, so that a class's
individuals are recognisably that class and a shared enum value visibly
gathers the individuals that use it.

**Related ADR:** [009 (instance-graph publishing, addressing, and
visualization)](../adr/009-instance-graph-publishing-and-addressing.md). The
encoding principle in Slice 4 is a contract for any future A-box surface and
should be recorded there as an amendment.

**Approach:** Vertical Slicing. Badges first (an independent, visible defect),
then the shared-component wiring, then the legend, then the typed encoding —
which is the substantive change and the one the legend then reflects for free.

**Scope boundary:** the **separate** A-box canvas. A *unified* canvas showing
classes and their individuals together, linked by `rdf:type`, stays deferred
(ADR-009's "one typed view"). Slices here are chosen not to foreclose it: once
individuals already carry their class's symbol, the unified view is mostly
adding class nodes, type edges, and an instance affordance.

---

## Vertical Slices

### Slice 1: Graph counts read consistently — independent

**Status:** Complete

**Priority:** Must Have

**User Value:** A reader can compare the two graphs' sizes at a glance,
and can tell which number is nodes and which is edges.

**Current state (measured 2026-07-27):** three formats for the same idea —
the schema-graph heading says `37 nodes, 60 edges` (JS-populated), its
sidebar entry says `37 / 60`, and the instance-graph heading says `28`,
which isn't a node count at all but the number of individuals. Only the
sidebar and selector buttons agree.

**Acceptance Criteria:**
- [x] Every graph section reports **nodes and edges** — sidebar entry,
  section heading, and each selector button — in one format.
- [x] The instance-graph heading reports the graph's nodes and edges, not
  a count of individuals.
- [x] The two numbers are distinguishable without prior knowledge (glyph or
  tooltip), on the schema graph as well as the instance graph.
- [x] With several curated graphs, both the heading and the sidebar entry
  describe the **selected** dataset and follow the selector — two badges for
  one graph must not disagree — while each button keeps its own dataset's
  numbers. On load, that selected dataset is the default one.
- [x] List sections (Classes, Slots, Enumerations) keep their single count —
  a list has one dimension, a graph has two.

### Slice 2: The instance graph gets the full renderer

**Status:** Complete

**Priority:** Must Have

**User Value:** The instance graph is explorable — it fills its canvas,
drags, zooms, and offers the layout picker — instead of clustering in a
corner of an empty box.

**Acceptance Criteria:**
- [x] The instance-graph canvas lays out to fill its viewport as the schema
  graph does, rather than occupying a fraction of it — the camera re-fits at
  the same settling checkpoints
  (`e2e_instance_graph_is_explorable_like_the_schema_graph` measures the
  painted extent).
- [x] Drag-to-pan, zoom, focus-on-hover (same hop depth as the schema
  graph), and the layout picker work on the instance graph; the picker's
  choice persists under its own key, since a good A-box layout can differ
  from the T-box's.
- [x] Switching datasets preserves that behaviour for the newly shown graph;
  a layout change re-creates the view the same way.
- [x] The schema graph is unchanged.
- [x] Window resizes reach the visualization (`viz.resize` + re-fit), so
  hit-testing and camera fit don't run against stale dimensions.

**What this slice deliberately does NOT do:** unify the two HTML/JS shells.
The reusable component is the `panschema-viz` `Visualization` — one
renderer, generic over the graph document — and this slice drives it fully
from the instance canvas via the same API calls the schema canvas makes.
But those calls now exist in *two* template shells (`graph_viz.html` and
`instance_graph.html`), duplicating the refit checkpoints, hover-focus
logic, badge formatting, and picker wiring. That duplication is temporary:
Slice 3 extracts the shared behaviours into one module, because its legend
AC already demands a single code path serving both canvases.

### Slice 3: One adaptive legend, and one shared graph shell

**Status:** Complete

**Priority:** Should Have

**User Value:** Each graph explains its own symbols, neither advertises a
symbol it doesn't use — and the page drives both canvases through one
shared module, so a behaviour fixed once is fixed everywhere.

**Acceptance Criteria:**
- [x] The legend enumerates only the node and edge kinds actually present in
  the graph it describes — a spec computed from the simulation (where rule
  participation is derived), with the row tables as the single source both
  the drawing and a queryable JSON summary filter through, so an assertion
  against the summary is an assertion about the drawn key.
- [x] The instance graph has a legend (toolbar toggle + panel); both graphs'
  legends come from one code path, and the key gains the two rows it never
  had — Individual nodes and assertion edges — without which an instance
  graph wasn't describable at all. The standalone full-key entry point
  remains for callers with no graph to inspect.
- [x] A schema with no enums shows no enum entry
  (`e2e_legends_adapt_to_what_each_graph_contains`; a schema whose slots are
  inline attributes likewise shows no Slot row, since no pills are drawn).
- [x] The panel sizes to its rows: the key's extent is computed from the
  same metrics the drawing walks with, and both shells size the legend
  canvas from it — a short key doesn't sit in a tall fixed box of dead
  space (`legend_extent_matches_the_row_count_arithmetic`, and the e2e
  bounds the panel's slack over the extent).
- [x] Exactness is tested both directions: a row appears if and only if the
  graph contains that kind, with the expected sets derived independently by
  walking the simulation rather than repeating the spec's logic
  (`legend_rows_are_exactly_the_kinds_present_in_the_graph`).
- [x] The behaviours Slice 2 duplicated across the two template shells —
  settle-and-refit, focus-on-hover, badge formatting, layout persistence,
  resize forwarding, legend rendering — live in one shared page module that
  both canvases call, so the shells hold only their own markup and data
  sources. A behaviour change in one graph that doesn't appear in the other
  is a bug, not a divergence.

### Slice 4: The A-box is typed — each type expands into its instances

**Status:** Complete

**Priority:** Must Have

**User Value:** The instance graph reads as the schema realized: an
individual looks like its class, and a shared enum value visibly gathers
the individuals that chose it — structure the current card literal loses.

**Encoding principle** (record as an ADR-009 amendment): *each schema-graph
node type expands, in the instance graph, into its concrete instances, drawn
with the same symbol the schema graph uses for that type.*

**Acceptance Criteria:**
- [x] An individual is drawn with its **class's** symbol and colour, labelled
  by the individual's name — not a uniform generic marker.
- [x] Each enum value *actually used* becomes one **shared** node with the
  enum's symbol and colour, labelled by the value; every individual using it
  links to that one node, so "which individuals share this value" is visible
  as structure.
- [x] Scalar literals stay attributes on the individual, not leaf nodes.
- [x] An object-property assertion is an edge labelled by its slot, styled to
  echo the schema graph's slot glyph, drawn as a plain directed arrow —
  cardinality decorations stay T-box-only, since an assertion is one concrete
  fact.
- [x] Individual identity (IRIs) is unchanged, so the docs, RDF, and
  graph-JSON exports still agree.
- [x] The legend (Slice 3) reflects the new kinds without separate authoring.
- [x] An individual is identifiable by class where it is *listed*, not only
  where it is drawn: the entity list shows labels alone today, so two
  individuals of different classes that share a display name are
  indistinguishable there. (Real case: a region and a wine both labelled
  `Bordeaux`, a grape and a wine both labelled `Cabernet Sauvignon` — both
  legitimately modelled, both ambiguous in the list.)

### Slice 5: Full shell parity — hover card and toolbar controls

**Status:** Complete

**Priority:** Should Have

**User Value:** The instance graph offers the same inspection affordances
the schema graph does: hover a node for its detail card, and toggle
labels, node/edge visibility, arrows, and focus-on-hover from a toolbar —
so a reader doesn't have to relearn the surface per graph.

**Scope notes:**
- Slice 2 deliberately covered the dogfood asks (fill, drag, zoom, layout
  picker, focus-on-hover behaviour); this slice is the remainder of the
  schema shell's chrome, routed through the shared module so each control
  lands once for both canvases.
- The **Groundings** toggle is N/A, not deferred: external grounding nodes
  are a T-box concept, and an A-box has none.
- The hover card is the piece with semantic urgency: the typed encoding's
  shared value nodes carry `usage_count` on the wire, and nothing surfaces
  it until a card does.

**Acceptance Criteria:**
- [x] Hovering an instance-graph node shows the detail card: an
  individual's types, literals, and IRI; a shared value node's enum and
  how many individuals chose it
  (`e2e_instance_graph_has_hover_card_and_toolbar_parity`).
- [x] Reset / zoom-in / zoom-out buttons, and Labels, Nodes, Edges,
  Arrows, and Focus-on-hover toggles work on the instance canvas, with the
  same persisted preferences behaviour as the schema canvas, re-applied
  whenever a dataset or layout switch re-creates the view.
- [x] The controls come from the shared shell, not a second copy — one
  wiring serving both canvases.
- [x] The schema graph is unchanged.
- [x] Camera fits actually take effect (consumer-found): the instance
  render loop advances the camera animation each frame, without which
  every `fit_to_bounds` — the settle-time fill, the reset button, resize
  re-fits — set a target the camera never reached. The e2e now demands a
  fitted *and* centered graph, proves reset recovers from a far pan, and
  was confirmed red without the fix.
- [x] The legend button reflects its pressed state like every other
  toggle (consumer-found: it only set `aria-pressed`, never the styled
  `active` class).
- [x] The `enum_value` metadata tag is spelled consistently with the
  `node_type` on the wire (the kind enum's lowercase tagging would have
  emitted `enumvalue`).

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: consistent graph counts | Must Have | — (independent) | Complete |
| Slice 2: full renderer for the A-box | Must Have | — | Complete |
| Slice 3: adaptive legend + shared shell | Should Have | Slice 2 | Complete |
| Slice 4: typed A-box encoding | Must Have | Slice 2 | Complete |
| Slice 5: full shell parity (hover card + toolbar) | Should Have | Slices 3–4 | Complete |

---

## Definition of Done

- [x] All Must Have slices complete with acceptance criteria checked
- [x] `cargo fmt --check`, `cargo clippy --all-targets --all-features -D warnings`, full test suite, and `cargo doc` clean
- [x] Mutation testing on the diff shows no missed mutants
- [x] Browser tests cover the interaction and encoding claims, not just the markup
- [x] CHANGELOG updated
- [x] ADR-009 amended with the encoding principle

---

## Things to Watch

- **Most of Slice 4 is serializer work, not renderer work.** The renderer
  already draws whatever shape the graph document carries, and the A-box
  already serializes to the same document type as the schema graph — so
  resist changing the renderer where changing the emitted node kinds is
  enough.
- **Shared enum-value nodes change node counts**, which Slice 1's badges
  report. Land Slice 1 first and the numbers stay honest as Slice 4 moves
  them.
- **A downstream consumer's preview A-box may be a subset of its worked
  example**, so datasets on one page can share record ids — the per-dataset
  anchor namespacing from feature 37 must survive any card/legend rework.
- **Camera fits are animated targets.** `fit_to_bounds` only takes effect
  when the render loop advances the camera animation each frame
  (`update_animation`); a loop that omits it turns every fit — settle
  refits, the reset button, resize — into a silent no-op, while a weak
  painted-extent assertion stays green. Assert fitted *and* centered.
- **Verify in a browser, not only in generated markup.** Every defect this
  feature responds to was visible on a rendered page and invisible in the
  HTML source.
