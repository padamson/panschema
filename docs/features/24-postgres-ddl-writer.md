# Feature 24: Postgres DDL writer

**Feature:** A new `Writer` that projects the LinkML IR to Postgres
`CREATE TABLE` DDL — `panschema generate --format postgres` — so a
relationally-backed application gets its database schema from the same
LinkML source as its Rust structs, with no hand-written SQL.

**User Story:** As a schema author whose LinkML schema is the single
source of truth for a Rust application backed by Postgres, I want
`panschema` to emit the table/column/constraint DDL for that schema, so I
never hand-write or hand-sync a Postgres schema that could drift from the
LinkML source — the same reason I already generate Rust structs instead
of hand-writing them.

**Related ADR:** None yet — candidate for one if slice 6 (inheritance
strategy) needs a documented decision record, the way
[005](../adr/005-graph-visualization-conventions.md) documents the graph's
per-edge-kind rendering conventions.

**Approach:** Vertical Slicing with Outside-In TDD, ordered by
value-per-difficulty: the constructs that are already IR-modeled and map
to SQL with no real design ambiguity ship first (slices 1–3); the
constructs with genuine relational-modeling design decisions — the
classic object/graph-to-relational impedance mismatch — ship later, each
its own slice, so the feature is useful long before the hardest cases are
solved.

---

## Why Now

Dogfooding `panschema generate --format rust` against a real schema (see
[feature 23](23-cross-writer-construct-coverage-diagnostics.md)'s
motivating case) surfaced the concrete need: a schema author moving from
LinkML authoring into building a Rust backend needs *both* the Rust types
and the database schema from the same source, and today only the former
exists. Hand-writing/hand-syncing Postgres DDL against a LinkML source
that keeps evolving is exactly the class of drift risk this project
exists to close (see feature 22's rationale).

LinkML's own Python toolchain already solves the hard part of this
problem — `linkml.transformers.relmodel_transformer` and its SQL/
SQLAlchemy generators establish real, documented conventions for turning
LinkML's document/graph shape into a normalized relational schema
(inheritance strategy, multivalued-slot linking tables, etc.). This
feature treats those conventions as **prior art to read, not code to
port** (different language, different IR) — they de-risk the design
decisions below by showing they've already been solved once, for the
same schema language.

---

## Design

### Non-goals (deliberately out of scope)

- **No migration/diff engine.** This writer emits the *current* target
  schema — a pure, stateless transform of the IR, exactly like every
  other writer. It has no concept of schema history and never computes
  an `ALTER`. Computing the delta between a live database and this
  writer's output is a fundamentally different, stateful problem
  (schema-history tracking or live-DB introspection, then diffing) that
  doesn't fit panschema's Reader → IR → Writer architecture — it's
  effectively a second product, comparable in scope to
  [sqldef](https://github.com/sqldef/sqldef) or
  [Atlas](https://atlasgo.io) themselves, both of which already solve it
  well. See "Migrating with the generated DDL" below for how this
  writer's output is meant to be consumed by one of those tools instead.
- **Postgres only, for now.** Enum syntax, array columns, and identifier
  quoting all diverge meaningfully across SQL dialects (Postgres/MySQL/
  SQLite). Building a dialect-general writer speculatively would mean
  designing for two audiences before either is confirmed. `RustWriter`
  and `OwlWriter` are each single-target for the same reason — reconsider
  multi-dialect only if a second dialect is an actual, not speculative,
  need.
- **No ORM entity code.** `RustWriter` already produces the Rust structs
  applications work with; this writer produces raw DDL, not `diesel`
  `schema.rs` output or `sea-orm` entities. Pairing generated structs
  with a query layer (`sqlx`, `diesel`, `sea-orm`) is the consuming
  application's choice, unconstrained by this writer.

### Naming and type mapping

- **Table names**: class name → `snake_case`, singular (matches the
  class name's own semantics — one row is one instance of the class).
  Reserved-word / identifier-length handling mirrors how `rust_writer.rs`
  already escapes Rust keywords — same pattern, different target
  language's reserved-word list and Postgres's 63-byte identifier limit.
- **Column names**: slot name → `snake_case` (LinkML slot names are
  already conventionally snake_case, so this is normalization, not
  translation, in practice).
- **Primary keys**: the effective slot marked `identifier: true` becomes
  the primary key column. A class with no `identifier` slot gets a
  synthetic `id uuid PRIMARY KEY DEFAULT gen_random_uuid()`.
- **Scalar range → column type** (the common LinkML built-in types):
  `string`→`text`, `integer`→`integer`, `float`/`double`→`double
  precision`, `boolean`→`boolean`, `date`→`date`, `datetime`→`timestamptz`.
- **Enum range** (a slot whose range is an `EnumDefinition`) → a native
  `CREATE TYPE <enum_name> AS ENUM (...)` from the permissible values,
  column of that type. Matches Postgres's closed-set semantics to
  LinkML's own closed permissible-value set more directly than a `CHECK`
  constraint or lookup table would.
- **Single-valued class range** (not multivalued) → a nullable-unless-
  `required` foreign key column (`<slot>_id`) referencing the target
  class's table primary key.

### Constraints from already-IR-modeled constructs

These three map to SQL with no real design ambiguity — the hard
modeling work already happened when they were added to the IR
([feature 14](14-slot-constraints.md),
[feature 17](17-class-validation-constructs.md)):

- `unique_keys` → `UNIQUE (col1, col2, ...)` / `CREATE UNIQUE INDEX`.
- `pattern` → `CHECK (col ~ 'pattern')` (Postgres native regex).
- `minimum_value` / `maximum_value` → `CHECK (col >= min AND col <= max)`.
- `rules` → a `CHECK` constraint built from the rule's pre/postcondition
  `slot_conditions`, e.g. `CHECK (status <> 'actual' OR region IS NOT
  NULL)` for the `equals_string` + `required` pair. The same
  `SlotCondition` vocabulary already modeled for the class-card "when …
  then …" sentence (`html_writer.rs`'s `describe_slot_condition`) maps
  each field to a SQL predicate instead of a markdown clause — `range` →
  (not directly expressible as a `CHECK`, skipped), `equals_string`/
  `equals_number` → `col = value`, `required` → `col IS NOT NULL`,
  `pattern` → `col ~ 'pattern'`, `minimum_value`/`maximum_value` → `col
  >= n` / `col <= n`. Notably more tractable than the deferred RDF
  projection (feature 17 slice 4) — standard `CHECK` constraints express
  a conditional-on-a-sibling-field requirement directly, where OWL
  restrictions cannot without SWRL.

### The genuinely hard cases (each its own later slice)

- **Multivalued class-range slots** (`Vec<T>` in the Rust output) need a
  linking/join table — real many-to-many (or one-to-many, depending on
  ownership direction) relational design, not a mechanical translation.
- **`is_a` inheritance** has three classic strategies (single-table with
  a discriminator, class-table with joins, concrete-table with
  duplication), each with real tradeoffs; this needs one opinionated
  default (single-table, provisionally — cheapest to implement and
  query for panschema's typically shallow hierarchies) documented as a
  decision, not assumed.
- **`any_of` polymorphic ranges** are the classic "polymorphic
  association" problem — no clean single SQL mapping (multiple nullable
  FK columns with an exactly-one-set `CHECK`, or a join table with a
  type discriminator). Candidate for permanent deferral, like feature 17
  slice 4, until a real schema needs it.

---

## Vertical Slices

### Slice 1: Core DDL — concrete classes, scalars, enums, single-valued class references

**Status:** Completed

**Priority:** Must Have — the walking skeleton; unblocks the common case
immediately.

**User Value:** `generate --format postgres` on a schema with no
multivalued slots, no inheritance, and no `any_of` produces a complete,
applicable `CREATE TABLE` script.

**Acceptance Criteria:**
- [x] `PostgresWriter` implements `Writer` (`format_id() == "postgres"`), registered in `FormatRegistry::with_defaults`.
- [x] One `CREATE TABLE` per concrete class with its effective scalar attributes as typed columns; `required` → `NOT NULL`.
- [x] Enum-range slots emit a `CREATE TYPE ... AS ENUM` (once per enum, before the tables that reference it) and a column of that type.
- [x] The effective `identifier` slot becomes the primary key; absent one, a synthetic `id uuid PRIMARY KEY DEFAULT gen_random_uuid()`.
- [x] A single-valued class-range slot becomes a nullable-unless-`required` foreign key column referencing the target table's primary key, ordered so referenced tables are declared first (or via deferred constraints) so the script applies in one pass.
- [x] Abstract classes with no concrete subclass still emit no table (nothing to instantiate); mixins fold their attributes into every class that mixes them in, matching how `rust_writer.rs` already flattens mixin fields.

**Notes:**
- Multivalued slots, `is_a`, and `any_of` are out of scope for this slice — a class using any of them (or referencing a class that is itself out of scope) is skipped with a diagnostic (`skipped_classes`, wired into `generate --format postgres` in `main.rs`), not silently incomplete.
- The FK column is named after the target's *actual* primary key column (`{slot}_{target_pk_name}`), not a hardcoded `_id` suffix — matches the convention in LinkML's own `relmodel_transformer.py` (read directly from `linkml/linkml` on GitHub while implementing this slice), and was a real bug in an earlier draft: a hardcoded suffix produced a column named `..._id` that didn't refer to an `id` column at all when the target's key was named something else.
- `is_a` is scoped more conservatively here than LinkML's own reference implementation, worth revisiting: `relmodel_transformer` doesn't choose between single-table/class-table/concrete-table inheritance strategies at all — it fully flattens every class's induced slots into its own table, which is mechanically identical to what this slice already does for mixins (via the shared `linkml_resolve::resolve_effective_slots`, which walks `is_a` too). Slice 6 may turn out to be "stop skipping and just flatten" rather than a genuine three-way design choice.
- Every rendered fixture is verified as real, parseable Postgres SQL via `pg_query` (a binding to Postgres's own C parser) — see [feature 28](28-postgres-ddl-writer-output-verification.md) slice 1, built alongside this slice rather than after it.

---

### Slice 2: Constraints — `unique_keys`, `pattern`, value bounds

**Status:** Completed

**Priority:** Must Have

**User Value:** Constraints already visible in the HTML docs (feature 14,
feature 17 slice 2) are enforced by the database itself, not just
documented.

**Acceptance Criteria:**
- [x] `unique_keys` emits a table-level `CONSTRAINT <table>_<key_name>_key UNIQUE (col1, col2, ...)` per key, resolved through the same effective-slot set as [feature 17 slice 2](17-class-validation-constructs.md) (inherited/mixed-in slots included) — not just `class.attributes` (`unique_keys_emit_a_table_level_unique_constraint`).
- [x] A `unique_keys` entry naming a slot the class doesn't have (already flagged by `diagnostics::unresolved_unique_key_slots`, which every writer's CLI path already warns on) is dropped from the emitted DDL rather than emitting a `UNIQUE` referencing a nonexistent column. The writer resolves independently of the CLI warning — it must not emit broken SQL even if a caller ignored the warning (`unique_key_naming_a_missing_slot_is_dropped_not_emitted`).
- [x] `pattern` emits an inline `CHECK (col ~ 'pattern')` on the column, single-quote-escaping the pattern the same way enum permissible values already are (`pattern_emits_an_inline_check_constraint`, `pattern_single_quotes_are_escaped`).
- [x] `minimum_value` / `maximum_value` emit a single inline `CHECK` on the column: both bounds combine into one `CHECK (col >= min AND col <= max)`; either alone emits just that side (`both_value_bounds_emit_one_combined_check`, `a_single_value_bound_emits_only_that_side`).
- [x] `diagnostics::classes_with_unprojected_constructs` no longer reports `unique_keys` as unprojected for `format == "postgres"` (it still flags `unique_keys` for every other non-HTML format, and still flags `rules` for postgres until slice 3) (`postgres_projects_unique_keys_so_only_rules_is_flagged`).
- [x] [linkml-coverage.md](../linkml-coverage.md) rows for `unique_keys`, `pattern`, and `minimum_value`/`maximum_value` now carry a Postgres `●◨` (full projection, syntax-verified via `pg_query`).

**Notes:**
- `unique_keys` is a table-level constraint (potentially multi-column) so
  it's appended as its own line alongside the column definitions, not
  inlined on one column's line, unlike `pattern`/value-bounds (both
  strictly single-column).
- No ordering/deferral concern like the slice 1 FK constraints: every
  column a `unique_keys`/`pattern`/value-bounds constraint references
  lives in the same table being declared, so all three go inline in the
  same `CREATE TABLE` statement — no `ALTER TABLE ... ADD CONSTRAINT`
  needed here.
- Every new construct gets `pg_query` syntax coverage (the established
  per-construct pattern from slice 1/[feature 28](28-postgres-ddl-writer-output-verification.md)
  slice 1) as it's added; the real-apply oracle
  ([feature 28](28-postgres-ddl-writer-output-verification.md) slice 2)
  is not extended here — proving these constraints actually *enforce*
  against real data is feature 28 slice 3's job, which depends on this
  slice existing first.

---

### Slice 3: `rules` as `CHECK` constraints

**Status:** Not Started

**Priority:** Should Have — closes the gap feature 17 slice 4 (RDF)
couldn't: a `rules` conditional requirement becomes an actually-enforced
database constraint, not just a rendered sentence.

**Acceptance Criteria:**
- [ ] Each rule with both `preconditions` and `postconditions` slot conditions built from `equals_string` / `equals_number` / `required` / `pattern` / `minimum_value` / `maximum_value` emits a `CHECK` constraint combining them (`precondition-not-true OR postcondition-true`, the standard SQL encoding of a conditional constraint).
- [ ] A `rules` entry whose conditions aren't expressible this way (a bare `range` condition, or a precondition/postcondition-only rule) is skipped with a diagnostic naming the rule, not silently dropped.

---

### Slice 4: Multivalued scalar slots as array columns

**Status:** Not Started

**Priority:** Should Have

**Acceptance Criteria:**
- [ ] A multivalued slot with a scalar range emits a Postgres array column (`text[]`, `integer[]`, ...) instead of being skipped.

---

### Slice 5: Multivalued class-range slots as linking tables

**Status:** Not Started

**Priority:** Could Have — the first of the genuinely hard relational-
design slices; scope the linking-table shape (naming, extra columns,
cardinality direction) as its own design pass when picked up, informed
by LinkML's `relmodel_transformer` conventions.

---

### Slice 6: `is_a` inheritance strategy

**Status:** Not Started

**Priority:** Could Have — needs a documented decision (candidate ADR)
before implementation: single-table (default candidate) vs. class-table
vs. concrete-table, with the tradeoffs from "Design" above weighed
against panschema's actual schemas' hierarchy shapes.

---

### Slice 7: `any_of` polymorphic ranges — deferred indefinitely

**Status:** 📋 Deferred

**Priority:** Won't Have (until a real schema needs it) — no clean single
SQL mapping exists; same deferral posture as feature 17 slice 4.

---

## Migrating with the generated DDL

This writer answers "what should the schema look like right now" — it is
**not** a migration tool (see "Non-goals" above). Two ways to turn its
output into an actual schema change against a live Postgres database,
both driven by re-running `panschema generate --format postgres` after
editing the LinkML schema and pointing the chosen tool at the fresh
output:

- **[sqldef](https://github.com/sqldef/sqldef)'s `psqldef`** —
  declarative, idempotent apply: `psqldef mydb < schema.sql` introspects
  the live database, computes whatever `ALTER`s are needed to converge it
  to match the file, and applies them (`--dry-run` previews first). No
  migration-file history is kept — the database itself, plus the LinkML
  schema's own git history, is the record of what changed. Simplest
  option; open source with no paid tier.
- **[Atlas](https://atlasgo.io)**, in its versioned-migrations mode —
  generates a discrete, reviewable migration file per change (`atlas
  migrate diff`, comparing the live database against this writer's
  output), which you review and commit before applying (`atlas migrate
  apply`) — the closer match to an alembic-style reviewable migration
  history. Its schema-diff/apply core is Apache-2.0; team/cloud features
  are a separate paid tier this workflow doesn't need.

Either way, adding a column is: edit the LinkML schema → `panschema
generate --format postgres` → hand the output to `psqldef` or `atlas
migrate diff` → (review, for Atlas) → apply. panschema's role stops at
the first step; the chosen tool owns introspecting the live database and
computing the delta.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: core DDL | Must Have | None | Completed |
| Slice 2: `unique_keys`/`pattern`/value bounds | Must Have | Slice 1 | Completed |
| Slice 3: `rules` as `CHECK` | Should Have | Slice 1 | Not Started |
| Slice 4: multivalued scalars as arrays | Should Have | Slice 1 | Not Started |
| Slice 5: multivalued class-refs as linking tables | Could Have | Slice 1 | Not Started |
| Slice 6: `is_a` inheritance strategy | Could Have | Slice 1 | Not Started |
| Slice 7: `any_of` polymorphic ranges | Won't Have | Slice 1 | 📋 Deferred |

---

## Definition of Done

- [x] Slices 1–2 acceptance criteria met (slices 3–6 as demand confirms; slice 7 deferred)
- [x] All tests passing: `cargo nextest run`
- [ ] Library documentation complete: `cargo doc`
- [x] Code formatted + clippy clean: `cargo fmt --check` + `cargo clippy --all-targets --all-features -- -D warnings`
- [x] README.md gains a section on `--format postgres` and the sqldef/Atlas migration workflow, mirroring the mdbook-panschema install section's shape
- [x] CHANGELOG.md updated
- [x] [linkml-coverage.md](../linkml-coverage.md) gains a writer column entry (or a dedicated table) for the new writer's per-construct coverage
