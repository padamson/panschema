# Datasets by Name - Implementation Plan

**Feature:** Name a dependency's published dataset in a consumer manifest,
instead of pathing into its checkout

**User Story:** As a repository that generates artifacts from a schema
package someone else publishes — a benchmark grader is the first — I want to
say *which named dataset* of that package to render, so that my manifest
keeps working when the package moves from a sibling checkout to a pinned
release I never have a path into.

**Related ADR:** [ADR-009](../adr/009-instance-graph-publishing-and-addressing.md)
decision 6 — a repository that authors data against a schema published
elsewhere documents its own A-box. This feature is the consumer half of the
same idea: reaching a dataset the *package* publishes.

**Approach:** Vertical Slicing with Outside-In TDD

---

## Implementation Strategy

A publish manifest already names its datasets: each `[[instances]]` entry
carries a `name`, the `data` file, and optionally the `schema` dependency the
dataset conforms to rather than the package's own. A consumer manifest has no
way to say any of that. It can only write a path:

```toml
[generate.wine]
ttl = "tests/fixtures/wine.ttl"
instances = ["../ontology-authoring-template/data/wine-instances.yaml"]
```

That works for a sibling checkout and breaks for anything else. With a
`github:` source the package lives in panschema's cache under a path the
consumer must not hard-code, and the consumer is left encoding a fact the
package already states — that this dataset conforms to some *other*
dependency's schema.

The resolution machinery is already in place. `source::Resolved` carries
`pkg_dir`, documented as the directory to read `panschema-publish.toml` from,
and `dataset_names`, the names that manifest lists. What is missing is the
manifest vocabulary and the lookup from a name to that entry's `data` path.

Two slices: naming a dataset of the dependency the entry is already about,
then naming one that a *different* dependency publishes against this entry's
schema. The second is what the first consumer actually needs, but the first
is where the resolution, the errors, and the tests are built.

---

## Vertical Slices

### Slice 1: A dataset of this entry's own dependency

**Status:** Complete

**User Value:** A consumer renders a dependency's published dataset by name,
with no path into the dependency, and the same manifest works whether that
dependency is a sibling checkout or a fetched release.

**Acceptance Criteria:**
- [x] `datasets = ["<name>"]` on a `[generate.<dep>]` entry renders the
      dataset that `<dep>`'s publish manifest lists under that name, from
      whichever location the dependency resolved to.
- [x] The output is byte-identical to what the equivalent `instances` path
      produces for the same dataset.
- [x] `datasets` and `instances` may both appear; datasets resolve after the
      paths, so the rendered order is declaration order within each.
- [x] A name the dependency does not publish fails naming the entry, the
      dependency, and every name it does publish.
- [x] Named datasets join the entry's declared set, so `verify` and the
      cross-graph pass cover them exactly as they cover pathed ones.
- [x] A dataset whose publish entry names a different schema is refused,
      with a remedy that works, rather than rendered against a schema it
      does not conform to.

**Notes:**
- The lookup reads the dependency's `panschema-publish.toml` from
  `Resolved.pkg_dir` and resolves the entry's `data` relative to it.
  `dataset_names` already proves the file parses there.
- Out of scope: the qualified `<dep>:<dataset>` form (slice 2), and any
  change to how datasets are published.

---

### Slice 2: A dataset another dependency publishes against this schema

**Status:** Complete

**User Value:** A consumer renders a dataset that one package publishes
against another package's schema — a benchmark written in the grader's schema
but shipped with the ontology it grades — by naming both.

**Acceptance Criteria:**
- [x] `datasets = ["<dep>:<name>"]` on a `[generate.<other>]` entry renders
      the dataset `<dep>` publishes under `<name>`, against `<other>`'s
      schema.
- [x] The qualified form is accepted only when the dataset's publish entry
      names that same schema dependency; a mismatch fails naming both.
- [x] A `<dep>` the consumer does not declare fails naming the declared
      dependencies.
- [x] The shipped skill and the manifest reference document both forms, and
      a consumer manifest in the test fixtures exercises each.

**Notes:**
- Bare `<name>` stays the dependency-of-this-entry form from slice 1, so a
  single-package consumer never writes a prefix.
- One rule covers both spellings: a dataset conforms to the schema its
  publish entry names, or to its publishing package's own when it names
  none, and it must be named under the block for that schema. Slice 1's
  self-referential case and slice 2's cross-package case fall out of it
  rather than being special-cased.
- Slice 1's refusal message pointed at `instances`, because no spelling
  reached a cross-schema dataset then. It now names the block and the
  qualified spelling, and the test asserts that spelling resolves — so the
  remedy is checked, not just worded.
- The colon is the separator because a dataset name is already a directory
  name in published output, where `/` is taken and `:` is not.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1 | Must Have | None | Complete |
| Slice 2 | Must Have | Slice 1 | Complete |

## Things to watch

- A bare name resolves only against the block's own dependency, so slice 1
  cannot reach a dataset that conforms to another package's schema — the
  qualified form in slice 2 is what makes that reachable. Until then the
  error points at `instances`, which does work, rather than at a block
  where the bare name would fail differently.
- `[check.<name>]` has no `datasets` key, so a check-only entry still
  declares its data by path. The union reads generate's names, which is
  what keeps `verify` honest for entries that generate; a check-only entry
  naming a package dataset is a separate addition.

- `fetch` resolves tags only, so a consumer cannot pin a package whose
  release predates the dataset it wants. That is a release-cadence problem
  on the publishing side, not something this feature can fix; it decides
  *when* a given consumer can adopt this, not whether the feature works.
- The publish manifest is read twice per dependency once this lands — once
  for `Resolved`, once for the dataset lookup. Worth one read if the second
  is hot, but correctness first: `Resolved` is built before the manifest's
  generate entries are walked.
- A dataset named by a consumer is data the consumer did not author. Its
  load warnings should read as the dependency's, not as a defect in the
  consumer's tree.

## Definition of Done

- [x] All acceptance criteria met
- [x] All slices Complete
- [x] All tests passing: `cargo nextest run --workspace`
- [x] Code formatted and clippy clean
- [x] README.md, CHANGELOG.md, and the shipped skill's manifest reference
      updated
