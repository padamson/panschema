---
name: panschema-dev
description: How panschema itself is built — the design principle (LinkML decides what a construct means, measured by a probe; the target decides how it is spelled), what each writer has decided for inheritance, abstract and mixin classes, class ranges and unprojectable constructs, the checklist for carrying a LinkML construct from the model through every writer, the test oracles, the local gates and their sandbox traps, and the viz, release and supply-chain mechanics. Use when changing panschema's model, a reader or writer, the viz, or the release and dependency setup; when asked "what is idiomatic here?", "how should this writer handle X?", "is this construct supported?", or "add support for <metaslot>"; and before agreeing to model a construct someone has asked for. Not for using panschema to build schemas, which is the shipped `panschema-usage` skill.
metadata:
  internal: true
---

# Developing panschema

panschema reads a schema into the LinkML model (`panschema-model`) and
projects it through one writer per format. Every decision about a
construct is made twice: once for what it means, once for how each target
spells it. This skill holds the rule for making those decisions and the
decisions already made, so the next construct is handled the way the last
one was.

## The design principle

Pick the option most idiomatic for both LinkML and the format being
written.

1. **LinkML decides what a construct means.** Read and project it the way
   LinkML's reference implementation does. Settle a question by measuring
   that implementation with a probe in `conformance/`, not by arguing from
   the metamodel's prose, which is often looser than the code.
2. **The target decides how the meaning is spelled.** Generated Rust is
   what a Rust author would write by hand for that meaning, generated DDL
   what a Postgres author would, and so on. LinkML's own generators are the
   first place to look, since they have made most of these calls, but they
   are evidence, not the rule: where `gen-sqlddl` creates a table for an
   abstract class, panschema does not, because no Postgres author would.
3. **Size is not the tiebreaker.** A smaller diff is no reason to ship a
   less idiomatic shape, and neither is avoiding a breaking change. When
   the idiomatic shape is bigger, slice it.

When the two halves pull apart (LinkML accepts something a target cannot
hold), the target's projection says so through a diagnostic, never by
emitting something that parses and is wrong.

## Measuring LinkML: the conformance probes

`conformance/` is a uv project pinning `linkml` and `linkml-runtime`. It is
a development tool: no test depends on it and CI does not run it.

    uv run --project conformance conformance/probe_<topic>.py

- One probe per question, small and self-explaining, with the panschema
  behavior it backs named in its docstring.
- A probe that runs `gen-owl`, `gen-rdf`, or anything that imports
  `linkml:types` fetches from `w3id.org`; allow that host.
- Record each probe in `conformance/README.md` (what it runs, what it
  found, which panschema behavior it is evidence for), and cite the probe
  from the coverage row it backs.
- Ask the validator too: `linkml-validate -s schema.yaml -C <Class>
  data.yaml` says what LinkML accepts, which is what `verify --data` should
  accept.

## What each writer has decided

These are the standing decisions. A new construct should fit them; a case
that cannot is a design question, not a local fix.

**All writers** read a class's slots through the model's resolver
(`linkml_resolve::resolve_effective_slots`), which flattens `is_a`
parents, mixins, `attributes`, and `slot_usage` in LinkML's
`class_ancestors` order (`conformance/probe_mixin_merge.py`). No writer
walks inheritance on its own.

**A class declared `abstract` or `mixin: true`** holds no records of its
own (`ClassDefinition::is_abstract_or_mixin`). LinkML enforces neither
flag (`conformance/probe_mixin_flag.py`), so the validator accepts a record
typed as either.

| Target | Inheritance | Abstract / mixin | Class-ranged slot | What it cannot project |
|---|---|---|---|---|
| Postgres | Effective slots flattened into each table; a class using `is_a` is skipped | No table (`gets_table`) | FK to the target's primary key; a multivalued one is a linking table `<owner>_<slot>` | Skipped with a warning from `skipped_classes`, `skipped_constraints`, `skipped_rules`; `--strict` does not refuse these |
| Rust | A class inherited from, or declared abstract or mixin, is a marker trait; a leaf is a struct implementing its ancestors' traits | Always a trait, doc-commented as such | The target's `<Class>Id` newtype when it has an identifier and is not inlined; a trait's range is `<Name>Kind`, an untagged closed enum of concrete descendants (empty while there are none) | A `// NOTE:` or `// WARNING:` comment at the site |
| SHACL | One open `sh:NodeShape` per class with `sh:targetClass`, a property shape per effective slot | Shape kept (as `gen-shacl` does) | `sh:class` | Rules with no conditional form, `unique_keys`, slot-level `is_a`: `projection_gaps` |
| JSON Schema | One closed `object` per class under `$defs`, slots flattened (`allOf` with `additionalProperties: false` rejects valid data) | Definition kept (as `gen-json-schema` does) | Shaped by its inlining (`linkml_resolve::holding`): an id, a list of `$ref`s, or an id-keyed object | Emitted permissively (`true`) rather than rejecting valid data |
| OWL / RDF | `rdfs:subClassOf` per `is_a` parent and per mixin | No axiom for either flag | `rdfs:range` the class IRI | Dropped metaslots listed in the coverage doc |
| HTML / graph | "Subclass of", "Mixes in", and edges | Card badge; the graph dims abstract nodes only | Link to the target's card | n/a |

Every writer reports what it drops through `Writer::projection_gaps`
(`panschema/src/io.rs`), which the CLI prints. Keep a writer's typed
diagnostic functions public and make `projection_gaps` render them, so the
warning and the data never drift.

## Carrying a LinkML construct through

Work test-first, in this order.

1. **Probe it.** What do LinkML's loader, generators, and validator do
   with it? Write the probe before deciding anything.
2. **Model it** in `panschema-model/src/linkml.rs`: a field with
   `#[serde(default, skip_serializing_if = ...)]`, added to the type's
   `new()`. An unmodeled key lands in the type's `unmodeled` map and draws
   the "parsed but not modeled" warning (`diagnostics::unmodeled_class_constructs`,
   ignore-list empty), which `--strict` refuses; modeling the field is what
   stops the warning.
3. **Name the concept once.** If two writers need the same judgment about
   the construct, put a predicate on the model type rather than repeating
   the field test in each writer.
4. **Project it in every writer**, by the table above, or report it in
   that writer's `projection_gaps`. A silent drop is the bug the coverage
   doc exists to prevent.
5. **Test each decision where it is made**, then against an oracle the
   code did not produce (below).
6. **Document it:** a row in `docs/linkml-coverage.md` (one column per
   writer, the probe cited), the feature spec in `docs/features/`, the
   CHANGELOG under Added, Changed, or Fixed with breaking changes marked,
   and `skills/panschema-usage/` if a schema author needs to know.

Fixture names are generic (`Stamped`, `Parcel`, `Grouping`). Never copy
class or slot names from a downstream schema into a fixture, a test, or a
doc; describe the construct with names of your own.

## Tests and oracles

- **Unit tests live in the source file** (`#[cfg(test)] mod tests`). The
  mutation gate runs `cargo mutants` over library code only, so a contract
  tested only from `tests/` kills no mutants.
- **External oracles**, one per writer family:
  - Rust: `syn` parses every render (the `ast` helpers in
    `rust_writer.rs`), and `tests/integration/rust_writer.rs` compiles
    `tests/fixtures/codegen.yaml` in a scratch crate and round-trips data
    through it. Every codegen construct must appear in that fixture.
  - Postgres: `pg_query` parses every render (`assert_valid_postgres_sql`);
    an ignored test applies the DDL to a real Postgres in CI.
  - SHACL and RDF: an `oxigraph` store answers SPARQL `ASK` queries.
  - HTML: `html5ever`, and the e2e tier drives a real browser
    (`playwright-rs`).
- **Fixtures vendored, never fetched.** `tests/fixtures/dogfood/` holds
  frozen copies of public consumer schemas
  (`scripts/vendor-dogfood-schemas.sh`); no test reaches another repo.

## Gates

The pre-commit hook runs fmt, clippy `-D warnings`, the full nextest
suite, the skill-doc tests, the skill version guard, and, when the
lockfile or supply-chain files change, `cargo deny`, `cargo vet`, and
`cargo lockpatrol`. Before committing, run only the tests you touched:

    cargo nextest run --workspace --features panschema/dev -E 'test(<name>)'

Always pass `--workspace --features panschema/dev` so every invocation
shares one build cache.

Traps in an agent sandbox:

- **Run `cargo nextest` and `git commit` as bare commands.** An env-var
  prefix, a pipe, a `&&` chain, or a loop takes them out of the sandbox
  exemption, and every e2e test then fails on Chromium's Mach port
  (`bootstrap_check_in ... Permission denied`). That failure is the
  sandbox, not the change.
- **Mutation testing:** `./scripts/mutants.sh` runs `cargo mutants
  --in-diff` over `HEAD~1..HEAD`. Commit, run it, add a test that kills any
  survivor, and amend before pushing; CI runs the same diff job.
- **A Gatekeeper stall** (nextest at 0% CPU on fresh test binaries) is
  macOS scanning them; wait it out.
- **`cargo doc` has pre-existing warnings.** Compare the count before
  and after a change rather than expecting zero.

## The viz

`panschema-viz` is a wasm crate embedded in the HTML output at build time.

- After changing it, run `./scripts/dev-install.sh`, which rebuilds the
  bundle with `wasm-pack` and installs `panschema`. `build.rs` only
  bootstraps a missing bundle and does not rebuild a stale one, and the
  `target/debug` binary and the installed one each embed the bundle they
  were compiled with.
- A gitignored `panschema/viz-bundle/` wins over `panschema-viz/pkg/`;
  delete it after a viz change, or local e2e tests the old wasm.
- The graph JSON (`graph_writer.rs` ↔ the viz's `KindMetadata` and
  `SlotSummary`) is a wire format. Change both sides together and check
  the rendered graph with the e2e suite; Rust tests alone pass while the
  graph is broken.
- Anything that moves node positions (layout, fit, packing) needs the
  whole e2e suite run locally with `--no-fail-fast`.

## Releases

A release is a version bump on `main` followed by a tag push.

1. Bump `[workspace.package].version` in `Cargo.toml` and the
   `panschema-model` dependency's `version` in `panschema/Cargo.toml`,
   refresh `Cargo.lock`, and update the install tag in `README.md`.
2. Date the CHANGELOG's `[Unreleased]` section and add its compare link,
   mark the ROADMAP rows shipped, and update the status line in
   `CLAUDE.md`. `6e032c7` (v0.6.0) is a worked example.
3. Once that commit's CI is green, push the tag `vX.Y.Z`. `release.yml`
   checks the tag against the crate versions, runs `cargo semver-checks`,
   builds the binaries, publishes both crates, and creates the GitHub
   release from the CHANGELOG section.

The shipped skill versions on its own cadence: bump `metadata.version` in
`skills/panschema-usage/SKILL.md` whenever its content changes (the hook
and CI enforce it).

## Supply chain

`cargo vet` gates every dependency against `supply-chain/`. When a bump
fails it, the options are a delta audit (`cargo vet diff`, then
`certify`), refreshing imported audits (`cargo vet` without `--locked`),
a trust scoped to one crate and publisher, or waiting for an imported
audit. A failure on a Dependabot PR is fixed on `main`, since the bot's
next force-push wipes anything on its branch; then the PR is rebased.
`cargo vet prune` before committing keeps the exemptions minimal.
