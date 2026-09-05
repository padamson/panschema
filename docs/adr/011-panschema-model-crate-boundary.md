# ADR-011: The `panschema-model` Crate Boundary

## Status

Accepted 2026-09-05. Implemented by [feature 46](../features/46-panschema-model-crate.md).
Amends ADR-003 and ADR-004, which assumed one crate.

## Context

ADR-003 made LinkML the internal representation and ADR-004 arranged
readers and writers around it. Both assumed a single crate, which held
while panschema's only consumers were its own binaries. The toolchain now
has a second kind of consumer: tools that reason over the model — the
first scores answers against benchmarks that are LinkML instance data —
and want the IR without the format libraries, templates, package manager,
and CLI that ship with the `panschema` crate.

Without a library boundary, such a tool reads panschema's Turtle
projection back through an RDF store. That is the failure ADR-003 exists
to prevent: a second, lossy reader. The projection drops the
schema-declared slot semantics (`expand_against`, `asserts_absence`,
`records_version_of`) and class `rules`.

### What crosses the boundary today (measured 2026-09-05)

The would-be model modules — `linkml`, `linkml_resolve`, `yaml_reader`,
`import_resolve`, `instances`, `primitives`, `rules`, `diagnostics`, and
the reader trait in `io` — total about 20k of the crate's 62k lines and
carry 368 unit tests (about 11.8k lines). Their production code reaches
the tool side in exactly these places:

- `io`'s `Writer` trait has a default method that consults per-format
  projection policy in `diagnostics`; the `Reader` trait and the error
  types reach nothing.
- `import_resolve` uses one registry method, the reader for a path.
- `instances` and `diagnostics` reference each other (expansion bindings
  one way, instance types the other), so they are one move.
- `instances`, `diagnostics`, and `validate` reach an element-identity
  family in `rdf_serializers` — about thirteen functions that mint
  ontology, class, slot, enum, enum-value, and record IRIs, match a
  class by name and enumerate its spellings, and expand CURIEs — all pure
  string logic over the schema and instance types, wrapping
  `linkml_resolve::expand_curie`. One of them needs `rules`; one logs a
  warning through `tracing`. `graph_writer` holds one more of the same
  kind.
- About seven `pub(crate)` items, including the identity matcher and the
  scalar display helper, are called from the tool side.

Two apparent couplings (`linkml→diagnostics`, `linkml_resolve→rust_writer`)
are documentation mentions only. Unit tests in the moving modules that
construct the default registry (about fifty-five), read a Turtle fixture,
or call the verifier depend on tool-side code and must relocate.

### Prior decisions this touches

ADR-007's addendum folded the `mdbook-panschema` crate back into
`panschema`, recording that a version-pinned path dependency between
workspace crates "created release lockstep friction" and that the
plugin's audience installed panschema anyway. Neither reason holds here:
the consumer is a library user that never installs the CLI, and the
friction is accepted deliberately below.

Issue #133 measured the single-crate edit loop and deferred a full
workspace split until template and wasm carve-outs proved insufficient.
This decision overrides that ordering on a different ground — a consumer
needs the model as a library — and does not claim the build-time benefit
#133 sought; see Consequences.

## Decision

1. **A second workspace crate, `panschema-model`, holds the IR and what
   is needed to read and reason over it.** `panschema` depends on it and
   re-exports every moved module at the path it had, so no consumer of
   `panschema` and neither binary changes. The registry constructor that
   names every reader and writer stays where it is.

2. **Membership is need-driven in sequence and rule-checked in
   substance.** A module crosses when a model consumer needs it, and
   only if every function in it is a statement about the IR: it depends
   on no format library (sophia, askama), no package-manager type, and
   no process-level I/O policy (stderr, the network); file reads behind
   a `Reader` are allowed. Enforcement is cargo-deny's `[bans]`: `sophia`,
   `askama`, `clap`, and `tokio` may appear only under `panschema`, and
   the check runs on every push. Modules that fail the rule stay however
   convenient moving them would be; modules that pass but that no
   consumer has needed stay too, listed in the feature spec.

3. **Element identity is model semantics.** ADR-009 fixed how records and
   classes mint IRIs; the functions that implement it, and the class-name
   matching they rest on, live in the model crate: schema-level identity
   beside `linkml_resolve::expand_curie`, instance-level identity in
   `instances`. The RDF and graph writers call them there and contain no
   IRI derivation of their own. Serialization is a projection of those
   IRIs, not their source.

4. **The reader trait crosses; the writer trait and the registry do
   not.** `Reader`, the error types, and a one-method `ReaderLookup`
   trait move, because a consumer needs the trait and the import
   resolver needs a lookup. `Writer` stays: its default method is
   per-format projection policy, and no model consumer writes.
   `FormatRegistry` stays and implements `ReaderLookup`.

5. **Versions are lockstep for the two published crates.** One version
   under `[workspace.package]`, inherited by `panschema` and
   `panschema-model`; the path dependency also states the version so
   `cargo publish` resolves it; a release runs one multi-package publish
   that orders the leaf first. `panschema-viz` keeps its own version and
   is marked never published. The model crate's first publish is a
   one-time manual token publish, since trusted publishing cannot mint a
   crate's first release. Until a release covers a consumer's needs, the
   consumer tracks git main with a `package =` dependency. The cost
   accepted: a CLI-only release also publishes a model crate that did
   not change.

## Consequences

- One model, one reader per format, no projection read back as if it
  were the source. A tool that needs a semantic the model carries gets it
  by depending on the model crate.
- Build time moves in one direction. A tool-side edit no longer
  recompiles the model; a model-side edit still rebuilds everything above
  it, so the in-workspace loop for model work does not shrink. The
  external consumer's build does. The feature records `--timings` before
  and after so this section can state numbers.
- The boundary becomes a public API. About seven items widen to `pub`
  now; every later addition to the model crate is public by construction
  and reviewed as such.
- Unit tests move with their modules; the ones that depend on tool-side
  code relocate to `panschema`'s tests. Mutation testing must run the
  workspace's tests against model mutants, or the writer and integration
  tests stop killing them.
- Load warnings become values: `load_schema_with_deps` returns them and
  the command layer prints them, which is also what makes the loader
  usable from a library.
- Two crates publish in order from one workflow step; a mistake fails at
  `cargo publish`, not silently.
- Feature 08 (bootstrap the IR from the LinkML metaschema) targets the
  model crate; its writer-driven regeneration check stays in `panschema`
  to keep the format libraries out of the model crate's tree.
