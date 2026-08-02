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

**Status:** Not started

**Priority:** Must Have

**User Value:** A domain root — nimbus's `Enterprise`, grounded in CCO
Organization — appears in its own A-box instead of vanishing, so it can be
described, linked, and used as the anchor other graphs reference.

Today the container record is dropped from every output: nimbus authored
`acme` with an id and a name and got no individual in RDF, no graph node,
and no card — six of seven records emitted.

**Acceptance Criteria:**
- [ ] When the `tree_root` class declares an `identifier` slot, its record
  emits as an individual everywhere the other records do: RDF, the instance
  graph, and the cards.
- [ ] When it does not, nothing changes — a pure container with no
  identifier stays unemitted, so a catalogue-style root does not start
  producing a spurious node.
- [ ] The root's own scalar slots remain available as dataset metadata, as
  they are today; emitting the record does not remove them from the
  metadata block.
- [ ] **Its class-ranged collection slots emit as references like any other
  slot's**, so the RDF states which records the root contains and under
  which predicate — `deployments` rather than an untyped association.
- [ ] Its scalar values and references behave exactly as any other record's.

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

**Status:** Not started

**Priority:** Must Have

**User Value:** A record can point at something in another graph. Without
this a cross-graph edge is either a dangling-reference error or a bare
string that loses its linkage — and it blocks a downstream eval framework
today.

**Acceptance Criteria:**
- [ ] A value at a class-ranged slot that is an absolute IRI, or a CURIE
  against a declared prefix, is treated as an **external reference**: it
  emits as an IRI object in RDF rather than a literal.
- [ ] Such a value is **exempt from the dangling-reference error** — it
  names no record in the file by design.
- [ ] External references are **reported in a summary** rather than passing
  silently, so an unresolvable target stays visible instead of becoming an
  unchecked edge.
- [ ] They are distinguishable in the instance graph from intra-dataset
  references, so a reader can see which edges leave the graph.
- [ ] A bare id continues to mean an intra-dataset reference and is still
  dangling-checked; this adds a case, it does not reinterpret the existing
  one.

**Notes:**
- Independent of scoping, which is why it can ship before slice 4 and
  unblock the consumer waiting on it.

---

### Slice 3: Cross-dataset collision detection

**Status:** Not started

**Priority:** Must Have

**User Value:** The silent merge becomes a loud error. Two datasets that
both define `api-gateway` are reported instead of quietly becoming one
individual.

**Acceptance Criteria:**
- [ ] `validate` accepts repeated `--data` and reports every id that mints
  to the same IRI across the given files, naming the id and each file.
- [ ] A single `--data` behaves exactly as it does now.
- [ ] `publish` performs the same check across its declared `[[instances]]`
  entries without being asked — it already knows the full set.
- [ ] Deliberately shared records — wine's preview being a subset of its
  worked example — are reportable but not automatically an error, since
  sharing is legitimate there. The check states what overlaps; policy about
  whether that is wrong belongs to the author.

**Notes:**
- Ordered before scoping deliberately: it makes the hazard observable
  first, and afterwards it is the natural regression test that scoping
  actually separated what it claimed to.

---

### Slice 4: Per-class dataset scoping

**Status:** Not started

**Priority:** Must Have

**Depends on:** Slices 1 and 3.

**User Value:** One schema serves many datasets safely — scoped facts stay
distinct per dataset while shared vocabulary stays shared.

**Acceptance Criteria:**
- [ ] An individual of a **scoped** class mints into a per-dataset
  namespace, so the same id in two datasets yields two distinct IRIs.
- [ ] An individual of a **global** class mints into the schema namespace,
  so the same id in two datasets yields **one** IRI — nimbus's `aws` and
  scimantic's `src-ghosh-2022` deliberately merge.
- [ ] Which classes are global is declared in the schema, not the manifest:
  it is a fact about the model, not about a deployment.
- [ ] **Bare ids stay forward-compatible.** The scope is applied at
  generation time and never written into the data file, so datasets
  authored before this ships need no rework. This property is explicitly
  requested by the filing repo and must not be traded away.
- [ ] Scoping is **off by default**: a schema that designates nothing
  behaves exactly as today, so wine's and scimantic's deliberately shared
  pairs are unaffected.
- [ ] With scoping on, slice 3's collision check reports no collisions for
  scoped classes across datasets, and still reports genuine ones for global
  classes.
- [ ] nimbus's cross-enterprise competency questions remain answerable
  across two estates — the concrete acceptance test for the global set.

**Notes:**
- **Open: how the dataset scope is derived.** The elegant candidate is the
  `tree_root` individual's IRI from slice 1 — the root record *is* the
  scope, and nimbus's one-file-per-enterprise shape says so directly. That
  needs no new configuration. Decide against the alternative of naming the
  scope in the manifest before building.
- **Open: how global classes are designated.** nimbus's ratio (3 of 13)
  argues for marking the exceptions rather than the rule. A class-level
  annotation keeps the fact with the model; confirm no consumer's ratio
  inverts this before committing.

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
| Slice 1: `tree_root` record emission | Must Have | — | Not started |
| Slice 2: external-IRI references | Must Have | — | Not started |
| Slice 3: cross-dataset collision detection | Must Have | — | Not started |
| Slice 4: per-class dataset scoping | Must Have | Slices 1, 3 | Not started |
| Slice 5: co-reference across schemas | Could Have | Slice 2 | Deferred |

---

## Things to Watch

- **Do not ship scoping as a per-dataset blanket.** It is the obvious
  reading of the problem and it breaks published competency questions in a
  consumer that already has them. The global/scoped split is the feature.
- **Preserve bare-id forward compatibility.** Applying the scope at
  generation time rather than storing it in data is what keeps every
  already-authored dataset working. It was requested explicitly.
- **Two repos are holding their follow-up open against this**, deliberately:
  one to verify the global/scoped distinction is honoured when scoping
  lands, one because external references gate its eval framework. Both
  expect to re-verify rather than take it on trust.
- **The CURIE-id workaround is currently undiscoverable.** A CURIE-form id
  already expands against the schema's prefixes — found only by reading the
  serializer. That belongs in the agent-facing reference now, independent
  of this feature.
