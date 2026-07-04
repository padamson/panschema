# Feature 28: Postgres DDL writer output verification & validation

**Feature:** Give `PostgresWriter` ([feature 24](24-postgres-ddl-writer.md))
the same "validate against the real target language" discipline the
Rust writer already has — today it has **zero** external validation,
the highest-risk gap of any writer in panschema.

**User Story:** As a panschema maintainer, I want confidence that
generated DDL is not just a Rust string that matches my own
expectations, but actually valid, applicable Postgres SQL that enforces
the constraints it claims to — so a writer regression is caught before
a consumer runs it against a real database.

**Related ADR:** None.

**Approach:** Vertical Slicing, fast-cheap tier first (no Docker, runs on
every `cargo test`), thorough tier second (real Postgres via Docker,
gated like the existing mutation-testing full/diff and dogfood-compile
tiering — routine locally opt-in, thorough in CI).

---

## Why Now

Scoping and starting to implement feature 24, I found two real issues by
reading LinkML's actual `relmodel_transformer.py` source rather than by
testing: a hardcoded `_id` FK-column suffix that doesn't match the
target's real primary-key name (a genuine bug my own hand-written tests
didn't catch, because they only checked that the writer matched *my own*
assumption of correct), and a scope decision (whether `is_a` needs three
inheritance strategies or can just flatten) that a real reference
implementation had already resolved differently than I'd assumed.
Neither of those would have been caught by more hand-written unit tests
in the same style — they needed an oracle that knows the target language
independently of this codebase. Unlike the Rust writer (where `rustc` has
been the oracle since day one), the Postgres writer has had none.

---

## Current State

Slice 1 (`pg_query` syntax validation) is in place, built alongside
[feature 24](24-postgres-ddl-writer.md) slice 1 rather than after it.
Slices 2–4 (real-Postgres apply, constraint-enforcement round-trip,
dogfood regression) are not yet built — until slice 2 lands, this
writer's V&V is verification-only (syntax), with no validation
(behavioral) tier yet. See [feature 25](25-rust-writer-output-verification.md)
for what the full two-tier bar looks like once both exist.

---

## Vertical Slices

### Slice 1: `pg_query` syntax validation — fast, every test run

**Status:** Completed

**Priority:** Must Have — cheap, no Docker, and would have caught real
issues immediately (a malformed `CREATE TABLE`, a bad identifier).

**User Value:** Every generated DDL string is confirmed to be real,
parseable Postgres SQL — not just a Rust string this codebase happens to
produce.

**Acceptance Criteria:**
- [x] Add `pg_query` (a Rust binding to `libpg_query`, which is Postgres's own C parser extracted as a standalone library) as a dev-dependency.
- [x] Every `PostgresWriter` unit test that asserts on rendered DDL also parses that output through `pg_query` and asserts zero parse errors — a real Postgres-syntax oracle, not a second copy of this codebase's own assumptions (`assert_valid_postgres_sql`, retrofitted into all 15 existing tests).
- [x] A schema-level test renders a representative multi-class fixture (enums, FKs, a synthesized PK, an identifier-derived PK) and confirms the *entire* script parses as one unit (`whole_script_with_enum_fk_and_both_pk_kinds_parses_as_one_unit`).

**Notes:**
- This tier caught the FK-column-naming bug's SQL *would* still have
  parsed fine either way (`_id` vs `_code` is just an identifier, not a
  syntax error) — `pg_query` validates syntax, not naming correctness.
  The naming bug itself was caught by reading LinkML's reference
  implementation, not by this tier; slice 3 (constraint-enforcement
  round-trip) is where a real Postgres would prove the *reference*
  target is right, not just that the identifier is legal.

**Notes:**
- Syntax-only: `pg_query` doesn't need a running database, so this tier
  is fast enough to run unconditionally, every `cargo test` invocation —
  the same tier `syn::parse_file` occupies for the Rust writer.
- Would not have caught the FK-naming issue (that's a semantic/naming
  choice, not a syntax error) — slice 2 is what closes that class of gap.

---

### Slice 2: Real-Postgres apply test via `testcontainers` — thorough, gated

**Status:** Not Started

**Priority:** Should Have

**User Value:** Generated DDL is confirmed to actually *apply* against a
real, running Postgres — not just parse — catching semantic errors
`pg_query` can't (a FK referencing a column that doesn't actually exist,
a type mismatch between a FK column and its target's primary key).

**Acceptance Criteria:**
- [ ] Add `testcontainers` (with its Postgres module) as a dev-dependency.
- [ ] A test spins up a disposable Postgres container, applies the full generated DDL for a representative fixture schema (the one from slice 1's whole-script test), and asserts it applies with no errors.
- [ ] Gated like the existing thorough tiers in this codebase (mutation testing's full-vs-`--in-diff` split, the dogfood full-compile-only-for-latest-release policy): fast/cheap by default, this slice's test is `#[ignore]`d locally (Docker startup cost) but runs in CI.

**Notes:**
- This is the tier that would have caught the FK-naming bug directly:
  applying DDL with a `_id`-suffixed column against a target whose real
  PK is `code` either fails outright (Postgres would create the column
  fine, since it's just a name — the *real* failure mode is the
  `REFERENCES provider (code)` clause, which is already correct in the
  fix; the naming mismatch is a confusion/correctness-adjacent issue
  more than a hard SQL error) — worth confirming empirically once this
  slice exists rather than asserting from the doc alone.

---

### Slice 3: Constraint-enforcement round-trip — does behavior match the model

**Status:** Not Started

**Priority:** Could Have

**User Value:** Beyond "the DDL applies," confirms the constraints
actually *behave* as modeled — a `CHECK`, `UNIQUE`, `NOT NULL`, or FK
genuinely rejects the row it's supposed to.

**Acceptance Criteria:**
- [ ] Using slice 2's real container, insert rows that should succeed and rows that should be rejected (a duplicate on a `unique_keys` tuple, a NULL on a required column, an out-of-pattern string, a value violating a `rules`-derived `CHECK`) and assert Postgres accepts/rejects each as modeled.

**Notes:**
- Natural home for validating feature 24 slices 2–3 (`unique_keys`,
  `pattern`, value bounds, `rules`-as-`CHECK`) once they're built — this
  is where "the CHECK constraint I generated for `rules` actually
  enforces the conditional requirement" gets proven, not just asserted
  in a docstring.

---

### Slice 4: Extend dogfood regression coverage — once a real schema uses covered constructs

**Status:** 📋 Deferred

**Priority:** Won't Have (until triggered)

**User Value:** Real, released dogfood schemas get Postgres-writer
regression coverage the same way they already get Rust-writer coverage
([feature 20](20-dogfood-schema-regression-fixtures.md)).

**Acceptance Criteria:**
- [ ] (when triggered) Vendored dogfood schemas also render through `PostgresWriter` + slice 1's `pg_query` check.

**Notes:**
- No real dogfood schema uses `rules`/`unique_keys`/`pattern`/value
  bounds yet (confirmed while building feature 17), so there's nothing
  to regression-test against today. Build this once a real schema
  actually exercises what the Postgres writer covers — TDD spirit, not
  speculative.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: `pg_query` syntax validation | Must Have | Feature 24 slice 1 | Completed |
| Slice 2: Real-Postgres apply via `testcontainers` | Should Have | Slice 1 | Not Started |
| Slice 3: Constraint-enforcement round-trip | Could Have | Slice 2, feature 24 slices 2-3 | Not Started |
| Slice 4: Dogfood regression coverage | Won't Have (until triggered) | A real schema using covered constructs | 📋 Deferred |

---

## Definition of Done

- [x] Slice 1 acceptance criteria met (Must Have)
- [ ] Slice 2 acceptance criteria met, or explicitly deferred with a reason
- [x] All tests passing: `cargo nextest run`
- [x] Code formatted + clippy clean: `cargo fmt --check` + `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] CHANGELOG.md updated
- [ ] CI workflow updated if slice 2's gated test needs a new job (Docker availability, timing)
