# Feature 37: In-Page Selector for Curated Instance Graphs

**Feature:** Let a schema page carry *several* curated instance graphs and
let the reader switch between them in place. Feature 36 made an instance
graph a first-class published artifact but embedded only the one entry
marked `exemplar`; every other `[[instances]]` entry rendered nothing. This
feature makes the Instance Graph section hold all declared curated graphs,
selectable client-side, and redefines `exemplar` as "default-selected"
rather than "the only one shown".

**User Story:** As an ontology author teaching a schema, I want a tiny
4-node preview that introduces *individual / node / edge* and a full worked
example that answers every competency question to sit in the same docs page,
switchable — so a reader compares the toy against the real thing without
navigating away.

**Related ADR:** [009 (instance-graph publishing, addressing, and
visualization)](../adr/009-instance-graph-publishing-and-addressing.md).
This feature amends decision 1: the exemplar/arbitrary split stops being
"embed one vs. nothing" and becomes **in-page selector for curated graphs
vs. sibling pages for arbitrary ones**. Arbitrary/production graphs —
addressable sub-pages and query-driven compact viz — stay deferred, and
remain the same retrieved-subgraph engine decision 3 describes.

**Approach:** Vertical Slicing with Outside-In TDD. The rendering path
first (so a page with N graphs is directly generatable and browser-testable
without the publish machinery), then publish carriage, then the docs and
ADR amendment.

**Scope boundary:** this is the *curated* job — a few small hand-picked
A-boxes whose purpose is to teach the schema. The many-and-possibly-huge
job (addressable per-dataset pages, whole-graph rendering refusal, retrieval
subgraphs) is explicitly not in scope; conflating the two is what made this
area feel stuck.

---

## Vertical Slices

### Slice 1: The page holds N curated graphs, switchable

**Status:** Not started

**Priority:** Must Have

**User Value:** `generate` can produce a schema page carrying more than one
instance graph, and a reader switches between them without leaving the page.

**Acceptance Criteria:**
- [ ] `--instances` accepts more than one dataset in a single `generate`
  invocation, each labelled: given two instance-data files, the HTML output's
  Instance Graph section offers both by name.
- [ ] Selecting a graph re-renders that dataset's viz canvas, its individual
  cards, its entity list, and its provenance line — the section describes
  exactly one dataset at a time, with no stale content from another.
- [ ] Switching is client-side: no navigation, no refetch. Every declared
  graph's payload is present in the page.
- [ ] Node/edge counts shown for a graph are that graph's own.
- [ ] With exactly one dataset supplied, the section renders as it does
  today (no selector chrome for a single graph).
- [ ] The soft "exemplars are teaching artifacts" warning applies per
  declared graph rather than to a single one, so a large graph is flagged
  whichever slot it occupies.
- [ ] Individual identity is unchanged: an individual's IRI is the same as
  the RDF/graph-JSON exports mint for the same data, per dataset.
- [ ] Browser-verified end to end: with two graphs declared, both labels
  render, the default one is shown first, and switching swaps the rendered
  node set (asserted on a node unique to each graph).

### Slice 2: `publish` carries every declared instance graph

**Status:** Not started

**Priority:** Must Have

**User Value:** A published versioned site shows all the curated graphs the
repository declares, not just one — so the wine-style "preview plus worked
example" is what a reader actually gets on the deployed site.

**Acceptance Criteria:**
- [ ] Every `[[instances]]` entry whose data file exists at a published ref
  is built and embedded for that version; a version whose ref predates a
  data file simply omits that graph (skip, don't fail), as with the single
  exemplar today.
- [ ] `exemplar = true` selects which graph is shown first; at most one
  entry may set it, and with none set the first declared entry is the
  default.
- [ ] The "declared but not published" note is gone — a declared curated
  graph is published, so there is nothing to warn about.
- [ ] A single-entry `[[instances]]` with `exemplar = true` publishes
  exactly as it does today (back-compatible).

### Slice 3: Documented role boundary

**Status:** Not started

**Priority:** Should Have

**User Value:** A reader of the design record learns which instance-graph
job the selector serves and which one is still deferred, so the next
consumer request lands in the right place.

**Acceptance Criteria:**
- [ ] ADR-009 records the amendment: curated graphs share the schema page
  via the selector; arbitrary graphs remain the deferred sibling-page and
  retrieval-subgraph path.
- [ ] `exemplar`'s meaning ("default-selected", at most one, first entry
  wins when unset) is documented where the publish config is documented.
- [ ] The multi-graph form of `--instances` appears in the CLI docs and
  README alongside the single-graph form.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: N graphs, switchable | Must Have | Feature 36 slices 2–3 | Not started |
| Slice 2: publish carries all | Must Have | Slice 1 | Not started |
| Slice 3: documented boundary | Should Have | Slices 1–2 | Not started |

---

## Definition of Done

- [ ] All Must Have slices complete with acceptance criteria checked
- [ ] `cargo fmt --check`, `cargo clippy --all-targets --all-features -D warnings`, full test suite, and `cargo doc` clean
- [ ] Mutation testing on the diff shows no missed mutants
- [ ] CHANGELOG updated
- [ ] ADR-009 amended

---

## Things to Watch

- **Stale-content bugs are the main risk.** The section shows a graph's
  cards, entity list, provenance, and counts; a selector that swaps the
  canvas but leaves any of those describing the previous dataset is the
  defect this feature most plausibly ships. Assert per-dataset on more than
  the canvas.
- **The single-graph path must not regress.** Every existing consumer
  declares zero or one instance graph; their output should be unchanged.
- **Payload size.** Every declared graph's payload ships in the page. This
  is acceptable precisely because curated graphs are small — which is why
  the per-graph size warning matters more once several are embedded.
- **Whitespace normalization in text matchers** when extending the browser
  tests, per the existing e2e caveats.
