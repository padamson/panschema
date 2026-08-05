# Feature 39: Schema Diff and Migration Generation

**Feature:** Two new commands. `panschema diff` reports what changed
between two versions of a schema, as a semantic, format-agnostic delta with
a compatibility verdict. `panschema migrate` renders that same delta as a
versioned Postgres migration file a migration runner can apply.

**User Story:** As an author whose LinkML schema is the source of truth for
generated Rust types and generated SQL DDL, I want editing the schema to
produce the migration that moves an existing database to match it — so that
evolving the model does not require me to hand-author DDL I don't know how
to write, and so a reviewer can see the change in schema terms before
seeing it in SQL.

**Related ADR:** to be written as ADR 010 in Slice 1 — the declarative
source / versioned artifact hybrid, and the append-only immutability rule
that follows from a checksumming runner (see *Constraints* below).

**Approach:** Vertical Slicing with Outside-In TDD.

**Scope boundary:** panschema generates migration *files*. It never
connects to a database to apply them, and never gains an apply/rollback
command — applying is the migration runner's job, and keeping that boundary
means there is no destructive command here to confuse with the generator.
Data migrations (backfills, value transformations) are out of scope in all
slices: only structural DDL is derivable from a schema delta.

---

## Prior Art

Researched 2026-07-28. The mechanics of "diff two schema states, emit a
versioned migration" are mature prior art, and it is worth being precise
about what is and is not new, so this feature borrows designs instead of
reinventing them.

**Not novel — the diff-and-plan pipeline.** Atlas names this shape
"Versioned Migration Authoring": load current state, compare against
desired state, write a new migration script. `sqldef` and Stripe's
`pg-schema-diff` do the equivalent for Postgres. Rust has three attempts —
`pgmold`, `renovate`, and `postgres_migrator` (which shells out to the
Python `migra` binary rather than diffing itself). None has meaningful
adoption, so reuse-by-dependency is unattractive, but their designs are
worth copying.

**Not novel — model-driven desired state.** Atlas has first-party
providers reflecting SQLAlchemy, Django, GORM, Prisma, EF Core and others
to DDL, and Diesel CLI's `migration generate --diff-schema` diffs its
`schema.rs` against a live database. Every one of these projects a
*storage-shaped* model, though: the input already describes tables and
columns.

**Novel — a semantic model as the desired state.** No verified prior art
takes a domain modeling language as the input side. LinkML in particular
has nothing: its CLI surface is 45 `gen-*` commands plus seven others, with
no diff, compare, or migrate; `linkml-runtime` has no schema-comparison
primitive across its utility modules; the `linkml` GitHub organization has
no diff/migration/evolution repository; and `linkml-diff`, `linkml-compare`,
`linkml-migrate` and similar names are unclaimed on PyPI. `gen-sqlddl`
emits `CREATE` only — zero `ALTER`. The nearest adjacent tool,
`linkml-map`, lists "database migrations (one version of a schema to
another)" as a use case, but its transformation spec is authored by hand:
no command derives one from two schemas, and its SQL compiler does
rebuild-and-copy (`CREATE TABLE` plus `INSERT … SELECT`) rather than
`ALTER`. `linkml-dataops` has a differ, but over instance data.

So the delta layer is greenfield as well as the projection layer. LinkML's
contribution is a stable metamodel to diff over, and existing deprecation
metaslots that can carry rename intent (see Slice 5).

**Universal posture worth adopting:** every tool in this space positions a
generated migration as a reviewable draft, never an authoritative artifact.
This feature should say the same thing in its own docs rather than implying
the output is trustworthy unreviewed.

---

## Constraints

These come from how a checksumming migration runner actually behaves, and
they are what make a naive "regenerate the DDL each time" design unsafe.

**Applied migrations are immutable, to the byte.** A runner in this family
hashes each migration's name, version, and *raw* SQL text with no
normalization, stores the hash, and compares on every run. A trailing
newline, a CRLF line ending, or a regenerated timestamp comment changes the
hash. The default behaviour on mismatch is to abort the whole run before
applying anything. Therefore: emitted migrations are append-only immutable
artifacts. The generator must never rewrite a file it has already emitted,
must never re-emit the full schema as a replacement, and must emit nothing
non-deterministic — no timestamps, no tool-version banners, no
iteration-order-dependent output.

**Tolerating a mismatch is worse than failing on one.** The runner's
"ignore divergence" setting downgrades the abort to a log line and then
*skips* the migration, leaving the database on the old schema while the
model says otherwise. Silent drift, permanently. The docs this feature adds
must steer consumers away from that setting, and must note that the
runner's *CLI* ships with the opposite default from its library.

**Appending is safe; inserting is not.** A new migration whose version is
at or below the highest already-applied version is rejected. So the
generator may only ever append the next version, and must never renumber.
A non-contiguous prefix exists for out-of-order insertion if that is ever
needed.

**No non-transactional statements.** Every migration is wrapped in a
transaction and there is no opt-out, so `CREATE INDEX CONCURRENTLY` and
similar cannot appear in generated output. A request for per-migration
opt-out was closed as not-planned upstream. This is a hard ceiling on
zero-downtime index builds and should be stated, not worked around.

---

## Design: two commands over one delta

`diff` is about the *schema*; `migrate` is about the *database*. They share
one delta engine, and each renders it differently — the same
`Reader → IR → Writer` shape the project already uses, with the delta as an
IR-level value and the two commands as writers over it.

The split follows prior art (Atlas separates `schema diff` from
`migrate diff`), but it earns its place for two better reasons. First,
`diff` is format-agnostic and useful to consumers who generate only Rust or
only SHACL and have no database at all — it also fulfils the "comparison"
promise already in the project's own one-line description, which currently
has nothing behind it. Second, it quarantines the hardest problem:
`diff` never has to *resolve* rename-versus-drop/add, because reporting the
ambiguity is a correct answer for a report. Only `migrate` must commit to an
interpretation, so only `migrate` needs the hint mechanism — which lets the
diff side ship complete while that design is still being argued.

---

## Vertical Slices

### Slice 1: `migrate` emits the initial migration

**Status:** Complete

**Priority:** Must Have

**User Value:** A consumer whose schema has never been applied to a
database gets its first migration file, in the layout a runner expects,
without hand-writing DDL or hand-naming files.

**Acceptance Criteria:**
- [x] `panschema migrate --schema <file> --migrations <dir>` writes the
  schema's full DDL as the first versioned migration in that directory, and
  reports the path it wrote.
- [x] The emitted file's name carries a version and a descriptive name in
  the layout a versioned runner discovers, and the version is the first.
- [x] Running the same command twice against an unchanged schema is a
  no-op that reports the migration already exists — it does not write a
  second file and does not rewrite the first.
- [x] The emitted SQL is byte-identical across runs and across machines:
  regenerating into an empty directory from the same schema produces the
  same bytes, with no timestamp, tool version, or other varying content.
- [x] Emitting into a directory that already contains migrations refuses
  rather than guessing, and says the directory is not empty.
- [x] A manifest can declare a migrations directory for a schema, so a
  manifest-driven run emits migrations alongside its other outputs.
- [x] `--help` states that the command writes files and never connects to
  a database.
- [x] ADR 010 records the declarative-source / versioned-artifact hybrid
  and the append-only immutability rule.

**Notes:**
- The DDL body is the existing Postgres projection; this slice is about
  file identity, determinism, and refusal behaviour, not new SQL.
- A consumer whose database already exists (tables created by some other
  tool) needs a baseline story — adopt-existing-database is deliberately
  out of scope here and is called out in *Open Questions*.
- **"A manifest-driven run" is `panschema migrate` with no `--schema`, not
  `generate`.** The manifest declares the directory in the same
  `[generate.<name>]` block as every other output, but `generate` skips the
  key. Folding an append-only artifact into the regenerate-everything
  command would make either `generate` refuse on its second run or docs
  builds append migrations as a side effect. ADR 010 decision 4 records the
  reasoning.
- The refusal and the no-op are distinguished by content, not by filename
  alone: a directory holding exactly the migration this schema would write,
  byte for byte, is the no-op; anything else refuses.

---

### Slice 2: `diff` reports a semantic schema delta

**Status:** Not started — deliberately parked, do not pick this up next

> Slices 2–6 are blocked behind Postgres writer coverage, not by anything in
> this feature. Feature 24 skips a class with a multivalued slot, which on a
> real consumer schema means eight of seventeen classes get no table — and
> those eight are the core of the domain. Building a delta engine and an
> incremental migration on a projection that drops half the model produces
> work that has to be unwound once the projection is fixed.
>
> Prerequisites, in order: feature 24 slice 4 (multivalued scalars as array
> columns), then slice 5 (multivalued class ranges as linking tables). Two
> independent consumers hit this same gap, so it is structural rather than
> one schema's problem.
>
> One scope change also falls out of it. Adopt-existing-database is listed
> under *Open Questions* as deliberately out of scope, but the consumer this
> feature exists for already has tables, named differently from what the
> schema projects (singular vs plural), plus tables the schema will never own.
> For them a baseline is the only route in, so it needs to become a slice
> rather than stay a question.

**Priority:** Must Have

**User Value:** An author can see what changed between two versions of a
schema in schema terms — classes, slots, enums, ranges, cardinality — for
any schema, whether or not a database is involved.

**Acceptance Criteria:**
- [ ] `panschema diff <old> <new>` reports the structural delta between two
  schema files: elements added, removed, and changed, naming the element
  and, for a change, what differed.
- [ ] The delta covers classes, slots (including inherited and mixed-in
  effective slots), enums and their permissible values, types, and
  slot-level facets (range, cardinality, required, pattern, bounds).
- [ ] Comparing a schema against itself reports no changes and exits zero.
- [ ] The old side can be named as a version-control reference rather than
  a file, so an author can diff the working tree against a released tag.
- [ ] A removed element and an added element that look like a rename are
  reported as a possible rename, without the report committing to that
  interpretation.
- [ ] Both a human-readable report and a machine-readable form are
  available, so the delta can be consumed by other tooling.
- [ ] Reading either side through the shared load path means an unreadable
  or invalid schema fails the same way it does elsewhere.

**Notes:**
- Format-agnostic by construction: both sides are read to the IR first, so
  a Turtle ontology and a LinkML YAML file are comparable.
- Extracting a schema at a version-control reference reuses the existing
  extraction the publish pipeline already performs.

---

### Slice 3: `diff` classifies compatibility

**Status:** Not started

**Priority:** Should Have

**User Value:** A CI job, or an author choosing a version bump, gets a
verdict rather than a list — is this change safe for consumers, safe for
existing data, or breaking?

**Acceptance Criteria:**
- [ ] Each change in the delta carries a classification: compatible,
  breaking for consumers of generated artifacts, or breaking for existing
  data.
- [ ] The report states the strongest classification present as an overall
  verdict.
- [ ] `--strict` exits non-zero when any breaking change is present, so a
  CI job can gate on it.
- [ ] The verdict names the version bump it implies, in the same vocabulary
  the release command already accepts.
- [ ] A change whose compatibility cannot be determined is reported as
  such rather than silently classified as safe.

**Notes:**
- Widening a range or adding an optional slot is compatible; narrowing,
  removing, or making something required is not. Enum value removal breaks
  data; enum value addition does not.
- This is the diagnostic half of what the roadmap has carried as "schema
  diff / compatibility checks".

---

### Slice 4: `migrate` emits an incremental migration

**Status:** Not started

**Priority:** Must Have

**User Value:** Editing the schema produces the next migration — the
`ALTER` statements that move a database matching the previous version to
one matching the new one — instead of requiring the author to write them.

**Acceptance Criteria:**
- [ ] `panschema migrate` against a schema whose previous version is
  identifiable emits exactly one new migration file, versioned one above
  the highest already present, containing only the delta's DDL.
- [ ] Added classes, added slots, added enum values, and relaxed
  constraints project to the corresponding statements; a change the
  projection cannot express is reported as an unsupported change rather
  than silently dropped, naming the element and what could not be
  expressed.
- [ ] Data-destroying statements are withheld unless explicitly allowed,
  and the refusal names what would be lost.
- [ ] Existing migration files are never modified or renumbered; a run
  that would need to insert below the highest version refuses and explains
  why.
- [ ] An unchanged schema emits no new migration and says so.
- [ ] Applying the emitted migration sequence in order against a real
  Postgres database produces the same catalog state as applying the head
  schema's full DDL to an empty database — verified against a real
  database, not by comparing DDL text.
- [ ] Generated output contains no statement that cannot run inside a
  transaction.

**Notes:**
- The equivalence check compares introspected catalog state, not DDL text,
  so it is immune to formatting churn. Prior art (Atlas, and one Rust tool)
  implements exactly this by replaying onto a throwaway database; this
  reuses the project's existing real-Postgres apply harness.
- The previous version comes from the migrations directory plus the schema
  snapshot the previous migration was generated from; how that snapshot is
  stored is an implementation decision recorded in ADR 010.

---

### Slice 5: renames are declared, not guessed

**Status:** Not started

**Priority:** Should Have

**User Value:** Renaming a slot or class preserves its data instead of
dropping and recreating it, because the author declares the rename in the
schema and the generator honours it.

**Acceptance Criteria:**
- [ ] A rename declared in the schema projects to a rename statement, and
  the data in the affected column or table survives the migration.
- [ ] Without a declaration, a removed element plus an added element
  remains a drop and an add — the generator never infers a rename from
  name or shape similarity.
- [ ] `diff` surfaces the possible-rename case (Slice 2) with the
  declaration that would resolve it, so the report tells the author how to
  fix it.
- [ ] A declaration that names an element which does not exist on the
  relevant side is reported as an error rather than ignored.
- [ ] Data preservation is verified by seeding rows into the replayed
  database and confirming they survive with their values intact — not by
  catalog comparison alone.

**Notes:**
- Hint annotations are chosen over interactive prompts: prompts cannot work
  in CI, and a hint carried in the schema survives regeneration. Prior art
  for both exists (`sqldef` uses in-file annotations; Atlas prompts).
- LinkML already defines `deprecated`,
  `deprecated_element_has_exact_replacement`, and
  `deprecated_element_has_possible_replacement`. The exact-replacement
  metaslot is the natural carrier but is single-valued with a
  `uriorcurie` range and has no inverse, so whether to use it directly or
  carry the hint in `annotations` is an *Open Question* to settle before
  this slice.
- **This slice exists because the standard verification cannot catch this
  bug.** Catalog-state equivalence (Slice 4) passes identically for a
  data-preserving rename and a data-destroying drop-plus-add — both reach
  the same final schema. That is why the oracle here is seeded data, and
  why this is the one place the feature must go beyond what prior art
  verifies.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: initial migration | Must Have | — | Not started |
| Slice 2: semantic schema delta | Must Have | — (independent of 1) | Not started |
| Slice 3: compatibility classification | Should Have | Slice 2 | Not started |
| Slice 4: incremental migration | Must Have | Slices 1–2 | Not started |
| Slice 5: declared renames | Should Have | Slice 4 | Not started |

---

## Things to Watch

- **Determinism is a test obligation, not a hope.** Any map iteration,
  hash ordering, or set traversal that reaches emitted output will produce
  files that differ between runs and break a checksumming runner on the
  next deploy. The byte-identity criterion in Slice 1 is the guard, and it
  needs to stay green as later slices add statement kinds.
- **The expressiveness ceiling is real.** A delta can describe changes the
  DDL projection cannot express, exactly as a model-driven tool's
  generated migrations are limited to what the model can say. The
  loud-about-gaps convention applies: report the unsupported change, name
  the element, never drop it silently.
- **The generated migration is a draft.** Docs should say so plainly, in
  line with how every comparable tool positions its output.
- **Consumer-facing runner pitfalls belong in the guide**, since they will
  bite users of this feature even though they are not panschema bugs: the
  ignore-divergence setting causing silent drift, the runner CLI's default
  differing from its library's, and the inability to run statements
  outside a transaction.

## Open Questions

- **Where does the rename hint live** — an existing LinkML deprecation
  metaslot, or an annotation? The existing metaslot is semantically apt but
  single-valued and `uriorcurie`-ranged, pointing at a URI rather than a
  slot name, with no inverse.
- **How is the previous schema version identified** — a snapshot committed
  beside the migrations, a version-control reference recorded per
  migration, or introspection of a live database? A snapshot keeps the
  whole feature database-free, which is the differentiator worth
  protecting.
- **Adopting an existing database.** A consumer whose tables were created
  by another tool needs a baseline migration that records "the schema is
  already at version N" without re-applying it. Prior art calls this a
  baseline; it is unaddressed here.
- **Should `migrate` ever be allowed to emit a squashed replacement?** One
  Rust tool offers a compaction command that rewrites history into a single
  migration. That directly conflicts with a checksumming runner, so the
  default answer is no — but a fresh-start-before-first-deploy case may
  justify it behind an explicit flag.
