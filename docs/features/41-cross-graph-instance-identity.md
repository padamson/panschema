# Feature 41: Cross-Graph Instance Identity

**Feature:** Make "many datasets per schema" safe and cross-graph
references expressible. Today every individual mints into the *schema's*
namespace regardless of which dataset it came from, so two datasets that
both use `id: api-gateway` silently merge into one individual, and a
reference into another graph is either a validation error or a bare string.

**User Story:** As someone running one schema over several datasets — one
per tenant, enterprise, study, or lab — I want each dataset's facts to stay
distinct while genuinely shared vocabulary stays shared, and I want a record
in one graph to be able to point at a record in another.

**Approach:** Vertical Slicing with Outside-In TDD.

---

## What three consumers reported

Filed from nimbus after an empirical trace of `instance_iri_string`;
scimantic, nimbus, and the ontology template each contributed their real
dataset shape. The design below is theirs, not invented here.

**The convergent finding — and the reason the obvious fix is wrong.**
Two consumers, in unrelated domains, independently reported that instance
data splits in two:

| | Must **not** merge across datasets | Must **merge** across datasets |
|---|---|---|
| scimantic | study-scoped acts, states, results (`interval-2026-08`) | bibliographic entities — `src-ghosh-2022` cited by two studies *is* one document; `paul-adamson` running both *is* one agent |
| nimbus | the enterprise's own estate — `Service`, `Deployment`, `Endpoint`, … (9 classes) | provider-neutral vocabulary — `ServiceType`, `Provider`, `Region` (3 classes) |

nimbus's generalisation: **instance data is reference/vocabulary
individuals (global) plus scoped facts (per-dataset).** Two independent
consumers converging on the same shape from different domains is the
strongest signal in the note.

**Blanket per-dataset namespacing would break published behaviour**, not
hypothetical behaviour. nimbus names competency questions already in ch01:
CQ2/CQ3 (which providers offer a given service type — portability) and CQ14
(which services depend on a given provider or region — outage blast radius,
*inherently* cross-enterprise). If `Provider`/`Region`/`ServiceType`
namespace apart per enterprise, none of those can be asked.

**Namespacing must also stay optional.** Wine's two curated graphs
deliberately *share* records — the preview is a strict subset of the worked
example — so single-namespace minting is correct there. scimantic's pair is
the same shape. The default must not change for them.

**Wine has the only active blocker:** its CQ&A eval framework is a second
graph under a separate schema whose records point *into* the wine A-box.
That is slice 2 below, and it gates work in another repo.

**Prior art in panschema:** the in-page selector already had to give each
dataset its own **anchor** namespace so two panels' reference links would
not cross. That is the presentation-layer half of this exact lesson;
`instance_iri_string` is the identity-layer remainder.

---

## Vertical Slices

### Slice 1: The `tree_root` record is emitted when it is a real individual

**Status:** Complete

**Priority:** Must Have

**User Value:** A domain root — nimbus's `Enterprise`, grounded in CCO
Organization — appears in its own A-box instead of vanishing, so it can be
described, linked, and used as the anchor other graphs reference.

Today the container record is dropped from every output: nimbus authored
`acme` with an id and a name and got no individual in RDF, no graph node,
and no card — six of seven records emitted.

**Acceptance Criteria:**
- [x] When the `tree_root` class declares an `identifier` slot, its record
  emits as an individual everywhere the other records do: RDF, the instance
  graph, and the cards.
- [x] When it does not, nothing changes — a pure container with no
  identifier stays unemitted, so a catalogue-style root does not start
  producing a spurious node.
- [x] The root's own scalar slots remain available as dataset metadata, as
  they are today; emitting the record does not remove them from the
  metadata block.
- [x] **Its class-ranged collection slots emit as references like any other
  slot's**, so the RDF states which records the root contains and under
  which predicate — `deployments` rather than an untyped association.
- [x] Its scalar values and references behave exactly as any other record's.

**Notes:**
- The discriminator is a signal already in the schema. A pure vessel has no
  identifier; a domain root does. No new flag, no manifest key.
- This is also the scope anchor slice 4 needs, which is why it comes first.
- **On containment edges — be truest to the data model.** A collection slot
  on the root is a *declared slot with a class range*, no different from one
  on any other class; suppressing its edges because the class happens to be
  `tree_root` would special-case the model to serve a rendering concern.
  Two weaker arguments were considered and rejected: that the IRI could
  encode containment once scoping lands (it cannot until slice 4, and
  conveying a relation through IRI structure is what triples exist to
  avoid), and that containment is file structure rather than an assertion
  (it is a declared slot, and dropping it loses the predicate name too).
- **The graph will hub, and that is the visualization's problem to solve.**
  A root with many records becomes a high-degree node. The renderer already
  owns affordances for this — focus mode with its hop depth, type filters,
  layout selection — and reducing clutter there is the right layer. Watch
  it on a large A-box and improve the view if it bites; do not distort the
  data to pre-empt it.

---

### Slice 2: External-IRI references

**Status:** Complete

**Priority:** Must Have

**User Value:** A record can point at something in another graph. Without
this a cross-graph edge is either a dangling-reference error or a bare
string that loses its linkage — and it blocks a downstream eval framework
today.

**Acceptance Criteria:**
- [x] A value at a class-ranged slot that is an absolute IRI, or a CURIE
  against a declared prefix, is treated as an **external reference**: it
  emits as an IRI object in RDF rather than a literal.
- [x] Such a value is **exempt from the dangling-reference error** — it
  names no record in the file by design.
- [x] External references are **reported in a summary** rather than passing
  silently, so an unresolvable target stays visible instead of becoming an
  unchecked edge.
- [x] They are distinguishable in the instance graph from intra-dataset
  references, so a reader can see which edges leave the graph.
- [x] A bare id continues to mean an intra-dataset reference and is still
  dangling-checked; this adds a case, it does not reinterpret the existing
  one.

**Notes:**
- Independent of scoping, which is why it can ship before slice 4 and
  unblock the consumer waiting on it.
- **An undeclared prefix is not a licence to skip checks.** `mystery:foo`
  stays an intra-dataset reference and is still dangling-checked; treating
  any colon as "outside" would let a typo silently disable the one check
  that catches it.
- **The muted node is the existing external-grounding rendering, reused.**
  A schema graph already draws what it names but does not define this way;
  a record in another dataset is the same fact at the A-box layer, so the
  legend row is retitled to cover both rather than a second notion of
  "outside" being invented.

---

### Slice 3: Cross-dataset collision detection

**Status:** Complete

**Priority:** Must Have

**User Value:** The silent merge becomes a loud error. Two datasets that
both define `api-gateway` are reported instead of quietly becoming one
individual.

**Acceptance Criteria:**
- [x] `validate` accepts repeated `--data` and reports every id that mints
  to the same IRI across the given files, naming the id and each file.
- [x] A single `--data` behaves exactly as it does now.
- [x] `publish` performs the same check across its declared `[[instances]]`
  entries without being asked — it already knows the full set.
- [x] Deliberately shared records — wine's preview being a subset of its
  worked example — are reportable but not automatically an error, since
  sharing is legitimate there. The check states what overlaps; policy about
  whether that is wrong belongs to the author.

**Notes:**
- Ordered before scoping deliberately: it makes the hazard observable
  first, and afterwards it is the natural regression test that scoping
  actually separated what it claimed to.
- **Keyed on the minted IRI, not the id.** A bare id and its CURIE form are
  the same individual; keying on the id would miss exactly the pair the
  check exists to find.
- **Repetition within one dataset is not reported here.** Identifier
  uniqueness already owns it, and reporting it twice would make one problem
  look like two.

---

### Slice 4: Per-dataset scoping

**Status:** Complete except the consumer-verified competency-question check

**Priority:** Must Have

**Depends on:** Slices 1 and 3.

**User Value:** One schema serves many datasets safely — scoped facts stay
distinct per dataset while shared vocabulary stays shared.

**Acceptance Criteria:**
- [x] Bare ids in **two different datasets** stop denoting one individual:
  acme's `api-gateway` and contoso's `api-gateway` become distinct. This is
  the whole of the remaining gap — a *shared* dataset already joins, because
  a CURIE id (`id: catalog:aws`) expands against the schema's prefixes, so
  the reference/scoped refactor needs nothing further from panschema.
- [x] A record in a dataset whose root **bears an identifier** mints under
  that root's IRI, so `acme/api-gateway` and `contoso/api-gateway` are
  distinct individuals.
- [x] A record in a dataset whose root is a **vessel** — no identifier —
  mints exactly as it does today. This is what makes scoping off by
  default.
- [x] **Two datasets sharing a root id share a scope**, so a teaching
  preview and the worked example it subsets keep denoting the same
  individuals with no opt-out needed.
- [x] Scoping applies **uniformly to every class**, with no global/scoped
  designation anywhere. Sharing is expressed by putting the shared records
  in their own dataset and naming them by CURIE, not by annotating classes.
- [x] **Bare ids stay forward-compatible.** The scope is applied at
  generation time and never written into the data file, so datasets
  authored before this ships need no rework. This property is explicitly
  requested by the filing repo and must not be traded away.
- [x] Scoping is **off by default**: a schema that designates nothing
  behaves exactly as today, so wine's and scimantic's deliberately shared
  pairs are unaffected.
- [x] With scoping on, slice 3's collision check reports no collisions for
  scoped classes across datasets, and still reports genuine ones for global
  classes.
- [ ] nimbus's cross-enterprise competency questions remain answerable
  across two estates — the concrete acceptance test for the global set.

**Notes:**
- **Settled: the scope is the `tree_root` individual's IRI.** The root
  record *is* the scope; nimbus's one-file-per-enterprise shape says so
  directly, and it needs no configuration. The manifest alternative was
  rejected — a deployment fact would then decide identity.
- **Settled: there is no global/scoped class designation.** All five
  consumers declined it. cuisineiq's finding retired it outright:
  `GroceryItemKind` holds both a shared catalogue item and one user's custom
  item, so a class-level flag *cannot express* the boundary. Dataset
  membership expresses both for free.
- **Sharing needs no scoping machinery at all.** A shared dataset's records
  are named by CURIE and already mint into the shared namespace, so this
  slice owes only the scoped half.

---

### Slice 6: Per-dataset `tree_root` selection

**Status:** Complete

**Priority:** Must Have

**User Value:** A schema can hold both a scoped root and a reference root —
`Enterprise` plus `ProviderCatalog`, `WineCatalog` plus `WineReference`,
`ProvenanceRecord` plus `Bibliography` — and each dataset is read against
the one it actually conforms to. Without this the refactor slice 4's design
depends on cannot be authored at all: panschema takes whichever root sorts
first and reads every dataset against it.

Four consumers asked for this independently, each for the same reason: the
classes their shared dataset would hold are the model's hub, so a separate
schema would duplicate or import them. One schema with two roots is the only
shape that does not distort the model.

**Acceptance Criteria:**
- [x] A schema may declare more than one `tree_root` class without becoming
  ambiguous: each dataset is read against the root its own top-level keys
  conform to, not against a fixed choice.
- [x] A single-`tree_root` schema behaves exactly as it does today, including
  a data file carrying keys the root does not declare.
- [x] When a schema has several roots, the one chosen for each dataset is
  **reported**, so an author can see which reading they got rather than
  inferring it from the output.
- [x] A file that matches **no** root, or matches two equally well, is a
  clear error naming the candidate roots — never a silent guess that yields
  an empty or half-read dataset.
- [x] The selection is made from the data as authored. Nothing is written
  into the data file and no manifest key is required, so a dataset stays
  portable and already-authored files keep working.

**Notes:**
- **Why inference rather than a declaration.** The roots consumers described
  hold disjoint collections — an estate's services versus a catalogue's
  providers — so the data says which root it is. Inferring costs no
  configuration, works identically for `--instances` and a manifest, and
  keeps the "modelling lives in the LinkML" line: which dataset is which is
  a fact about the file, not a fact about the model.
- **An explicit override stays available if inference proves insufficient.**
  Adding one later is additive; shipping a required key now could not be
  withdrawn. The error message is what makes the gap visible if it appears.
- **Silence was the actual bug.** The old code picked the first root by sort
  order and read on, so a catalogue file read against an estate root
  produced a plausible, wrong, near-empty dataset. Erroring loudly is most
  of this slice's value.
- **A vacuous pass is the residual hazard**, which is why `validate` reports
  the record count alongside the chosen root: a dataset read against the
  wrong root has no violations *and* no records, and "conforms" alone cannot
  be told apart from a real pass.
- **The shared half of the refactor already works, via CURIE ids.** A
  catalogue authored with `id: catalog:aws` emits `catalog:aws`, which an
  estate's `on_provider: catalog:aws` resolves to — verified end to end.
  Authored *bare*, the same record mints into the schema namespace and
  nothing joins, silently; that asymmetry is the thing to document rather
  than a defect to fix, since it follows LinkML's own rule that a bare
  identifier is a relative IRI against `default_prefix` while a CURIE
  expands. What slice 4 still owes is the **scoped** half.

---

### Slice 5: Co-reference across schemas

**Status:** Deferred

**Priority:** Could Have

Asserting that this graph's individual and another graph's individual denote
the same thing (`owl:sameAs` / `skos:exactMatch`). scimantic names a concrete
future pair — its A-box and scidatica's data meeting at a `dcat:Dataset`
node. Build when a real pair needs joining, not before.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: `tree_root` record emission | Must Have | — | Complete |
| Slice 2: external-IRI references | Must Have | — | Complete |
| Slice 3: cross-dataset collision detection | Must Have | — | Complete |
| Slice 4: per-dataset scoping | Must Have | Slices 1, 3, 6 | Consumer check pending |
| Slice 6: per-dataset `tree_root` selection | Must Have | — | Complete |
| Slice 5: co-reference across schemas | Could Have | Slice 2 | Deferred |

---

## Things to Watch

- **Do not ship scoping as a per-dataset blanket.** It is the obvious
  reading of the problem and it breaks published competency questions in a
  consumer that already has them. The global/scoped split is the feature.
- **Preserve bare-id forward compatibility.** Applying the scope at
  generation time rather than storing it in data is what keeps every
  already-authored dataset working. It was requested explicitly.
- **Slice 4's adoption depends on slice 6.** Scoping can ship without root
  selection, but the refactor it exists to enable cannot be authored until a
  schema can carry two roots — so shipping 4 alone would leave four repos
  unable to use it.
- **Two repos are holding their follow-up open against this**, deliberately:
  one to verify the global/scoped distinction is honoured when scoping
  lands, one because external references gate its eval framework. Both
  expect to re-verify rather than take it on trust.
- **The CURIE-id workaround is currently undiscoverable.** A CURIE-form id
  already expands against the schema's prefixes — found only by reading the
  serializer. That belongs in the agent-facing reference now, independent
  of this feature.
