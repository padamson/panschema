# CLI surface

`panschema` with no subcommand prints help and exits 2. There is no
bare no-subcommand form.

## Exit status

| Code | Meaning |
|---|---|
| 0 | The command did what it was asked, and any check it ran found nothing |
| 1 | A check ran to the end and found something: `generate --check` or `fetch --check` drift, `verify --schema`/`--data` violations, or what `--strict` refuses on `generate`, bare `verify`, or `verify --entry` |
| 2 | The command could not run: a bad flag, a file that does not parse (the manifest, the lockfile, a schema, or a data file), a dependency that does not resolve, a missing file, or a refusal such as `release` on a dirty tree |

The split is between a finding and an error, not between a transient
failure and a permanent one: a 2 from a network failure may pass on a
rerun, and a 2 from a typo in `panschema.toml` will not. A panic exits 101,
Rust's default, and is a bug in panschema.

| Subcommand | What it does |
|---|---|
| `generate` | Render a schema to an output format. With no `--schema`, discovers `panschema.toml` and generates every manifested schema |
| `verify` | With `--schema`/`--data`: check a LinkML **instance-data** file against a schema, listing every violation and exiting 1 if there are any. With **no flags**: read `panschema.toml` and check everything it declares — conformance, cross-graph resolution, stated absences — writing nothing; findings warn, `--strict` fails on them. With `--entry <name>`/`--data`: the same checks for that one manifest entry, reading the named files (relative to the working directory) in place of its datasets, against its `resolve_against` siblings as declared — so a fixture that must fail sits beside one that must pass, each run holding only its own findings: the schema's own diagnostics print without failing it (bare `verify --strict` refuses those), and with no `resolve_against` sibling the claims a file states are findings, not a pass. Pass `--strict`, or findings only warn. *Not* schema-vs-metaschema verification |
| `migrate` | Write the schema's Postgres DDL as a versioned migration file. Writes files only — it never connects to a database |
| `publish` | Build versioned HTML docs per git ref, per `[publishing]` in `panschema-publish.toml` |
| `serve` | Hot-reload dev server for HTML output |
| `init` | Scaffold a `panschema-publish.toml` |
| `add` | Add a schema dependency to `panschema.toml` and fetch it |
| `fetch` | Resolve every dependency; lock the pins (`github:` sources) in `panschema.lock` |
| `fetch --check` | Verify every pin against the lockfile and exit 1 on drift; writes nothing |
| `release` | Bump the schema version in `panschema-publish.toml`, optionally commit/tag/push |
| `completions` | Emit a shell completion script |
| `styleguide` | Component preview page (requires the `dev` feature) |

## Flags worth knowing

- `--instances <PATH>` — repeatable. Several are only meaningful for `html`;
  the single-A-box formats reject more than one.
- `--data <PATH>` (`verify`) — also repeatable. Each file is verified on
  its own, then the set is checked for ids that mint to the same IRI across
  files. That overlap is **reported, not an error** — a preview that is a
  subset of a worked example shares records on purpose.
- `--message-format json` (`verify`) — findings as JSON Lines on stdout, each
  with a stable `kind`; see [below](#verify---message-format-json). The
  default, `human`, writes findings to stderr and the per-file form's
  `conforms` lines to stdout.
- `--verbose` (`generate`, `verify`) — references that leave a dataset
  (CURIEs or IRIs into another graph) are noted, not checked: by default
  the note gives the count and names five, sorted by referrer id as text (so `d10` comes before `d2`). `--verbose` names
  every one, which is what you want when chasing a specific anchor.
- `--strict` — **narrower than it sounds.** It fails on unmodeled
  constructs, dangling references, colliding slot definitions (the same
  slot name defined at several sites, which would mint one RDF property
  IRI), untyped slots, rule constants that name no permissible value of
  their slot's enum, defective `asserts_absence`/`expand_against`/`records_version_of`/`asserts_value`/`bounds_count_of` declarations,
  and instance-data violations. It does *not* promote
  unprojected-construct, Postgres-skip, or SHACL-skip warnings to errors.
- **Pointing at a record in another graph.** In instance data, a
  class-ranged value that is an absolute IRI or a CURIE against a prefix the
  schema declares (`catalog:aws`) is an *external reference*: it emits as an
  IRI in RDF, is exempt from the dangling check, and is listed in a
  cross-graph summary. A **bare** id always means "a record in this file"
  and is still dangling-checked — including one carrying an undeclared
  prefix, which is read as a typo, not as a link outward.
- `--offline` / `--refresh-labels` — control upstream label fetching for
  external groundings. Fail-open: unreachable sources fall back to CURIEs.
- `--no-graph`, `--viz-mode` — HTML only; warn if used with another format.
- `--check` (`generate`) — the committed-codegen drift gate: regenerates to
  a temporary location, byte-compares against every declared output, exits
  1 naming what drifted or is missing, writes nothing. HTML outputs
  are directories and are skipped with a warning. Replaces consumer-side
  regenerate-and-git-diff wrapper scripts.
- `--rust-time <chrono|jiff>` (`generate`) — time crate for `--format rust`
  temporal fields; keep it in step with the manifest's `rust_time` so a
  by-hand regenerate can't silently flip a jiff module back to chrono.

## Common recipes

    # docs for one schema
    panschema generate --schema schema/my.yaml --output site/

    # docs plus curated instance graphs
    panschema generate --schema schema/my.yaml \
        --instances data/preview.yaml --instances data/full.yaml \
        --output site/

    # every artifact declared in panschema.toml
    panschema generate

    # is this instance data conformant?
    panschema verify --schema schema/my.yaml --data data/full.yaml

    # does this fixture's claim fail against the manifest's sibling graphs?
    panschema verify --entry bench --data tests/fixtures/wrong-value.yaml --strict

    # the same, as JSON Lines a script matches by kind
    panschema verify --entry bench --data tests/fixtures/wrong-value.yaml --strict \
        --message-format json

    # first migration for a database that has never seen this schema
    panschema migrate --schema schema/my.yaml --migrations db/migrations/

    # same, for every schema whose manifest declares a migrations directory
    panschema migrate

## `verify --message-format json`

With `--message-format json` the human lines stay on stderr, unchanged,
and stdout carries one JSON object per line and nothing else:

- each finding, as it is found:
  `{"reason":"finding","kind":"value-claim/differs","level":"error","file":"tests/fixtures/wrong-value.yaml","place":"q1.expected_values[0]","entry":"bench","message":"…"}`.
  `file`, `place`, and `entry` are `null` when there is none (`entry` is
  `null` outside a manifest).
- each data file, as it is read, with the `tree_root` class it was read
  against (`null` when none was chosen) and the records it held:
  `{"reason":"dataset-read","entry":"bench","file":"tests/fixtures/wrong-value.yaml","root":"Bench","records":3}`.
  A file read against the wrong root conforms with no records, so a
  fixture's script checks `records` as well as the findings. A file that
  cannot be read as data has a finding and no such object.
- after every finding, the counts of the reference check and of each claim
  family that ran, per entry:
  `{"reason":"summary","entry":"bench","family":"value-claim","checked":4,"failed":1,"uncheckable":0}`.
  For `reference`, `checked` counts the references into a sibling's
  namespace, and `uncheckable` those in no covered namespace, counted only
  where the entry sets `require_namespace_coverage`.
- last, `{"reason":"verify-finished","success":false,"findings":5}`. A run
  that cannot finish (exit 2) writes none, so a stream without it is a run
  that did not finish.

`level` is what the finding does to this run: `error` when it counts
against the exit code as invoked (a violation in the per-file form;
under `--strict`, anything bare `verify` or `--entry` would otherwise
report as a `warning`), `warning` when it is reported without failing,
`note` when it is informational. A note stays a `note` under `--strict`,
which never fails on one. A schema load warning is an `error` only under
bare `verify --strict`, and only for a defect `--strict` refuses (the
first schema row below); checking data with `--entry` or `--schema`, every
one is a `warning`. Its `file` is the schema file it came from: the import
a reader dropped a construct from or whose definition the root overrode,
and otherwise the root schema. Match on `kind`, never on `message`, whose
wording can change. A reader that stops reading stdout early (`| head -1`,
`| grep -q`) does not change the exit code. A schema's load warnings are
reported each time it loads, under the entry being checked, so a
sibling's repeat under each entry that resolves against it, always at
`warning`: its own entry is the one they count against. The note on
references that leave a dataset stays on stderr. Every kind:

| Finding | Kinds |
|---|---|
| A record that does not conform to the schema | `conformance/required-slot-absent`, `conformance/single-valued-slot-repeated`, `conformance/fewer-values-than-minimum`, `conformance/more-values-than-maximum`, `conformance/designator-names-another-class`, `conformance/designator-not-a-string`, `conformance/invalid-pattern`, `conformance/object-names-no-class-range`, `conformance/value-shape-outside-range`, `conformance/reference-outside-union`, `conformance/scalar-kind-outside-range`, `conformance/not-a-permissible-value`, `conformance/pattern-mismatch`, `conformance/below-minimum`, `conformance/above-maximum`, `conformance/not-numeric-under-bound`, `conformance/outside-specialized-slot`, `conformance/rule-postcondition-failed`, `conformance/rule-alternatives-failed`, `conformance/dangling-reference`, `conformance/undeclared-field`, `conformance/no-root-matched`, `conformance/expansion-gap`, `conformance/duplicate-identifier`, `conformance/unusable-collection-entry`, `conformance/undeclared-inlining`, `conformance/root-id-collides-with-record`, `conformance/data-not-a-mapping` |
| A reference into a `resolve_against` sibling's namespace | `reference/unresolved`, `reference/uncovered` |
| An `asserts_absence` claim | `absence-claim/contradicted`, `absence-claim/uncheckable`, `absence-claim/unchecked` |
| A `records_version_of` pin | `version-pin/mismatched`, `version-pin/uncheckable`, `version-pin/unchecked` |
| An `asserts_value` claim | `value-claim/differs`, `value-claim/unset`, `value-claim/slot-not-carried`, `value-claim/not-among`, `value-claim/uncheckable`, `value-claim/unchecked` |
| A record over its `bounds_count_of` bound | `count-bound/exceeded` |
| Datasets that overlap (a note) | `dataset/iri-collision`, `dataset/unintended-split` |
| A schema defect bare `verify --strict` refuses | `schema/unmodeled-construct`, `schema/dangling-reference`, `schema/property-iri-collision`, `schema/class-iri-collision`, `schema/untyped-slot`, `schema/impossible-rule-value`, `absence-claim/invalid-declaration`, `anchor-expansion/invalid-declaration`, `version-pin/invalid-declaration`, `value-claim/invalid-declaration`, `count-bound/invalid-declaration` |
| Any other schema load warning | `schema/construct-dropped`, `schema/root-overrides-import`, `schema/identifier-key-conflict`, `schema/unique-key-names-no-slot`, `schema/unexpandable-uri`, `schema/specialization-without-parent`, `schema/unmodeled-annotation-nesting`, `schema/unknown-panschema-annotation`, `schema/multiple-tree-roots`, and `schema/no-tree-root` (from `panschema-model`'s `load_dataset` only) |
| The entry's `[check.<name>]` policy (a note) | `check/no-resolve-against`, `check/sibling-declares-no-datasets` |

## `migrate` and the runner's rules

The output targets a checksumming versioned runner (`refinery` and its
family), and three of that runner's rules shape what the command will do.

- **Applied migrations are immutable to the byte.** The runner hashes each
  file's name, version, and raw SQL with no normalization, so a trailing
  newline or a regenerated banner changes the hash and aborts the next run.
  panschema therefore never rewrites a file it has emitted, and the SQL it
  writes carries no timestamp and no tool version. (This is why the DDL in a
  migration lacks the `@generated by panschema v…` line that `generate
  --format postgres` emits.)
- **Appending is safe, inserting is not.** A migration at or below the
  highest applied version is rejected, so the command only ever appends and
  never renumbers.
- **Everything runs in a transaction, with no opt-out.** So
  `CREATE INDEX CONCURRENTLY` and its relatives cannot appear in generated
  output. That is a real ceiling on zero-downtime index builds, not
  something to work around.

Two behaviors follow from all that. Re-running against an unchanged schema
is a no-op that says the migration already exists. Running against a
directory that already holds migrations panschema did not just write is
refused rather than guessed at — incremental migration is a later slice.

A generated migration is a draft to review, not an authoritative artifact.
Read it before applying it. And avoid the runner's "ignore divergence"
setting: it downgrades a checksum mismatch to a log line and *skips* the
migration, which leaves the database on the old schema while the model says
otherwise. Note that the runner's CLI ships with the opposite default from
its library.
