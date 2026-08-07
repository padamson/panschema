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

**Status:** Complete

**Priority:** Must Have

**User Value:** A retrieval/analysis app gets the A-box as a typed,
traversable graph document — the same shape the schema graph already ships
in, produced as its own explicitly named artifact.

**Acceptance Criteria:**
- [x] `GraphData` carries `graph_kind: "schema" | "instance"`; the
  `format_version` bumps additively (the field defaults to `schema` on
  read, so pre-bump documents still parse). panschema-viz mirrors the
  field; both sides change together and the rendered graph is
  browser-verified.
- [x] A new `instance-graph-json` format renders the A-box as its own
  document at exactly the path `--output` names (individuals as typed
  nodes with their literal metadata, reference edges labelled by slot) —
  one invocation, one artifact, per ADR-009's pandoc-model addressing.
  Source precedence matches the HTML section: `--instances` data wins,
  else the schema's embedded OWL individuals. The manifest carries it as
  an `instance-graph-json` key beside `graph-json`.
- [x] Node identity in the instance graph document uses the same IRI
  minting as Slice 1's RDF, so graph-JSON traversal and SPARQL agree on
  which individual is which.
- [x] `graph-json` stays the T-box document, always — `--instances` does
  not graft an A-box onto it (it warns that the flag is ignored there).

### Slice 3: Instance-graph navigation + unified cards

**Status:** Complete

**Priority:** Must Have

**User Value:** A reader finds the exemplar A-box from the sidebar like any
schema section, with a card per individual regardless of how the data was
authored.

**Acceptance Criteria:**
- [x] When the page has an instance graph, the sidebar shows an **Instance
  Graph** entry with node/edge count badges, after Schema Graph's T-box
  sections; without one, the entry is absent.
- [x] The section shows the data's provenance (source file name) alongside
  the canvas.
- [x] Per-individual cards render for LinkML-data instances, not just
  OWL-embedded individuals (one card path over `InstanceSet`): type, slot
  values, and references as links to the referenced individual's card.
- [x] Rendering an exemplar beyond a few hundred nodes warns that exemplars
  are curated teaching artifacts (the ADR-009 role boundary), without
  refusing to render.
- [x] Browser e2e: the sidebar entry navigates to the section and the cards
  render for a LinkML-data fixture.

### Slice 4: `publish` carries the exemplar

**Status:** Complete

**Priority:** Must Have

**User Value:** The published, versioned site shows each version's instance
graph — the docs a consumer actually deploys stop silently dropping the
A-box.

**Acceptance Criteria:**
- [x] `panschema-publish.toml` accepts zero-or-more `[[instances]]` entries
  (`name`, `data`, optional `exemplar` — at most one exemplar; a second is
  a validation error). Unknown keys fail loudly, matching the manifest's
  existing strictness.
- [x] `publish` builds each version with that version's own data: the data
  file is extracted at each ref like the schema is; a ref where the file
  doesn't exist publishes that version without an instance graph (a note,
  not an error).
- [x] The edge/worktree build renders the working-tree data file.
- [x] The exemplar appears embedded in the published schema page with its
  sidebar entry (Slice 3's rendering, through the publish path).

### Slice 5: Instances in the consumer manifest (dataset-first repos)

**Status:** Complete

**Priority:** Should Have

**User Value:** A repository that only authors instance data — its schema is
a published dependency, not a local file — gets the same documented,
exported instance graph from a manifest-driven build (ADR-009 decision 6).

**Acceptance Criteria:**
- [x] `[generate.<name>]` accepts an `instances` key — a **list** of LinkML
  instance-data files, the manifest analog of the repeatable
  `generate --instances`. Feature 37 made curated graphs plural, so the key is
  plural too: several render behind the in-page selector in declaration order,
  each labelled by its file stem. Paths resolve relative to the manifest, like
  every output path, and the configured exports (Slices 1–2) receive them too.
  A format that emits a single A-box rejects several rather than choosing one.
- [x] It works when the named schema is a fetched dependency (`github:` or
  `path:` source): the data validates and renders against the pinned
  schema version, and the section's provenance shows the data file
  (`manifest_instances_render_the_local_a_boxes_with_the_imported_schema`).
- [x] An `instances` path that doesn't exist fails with a diagnostic naming
  the schema and the path, matching the manifest's existing strictness
  (`manifest_instances_path_that_does_not_exist_fails`).

### Slice 6: Dataset-first versioned publishing — the `/data/` space

**Status:** Not Started

**Priority:** Should Have

**User Value:** A repo that authors instance data against someone else's
schema can publish that data as a versioned, addressable artifact of its
own. The motivating case is a domain repo holding evaluation records that
conform to a contract another repo owns: the records change on the domain
repo's schedule and are its product, so they need a history a reader can
navigate — beside, and separately addressable from, that repo's own schema
docs.

**What is versioned here is the instance graph, not the schema.** ADR-009
decision 6 settles this: publishing such a repo's docs is versioned "by its
own tags — dataset versions, not schema versions," and lands in the
deferred `/data/` space from decision 1. The consumed schema is a **pinned
dependency**, and the pin "is carried here by the pin the repo already
commits" — the lockfile. Re-publishing the dependency's schema docs per
consumer tag would duplicate what the owning repo already publishes, on an
axis that isn't the consumer's to move.

**Already done, and the reason this slice is narrow.** Slice 5 covers the
*rendering*: a manifest naming an external schema plus an `instances` list
emits a complete page, instance graph included — verified against a
path-source dependency. What is missing is that it is a **single current
build** with no `<tag>/` history and no alias.

**Acceptance Criteria:**
- [ ] A repo whose `[[instances]]` entries are the primary artifact can
  publish them versioned by its **own** git tags, with the schema resolved
  as a pinned dependency rather than a local file.
- [ ] Each published version records the schema version its data was
  rendered against, from the lockfile pin — so a reader can tell a data
  change from a contract change without diffing.
- [ ] The versioned data lands in its own space, addressable independently
  of any schema-docs tree, with a `current/` alias on the same terms the
  schema tree has — so a `[[book_link]]` entry can point at a stable path
  (feature 21 slice 5).
- [ ] A ref whose data file is absent is skipped with a note rather than
  failing the run — the rule the exemplar already follows, so adding this
  does not make old tags unpublishable.
- [ ] It is opt-in: a repo that declares no dataset-first publishing emits
  exactly what it does today.

**Notes:**
- **Slice 5's `github:`-source claim is not actually covered by a test.**
  Its AC says the dependency may be a `github:` or `path:` source, but
  `manifest_instances_render_the_local_a_boxes_with_the_imported_schema`
  writes `path = "./wine-pkg"`. Both routes share `resolve_source`, so the
  remote case is an inference rather than verified behaviour — and this is
  the slice that makes the remote case the normal one, so it is worth a
  test on the way in.
- ADR-009 decision 1 notes this space "doubles the versioning surface
  (`publish` would need a second tag-resolution scheme)". That cost is the
  substance of the slice, not an aside.
- No consumer can exercise this end to end yet — the contract in the
  motivating case is unreleased. That is the demand-driven trigger to wait
  for, not a reason to guess the shape now.

### Deferred (post-36, on demand)

- Sibling pages for additional named instance graphs
  (`<version>/instances/<name>/`) — no consumer has more than one dataset.
- Subgraph extraction (`InstanceSet → InstanceSet`) shared by large-graph
  visualization and retrieval — build when a large graph or a retrieval
  app demands it (ADR-009 decision 3).
- Streaming/paginated exports for large instance graphs.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: A-box in RDF family | Must Have | — | Complete |
| Slice 2: instance graph JSON | Must Have | Slice 1 (shared IRI minting) | Complete |
| Slice 3: nav + unified cards | Must Have | — | Complete |
| Slice 4: publish carries the exemplar | Must Have | Slice 3 | Complete |
| Slice 5: instances in the consumer manifest | Should Have | Slices 1–3 | Not Started |

## Definition of Done

- [x] Slices 1–4 complete (Slice 5 when a dataset-first consumer demands
  it): one `InstanceSet` feeds docs, validate, RDF, and graph JSON, with
  IRIs agreeing across outputs; the exemplar is navigable locally and on
  the published site.
- [x] `cargo nextest run` green; fmt/clippy/doc clean; wire-format changes
  browser-verified (panschema-viz updated in the same slice).
- [x] README.md + CHANGELOG.md updated; linkml-coverage notes the A-box
  export coverage.
