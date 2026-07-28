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

**Status:** Not started

**Priority:** Must Have

**User Value:** The instance graph is explorable — it fills its canvas,
drags, zooms, and offers the layout picker — instead of clustering in a
corner of an empty box.

**Acceptance Criteria:**
- [ ] The instance-graph canvas lays out to fill its viewport as the schema
  graph does, rather than occupying a fraction of it.
- [ ] Drag-to-pan, zoom, focus-on-hover, and the layout picker work on the
  instance graph.
- [ ] Switching datasets preserves that behaviour for the newly shown graph.
- [ ] The schema graph is unchanged.

### Slice 3: One adaptive legend, serving both graphs

**Status:** Not started

**Priority:** Should Have

**User Value:** Each graph explains its own symbols, and neither advertises
a symbol it doesn't use.

**Acceptance Criteria:**
- [ ] The legend enumerates only the node and edge kinds actually present in
  the graph it describes.
- [ ] The instance graph has a legend; both graphs' legends come from one
  code path.
- [ ] A schema with no enums shows no enum entry.

### Slice 4: The A-box is typed — each type expands into its instances

**Status:** Not started

**Priority:** Must Have

**User Value:** The instance graph reads as the schema realized: an
individual looks like its class, and a shared enum value visibly gathers
the individuals that chose it — structure the current card literal loses.

**Encoding principle** (record as an ADR-009 amendment): *each schema-graph
node type expands, in the instance graph, into its concrete instances, drawn
with the same symbol the schema graph uses for that type.*

**Acceptance Criteria:**
- [ ] An individual is drawn with its **class's** symbol and colour, labelled
  by the individual's name — not a uniform generic marker.
- [ ] Each enum value *actually used* becomes one **shared** node with the
  enum's symbol and colour, labelled by the value; every individual using it
  links to that one node, so "which individuals share this value" is visible
  as structure.
- [ ] Scalar literals stay attributes on the individual, not leaf nodes.
- [ ] An object-property assertion is an edge labelled by its slot, styled to
  echo the schema graph's slot glyph, drawn as a plain directed arrow —
  cardinality decorations stay T-box-only, since an assertion is one concrete
  fact.
- [ ] Individual identity (IRIs) is unchanged, so the docs, RDF, and
  graph-JSON exports still agree.
- [ ] The legend (Slice 3) reflects the new kinds without separate authoring.
- [ ] An individual is identifiable by class where it is *listed*, not only
  where it is drawn: the entity list shows labels alone today, so two
  individuals of different classes that share a display name are
  indistinguishable there. (Real case: a region and a wine both labelled
  `Bordeaux`, a grape and a wine both labelled `Cabernet Sauvignon` — both
  legitimately modelled, both ambiguous in the list.)

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: consistent graph counts | Must Have | — (independent) | Complete |
| Slice 2: full renderer for the A-box | Must Have | — | Not started |
| Slice 3: adaptive legend | Should Have | Slice 2 | Not started |
| Slice 4: typed A-box encoding | Must Have | Slice 2 | Not started |

---

## Definition of Done

- [ ] All Must Have slices complete with acceptance criteria checked
- [ ] `cargo fmt --check`, `cargo clippy --all-targets --all-features -D warnings`, full test suite, and `cargo doc` clean
- [ ] Mutation testing on the diff shows no missed mutants
- [ ] Browser tests cover the interaction and encoding claims, not just the markup
- [ ] CHANGELOG updated
- [ ] ADR-009 amended with the encoding principle

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
- **Verify in a browser, not only in generated markup.** Every defect this
  feature responds to was visible on a rendered page and invisible in the
  HTML source.
