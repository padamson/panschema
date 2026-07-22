# ADR-009: Instance-Graph Publishing, Addressing, and Visualization

## Status
Proposed

## Context

ADR-008 gave instance data (the A-box) one internal model: any input format
becomes an `InstanceSet`, and consumers — the HTML instance graph, `validate`
— operate on that model. What it did not decide is how instance graphs are
**documented, addressed, published, and exported**, and that gap now blocks
real consumers: the docs render an instance graph but give it no navigation
entry, `publish` drops it from the built site entirely, and the machine
exports (`graph-json`, the RDF family) emit the T-box only.

The design question, posed by the downstream ontology-authoring work: **what
is the best implementation for documenting and visualizing a schema versus
its instance graph(s) — comprehensive yet compact — given one schema but many
(and possibly large) instance graphs?**

Two distinctions drive everything:

- **T-box vs A-box.** A *schema* is one reusable, versioned artifact whose
  graph nodes are classes. An *instance graph* is a set of **individuals**
  (each an instance of a class) plus the edges asserted between them via the
  schema's slots. It **conforms to** a `(schema, version)` pair —
  `validate` takes two arguments precisely because the instance graph varies
  while the schema is fixed. There are many instance graphs per schema.
- **Exemplar vs arbitrary.** An *exemplar* instance graph is one small,
  curated A-box published to *illustrate* a schema — a teaching artifact
  that belongs with the schema docs. *Arbitrary* instance graphs are the
  many, possibly large (10k+ node), conforming datasets a deployment loads
  and queries — data artifacts with their own lifecycle, never rendered
  whole.

Conflating these two roles as "the schema's individuals" is what made the
earlier feature sketches feel ad hoc. An instance graph is a **first-class
artifact rendered from `(schema, data)`, addressable on its own** — a schema
page may *feature one exemplar* and *link to others*, but must not bake in
one canonical A-box.

## Decisions

### 1. Addressing: instance graphs live under the schema version they conform to

An instance graph's identity is `(dataset name, schema version)`. The
published layout encodes that pin structurally:

- The **exemplar** renders as a section *embedded in the schema page* (the
  presentation feature 18 shipped, kept deliberately — the one-page docs are
  the product's signature) and gains a sidebar entry, making it addressable
  as `<version>/index.html#instance-graph`.
- **Additional** instance graphs, when a consumer ever declares more than
  one, become sibling sub-pages: `<output>/<version>/instances/<name>/`.
  Deferred until demanded — every current consumer has exactly zero or one.
- A fully **separate `/data/<name>/` space** with its own versioning axis
  is deferred, not rejected: it is the right shape when a dataset's
  lifecycle is independent of the schema's (see decision 6), and it doubles
  the versioning surface (`publish` would need a second tag-resolution
  scheme), so it waits for that demand rather than shipping speculatively.

This decision covers the **schema-repository shape**, where instance data
lives in the same repository as its schema; there, per-version A-boxes come
for free: `publish` already extracts each file at each git ref via
`git show <ref>:<path>`. A version whose ref predates the data file simply
has no instance graph — skip, don't fail. Repositories that are *only*
about instance graphs are decision 6.

### 2. Declaration: `[[instances]]` in `panschema-publish.toml`

```toml
[[instances]]
name = "catalog"                 # dataset identity (dir name for sub-pages)
data = "data/catalog-instances.yaml"
exemplar = true                  # at most one; embeds in the schema page
```

Zero-or-more entries; each is built per published version *if the file
exists at that ref*. This is the publish-side analog of `generate
--instances`, which stays as the ad-hoc/CLI form of the same input.

### 3. Visualization: the compact view of a large instance graph IS a retrieved subgraph

The T-box graph is bounded and rendered whole. The exemplar A-box is small
*by role* — it is a curated teaching artifact — so it also renders whole,
with the same force layout. That role, not a node-count threshold, is the
scale boundary: arbitrary/production instance graphs are never drawn whole,
and today they are not drawn at all (they are runtime artifacts for
oxigraph/Postgres).

When large-graph visualization is demanded, it enters as **neighborhood
extraction, not rendering effort**: entry points (search/entity-link) →
k-hop expansion along typed edges → a small `InstanceSet` → the existing
whole-graph renderer. That operation — `subgraph(entry_points, hops,
edge_filter) : InstanceSet → InstanceSet` — is *closed over the instance
model*, and it is the same operation a graphRAG retrieval loop performs
before verbalizing a subgraph. The hypothesis that the compact view and the
retrieval engine are one core is **adopted**: when either the visualization
or a downstream retrieval app needs subgraph extraction, it is built once,
on `InstanceSet`, in the panschema library — and its output serializes as
the same graph document the viz already renders. Until then, nothing is
built; the exports below are shaped so this stays possible (stable IRIs,
typed nodes/edges, versioned wire format).

A soft guard documents the role boundary: rendering an exemplar beyond a few
hundred nodes warns that exemplars are teaching artifacts and a subset (or,
later, the extraction path) is the intended tool.

### 4. Exports: one `InstanceSet`, every writer; T-box and A-box stay distinct documents

The same `InstanceSet` that feeds the HTML viz and `validate` flows through
the shared serialization layer:

- **RDF family** (`ttl`, `jsonld`, `rdfxml`, `ntriples`): each instance
  emits as a `NamedIndividual` — `rdf:type` its class URI, `rdfs:label`
  from its display name, data properties with XSD datatypes derived from
  slot ranges, object properties for id-resolved references — **using the
  same IRI minting as the HTML/graph exports**, so every artifact agrees on
  identity. Emitted into the same output as the T-box when instances are
  supplied: the resulting file is a self-contained knowledge graph (what a
  triple store loads, what the book's SPARQL runs against). `--strict`
  applies (dangling instance references fail the build).
- **Graph JSON**: stays T-box-only by default. With instances supplied, the
  instance graph is emitted as a **separate graph document** of the same
  `GraphData` shape — the A-box is its own artifact, mirroring the two
  embedded canvases in the HTML. The wire format gains a `graph_kind:
  "schema" | "instance"` discriminator and the `format_version` bumps
  (additive; consumers that ignore unknown fields keep working), so a
  consumer holding a graph file can tell which kind it has. A retrieved
  subgraph (decision 3) serializes as this same document.

Large-graph exports (streaming, pagination) are explicitly out of scope
until a consumer has a large graph; the query-scoped export falls out of
decision 3's extraction operation when it lands.

### 5. Page content: A-box section mirrors the T-box sections

The schema page's T-box sections (Schema Graph / Classes / Slots / Enums /
Types) get an A-box sibling: an **Instance Graph** sidebar entry with
node/edge count badges, holding the instance canvas and the per-individual
cards (typed, slot values shown, references as links), plus provenance (the
source data file) and conformance status (which schema version the data
validates against). The card/hover machinery is shared with the T-box
sections. Individual cards currently render only for OWL-embedded
individuals; unifying them over `InstanceSet` (so LinkML-data instances get
cards too) is part of this work.

### 6. Dataset-first repositories: the consumer manifest is the path

A repository can exist *only* to build and document instance graphs against
a schema published elsewhere — it authors data, not classes. The design
serves this shape through the machinery that already exists for consuming
remote schemas, not through a parallel system:

- The dataset repository declares the schema as a **manifest dependency**
  (`panschema.toml` `[schemas]`, `github:`/`path:` source), fetched and
  **pinned by the lockfile** — the schema-version pin that decision 1
  encodes in a schema repo's URL structure is carried here by the pin the
  repo already commits.
- `[generate.<name>]` gains an **`instances` key** (the manifest analog of
  `generate --instances`), so a manifest-driven build renders the imported
  schema's docs *featuring the local A-box* — sidebar entry, cards,
  exports, all of it — with provenance showing the data file and the
  pinned schema version it conforms to.
- `validate` needs nothing new: it already takes the fetched schema's path
  and the local data.

Publishing such a repository's docs versioned by **its own tags** — dataset
versions, not schema versions — is exactly the deferred `/data/` space from
decision 1: instance-first publishing is that space's real customer, and it
should arrive as "publish where `[[instances]]` entries are the primary
artifact and the schema is a pinned dependency rather than a local file."
Deferred until a dataset-first repo wants *versioned publishing* (the
manifest path above covers building and deploying current docs today).

## Consequences

- Three consumer shapes are served by **one model**: a schema repo
  publishing an exemplar in its docs, a deployment loading conforming data
  at runtime, and a dataset-first repo importing a published schema — the
  same data file renders as the exemplar section, exports as a loadable RDF
  A-box, and ingests through the typed Postgres/Rust path, with identity
  (IRIs) agreeing across all of them because minting is shared.
- The graph-json `format_version` bump is a wire-format change: panschema
  and panschema-viz must change together, and downstream graph-json
  consumers should key on `graph_kind`.
- `publish` grows its first non-schema input; failure semantics (file
  missing at a ref) must be "skip with a note", not error, or old tags
  become unpublishable.
- The subgraph-extraction operation, when built, lands in the panschema
  library on `InstanceSet` — not in the viz, and not in a consumer app —
  so visualization and retrieval stay one core.

Implementation is specced as
[feature 36](../features/36-instance-graph-publishing-and-exports.md),
which supersedes the earlier ad-hoc blocker sketches.
