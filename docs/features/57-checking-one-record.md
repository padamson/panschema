# Feature 57: Checking One Record

**Feature:** `panschema-model` checks one record against its class: its
`rules`, its slot constraints, and the records inlined in it, with the same
findings `verify --data` reports. A service can check a request body before
it writes anything, with no RDF, template, CLI, or async library in its
build.

**User Story:** As the author of a service whose records are defined by a
LinkML schema, I want to check each record a client sends against the
schema's rules and constraints, and reject it with the rule that failed,
so the rules live in the schema alone and never in a second hand-written
copy.

**Related:** [Feature 46](46-panschema-model-crate.md) (the model crate and
its dependency fence), [ADR-011](../adr/011-panschema-model-crate-boundary.md)
(which modules cross into it), [Feature 56](56-testing-declared-claims.md)
(the findings' `kind`, which a program matches on).

**Approach:** Vertical Slicing with Outside-In TDD

---

## Why

`verify --data` already checks records against rules and slot
constraints natively, over the instance model and with no RDF graph, but
the code lives in the `panschema` crate, and a request handler that
depends on `panschema` compiles the RDF stack, the templates, and the CLI
to reach it. It also reads only whole datasets (a `tree_root` container of
records), not the one record a write endpoint receives.

The alternatives each keep a second copy of the rules. A service can
hand-code them, and review then keeps the two copies in step. The
Postgres writer projects most rules as `CHECK` constraints, but a
violation surfaces as a database error rather than a finding naming the
rule, and some rules have no `CHECK` form.

## The shape

    pub fn validate_record(
        schema: &SchemaDefinition,
        class: &str,
        record: &impl serde::Serialize,
    ) -> Result<Vec<Violation>, RecordError>

- **The record is anything `Serialize`:** a `serde_json::Value` from a
  request body, or a struct `panschema generate --format rust` wrote. It
  is read through `serde_norway`, which the crate already depends on, so
  checking JSON adds no JSON library to the model crate.
- **The findings are `verify`'s.** Each `Violation` has the `kind` and
  message `verify --data` prints for the same record. A rule failure
  names the rule by its title, or by its position when it has none
  (`rules::rule_label`), and names the slot whose condition failed.
- **One record, and what it inlines.** A reference to another record is
  not checked: the record arrived alone, so whether the target exists, and
  whether its class fits a union range, is the caller's question to put
  to its own store. A reference to a record inlined in the same body is
  checked, as `verify` checks it. For the same reason, what a record
  holding this one would state is unknown: a bare value at an
  `expand_against` slot whose base a holder states is read as written,
  with no finding, and a `bounds_count_of` bound set by a holder is not
  checked.
- **Read as its JSON reads.** A struct's enum variant, which
  `serde_norway` writes as a YAML tag, is read as the one-key mapping JSON
  writes, and a number `serde_json` carries as text (its
  `arbitrary_precision` feature) as a number, so a struct and its JSON get
  the same findings.
- **A record is not a dataset.** The checks that belong to a container
  (choosing a `tree_root`, the container's id colliding with a record's,
  identifiers unique across the dataset's collections) do not apply.
- **What cannot be checked at all is an error**, not a finding: a class
  the schema does not declare, a value that is not a mapping, or one
  `serde_norway` cannot represent. A finding has a kind `verify` reports,
  and `verify` never reads a lone record.

---

## Vertical Slices

### Slice 1: The validator moves into the model crate

**Status:** Completed

**User Value:** A library that depends only on `panschema-model` can
validate a dataset the way `verify --data` does.

**Acceptance Criteria:**
- [x] `panschema_model::validate::validate_instances` and
  `validate_instance_data` exist, and `panschema::validate` still names
  them.
- [x] `verify --data` reports the same findings for every fixture it
  reported them for before.
- [x] The model crate's dependency check
  (`cargo deny --config deny-model.toml check bans`) passes.

**Notes:**
- The module passes ADR-011's rule: every function in it is a statement
  about the model, and its one new dependency, `regex`, is neither a
  format nor a runtime library.

### Slice 2: One record, by class

**Status:** Completed

**User Value:** A service checks a request body against its class and
returns the failed rules by name.

**Acceptance Criteria:**
- [x] `validate_record` returns no findings for a record that satisfies
  its class's rules and slot constraints.
- [x] A record that fails a rule's postcondition gets a finding naming
  the rule's label and the slot, as `verify --data` reports for the same
  record in a dataset.
- [x] A record that breaks a slot constraint (a required slot absent, a
  value outside its enum, a value outside its bounds or pattern) gets the
  finding `verify --data` reports for it.
- [x] A record inlined in the one checked is checked too, and its
  findings name it by its place.
- [x] A reference to a record outside the one checked is not a finding.
- [x] A `serde_json::Value` and a struct serialized from the same record
  get the same findings, including a struct holding an enum variant.
- [x] A bare `expand_against` value whose base a holder would state is
  not a finding; an expansion gap that is the record's own still is.
- [x] A class the schema does not declare is an error naming the class.
- [x] A value that is not a mapping is an error naming the class.

**Notes:**
- A rule failure's label comes from `ViolationKind::rule_label`.
- A record with no identifier is named `(record)` in its findings, and a
  record inlined in it by its place beneath that (`(record).contents[0]`).
- Each call works out the schema's facts again (effective slots, and
  any `expand_against` and `bounds_count_of` bindings), so its cost grows
  with the schema, not only with the record. A prepared form that works
  them out once per schema is the follow-up, if a service measures the
  need.
- Out of scope: rules that read another record (a reference resolving
  within a scope the caller owns, say). Those need the caller's store.
- Out of scope: checking a record through the SHACL shapes. `verify` is
  the native check, and the gaps between it and the SHACL projection are
  that writer's to close.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1 | Must Have | None | Completed |
| Slice 2 | Must Have | Slice 1 | Completed |
