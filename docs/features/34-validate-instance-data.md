# Feature 34: `panschema validate --data` — native instance-data validator

**Feature:** A `validate` subcommand that checks a LinkML **instance-data**
file (an A-box) against its schema's constraints and reports every violation,
exiting non-zero when the data doesn't conform. `panschema validate --input
schema.yaml --data data.yaml` walks each record against its class's effective
slots — required/cardinality, range type, enum membership, `pattern`, numeric
bounds — plus cross-record reference integrity.

**User Story:** As someone building a graphRAG application (or an LLM agent
constructing an instance graph), I want to validate a LinkML data file against
the schema and get a precise, per-record list of what's wrong — so the agent
loop, or a human author, can fix the data until it conforms, staying entirely
in LinkML + JSON.

**Related ADR:** [003 (LinkML as internal representation)](../adr/003-linkml-as-internal-representation.md).
Consumes the instance model and reference-integrity check from
[feature 33](33-linkml-instance-reader.md); the constraint set it enforces is
the same one the [JSON-Schema writer](32-json-schema-writer.md) projects and the
class/slot cards document.

**Approach:** Vertical Slicing with Outside-In TDD.

---

## Design decisions

### A native validator, not the generated JSON Schema

An obvious shortcut is to validate the data JSON against the schema the
[JSON-Schema writer](32-json-schema-writer.md) emits, using the `jsonschema`
crate. It's rejected for now:

- The JSON-Schema writer is **incomplete**: enum ranges, `pattern`,
  `minimum_value`/`maximum_value`, and class-ranged references aren't projected
  yet (a non-scalar range emits the permissive "any" schema). Validating
  through it would silently pass data that violates exactly the constraints a
  validator most needs to catch.
- It would promote `jsonschema` from a dev-dependency to a runtime dependency
  for a check that's still a subset of the IR's constraints.

A **native validator over the IR** enforces the full constraint set directly,
reuses the effective-slot resolver and the reference-integrity check already
built, and stays the single source of truth as the JSON-Schema writer catches
up. (Once that writer is complete, validating generated-JSON-Schema-against-data
becomes a valuable *cross-check oracle* in the test suite — a follow-up, not the
product path.)

### Validate the raw data tree, not the display `InstanceSet`

`InstanceSet` (feature 33) is a *display* model: it stringifies literal values
and drops their original scalar type, which a `pattern`/bounds/enum check needs.
The validator walks the **raw `serde_yaml` data tree** against the schema's
effective slots, and reuses the `InstanceSet` only for the cross-record
reference-integrity pass (`dangling_instance_references`), where stringified
ids are sufficient. Extracting a shared container-walk that both the graph
exporter and the validator drive is a later refactor, not a prerequisite.

### Exit-code semantics

A validator that only warns isn't a validator. `validate` reports **every**
violation it finds (not just the first), then exits **non-zero** if the data
has any violation and **zero** when it conforms — so CI and an agent loop can
branch on the exit code. There is no `--strict`: validation is inherently
strict. (`generate --instances --strict` keeps its warn-or-fail behavior for
the *rendering* path; `validate` is the dedicated conformance gate.)

---

## Vertical Slices

### Slice 1: Walking skeleton — the command, required-presence + reference integrity

**Status:** Complete

**Priority:** Must Have

**User Value:** `panschema validate --input schema.yaml --data data.yaml`
reports missing required slots and dangling references per record and exits
non-zero when the data doesn't conform, zero when it does.

**Acceptance Criteria:**
- [x] A `validate` subcommand takes `--input <schema>` and `--data <instance-file>`, reads both, and walks each record in the `tree_root` container against its class's effective slots.
- [x] A required slot absent from a record is reported as a violation naming the record, its class, and the missing slot; a reference whose target names no record in the data is reported naming the record, the property, and the missing id (reusing `diagnostics::dangling_instance_references`).
- [x] Every violation is printed; the command exits non-zero if there is at least one and zero when the data fully conforms. A data file that isn't a mapping yields a single structural violation rather than panicking.
- [x] Tests: a conforming data file validates clean (exit zero); a missing-required-slot and a dangling-reference case each fail (unit tests + a CLI exit-code integration test). An identifier supplied as an identifier-keyed collection's map key satisfies its required identifier slot.

### Slice 2: Cardinality and range-kind checks

**Status:** Not Started

**Priority:** Must Have

**Depends on:** Slice 1.

**User Value:** A single value where the schema expects a list (or vice versa),
a collection outside its `minimum_cardinality`/`maximum_cardinality`, or a
class-ranged slot holding a bare scalar that isn't a valid reference, is caught.

**Acceptance Criteria:**
- [ ] A non-multivalued slot given a list, or a multivalued slot's collection outside its effective cardinality bounds, is a violation naming the record, slot, and the bound it broke.
- [ ] A value's *kind* is checked against the slot's range kind: a class-ranged slot value must be a reference id or an inlined object (not a bare non-reference scalar where the range is a class), and a scalar-ranged slot must not hold a mapping.
- [ ] Tests cover each: too-many/too-few, single-vs-list mismatch, and a kind mismatch.

### Slice 3: Value-constraint checks — enum, pattern, numeric bounds

**Status:** Not Started

**Priority:** Must Have

**Depends on:** Slice 2.

**User Value:** A value that isn't a permissible enum value, doesn't match a
slot's `pattern`, or falls outside `minimum_value`/`maximum_value` is caught —
the constraints the class/slot cards advertise are now enforced against data.

**Acceptance Criteria:**
- [ ] An enum-ranged value that isn't one of the range enum's permissible values is a violation naming the record, slot, the value, and (briefly) the allowed set.
- [ ] A string value that doesn't match its slot's `pattern`, and a numeric value outside `minimum_value`/`maximum_value`, are each violations.
- [ ] A value whose scalar type can't satisfy a numeric-bounded or pattern-constrained slot is reported rather than panicking.
- [ ] Tests cover an out-of-enum value, a pattern miss, and an out-of-bounds number.

### Slice 4: Identifier uniqueness and `any_of` ranges

**Status:** Not Started

**Priority:** Should Have

**Depends on:** Slice 3.

**User Value:** Two records sharing an identifier, or a polymorphic `any_of`
value matching none of its branches, is caught — the last common conformance
gaps for agent-built data.

**Acceptance Criteria:**
- [ ] Two records of the same class with the same identifier value are reported as a duplicate-identifier violation.
- [ ] A value at an `any_of`-ranged slot that satisfies none of the branch ranges is a violation; one that satisfies at least one branch passes.
- [ ] Tests cover a duplicate identifier and an `any_of` miss/hit.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: command + required-presence + reference integrity | Must Have | — | Complete |
| Slice 2: cardinality + range-kind | Must Have | Slice 1 | Not Started |
| Slice 3: enum + pattern + numeric bounds | Must Have | Slice 2 | Not Started |
| Slice 4: identifier uniqueness + `any_of` | Should Have | Slice 3 | Not Started |

## Definition of Done

- [ ] Slices 1–3 met (slice 4 recommended); `validate` enforces the constraint set the JSON-Schema writer and the class/slot cards describe.
- [ ] `cargo nextest run` green; `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo doc`.
- [ ] A conforming and a non-conforming checked-in fixture prove the exit-code contract end-to-end.
- [ ] README.md + CHANGELOG.md updated; [linkml-coverage.md](../linkml-coverage.md) notes instance-data validation.
