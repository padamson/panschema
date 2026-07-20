# Feature 35: External `subclass_of` groundings in the schema graph

**Feature:** Draw each class's external `subclass_of` grounding — a `subclass_of`
naming an upstream ontology category by IRI/CURIE (e.g. `Service →
cco:ont00000995`) — as an edge to an **external category node** in the schema
graph, labelled by the category's cached upstream `rdfs:label`. Today the graph
shows only internal `is_a`; an ontology's entire external-grounding story is
invisible, so a class's real parent doesn't appear.

**User Story:** As someone authoring a LinkML ontology that grounds its classes
in an upper ontology (BFO/CCO/…), I want those groundings drawn in the schema
graph — as legible, labelled external nodes — so a reader sees what each class
*is*, and the graph works as the "island detector" it's meant to be, instead of
every class looking parentless.

**Related ADR:** [005 (graph visualization conventions)](../adr/005-graph-visualization-conventions.md).
Consumes the modeled `ClassDefinition.subclass_of` (rendered in HTML/OWL today,
[linkml-coverage.md](../linkml-coverage.md) "graph ignores") and the cached
upstream label store ([`labels.rs`], the same mechanism that labels external
slot ranges).

**Approach:** Vertical Slicing with Outside-In TDD. Because the graph JSON is a
wire format shared with the `panschema-viz` wasm crate, a new node type must be
mirrored on both sides in the same slice, and the rendered graph is verified by
a browser e2e (a green Rust suite alone can't prove the viz renders it).

---

## Design decisions

- **The `subclass_of` field is the hook.** panschema already models
  `ClassDefinition.subclass_of: Option<String>` (distinct from `is_a`, the
  internal parent). nimbus grounds each class in exactly one external category
  via `subclass_of`, so this is the field to draw — no new IR.
- **One external node per distinct grounding IRI, shared.** Two classes grounded
  in the same category share one external node (so the graph reads as a small
  set of upstream anchors, not a copy per class). Node id is the resolved IRI.
- **Label from the cached upstream store, CURIE fallback.** An external
  category's node label is its cached `rdfs:label` (`LabelStore::lookup(iri)`),
  the same store that labels external slot ranges; when the label isn't cached,
  the CURIE/local-name is the fallback. The store is threaded into
  `schema_to_graph`; the standalone `graph-json` output (no store) degrades to
  the CURIE label.
- **Visually secondary.** External nodes are muted/dashed — clearly "outside
  this schema, an upstream category" — and never abstract/interactive-heavy.
  The edge is a `subclass_of` (rendered like `is_a`, since it *is* subclassing).
- **Hover surfaces the IRI + definition.** The external node's hover card shows
  the full IRI and the cached definition (parallel to the external-slot-range
  treatment). Click-to-resolve-the-IRI is a welcome bonus, not required.

---

## Vertical Slices

### Slice 1: External nodes + `subclass_of` edges (writer + viz mirror)

**Status:** Complete

**Priority:** Must Have

**User Value:** A class grounded via `subclass_of` shows an edge to a labelled
external category node in the rendered schema graph.

**Acceptance Criteria:**
- [x] `graph_writer` emits, for each class with a `subclass_of`, one `External` node (id = resolved IRI, label = cached `rdfs:label` or CURIE fallback, `uri` = the IRI) and a `subclass_of` edge from the class to it; distinct classes grounded in the same IRI share one node. The label store is threaded into `schema_to_graph` (the HTML path passes its populated store; the standalone `graph-json` writer and unit tests pass `None` → CURIE label).
- [x] `panschema-viz` mirrors the new `NodeType::External` (wire-format parity) and renders it muted/dashed, distinct from the schema's own class nodes; the wasm bundle is refreshed.
- [x] A "Groundings" graph control toggles external-node visibility (persisted), revealed only when the graph actually has external nodes; hiding them drops the nodes, their edges, and their labels (the type filter now suppresses hidden-node labels, which the label pass previously ignored).
- [x] Tests: a unit test on the graph JSON (external node + shared-node dedup + edge), an e2e that paints an external node and its edge from a fixture with a `subclass_of` grounding, and an e2e that the toggle hides them.

### Slice 2: External node hover + legend

**Status:** Not Started

**Priority:** Should Have

**Depends on:** Slice 1.

**Acceptance Criteria:**
- [ ] Hovering an external node shows its IRI and cached definition (from the store's `TermInfo`), parallel to the external-slot-range hover.
- [ ] The graph legend documents the external-category node (and its edge), so the muted/dashed treatment is explained.
- [ ] e2e verifies the hover content and the legend entry.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: external nodes + edges (writer + viz) | Must Have | — | Complete |
| Slice 2: hover + legend | Should Have | Slice 1 | Not Started |

## Definition of Done

- [ ] Slices 1–2 met; a `subclass_of`-grounded class shows a labelled, muted
  external category node and edge in the rendered graph, hover-documented.
- [ ] `cargo nextest run` green; `cargo fmt --check`; `cargo clippy
  --all-targets --all-features -- -D warnings`; `cargo doc`; the graph e2e
  paints the external node (browser-verified, not just unit-green).
- [ ] README.md + CHANGELOG.md updated; [linkml-coverage.md](../linkml-coverage.md)
  flips `subclass_of` (external) from "graph ignores" to drawn.
