# Feature 53: Self-Describing Datasets

**Feature:** Every instance graph `generate` writes names the dataset it
holds, its version, and its schema. An entry holding several datasets can
write one file per dataset. A consumer can generate a dependency's LinkML
sources (its schema with imports merged, and its data as published) the
way it generates the dependency's Turtle.

**User Story:** As the author of a tool that reads published graphs (a
grader, a retriever), I want each graph to say which dataset it holds and
at what version, so my users pass me file paths and never type a dataset
name and version that can drift from the files they load.

**Related:** [ADR 009](../adr/009-instance-graph-publishing-and-addressing.md)
(a dataset's identity is `(dataset name, schema version)`),
[Feature 46](46-panschema-model-crate.md) (the model crate readers load
LinkML sources with), [Feature 47](47-datasets-by-name.md) (`datasets =
["<dep>:<name>"]`, the resolution this feature reuses).

**Approach:** Vertical Slicing with Outside-In TDD

---

## Why

A Turtle file states its schema (`owl:Ontology` with `owl:versionInfo`),
but nothing in it names the dataset: the manifest name a benchmark pins
(`worked-example`) appears nowhere in the file. So a grader takes the
dataset name and version as flags, and every consumer's CI types a pair
that can drift from the files it loads. One consumer's CI derives the
pair instead, by joining its two manifests in a shell script.

A wrong file path fails loudly; a wrong name or version passes silently.
Once the graph names its dataset, a reader needs only the paths, and it
can check that it was handed the dataset it expected.

The LinkML half follows from the same seam. A Rust tool that reads a
dependency through `panschema-model` needs the sources the model reads,
resolved by name and pinned by the lock, the way `ttl =` gives it the
Turtle. Today `[generate.<name>]` writes every projection of a dependency
except its LinkML.

## Design

**Existing vocabulary only.** The dataset node uses DCAT and Dublin Core
terms. panschema defines no terms of its own for this, and publishes no
index of its outputs: each file describes itself, so nothing can
disagree with it.

```turtle
[] a dcat:Dataset ;
   dcterms:identifier "worked-example" ;
   dcat:version "0.2.1" ;
   dcterms:conformsTo <https://w3id.org/wine-linkml/> .
```

The node is a blank node. A dataset has no IRI of its own (ADR-009
defers the `/data/` space one would live under), and a reader needs only
the literals.

**A declared dataset is one some package's `[[instances]]` lists.** A
file `generate` writes from is a dataset when the consumer's own package
(through a `path = "."` entry) or a dependency (through `datasets =
["<dep>:<name>"]`) declares it. Its name is that `[[instances]]` `name`.
Its version is the publishing package's `[schema] version`, the field the
pin check compares and `panschema release` bumps (not the schema file's
`version:`, which carries a `-dev` suffix between releases). It conforms
to its package's schema, or to the dependency its `[[instances]]` entry
names with `schema =`.

**One file per dataset.** An output path containing `{dataset}` writes
one file per dataset in the entry, with the dataset's name substituted.
Today a single-graph format (TTL and its siblings) refuses more than one
instance file rather than invent a dataset nobody declared; with the
placeholder each file holds exactly one declared dataset, so nothing is
invented. A path without the placeholder keeps the one-dataset rule.

**LinkML sources: a merged schema and byte-copied data.**
`linkml_schema` writes the entry's schema with its imports merged into
one file that `panschema_model::load_dataset` loads standalone. The merge
works on the YAML documents, not through the IR, so constructs the IR
does not model (class expressions, `subsets`, `comments`, …) survive
into the file. Collisions and provenance follow what `gen-linkml
--mergeimports` does, measured by a probe in `conformance/`.
`linkml_data` writes each named dataset as a byte copy of the file its
package publishes, so a consumer's committed fixtures change only when
the producer's data does.

### Configuration

```toml
[generate.inventory]
ttl = "site/artifacts/inventory-{dataset}.ttl"   # one file per dataset
instances = ["data/shared.yaml", "data/site-a.yaml"]

[generate.cqa]
ttl = "tests/fixtures/wine-benchmark.ttl"
linkml_schema = "tests/fixtures/cqa.yaml"        # imports merged
linkml_data = "tests/fixtures/{dataset}.yaml"    # byte copies
datasets = ["wine:benchmark"]
```

---

## Vertical Slices

### Slice 1: The instance graph names its dataset

**Status:** Completed

**User Value:** A graph handed over on its own says which dataset it
holds, at what version, under which schema, so a reader takes file paths
and nothing else.

**Acceptance Criteria:**
- [x] `generate` writing a declared dataset to TTL emits one
  `dcat:Dataset` node carrying the dataset's `[[instances]]` name as
  `dcterms:identifier`, the publishing package's `[schema] version` as
  `dcat:version`, and the schema's IRI as `dcterms:conformsTo`.
- [x] JSON-LD, RDF/XML, and N-Triples outputs of the same dataset carry
  the same triples.
- [x] A dataset named from a dependency (`datasets = ["wine:benchmark"]`)
  carries the dependency's version, and one whose `[[instances]]` entry
  declares `schema = "cqa"` conforms to the cqa schema's IRI.
- [x] An instance file no package declares is written as before, with no
  dataset node and no new warning.
- [x] A declared dataset listed by path under the block for a schema its
  package says it does not conform to is refused, as naming it is.
- [x] Schema-only outputs (SHACL, JSON Schema, OpenAPI, Rust, Postgres)
  are byte-identical to before.

**Notes:**
- The join from a generated file to the package and `[[instances]]`
  entry that declares it lives in the `panschema` library
  (`source::DeclaredDatasets`), not in `main.rs`.

---

### Slice 2: One file per dataset

**Status:** Not Started

**User Value:** An entry holding several datasets publishes each as its
own graph, loadable alone, without one `[schemas]` alias per dataset.

**Acceptance Criteria:**
- [ ] A `ttl`, `jsonld`, `rdfxml`, `ntriples`, or `instance-graph-json`
  path containing `{dataset}` writes one file per dataset in the entry,
  named by substituting each dataset's name.
- [ ] Each file holds only its own dataset's records and its own dataset
  node.
- [ ] `{dataset}` with an instance file no package declares is refused,
  naming the file.
- [ ] Two datasets that substitute to the same path are refused, naming
  both.
- [ ] `generate --check` reports drift in each per-dataset file.
- [ ] A path without `{dataset}` keeps the one-dataset rule and its
  error message.

---

### Slice 3: A dependency's merged schema

**Status:** Not Started

**User Value:** A tool that reads a dependency through `panschema-model`
gets its schema as one file that loads standalone, pinned by the lock.

**Acceptance Criteria:**
- [ ] A probe in `conformance/` records what `gen-linkml --mergeimports`
  does when a main file and an import define the same element, and what
  it does with each import's prefixes and provenance.
- [ ] `linkml_schema` writes the entry's schema with its local and
  cross-package imports merged, matching the probe on each recorded
  behavior; `linkml:` imports stay as imports.
- [ ] The merged file loads with `panschema_model::load_dataset` and
  reads the same classes, slots, enums, and types as the unmerged schema.
- [ ] A construct the IR does not model, present in an import, is present
  in the merged file.
- [ ] Two runs write byte-identical files, and `generate --check` reports
  drift.
- [ ] A `github:` dependency's schema comes from the locked extraction.

---

### Slice 4: A dependency's data, as published

**Status:** Not Started

**User Value:** A tool's fixtures come from a producer's published data
by name, and change only when that data does.

**Acceptance Criteria:**
- [ ] `linkml_data` writes each dataset the entry names as a byte copy of
  the file its package publishes, resolved by name the way `ttl =`
  resolves it.
- [ ] With several datasets the path must contain `{dataset}`, and is
  refused otherwise, naming the datasets.
- [ ] `generate --check` reports drift.
- [ ] The README and the shipped agent skill document the dataset node,
  `{dataset}`, `linkml_schema`, and `linkml_data`, and the skill's drift
  test covers the new keys.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: The instance graph names its dataset | Must Have | None | Completed |
| Slice 2: One file per dataset | Must Have | Slice 1 | Not Started |
| Slice 3: A dependency's merged schema | Must Have | None | Not Started |
| Slice 4: A dependency's data, as published | Must Have | Slice 2 | Not Started |

---

## Things to watch

- **Composition stays with the reader.** A dataset whose records
  reference another dataset is graded with both graphs passed. If a
  reader ever needs the graph to say what it requires, that is one more
  triple (`dcterms:requires`) on the same node, not an index file.
- **The merge is not a LinkML writer.** A writer from the IR (for OWL →
  LinkML conversion) is separate work. The merge reads and writes YAML
  documents and never passes through the IR, so it loses nothing the IR
  drops.

---

## Definition of Done

- [ ] All acceptance criteria met, every slice Completed
- [ ] All tests passing: `cargo nextest run --workspace --features panschema/dev`
- [ ] `cargo doc` builds cleanly
- [ ] `cargo fmt --check` and `cargo clippy --all-targets --all-features -- -D warnings` clean
- [ ] README.md and the agent skill updated
- [ ] CHANGELOG.md updated
