# Feature 18: Exemplar individuals in the graph — the worked-example scenario

**Feature:** Render the OWL individuals panschema already ingests (today only as
HTML cards) as nodes + property-assertion edges in the schema graph, turning a
small, curated set of worked-example instances into a visual A-box scenario
alongside the T-box.

**User Story:** As an ontology author documenting a worked example, I want the
handful of `NamedIndividual`s in my ontology to appear as graph nodes — typed,
linked to their class, connected by their property assertions — so a reader sees
a concrete "how it all fits together" scenario, not just the abstract schema.

**Related ADR (if applicable):** None — extends the graph writer
([feature 04](04-schema-force-graph-visualization.md)), which is T-box-only
today (`NodeType::Class | Slot | Enum | Type`, no `Individual` variant), and
reuses the OWL individual ingestion that already populates the
`panschema:individuals` annotations and the HTML individual cards
([feature 02](02-core-ontology-documentation.md)).

**Approach:** Vertical Slicing with Outside-In TDD.

---

## Why Now / Scope

panschema already *reads* OWL individuals (`owl_reader` extracts
`owl:NamedIndividual`s into `panschema:individuals` annotations) and *renders*
them as cards, but the graph never draws them, so a worked example reads as a
list of cards rather than a scenario. A single concrete instance graph makes an
abstract ontology click in a way the class boxes alone don't — the payoff the
N&M "instances" step is built around.

This is deliberately the small, hand-curated exemplar case (individuals baked
into the ontology to illustrate it), not an A-box inventory. The living-catalog
use case stays out of panschema by design; the consuming application owns that.
This feature only visualizes what panschema already ingests — it adds no new
inventory capability.

**Authoring caveat:** individuals are ingested only from **OWL/Turtle** today
(the LinkML/YAML reader ignores them). So this feature visualizes worked
examples authored as `NamedIndividual`s in TTL. A LinkML-native instance-data
authoring path is a separate, larger concern — see slice 4 (deferred).

**Wire-format note:** a new `Individual` node kind changes the
`graph_writer` (Rust) ↔ `panschema-viz` (wasm) JSON contract. Both sides must
move together, and the rendered graph must be hover-tested — a writer/viz JSON
change can pass every Rust test and CI while the rendered graph is broken.
Refresh the viz bundle with `scripts/dev-install.sh` and dogfood the actual
graph, don't trust the unit tests alone.

---

## Vertical Slices

### Slice 1: Individual nodes + type edges (walking skeleton)

**Status:** Not Started

**Priority:** Should Have

**User Value:** A schema with `NamedIndividual`s shows one node per individual,
typed and linked by an `instance_of` edge to its class node — the first visible
A-box layer on the graph.

**Acceptance Criteria:**
- [ ] `graph_writer` gains an `Individual` node kind, emitting one node per ingested individual (read from the `panschema:individuals` annotations), labelled by its local name / IRI.
- [ ] Each individual node has an `instance_of` edge to the class node named by its `rdf:type` (when that class is in the schema).
- [ ] The instance layer is gated so the default graph stays T-box-only — it does not appear unless explicitly enabled (a viz toggle / graph aspect), keeping the abstract view uncluttered.
- [ ] `panschema-viz` renders the new node kind (visually distinct from class/slot/enum/type) and the `instance_of` edge; the writer↔viz JSON contract carries the new kind.
- [ ] Test: a schema with one `NamedIndividual` produces a graph node of kind `Individual` with an `instance_of` edge to its class (`graph_emits_individual_node_with_instance_of_edge`). Rendered-graph hover-tested per the wire-format note.

**Notes:**
- No new ingestion — strictly visualize the already-extracted individuals.

---

### Slice 2: Property-assertion edges between individuals

**Status:** Not Started

**Priority:** Should Have

**User Value:** Object-property assertions between two individuals draw as edges,
so the worked example reads as a connected scenario (`prod-env` —hosts→
`eks-cluster` —runs→ `svc-a`), not isolated dots.

**Acceptance Criteria:**
- [ ] An object-property assertion whose subject and object are both individuals emits an edge labelled by the property between their nodes.
- [ ] Datatype-property values (literal-valued assertions) attach to the individual node as hover metadata rather than as edges.
- [ ] Test: two individuals linked by an object property produce a labelled edge; a literal-valued assertion produces node metadata, not an edge.

---

### Slice 3: Visual distinction + hover card reuse

**Status:** Not Started

**Priority:** Could Have

**User Value:** Individuals are visually unmistakable (distinct node shape/style)
and hovering one reuses the existing individual-card content, so the instance
layer is legible at a glance and toggleable.

**Acceptance Criteria:**
- [ ] Individual nodes use a distinct shape/style from T-box nodes; the instance-layer toggle is exposed in the viz alongside the existing layout/mode pickers.
- [ ] Hovering an individual node surfaces the same content the HTML individual card shows (type, property values).

---

### Slice 4: LinkML-native instance-data authoring path — deferred

**Status:** 📋 Deferred

**Priority:** Could Have

**User Value:** Author worked-example instances in LinkML (data records) rather
than only OWL/Turtle, so a LinkML-first schema can ship a worked example without
dropping to TTL.

**Why deferred:** This crosses from "visualize what we already ingest" into
"ingest a new instance-data source," which overlaps the A-box boundary the
project deliberately keeps out of the doc generator. The OWL/TTL authoring path
(slices 1–3) covers the worked-example need first; the LinkML data-reader is its
own feature, picked up only if a consumer needs LinkML-native exemplars.

**Acceptance Criteria:**
- [ ] (when undeferred) A LinkML data-instance source is ingested into the same individual representation the graph and cards consume, with tests pinning the round-trip.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: Individual nodes + type edges | Should Have | Feature 04 | Not Started |
| Slice 2: Property-assertion edges | Should Have | Slice 1 | Not Started |
| Slice 3: Visual distinction + hover | Could Have | Slice 1 | Not Started |
| Slice 4: LinkML instance-data path | Could Have | Slice 1 | 📋 Deferred |

---

## Definition of Done

The feature is complete when ALL of the following are true:

- [ ] Slices 1–2 acceptance criteria met (slice 3 optional; slice 4 deferred)
- [ ] All tests passing: `cargo nextest run`
- [ ] Rendered graph dogfood-verified (writer↔viz JSON change hover-tested, not just unit-tested)
- [ ] Library documentation complete: `cargo doc`
- [ ] Code formatted + clippy clean: `cargo fmt --check` + `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] README.md updated
- [ ] CHANGELOG.md updated
- [ ] [linkml-coverage.md](../linkml-coverage.md) updated if any individual-related metaslot coverage changes
