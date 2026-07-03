# Feature 23: Diagnostics for IR-modeled constructs a writer doesn't project

**Feature:** Warn when a schema uses a class-level construct that panschema
*does* model in the IR — so it never trips feature 22's unmodeled-construct
guard — but that the target format's writer doesn't project, so it renders
in HTML and silently vanishes from every other output.

**User Story:** As a schema author who declares `rules` or `unique_keys`
and then runs `generate --format rust` (or any RDF format), I want a
warning naming the construct, the class, and the format that's dropping
it — instead of discovering later that a constraint I modeled and saw
rendered in the docs was never enforced or emitted anywhere else.

**Related ADR:** None. Sibling to
[feature 22](22-unsupported-construct-diagnostics.md): that feature
guards the parse → IR boundary (a key with no IR field at all); this one
guards the IR → writer boundary (a field every writer *could* read, but
a specific one doesn't). Complements
[feature 17](17-class-validation-constructs.md), whose `rules` and
`unique_keys` are the two constructs this guards today.

**Approach:** Vertical Slicing with Outside-In TDD.

---

## Why Now

Dogfooding `panschema generate --format rust` against a real schema with
`rules` surfaced two live problems in the ad hoc warning feature 17 slice 1
added:

1. **Wrong wording.** The slice-1 warning hardcoded "does not yet emit to
   RDF/OWL" and fired for *any* non-HTML format — including `rust`, which
   has nothing to do with RDF. A schema author running Rust codegen would
   read a warning about a format they never asked for.
2. **Silent gap for `unique_keys`.** Feature 17 slice 2 added `unique_keys`
   with no equivalent warning at all. It renders in HTML and is dropped by
   every other writer with zero signal — exactly the class of bug feature
   22 exists to close, just one step later in the pipeline (IR → writer,
   not parse → IR).

Both are fixed by one shared, format-aware mechanism instead of two
one-off, construct-specific warnings — and the next IR-modeled-but-not-
fully-projected construct (e.g. feature 17 slice 3's boolean class
expressions) extends the same list rather than needing its own warning
wired in by hand.

---

## Design

- `classes_with_unprojected_constructs(schema, format)` walks every class
  and reports each populated `rules` / `unique_keys` field as an
  `UnprojectedConstruct { class, construct }`, *unless* `format` is
  `"html"` (case-insensitive) — the only writer that currently projects
  both fully.
- `UnprojectedConstruct::message(&self, format)` takes the format so the
  warning names what was actually requested ("does not emit to the `rust`
  format") instead of a hardcoded target.
- This replaces feature 17 slice 1's `classes_with_rules_unsupported_in_rdf`
  (deleted) and the missing `unique_keys` equivalent. It does **not**
  replace `unresolved_unique_key_slots` (feature 17 slice 2) — that's a
  structural correctness check (a key names a slot the class doesn't
  have), orthogonal to whether a writer projects the construct at all, and
  stays its own format-independent warning.
- Deliberately a short, explicit list (`rules`, `unique_keys`) rather than
  a generic reflect-over-every-IR-field framework — the two known cases
  are cheap to enumerate by hand; a generic version would need to know
  which fields each writer *intends* not to cover (e.g. `abstract` is
  legitimately RDF-inert, not a drop) versus which are gaps, which isn't
  derivable structurally the way feature 22's `serde(flatten)` catch-all
  is. Extend the list by hand as new constructs land with partial writer
  coverage; reconsider a generic mechanism only if that list grows large
  enough to be its own maintenance burden.

---

## Vertical Slices

### Slice 1: Generalize the writer-projection warning; fix the format-name bug; cover `unique_keys`

**Status:** Completed

**Priority:** Must Have — the format-name bug is a live, misleading-message
defect (not just a gap), and `unique_keys` dropping without any warning is
the exact silent-drop class feature 22 exists to prevent.

**User Value:** Running `generate --format rust` (or any RDF format) on a
schema with `rules` and/or `unique_keys` prints an accurate warning naming
the real format for each construct that won't appear in that output.

**Acceptance Criteria:**
- [x] `classes_with_unprojected_constructs(schema, format)` replaces `classes_with_rules_unsupported_in_rdf`; empty for `format == "html"` (case-insensitive), reports `rules` and `unique_keys` for every other format (`classes_with_unprojected_constructs_covers_rules_and_unique_keys`, `classes_with_unprojected_constructs_empty_for_html`, `classes_with_unprojected_constructs_empty_when_neither_present`).
- [x] `UnprojectedConstruct::message(format)` names the actual requested format, not a hardcoded one — `--format rust` produces a message naming `rust`, not `RDF/OWL` (`unprojected_construct_message_names_the_requested_format`).
- [x] `generate --format rust` and `generate --format ttl` each warn about a schema's `rules` *and* `unique_keys` with the correct format name; `generate --format html` warns about neither (`cli_generate_non_html_warns_unprojected_constructs`).

**Notes:**
- `main.rs`'s call site moves out of the HTML/else format branch — the function itself decides via the `format` argument, so one unconditional call site covers every format.
- Naming the requested format explicitly (rather than a fixed target) surfaced a related, pre-existing drift bug while testing this fix: both `--format` CLI help strings were missing `rust` (and the top-level one was also missing `graph-json`) — hand-written strings disconnected from `FormatRegistry`. Fixed alongside this slice: `FormatRegistry::writer_format_ids()` is now the definitive list, and `help_text_lists_every_registered_writer_format` asserts both help strings contain every registered format id, so the next writer added without a help-string update fails a test instead of drifting silently. No separate CHANGELOG entry — the stale text was introduced in this same unreleased cycle (the Rust writer's own `Added` bullet), so it never shipped.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: generalize + fix + cover `unique_keys` | Must Have | Feature 17 slices 1–2 | Completed |

---

## Definition of Done

- [x] Slice 1 acceptance criteria met.
- [x] All tests passing: `cargo nextest run`
- [x] Library documentation complete: `cargo doc`
- [x] Code formatted + clippy clean: `cargo fmt --check` + `cargo clippy --all-targets --all-features -- -D warnings`
- [x] README.md — no new bullet needed; this is a bugfix + parity fix on an existing diagnostic, same posture as other modeled-construct additions
- [x] CHANGELOG.md updated
- [x] [linkml-coverage.md](../linkml-coverage.md) and [feature 17](17-class-validation-constructs.md) references to the old `classes_with_rules_unsupported_in_rdf` name updated
