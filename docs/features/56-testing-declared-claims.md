# Feature 56: Testing declared claims

**Feature:** A schema repo can test the claims its slots declare against the manifest's graphs, and read what `verify` found as data.

**User Story:** As the author of a schema whose slots declare claims against a sibling graph (`asserts_value`, `asserts_absence`, `records_version_of`), I want to check a fixture file against the manifest's graphs, be told when a form of `verify` read claims it did not check, and match each finding by its kind, so that I can keep must-pass and must-fail fixtures side by side without copying the manifest or matching message text.

**Related ADR:** None. Builds on [Feature 44](44-schema-declared-slot-semantics.md) (the claim families and the `[check.<name>]` pass) and [Feature 50](50-structured-verify-findings.md) (a kind on every conformance violation).

**Approach:** Vertical Slicing with Outside-In TDD

---

## Why

Bare `verify` checks every instance file a manifest lists and has one outcome for all of them, so a fixture that must fail cannot be listed beside one that must pass. A schema repo that tests its claims today copies `panschema.toml` and `panschema.lock` to a scratch directory per fixture, rewrites the entry's `instances` line to name the fixture, and runs bare `verify` there. That makes two lines of the manifest a template.

The per-file form, `verify --schema S --data F`, reads no cross-graph claim. Run on a file with a false value claim it prints "conforms" and exits 0, as `linkml-validate` does. Conformance is the right answer to the per-file question, but nothing tells the author that the claims in the file went unread.

The fixture script then decides pass or fail by matching message text, so rewording a message breaks the consumer's CI. Every conformance violation already carries a typed kind (Feature 50); the cross-graph findings and the schema's load diagnostics do not, and none of it is printed in a form a program can read.

---

## Vertical Slices

### Slice 1: Per-file `verify` says which claims it did not check

**Status:** Completed

**User Value:** An author who runs the per-file form on data that states claims learns that they went unchecked and which form checks them.

**Acceptance Criteria:**
- [x] `verify --schema S --data F` on data that states claims of a family the schema declares (`asserts_value`, `asserts_absence`, `records_version_of`) prints one note per family and file giving how many claims the file states, then one hint naming the manifest form that checks them (`verify --entry`), with the files to pass it.
- [x] The count is every claim bare `verify` reads in the file, the ones it evaluates and the ones it reports uncheckable together: value claims count entries, absence claims and version pins count records.
- [x] Data that declares a claim slot but states no claim, and a schema that declares none, print no such note.
- [x] The note does not change the outcome: data that conforms still prints "conforms" and exits 0.

**Notes:**
- Each family's count sits beside its declared-slot count in the model crate's slot-semantics family table, so a new family checked against siblings states both.
- References the per-file form leaves unresolved are already listed by the external-reference note; this slice adds nothing for them.

---

### Slice 2: Check named files as one entry's data

**Status:** Completed

**User Value:** A fixture is checked against the manifest's sibling graphs with one command run from the repo, with no copy of the manifest.

**Acceptance Criteria:**
- [x] `verify --entry <name> --data F [--data G ...]` checks only the manifest entry `<name>`, reading the named files in place of the instances it declares, with every `[check.<name>]` policy and every `resolve_against` sibling loaded as the manifest declares them.
- [x] Findings are reported as bare `verify` reports them: every finding, a file that does not conform included, warns, and with `--strict` any finding exits 1. The command exits 2 when it cannot run.
- [x] The result holds the named files' findings alone: the entry's schema diagnostics print but do not fail the run, and a file named twice is checked once.
- [x] When the entry names no `resolve_against` sibling, each claim a named file states is reported as not checked, which `--strict` refuses. A file stating no claim is unaffected.
- [x] An `--entry` naming no `[schemas]` entry, `--entry` without `--data`, and `--entry` with `--schema` are refused with exit 2.
- [x] Data paths are read relative to the working directory, as `--data` is read in the per-file form.

**Notes:**
- Other entries are not checked, so a fixture's result holds only its own findings.
- Every `[schemas]` entry is still resolved, as in each manifest-driven command, and each run loads the siblings again. A suite that runs one process per fixture pays that per fixture; Slice 3's per-file findings let it name every fixture in one run.

---

### Slice 3: Findings as JSON Lines

**Status:** Completed

**User Value:** A script matches each finding by its kind and place, and is not broken when a message is reworded.

**Acceptance Criteria:**
- [x] Every conformance violation and every cross-graph finding has a kind: an unresolved or uncovered reference, a contradicted or uncheckable absence claim, a mismatched or uncheckable version pin, a value claim that differs, is unset, names a slot the record's class does not carry, names a record outside its `among` values, or cannot be checked, the notes on datasets that overlap and claims left unchecked, and the notes on a `[check.<name>]` policy that checks nothing. Kinds are `<family>/<case>` strings, a claim family's kinds carry the family's own name (`value-claim/not-among`), and the same case always yields the same kind.
- [x] `verify --message-format json` writes one JSON object per line to stdout, in either form: one per finding as it is found and one per data file as it is read, then the summary lines, then one closing object with the outcome. Stdout carries nothing else, and the human findings on stderr are unchanged.
- [x] Each finding object carries `reason: "finding"`, a `kind`, a `level`, the `file` it was found in, the `place` in the data it names (a record id, or an entry's place such as `q-07.claims[0]`), the manifest `entry` in manifest mode, and the `message` the human line prints.
- [x] `level` says what the finding does to the run: `error` when it counts against the exit code as the command was invoked, `warning` when it is reported without failing the run, `note` when it is informational. A schema load warning is the exception until Slice 4 levels it.
- [x] Each data file read as instance data writes an object naming the `tree_root` class it was read against and the number of records it held, so a file read against the wrong root, which conforms with no records, is told from one that conforms.
- [x] Each family that ran writes a summary object with its counts (checked, failed, uncheckable), as the human summary line reports them. Where the entry requires namespace coverage, the reference summary counts a reference in no covered namespace as uncheckable, so no summary reads clean in a run `--strict` fails for that family.
- [x] The closing object says whether the run succeeded and how many findings it reported, and is written whenever the check runs to its end. A run that cannot finish (exit 2) writes none, so a stream without a closing object is a run that did not finish. The exit code is the one the human form returns, and a consumer that stops reading stdout early does not change it.
- [x] `--message-format` takes `human` (the default) or `json`; any other value is refused with exit 2.

**Notes:**
- The shape and the flag follow `cargo --message-format json`: a `reason` field on every line, objects written as they are produced. `--format` already names the artifact `generate` writes, and `generate` reports the same findings, so the report's format takes a name free on both.
- Kinds are part of the library API: every finding type says its kind and place through one trait, the only thing the reporter accepts, so a finding cannot be reported without a kind. The kinds are listed in one place, which keeps the shipped skill's table of them complete.
- Until Slice 4, a schema load warning is reported with the kind `schema/load-warning` at level `warning`. Loader warnings are strings until then, so the reporter cannot tell the ones bare `verify --strict` refuses from the rest, and those are reported at `warning` in a run they fail.
- Considered and left out: findings declared as expected (`--expect <kind>`, a manifest list of must-fail fixtures), which need kinds first and can follow; SARIF, whose annotations need source positions the data reader does not keep; a SHACL validation report, which needs kinds published as IRIs; and each finding's fields as a typed object, which would make every field name a wire contract.

---

### Slice 4: Schema load diagnostics carry a kind

**Status:** Not Started

**User Value:** The JSON output names a schema defect by kind as well, so no finding `verify` reports is outside the machine-readable contract.

**Acceptance Criteria:**
- [ ] Every warning a schema load raises (an unmodeled construct, a dangling reference, a colliding definition, an untyped slot, an impossible rule value, a defective slot-semantics declaration, an import collision the root schema wins, and the rest) reaches `verify --message-format json` as a finding with its own kind and the schema file it came from.
- [ ] Each load warning's `level` says what it does to the run: `error` under bare `verify --strict` for a warning `--strict` refuses, `warning` otherwise.
- [ ] The human warning line for each is unchanged.
- [ ] A library caller of the model crate's loader reads each warning's kind and message without parsing text.

**Notes:**
- Changes the type of the loader's warnings from strings to a typed warning, a breaking change for library callers of `panschema-model`.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1 | Must Have | None | Completed |
| Slice 2 | Must Have | None | Completed |
| Slice 3 | Must Have | None | Completed |
| Slice 4 | Must Have | Slice 3 | Not Started |

---

## Definition of Done

The feature is complete when ALL of the following are true:

- [ ] All acceptance criteria from user story are met
- [ ] All vertical slices marked as "Completed"
- [ ] All tests passing: `cargo nextest run`
- [ ] Library documentation complete with examples: `cargo doc`
- [ ] Code formatted: `cargo fmt --check`
- [ ] No clippy warnings: `cargo clippy -- -D warnings`
- [ ] README.md updated
- [ ] CHANGELOG.md updated
- [ ] The shipped `panschema-usage` skill describes `--entry` and `--message-format json`
