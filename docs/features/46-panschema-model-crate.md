# panschema-model Crate - Implementation Plan

**Feature:** Extract the LinkML model into its own crate, `panschema-model`

**User Story:** As a Rust tool that reasons over schemas and instance data
— a consumer that scores answers against LinkML instance-data benchmarks
is the first — I want to read the LinkML IR (schema, instance model,
anchor expansion, minted IRIs) through a library that carries none of
panschema's format, template, or CLI dependencies, so that every tool
consumes one model and none re-derives it from a lossy projection.

**Related ADR:** [ADR-011](../adr/011-panschema-model-crate-boundary.md)
records the crate boundary, its membership rule, the measured couplings,
and the versioning model. This plan sequences the work; the ADR holds the
evidence.

**Approach:** Vertical Slicing with Outside-In TDD

---

## Implementation Strategy

Today the whole toolchain is one crate. A consumer that wants only the
model has been reading panschema's Turtle projection back through an RDF
store, and that projection drops the schema-declared semantics
(`expand_against`, `asserts_absence`, `records_version_of`) and class
`rules` the consumer's next version needs. One IR exists so that never
has to happen; the library boundary is what makes it unnecessary.

The extraction is need-driven in its *sequencing* and rule-checked in its
*membership*. ADR-011's rule decides whether a module may cross; the
first consumer's one entry point — load a LinkML YAML schema, read a
dataset with anchor expansion, get each record's minted IRI — decides
what crosses first. Modules that pass the rule but that no consumer has
needed stay behind and are listed below with the reason.

The module graph, measured in ADR-011, cuts cleanly in two places: a set
of true leaves (the schema model, its resolution rules, the YAML reader,
the reader trait), and one strongly-connected core (the instance model,
diagnostics, rules, import resolution, and element identity) whose
members reference each other and must cross together. Two code slices
follow that shape; a third gives the consumer its entry and fixes the
release path.

`panschema` re-exports every moved module at its existing path, so
library consumers and both binaries see no change; the one registry
constructor that names every writer never moves.

---

## Vertical Slices

### Slice 1: The leaves

**Status:** Not Started

**User Value:** A consumer can depend on `panschema-model` alone, parse a
LinkML YAML schema, and resolve a class's effective slots, without
compiling any RDF, template, CLI, or async library.

**Acceptance Criteria:**
- [ ] A crate depending only on `panschema-model` parses the reference
      LinkML YAML schema into `SchemaDefinition` and resolves a class's
      effective slots with the same result `panschema` gives.
- [ ] `cargo deny check bans` fails if `sophia`, `askama`, `clap`, or
      `tokio` appears anywhere under `panschema-model`.
- [ ] Every `panschema::` type, module, and function path that resolved
      before the slice resolves to the same item after it, including
      `panschema::io::FormatRegistry::with_defaults()`.
- [ ] Every unit test that moves still runs, and CI, clippy, rustdoc, and
      mutation testing run against the new crate on every push.
- [ ] `panschema-viz` declares that it is never published.

**Notes:**
- Modules: `linkml`, `linkml_resolve`, `yaml_reader`, and from `io` the
  `Reader` trait, `IoError`/`IoResult`, and a new `ReaderLookup` trait
  (one method: the reader for a path) that the import resolver will take
  in slice 2. `Writer` and `FormatRegistry` stay in `panschema`:
  `Writer`'s default method consults per-format projection policy, and
  the registry is the tool-side dispatch table. `FormatRegistry`
  implements `ReaderLookup`.
- Fixtures the moved tests read (`sample_schema.yaml`, the `imports/`
  set, and a non-YAML file for the invalid-input case) move to
  `panschema-model/tests/fixtures`; `YamlReader` gains a from-string
  entry so doctests need no filesystem.
- Files that must change for the coverage AC: `.github/workflows/test.yml`
  (every `-p panschema` step), `.cargo/mutants.toml` (`examine_globs`
  and `test_workspace = true`, since cargo-mutants runs only the
  mutated package's tests by default), `scripts/mutants.sh`, and
  `CLAUDE.md` (the workspace now has three members).
- Versioning per ADR-011 decision 5.

---

### Slice 2: The core

**Status:** Not Started

**User Value:** A consumer of `panschema-model` reads a LinkML dataset into
the instance model with anchor expansion, resolves every name against the
schema, and gets the IRI panschema mints for each record and class; a
schema's `imports:` resolve and its load diagnostics — including
defective schema-declared semantics — come back as values the consumer
decides how to report.

**Acceptance Criteria:**
- [ ] For every record and class in the reference dataset, the IRI the
      model crate mints, the IRI in panschema's Turtle A-box, the `uri`
      on the graph-JSON node, and the scope prefix in the instance model
      are one string (asserted by one test across all four).
- [ ] Every RDF and graph output is byte-identical to before the slice.
- [ ] Loading a schema through the model crate with a reader lookup
      holding only the YAML reader resolves local and builtin imports as
      before and returns the same diagnostics `panschema` prints; the
      command layer prints them, the library does not.
- [ ] The three schema-declared slot-semantics families are readable
      through the model crate.
- [ ] `cargo deny check bans` still passes; every moved test runs.

**Notes:**
- Modules: `instances`, `primitives`, `diagnostics` (whole: every function
  in it is a statement about the IR), `rules`, `import_resolve`, and the
  element-identity family from `rdf_serializers` — schema-level (ontology,
  class, slot, enum and enum-value IRIs, the class-name matcher and its
  spelling inverse, the CURIE expansion) joins `linkml_resolve` beside
  `expand_curie`; instance-level (record IRIs, the by-id index, the
  namespace) joins `instances`; `graph_writer`'s node-URI resolution joins
  the same family. The family's agreement test crosses with it. The
  RDF-local expansion wrapper returns its outcome instead of logging, and
  the serializer keeps the warning.
- `load_schema_with_deps` returns its warnings in the import report;
  `eprintln!` leaves the library.
- Tests that reach tool-side code relocate rather than move: the
  registry-based loader tests (about fifty-five) and the ones that read a
  Turtle fixture or call the verifier move to `panschema`'s tests, or
  re-fixture in YAML where the contract allows. A Turtle `imports:`
  entry resolves through the model crate only when the consumer registers
  an OWL reader; the YAML-only lookup reports it unresolved.
- About seven items widen from `pub(crate)` to `pub`.

---

### Slice 3: The consumer's entry and the release path

**Status:** Not Started

**User Value:** A tool reads a schema and its dataset through one
documented entry and gets expanded IRIs, and a release publishes the
model crate before the CLI without anyone remembering the order.

**Acceptance Criteria:**
- [ ] The model crate's documentation carries one worked example that
      loads a schema, reads a dataset, and prints each record's IRI, and
      that example runs as a doctest.
- [ ] README and the shipped skill name the crate and what it is for; the
      CHANGELOG records the extraction as one entry and names the single
      deliberate API change, if any remains.
- [ ] A release publishes `panschema-model` before `panschema` from one
      workflow step, and a dry run of that step passes on the release
      branch.

**Notes:**
- The entry composes what exists: `import_resolve::load_schema`, the
  instance model's `from_linkml_data`, and the record-IRI function.
- Release: `cargo publish --workspace --exclude panschema-viz` orders the
  crates itself; the model crate's first publish is a one-time manual
  token publish, because trusted publishing cannot mint a crate's first
  release.
- Deliberately not pulled, with the reason: `validate` qualifies under the
  rule (it is conformance of instance data against the IR) and crosses
  when a consumer wants conformance through the crate; `casing` (only the
  Rust and Postgres writers use it); `labels` (fetches over the network);
  `Writer` and the registry (tool-side dispatch and projection policy);
  the manifest, cache, lockfile, source, publish, server, HTML, and
  mdbook machinery (tool policy); the OWL reader and every writer
  (format libraries).

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1 | Must Have | None | Not Started |
| Slice 2 | Must Have | Slice 1 | Not Started |
| Slice 3 | Must Have | Slice 2 | Not Started |

## Things to watch

- `crate::` paths and rustdoc links across the moved tests: the moved
  modules keep `crate::` valid among themselves; anything reaching a
  tool-side module is a relocation, not an edit.
- Build time moves in one direction only: a tool-side edit stops
  recompiling the model; a model-side edit still rebuilds everything
  above it. The measurable win is the external consumer's build. The
  Definition of Done records `--timings` before and after so the ADR
  states what actually happened.
- Feature 08 (bootstrap the IR from the metaschema) now targets this
  crate; its writer-driven regeneration check stays in `panschema`, or a
  dev-dependency cycle pulls the format libraries back into the model
  crate's tree.

## Definition of Done

- [ ] All acceptance criteria met
- [ ] All slices Complete
- [ ] All tests passing: `cargo nextest run --workspace`
- [ ] Library documentation builds with examples: `cargo doc`
- [ ] Code formatted and clippy clean
- [ ] `cargo build --timings` before and after, one model-side and one
      tool-side edit, recorded in ADR-011
- [ ] README.md and CHANGELOG.md updated
