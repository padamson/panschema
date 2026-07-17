# Feature 18: Exemplar individuals as a separate instance graph

**Feature:** Render the OWL individuals panschema already ingests (today only as
HTML cards) as a **separate instance graph** — a small force-directed A-box
scenario embedded in the Individuals section of the HTML docs, distinct from the
class/schema graph. A handful of worked-example `NamedIndividual`s become a
connected visual — typed, linked by their property assertions — right where the
reader is already reading the individual cards.

**User Story:** As an ontology author documenting a worked example, I want the
handful of `NamedIndividual`s in my ontology to appear as a connected instance
graph — each typed, joined by its property assertions — so a reader sees a
concrete "how it all fits together" scenario, not just a list of cards, and
without cluttering the abstract schema graph.

**Related ADR:** [005 (graph-visualization conventions)](../adr/005-graph-visualization-conventions.md).
Reuses the graph writer's JSON shape ([feature 04](04-schema-force-graph-visualization.md))
and `panschema-viz`, and the OWL individual ingestion that already populates the
`panschema:individual*` annotations and the HTML individual cards
([feature 02](02-core-ontology-documentation.md)).

**Approach:** Vertical Slicing with Outside-In TDD. The exporter emits a
self-contained *instance* graph JSON first (Rust-testable), then the HTML
renders a second viz over it in the Individuals block.

---

## Design decision: a separate instance graph, not an overlay on the schema graph

An earlier draft of this feature overlaid individuals onto the main schema graph
as a gated A-box layer. That is the wrong shape, for five reasons:

1. **Two semantic levels on one canvas.** The schema graph is the **T-box**
   (types, `subclass_of`, domain/range). Individuals are the **A-box** (things,
   `rdf:type`, instance-to-instance property assertions). Overlaying them makes
   one "node" mean either a type or a thing, and one "edge" mean subclassing,
   typing, *or* a property assertion — exactly the conflation ADR-005 works to
   avoid for edge kinds.
2. **Clutter.** The schema graph is already dense; even a curated handful of
   individuals plus their type and assertion edges muddies the "understand the
   schema" view.
3. **Contextual coherence.** Individuals already have their own HTML section
   (typed cards with property assertions). A viz *in that block* means the
   reader reads the individuals and then sees them as a graph — no context
   switch, and the graph is scoped to exactly what's documented above it.
4. **It is the knowledge graph.** For the graphRAG worked examples the
   downstream consumers are building, the A-box is what queries retrieve over. A
   dedicated instance graph shows "this is the KG your answers are grounded in"
   far better than instances sprinkled into a schema diagram.
5. **Cheap.** It reuses `panschema-viz` wholesale — the same force-graph
   component fed an *instance* graph-JSON instead of the schema graph-JSON. The
   new work is the exporter side; the renderer is largely free.

The one thing an overlay offered — **seeing an individual anchored to its
class** — is preserved: each instance node is labelled with its type (e.g.
`chateau-morgon : Wine`), and the type is a resolvable reference back to the
class card, so the class↔instance link stays legible without pulling the A-box
into the T-box.

## Why Now / Scope

panschema already *reads* OWL individuals (`owl_reader` extracts
`owl:NamedIndividual`s into `panschema:individual*` annotations) and *renders*
them as cards, but never as a graph, so a worked example reads as a list rather
than a scenario. A concrete instance graph makes an abstract ontology click in a
way the class boxes alone don't — the payoff the N&M "instances" step is built
around, and the pathfinder several downstream showcases (wine, nimbus, cuisineiq)
are waiting on for their Step 7.

This is deliberately the small, hand-curated exemplar case, **not** an A-box
inventory — the living-catalog use case stays out of panschema by design. This
feature only visualizes what panschema already ingests.

**Authoring caveat:** individuals are ingested only from **OWL/Turtle** today
(the LinkML/YAML reader ignores them). So this visualizes worked examples
authored as `NamedIndividual`s in TTL. A LinkML-native instance-data path is a
separate, larger concern — slice 4 (deferred).

**Wire-format note:** the instance graph reuses the existing `graph_writer` ↔
`panschema-viz` JSON contract (`GraphData` / `GraphNode` / `GraphEdge`) with a
new `Individual` node kind. Both sides move together, and the rendered graph
must be hover-tested — a writer/viz JSON change can pass every Rust test and CI
while the rendered graph is broken. Refresh the viz bundle
(`scripts/dev-install.sh`) and dogfood the actual graph, and drive it with a
playwright-rs e2e (assert painted pixels / DOM, not just Rust state).

---

## Vertical Slices

### Slice 1: Exporter — the instance graph JSON (walking skeleton)

**Status:** Not Started

**Priority:** Must Have

**User Value:** The generator emits a self-contained instance-graph JSON — one
node per ingested individual (typed), and an edge per object-property assertion
that links two individuals — so the viz side has a graph to render. Data
foundation for the rest.

**Acceptance Criteria:**
- [ ] `graph_writer` gains a path that builds a **separate** `GraphData` (not merged into the schema graph) from the `panschema:individual*` annotations: one node per individual (kind `Individual`), labelled by its local name, carrying its type (a reference resolvable to the class card).
- [ ] An object-property assertion whose subject and object are **both** individuals emits an edge labelled by the property between their two instance nodes.
- [ ] Datatype (literal-valued) assertions attach to the individual node as hover metadata, not as edges.
- [ ] The schema graph is unchanged — no `Individual` nodes leak into it; the instance graph is a distinct artifact.
- [ ] Output is empty/absent for a schema with no individuals (nothing to render), and byte-stable.
- [ ] Test: a fixture with two individuals linked by an object property plus a literal-valued assertion produces an instance `GraphData` of two `Individual` nodes, one labelled property edge, and the literal as node metadata (`instance_graph_emits_typed_nodes_and_assertion_edges`).

### Slice 2: HTML + viz — render the instance graph in the Individuals block

**Status:** Complete

**Priority:** Must Have

**Depends on:** Slice 1.

**User Value:** A reader scrolling to the Individuals section sees the worked
example as a live, force-directed graph beneath the cards — instances joined by
their assertions.

**Acceptance Criteria:**
- [ ] `panschema-viz` renders the `Individual` node kind with a distinct shape/style (ADR-005: reads in grayscale), and the assertion edges.
- [ ] The Individuals HTML section embeds a second canvas + the instance graph JSON and instantiates a `panschema-viz` view over it (reusing the component; chrome trimmed to what an instance graph needs — no schema-only controls).
- [ ] The section is omitted entirely when the schema has no individuals.
- [ ] e2e (playwright-rs): a fixture ontology with individuals renders an instance graph whose canvas paints the individual nodes and at least one assertion edge; hover-verified, not just Rust-asserted.

### Slice 3: Type anchoring + hover-card reuse + polish

**Status:** Deferred — revisit alongside the LinkML instance reader

**Priority:** Should Have

**Depends on:** Slices 1–2.

**Note:** Deferred in favour of the LinkML+JSON instance program (JSON-Schema
writer → LinkML instance reader → `panschema validate --data`). The instance
graph currently renders (Slice 2) with pan/zoom but no hover-card reuse; hover
reuse and type anchoring are picked up when the instance viz is pointed at the
LinkML instance source, so both land together rather than being built twice.
Two known follow-ups to fold in then: the instance viz loads its own copy of
the wasm module (share one across both viz), and hover-card reuse.

**User Value:** Each instance reads as "an X" at a glance, and hovering one shows
the same content as its card, so the instance graph and the cards can't drift.

**Acceptance Criteria:**
- [ ] An instance node's type is shown on the node (label suffix and/or hover) and resolves to the class card; optionally a faint "type anchor" to the class is available without dumping the A-box into the schema graph.
- [ ] Hovering an instance node surfaces the same content the HTML individual card shows (type, property values) — reusing the rendered card where practical (the schema graph's one-source-of-truth pattern).

### Slice 4: LinkML-native instance-data authoring path — deferred

**Status:** 📋 Deferred

**Priority:** Could Have

**User Value:** Author worked-example instances in LinkML (data records) rather
than only OWL/Turtle, so a LinkML-first schema can ship a worked example without
dropping to TTL.

**Why deferred:** This crosses from "visualize what we already ingest" into
"ingest a new instance-data source," overlapping the A-box boundary the project
deliberately keeps out of the doc generator. The OWL/TTL path (slices 1–3)
covers the worked-example need first.

**Acceptance Criteria:**
- [ ] (when undeferred) A LinkML data-instance source is ingested into the same individual representation the graph and cards consume, with tests pinning the round-trip.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: instance graph JSON exporter | Must Have | Feature 04 | Not Started |
| Slice 2: render in Individuals block | Must Have | Slice 1 | Not Started |
| Slice 3: type anchoring + hover reuse | Should Have | Slices 1–2 | Not Started |
| Slice 4: LinkML instance-data path | Could Have | Slice 1 | 📋 Deferred |

---

## Definition of Done

The feature is complete when ALL of the following are true:

- [ ] Slices 1–2 acceptance criteria met (slice 3 recommended; slice 4 deferred)
- [ ] All tests passing: `cargo nextest run`
- [ ] Rendered instance graph dogfood- and e2e-verified (writer↔viz JSON change hover-tested, not just unit-tested)
- [ ] Library documentation complete: `cargo doc`
- [ ] Code formatted + clippy clean: `cargo fmt --check` + `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] README.md + CHANGELOG.md updated
- [ ] [linkml-coverage.md](../linkml-coverage.md) updated if any individual-related coverage changes
