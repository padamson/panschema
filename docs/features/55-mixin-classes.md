# Feature 55: Mixin Classes

**Feature:** A class declared `mixin: true` is modeled. Its slots already
flow into every class that lists it in `mixins:`; with the flag modeled, it
stops drawing an unmodeled-construct warning, gets no Postgres table, is
always a Rust trait, and wears a "mixin" badge in the schema docs.

**User Story:** As a schema author, I want to put the fields several
record classes share on one mixin class, so each record class takes them
on by listing the mixin, and the generated outputs treat the mixin as the
slot bundle it is rather than as a record type of its own.

**Related:** [Feature 24](24-postgres-ddl-writer.md) (the Postgres writer,
which already flattens mixin slots and emits no table for an abstract
class), [Feature 40](40-agent-facing-skill-that-cannot-drift.md) (the
unmodeled-construct warning `--strict` refuses).

**Approach:** Vertical Slicing with Outside-In TDD

---

## Why

LinkML's metamodel defines `mixin` as a class "intended to be inherited
from without being an is_a parent" (its alias is `trait`). Until now
panschema read the flag as an unknown key: every run warned that `mixin`
"is parsed but not modeled", `--strict` failed on the warning, and the
writers treated the mixin as an ordinary class, so the DDL created a table
for a slot bundle no row is ever inserted into.

## What each target does

The reference implementation decides what the flag means, and each target
decides how that meaning is spelled. `conformance/probe_mixin_flag.py`
measures the reference: `gen-sqlddl`, `gen-shacl`, `gen-json-schema`,
`gen-pydantic` and `gen-owl` emit a table, a shape, a definition, and a
model for a mixin exactly as for an abstract class, and `linkml-validate`
accepts an instance typed as either. LinkML does not enforce either flag;
both state that the class holds no records of its own.

- **Postgres: no table.** A Postgres author repeats shared columns on each
  table and never writes a table that cannot get a row. panschema already
  departs from `gen-sqlddl` this way for `abstract`; a mixin takes the same
  path, including the cascade that skips a class whose slot ranges over a
  class with no table. That reaches a mixin's own slot too: a mixin slot
  ranging over a mixin lands on each mixing class, which is then skipped,
  since the column would reference a table that is never created.
- **Rust: always a trait.** Rust has no inheritance, so a class other
  classes inherit from is a marker trait the inheriting structs implement,
  and a slot ranging over it takes the closed enum of its concrete
  descendants. That role was decided by use alone, so a declared mixin
  nothing listed yet was a struct, and its type changed when the first
  class listed it. A class declared `mixin`, and one declared `abstract`,
  is now a trait from the start, doc-commented as what it is. Until a class
  implements it, its closed enum has no variant: a slot ranging over it can
  hold no value, and a value in the data is refused. This is stricter than
  `verify --data`, which accepts a record typed as the mixin itself; the
  Rust projection already took that position for every class with
  descendants.
- **SHACL: the shape stays.** Shapes are open and one per class, and the
  OWL output asserts each mixing class `rdfs:subClassOf` the mixin. SHACL
  targeting follows `rdfs:subClassOf` in the data graph, so with the
  ontology merged the mixin's shape applies to every mixing record; without
  it, the shape targets nothing.
- **JSON Schema: the definition stays.** A `$defs` entry validates nothing
  until something references it, and a slot whose range is the mixin needs
  it as its `$ref` target. Slots already flatten onto each class.
- **Docs: a badge.** The class card shows "mixin" beside the title, as it
  shows "abstract", so a reader sees where the shared slots come from.

---

## Vertical Slices

### Slice 1: The flag is modeled

**Status:** Completed

**User Value:** A schema with a mixin class passes `verify --strict` and
`generate --strict`, its DDL creates no table for the mixin, and its docs
mark the mixin as one.

**Acceptance Criteria:**
- [x] A class with `mixin: true` draws no unmodeled-construct warning.
- [x] The Postgres DDL creates no table for a mixin class, and the mixing
  classes' tables still carry its columns.
- [x] A class whose slot ranges over a mixin class is skipped with a
  diagnostic saying the range is a mixin with no table of its own; one
  ranging over an abstract class says abstract.
- [x] An enum referenced only by classes that get no table gets no
  `CREATE TYPE`.
- [x] The generated Rust declares a class declared `mixin` or `abstract`
  as a trait, including one no class inherits from yet, and its doc
  comment says which it is.
- [x] A slot ranging over a trait no class implements yet holds no value
  in the generated Rust, and the generated module compiles and refuses a
  value given for it.
- [x] The class card for a mixin class shows a "mixin" badge.
- [x] The SHACL shapes and JSON Schema definitions are unchanged.
- [x] `docs/linkml-coverage.md` has a `mixin` row.

**Notes:**
- `verify --data` accepts a record typed as a mixin class, as
  `linkml-validate` does and as it already does for an abstract class.
- A flag spelled as YAML 1.1 reads it (`mixin: yes`) is a load error, as
  it is for `abstract` and every other boolean in the model; LinkML's
  loader accepts those spellings. Out of scope here, since it is one
  question for every boolean.
- Out of scope: a foreign key for a slot ranging over an abstract or mixin
  class, which would have to reference any of several tables.
- Out of scope: marking a mixin in the schema graph (dimming the node and
  naming it in the hover card, as the graph does for `abstract`), which
  needs a new field in the graph JSON the visualization reads.
- Out of scope: accessor methods on the generated traits, so generic code
  over a trait can read the inherited fields. That would apply to `is_a`
  parents as well as mixins.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1 | Must Have | None | Completed |
