# Feature 36: Instance-Graph Publishing and Exports

**Feature:** Finish wiring the instance model (`InstanceSet`) through
navigation, publishing, and the machine exports, so an instance graph is a
first-class, addressable artifact: the exemplar A-box appears in the schema
docs' navigation, survives `publish` onto the built site, and flows into the
RDF family and graph JSON with identity shared across every output.

**User Story:** As an ontology author publishing a schema with a curated
exemplar instance graph, I want that A-box navigable in the docs, present on
the published site, and exported as RDF + graph JSON — so a reader can browse
it, a triple store can load it, and a retrieval app can traverse it, all
agreeing on which node is which.

**Related ADR:** [009 (instance-graph publishing, addressing, and
visualization)](../adr/009-instance-graph-publishing-and-addressing.md) — the
design study whose decisions this implements. Builds on
[ADR-008](../adr/008-instance-data-reader-architecture.md) (one `InstanceSet`
model) and features 18/33/34.

**Approach:** Vertical Slicing with Outside-In TDD. Exports first (the
deepest seam — a book's SPARQL litmus and a retrieval app both wait on it),
then navigation, then publish carriage.

---

## Vertical Slices

### Slice 1: A-box in the RDF family

**Status:** Complete

**Priority:** Must Have

**User Value:** `generate --format ttl --instances data.yaml` (and the other
RDF formats) emits a self-contained knowledge graph — schema plus
individuals — that a triple store loads and SPARQL queries directly.

**Acceptance Criteria:**
- [x] With an instance-data file supplied, every RDF-family output
  (`ttl`, `jsonld`, `rdfxml`, `ntriples`) contains one `owl:NamedIndividual`
  per instance: `rdf:type` its class URI, `rdfs:label` from its display
  name, data-property assertions with XSD datatypes derived from the slot's
  range, and object-property assertions for id-resolved references.
- [x] Individual IRIs match the HTML/graph exports' identity for the same
  data (shared minting: `instance_iri_string` is the one derivation; the
  graph-JSON export adopts it in Slice 2), so a node in the docs and a
  subject in the TTL are the same IRI.
- [x] The emitted graph is loadable and queryable in a real triple store:
  a SPARQL query over a loaded fixture returns an individual by type and
  follows an object-property edge to a referenced individual (oxigraph
  oracle, matching the existing RDF verification tier).
- [x] Without `--instances`, output is byte-identical to today (T-box only).
- [x] `--strict` fails the build on a dangling instance reference; the
  default warns (the feature-33 diagnostic path).

### Slice 2: Instance graph JSON export

**Status:** Not Started

**Priority:** Must Have

**User Value:** A retrieval/analysis app gets the A-box as a typed,
traversable graph document — the same shape the schema graph already ships
in, distinguishable by a `graph_kind` field.

**Acceptance Criteria:**
- [ ] `GraphData` carries `graph_kind: "schema" | "instance"`; the
  `format_version` bumps additively. Existing consumers of schema graph
  JSON keep deserializing (panschema-viz mirrors the field; both sides
  change together and the rendered graph is browser-verified).
- [ ] With an instance-data file supplied, the graph-json output path also
  emits the instance graph as its own document (individuals as typed nodes
  with their literal metadata, reference edges labelled by slot), alongside
  the schema graph document — not merged into it.
- [ ] Node identity in the instance graph document uses the same IRI
  minting as Slice 1's RDF, so graph-JSON traversal and SPARQL agree on
  which individual is which.
- [ ] Without instances, graph-json output is unchanged apart from the new
  discriminator field.

### Slice 3: Instance-graph navigation + unified cards

**Status:** Not Started

**Priority:** Must Have

**User Value:** A reader finds the exemplar A-box from the sidebar like any
schema section, with a card per individual regardless of how the data was
authored.

**Acceptance Criteria:**
- [ ] When the page has an instance graph, the sidebar shows an **Instance
  Graph** entry with node/edge count badges, after Schema Graph's T-box
  sections; without one, the entry is absent.
- [ ] The section shows the data's provenance (source file name) alongside
  the canvas.
- [ ] Per-individual cards render for LinkML-data instances, not just
  OWL-embedded individuals (one card path over `InstanceSet`): type, slot
  values, and references as links to the referenced individual's card.
- [ ] Rendering an exemplar beyond a few hundred nodes warns that exemplars
  are curated teaching artifacts (the ADR-009 role boundary), without
  refusing to render.
- [ ] Browser e2e: the sidebar entry navigates to the section and the cards
  render for a LinkML-data fixture.

### Slice 4: `publish` carries the exemplar

**Status:** Not Started

**Priority:** Must Have

**User Value:** The published, versioned site shows each version's instance
graph — the docs a consumer actually deploys stop silently dropping the
A-box.

**Acceptance Criteria:**
- [ ] `panschema-publish.toml` accepts zero-or-more `[[instances]]` entries
  (`name`, `data`, optional `exemplar` — at most one exemplar; a second is
  a validation error). Unknown keys fail loudly, matching the manifest's
  existing strictness.
- [ ] `publish` builds each version with that version's own data: the data
  file is extracted at each ref like the schema is; a ref where the file
  doesn't exist publishes that version without an instance graph (a note,
  not an error).
- [ ] The edge/worktree build renders the working-tree data file.
- [ ] The exemplar appears embedded in the published schema page with its
  sidebar entry (Slice 3's rendering, through the publish path).

### Slice 5: Instances in the consumer manifest (dataset-first repos)

**Status:** Not Started

**Priority:** Should Have

**User Value:** A repository that only authors instance data — its schema is
a published dependency, not a local file — gets the same documented,
exported instance graph from a manifest-driven build (ADR-009 decision 6).

**Acceptance Criteria:**
- [ ] `[generate.<name>]` accepts an `instances` key (path to a LinkML
  instance-data file), the manifest analog of `generate --instances`; a
  manifest-driven `generate` renders that schema's docs with the instance
  graph (Slice 3's section) and feeds the configured exports (Slices 1–2).
- [ ] It works when the named schema is a fetched dependency (`github:` or
  `path:` source): the data validates and renders against the pinned
  schema version, and the section's provenance shows the data file.
- [ ] An `instances` path that doesn't exist fails with a diagnostic naming
  the manifest entry, matching the manifest's existing strictness.

### Deferred (post-36, on demand)

- Sibling pages for additional named instance graphs
  (`<version>/instances/<name>/`) — no consumer has more than one dataset.
- Dataset-first *versioned publishing* (a `/data/` space versioned by the
  dataset repo's own tags, schema as a pinned dependency) — ADR-009
  decision 6's growth path; the manifest build above covers current-docs
  deployment today.
- Subgraph extraction (`InstanceSet → InstanceSet`) shared by large-graph
  visualization and retrieval — build when a large graph or a retrieval
  app demands it (ADR-009 decision 3).
- Streaming/paginated exports for large instance graphs.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: A-box in RDF family | Must Have | — | Complete |
| Slice 2: instance graph JSON | Must Have | Slice 1 (shared IRI minting) | Not Started |
| Slice 3: nav + unified cards | Must Have | — | Not Started |
| Slice 4: publish carries the exemplar | Must Have | Slice 3 | Not Started |
| Slice 5: instances in the consumer manifest | Should Have | Slices 1–3 | Not Started |

## Definition of Done

- [ ] Slices 1–4 complete (Slice 5 when a dataset-first consumer demands
  it): one `InstanceSet` feeds docs, validate, RDF, and graph JSON, with
  IRIs agreeing across outputs; the exemplar is navigable locally and on
  the published site.
- [ ] `cargo nextest run` green; fmt/clippy/doc clean; wire-format changes
  browser-verified (panschema-viz updated in the same slice).
- [ ] README.md + CHANGELOG.md updated; linkml-coverage notes the A-box
  export coverage.
