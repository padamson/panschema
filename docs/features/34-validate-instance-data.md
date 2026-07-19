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

**Related ADR:** [003 (LinkML as internal representation)](../adr/003-linkml-as-internal-representation.md)
and [008 (Instance-data reader architecture)](../adr/008-instance-data-reader-architecture.md) — the
validator consumes the instance model, so any A-box format validates through one path.
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

### Validate the instance model, not the on-disk format ([ADR-008](../adr/008-instance-data-reader-architecture.md))

Like a schema in any format becomes the `SchemaDefinition` IR before anything
consumes it (ADR-004), instance data in any format becomes the `InstanceSet`
model before validation. The validator has two layers:

- `validate_instances(schema, &InstanceSet)` — the **format-agnostic core**. It
  checks each record's typed, slot-keyed `slot_values` against its class's
  effective-slot constraints, plus reference integrity. Any reader's
  `InstanceSet` — LinkML data, OWL individuals, future JSON — validates through
  it.
- `validate_instance_data(schema, &yaml)` — the LinkML **adapter**: it handles
  structural errors, builds the `InstanceSet` via `from_linkml_data`, then calls
  the core.

To make this work, `Instance` was enriched (ADR-008) with `slot_values` — the
complete authored assignments keyed by slot *name* and *typed* (`Scalar` /
`Reference`), which the earlier display-only `literals` (stringified,
label-keyed) couldn't serve. The display fields remain a projection alongside
it.

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

### Slice 2: Cardinality checks

**Status:** Complete

**Priority:** Must Have

**Depends on:** Slice 1.

**User Value:** A single value where the schema expects one (a list given to a
single-valued slot), or a collection outside its
`minimum_cardinality`/`maximum_cardinality`, is caught.

**Acceptance Criteria:**
- [x] A non-multivalued slot given more than one value is a violation naming the record and slot; a multivalued slot's value count below `minimum_cardinality` or above `maximum_cardinality` is a violation naming the bound it broke. Counts come from the model's `slot_values` (so a YAML list on a single-valued slot is seen as N values).
- [x] Tests cover single-valued-given-a-list, below-minimum, and above-maximum, plus a conforming `2..3` case.

**Note — range-kind is deferred.** The other half of a "value kind" check — a
mapping where a scalar range is declared, or a non-identifier scalar where a
class range is declared — isn't cleanly detectable from the model today: the
LinkML reader *drops* a value it can't interpret at a slot's range kind, so it
never reaches `slot_values` (it surfaces indirectly as an absent required slot).
Catching it precisely needs the reader to *preserve* mismatched values (a small
model addition — an "unrecognized value" it records rather than drops). Split
into its own slice below rather than bundled here.

### Slice 2b: Range-kind mismatch (reader preserves dropped values)

**Status:** Complete

**Priority:** Should Have

**Depends on:** Slice 2.

**User Value:** A value of the wrong *kind* for its slot's range — an object
where a scalar is expected, or a non-identifier scalar where a class reference
is expected — is reported precisely, not just as a downstream "absent" symptom.

**Acceptance Criteria:**
- [x] The instance reader records a value it can't interpret at a slot's range kind as `InstanceValue::Unexpected(kind)` (rather than dropping it), keeping it out of the display `literals`/`references` so the instance graph is unchanged.
- [x] A mapping at a scalar-ranged slot, and a non-reference scalar (a number) at a class-ranged slot, are each violations naming the record, slot, the actual kind, and the declared range.
- [x] Tests cover both mismatches; the instance-graph e2e confirms display output is unaffected.

### Slice 3: Value-constraint checks — enum membership, numeric bounds

**Status:** Complete

**Priority:** Must Have

**Depends on:** Slice 2.

**User Value:** A value that isn't a permissible enum value, or that falls
outside `minimum_value`/`maximum_value`, is caught — the constraints the
class/slot cards advertise are now enforced against data.

**Acceptance Criteria:**
- [x] An enum-ranged value that isn't one of the range enum's permissible values (matched against the value key or its `text`) is a violation naming the record, slot, the value, and the enum.
- [x] A numeric value below `minimum_value` or above `maximum_value` is a violation; a non-numeric value at a numeric-bounded slot is reported (not panicked).
- [x] Tests cover an out-of-enum value, below-minimum, above-maximum, a non-numeric-at-bounded-slot, and a conforming case. Both checks read the typed `slot_values`, so no re-parsing.

**Note — `pattern` split out.** `pattern` validation needs a regex engine, which
isn't a direct dependency yet; adding one carries a supply-chain cost, so it is
its own slice (3b) rather than bundled here.

### Slice 3b: `pattern` validation (adds a regex dependency)

**Status:** Complete

**Priority:** Should Have

**Depends on:** Slice 3.

**User Value:** A string value that doesn't match its slot's `pattern` is caught
— the last per-value constraint the slot cards advertise.

**Acceptance Criteria:**
- [x] `regex` is added as a direct dependency (already covered by the cargo-vet audit imports — no new exemptions). A string value not matching its slot's `pattern` is a violation naming the record, slot, and pattern; matching uses partial (`find`) semantics, consistent with panschema's SHACL `sh:pattern` and Postgres `~` projections. An invalid `pattern` in the schema is reported once per slot, not panicked.
- [x] Tests cover a pattern match and a miss.

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
| Slice 2: cardinality | Must Have | Slice 1 | Complete |
| Slice 2b: range-kind mismatch (reader preserves dropped values) | Should Have | Slice 2 | Complete |
| Slice 3: enum membership + numeric bounds | Must Have | Slice 2 | Complete |
| Slice 3b: `pattern` (adds regex dependency) | Should Have | Slice 3 | Complete |
| Slice 4: identifier uniqueness + `any_of` | Should Have | Slice 3 | Not Started |

## Definition of Done

- [ ] Slices 1–3 met (slice 4 recommended); `validate` enforces the constraint set the JSON-Schema writer and the class/slot cards describe.
- [ ] `cargo nextest run` green; `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo doc`.
- [ ] A conforming and a non-conforming checked-in fixture prove the exit-code contract end-to-end.
- [ ] README.md + CHANGELOG.md updated; [linkml-coverage.md](../linkml-coverage.md) notes instance-data validation.
