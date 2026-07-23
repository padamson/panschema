# Feature 22: Diagnostics for silently-dropped LinkML constructs

**Feature:** Warn (and, under a strict mode, error) when a schema uses a LinkML construct panschema parses but does not model — so a real constraint can't ship silently ignored.

**User Story:** As a schema author, when I write a construct panschema doesn't yet support (`unique_keys`, a boolean class expression), I want a clear diagnostic that it was parsed but will not render or emit — instead of it vanishing with no signal — so I can rework or defer it deliberately.

**Related ADR:** None. Complements [07-schema-validation.md](07-schema-validation.md) (structural well-formedness) and [17-class-validation-constructs.md](17-class-validation-constructs.md) (which *models* these constructs); this feature makes their *absence of support* loud in the meantime.

**Approach:** Vertical Slicing with Outside-In TDD.

> The canonical list of unmodeled constructs lives in [`docs/linkml-coverage.md`](../linkml-coverage.md); this feature turns the highest-value subset from silent to loud. It is a prerequisite guard — cheap, and it makes every later modeling feature safe to attempt (a producer learns immediately when something isn't wired).

---

## Design

`serde` currently ignores unknown YAML keys (no `deny_unknown_fields`), so unmodeled constructs are dropped at parse with no trace. To detect them, capture them:

- Add a `#[serde(flatten)]` catch-all map to `ClassDefinition` (`unmodeled: BTreeMap<String, serde_yaml::Value>`). All the target constructs are class-level, so one catch-all suffices for the first slice. The catch-all is populated only by the YAML reader; the OWL reader builds the IR programmatically and leaves it empty.
- A diagnostics pass warns on **every** captured key. The ignore-list (`IGNORED_CLASS_KEYS`) is a *denylist that starts empty* — nothing is silenced up front. Warning by default is what catches the drops we didn't anticipate (a construct we never enumerated, one LinkML adds later, a producer's typo of a real field); an allowlist would only catch known drops, leaving the exact blind spot the guard exists to close. A key is added to the ignore-list only once identified as one whose non-rendering is correct-by-definition (LinkML's equivalent of a code comment), *with its reason* — never speculatively, and never a semantic construct (those get modeled, or stay loud). When a construct becomes modeled, it maps to a real field and leaves the catch-all on its own.
- Each finding is one diagnostic naming the key and its owning class.

Surfacing mirrors the existing collision/unresolved-slot warnings: `eprintln!("warning: …")` during `generate` (and `serve`). A new `--strict` flag turns the warnings into a non-zero exit.

---

## Vertical Slices

### Slice 1: Detect + warn on unmodeled class-level constructs

**Status:** Completed

**User Value:** A schema that carries a `unique_keys` / boolean-expression block on a class produces a warning at generate time instead of silently dropping it.

**Acceptance Criteria:**
- [x] Generating a schema whose class carries *any* unmodeled key emits a warning naming the key and the class — by default, even for a key not previously enumerated.
- [x] Modeled keys never warn (they map to fields, never reaching the catch-all).
- [x] A key placed on the ignore-list is silenced (mechanism exists, even though the list ships empty).
- [x] Existing output is unchanged (the construct is still dropped from rendering — this slice only reports).

The ignore-list (`IGNORED_CLASS_KEYS` in `crate::diagnostics`) is a denylist that **starts empty** — every unmodeled key warns until a specific one is identified as safe to silence, with its reason. Detection is tested against a *fabricated* key (with an injected ignore-list), decoupled from the real list — the warn-by-default behavior and the ignore path are what's pinned, not any specific real key.

### Slice 2: `--strict` turns the warnings into an error

**Status:** Completed

**User Value:** CI can fail a build that would silently drop a constraint.

**Acceptance Criteria:**
- [x] `generate --strict` exits non-zero when any unmodeled construct is present, after listing them (works for both `--schema` and manifest modes).
- [x] Without `--strict`, the same schema warns and exits zero.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1 — detect + warn | Must Have | None | Completed |
| Slice 2 — `--strict` errors | Should Have | Slice 1 | Completed |

---

## Definition of Done

- [x] Slice ACs met; all slices Completed.
- [x] `cargo nextest run`, `cargo fmt --check`, `cargo clippy … -D warnings` clean.
- [x] README.md + CHANGELOG.md updated.
- [x] `docs/linkml-coverage.md` notes that these constructs now warn (no longer silent).
