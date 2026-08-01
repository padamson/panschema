# Feature 34: `panschema validate --data` — native instance-data validator

**Feature:** A `validate` subcommand that checks a LinkML **instance-data**
file (an A-box) against its schema's constraints and reports every violation,
exiting non-zero when the data doesn't conform. `panschema validate --schema
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

**User Value:** `panschema validate --schema schema.yaml --data data.yaml`
reports missing required slots and dangling references per record and exits
non-zero when the data doesn't conform, zero when it does.

**Acceptance Criteria:**
- [x] A `validate` subcommand takes `--schema <schema>` and `--data <instance-file>`, reads both, and walks each record in the `tree_root` container against its class's effective slots.
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

### Slice 4: Identifier uniqueness

**Status:** Complete

**Priority:** Should Have

**Depends on:** Slice 3.

**User Value:** Two records sharing an identifier is caught — a common
agent-data bug.

**Acceptance Criteria:**
- [x] Two top-level (collection) records that claim the same identifier are reported as a duplicate-identifier violation. The reader dedupes records by id for display, so it records the collision in `InstanceSet.duplicate_ids` for the validator to read.
- [x] The same entity inlined in one place and listed as a top-level record (one entity referenced two ways, sharing an id) is *not* a duplicate — only two distinct top-level records are.
- [x] Tests cover a duplicate identifier and the inlined-same-entity non-duplicate case.

### Slice 4b: `any_of` polymorphic ranges — reader and validator

**Status:** Complete

**Priority:** Should Have

**Depends on:** Slice 4.

**User Value:** A slot whose range is an `any_of` class union carries real
references — so the instance graph draws edges, RDF asserts object
properties, integrity checks fire, and a value pointing at the wrong kind of
record is caught. Before this the union was invisible to the reader: with no
outer `range:` to read, values fell through to the schema's `default_range`
and ingested as string literals, so none of those checks could run.

**Acceptance Criteria:**
- [x] A value at a slot whose range is a union of classes ingests as a
  reference, whether the union is declared on the slot or narrowed onto a
  subclass through `slot_usage` — including when the narrowing is itself a
  union (`un_narrowed_any_of_union_values_ingest_as_references`,
  `slot_usage_any_of_narrowing_ingests_references`).
- [x] A reference at a union-ranged slot whose target's class is none of the
  union members is a violation naming the permitted classes; a target whose
  class *descends from* a member through `is_a` conforms
  (`a_union_reference_outside_the_permitted_classes_is_a_violation`,
  `a_union_reference_to_a_subclass_of_a_permitted_class_conforms`).
- [x] A reference naming no record in the set yields exactly one report — the
  integrity pass's — not a second from the branch check
  (`a_dangling_union_reference_is_reported_once`).
- [x] A value that can be neither reference nor literal at a union slot names
  the permitted classes rather than an unknown range
  (`an_unusable_value_at_a_union_slot_names_the_permitted_classes`).
- [x] The RDF family and `instance-graph-json` carry union-slot references as
  object-property assertions and labelled edges
  (`a_union_ranged_slot_emits_an_object_property_assertion`,
  `a_union_ranged_slot_becomes_an_assertion_edge`).

**Notes:**
- **Ingestion policy.** A union whose members are *all* classes makes string
  values references. A union mixing classes with types or enums keeps strings
  as scalars, since a string could legitimately be either — displaying edges
  only for all-class unions is a documented limitation, not an oversight. An
  inlined object is built as a record only when exactly one member is a
  class; with several it is ambiguous which was meant, and the value is
  reported rather than guessed.
- Branch checking applies to unions only. A single class range is the slot's
  own declared range and is left to the existing checks.
- Union membership walks `is_a`, not mixins: a union branch names a class an
  instance is expected to *be*.

### Slice 5: The conformance check runs on the way into an output

**Status:** Complete

**Priority:** Must Have

**Depends on:** Slice 1.

**User Value:** An A-box embedded in generated docs or a published site is
held to the same conformance bar as one checked by the standalone command, so
a violating exemplar can't reach a deployed page unnoticed.

**Acceptance Criteria:**
- [x] Supplying instance data to an output reports every violation the
  standalone validator reports, not only dangling references — a duplicate
  identifier, a missing required slot, a bad enum value all surface
  (`generate_reports_conformance_violations_in_the_instance_data`).
- [x] `--strict` fails the build on any violation, matching the existing
  reference-integrity behaviour it subsumes.
- [x] `publish` applies the same check per version, naming the version, and
  reports rather than aborts — a note on an old tag must not make it
  unpublishable.
- [x] One code path decides what "conforming" means, so the standalone
  command and the embedding paths cannot drift.

### Slice 6: A field the class doesn't declare is a violation

**Status:** Complete

**Priority:** Must Have

**Depends on:** Slice 5.

**User Value:** A misspelled slot name is reported instead of quietly becoming
a new ontology property. Before this, `colour` on a class declaring `color`
validated clean, rendered in the docs, and emitted as
`<…#colour> "red"` in RDF — inventing a property in the schema's own namespace
while the slot actually meant stayed absent.

**Acceptance Criteria:**
- [x] A field the record's class doesn't declare is a violation naming the
  record, the field, and the class, and saying what becomes of it — it renders
  and emits rather than being dropped
  (`a_field_the_class_does_not_declare_is_a_violation`).
- [x] It reaches every path the conformance check reaches: the standalone
  command, `generate --instances`, and `publish`; `--strict` fails.
- [x] Reported per record, so the same misspelling in several records names
  each one.
- [x] No false positives on a real hand-authored A-box (verified against a
  downstream consumer's curated data files, which continue to conform).

---

### Slice 7: Class-level `rules` are enforced

**Status:** Complete

**Priority:** Should Have

**Depends on:** Slice 5.

**User Value:** A conditional requirement written as a LinkML `rule` is
machine-checked by the same command that checks everything else. Before this,
a rule rendered in the docs and projected to SHACL but `validate` skipped it,
so the only way to check one was to run an external SHACL engine over
generated shapes — real, but not a single-tool check.

**Acceptance Criteria:**
- [x] A record whose class carries a rule whose precondition holds, and which
  fails the postcondition, is a violation naming the rule, the class, and the
  slot that failed
  (`a_record_failing_a_rules_postcondition_is_a_violation`).
- [x] A record whose precondition does *not* hold is left alone — the point of
  a conditional requirement is that the unconditioned case may omit what the
  conditioned case must carry
  (`a_record_whose_precondition_does_not_hold_is_left_alone`).
- [x] A record satisfying the postcondition conforms
  (`a_record_satisfying_a_rules_postcondition_conforms`).
- [x] The condition facets the SHACL projection already covers are enforced
  here too, so the two checks agree on what a rule means: `equals_string` /
  `equals_number`, `value_presence`, `required`, and both `any_of` forms
  (whole-condition alternatives and single-slot value alternatives). Numeric
  bounds, `pattern`, and cardinality inside a condition are enforced as well.
- [x] A rule is named by its `title`, or by its 1-based position when
  untitled (`an_untitled_rule_is_named_by_its_position`).
- [x] It reaches every path the conformance check reaches, since it runs
  inside the shared core: the standalone command, `generate --instances`, and
  `publish`; `--strict` fails.

**Notes:**
- A `range:` inside a slot condition is a type assertion rather than a value
  test and is not evaluated; the slot's own declared range is already checked
  for every record.
- Rules are read off the class directly, matching every other projection of
  `rules` — inherited rules are not resolved by any of them, so this
  introduces no new inheritance semantics.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: command + required-presence + reference integrity | Must Have | — | Complete |
| Slice 2: cardinality | Must Have | Slice 1 | Complete |
| Slice 2b: range-kind mismatch (reader preserves dropped values) | Should Have | Slice 2 | Complete |
| Slice 3: enum membership + numeric bounds | Must Have | Slice 2 | Complete |
| Slice 3b: `pattern` (adds regex dependency) | Should Have | Slice 3 | Complete |
| Slice 4: identifier uniqueness | Should Have | Slice 3 | Complete |
| Slice 4b: `any_of` polymorphic ranges (reader + validator) | Should Have | Slice 4 | Complete |
| Slice 5: conformance check on the way into an output | Must Have | Slice 1 | Complete |
| Slice 6: undeclared fields are violations | Must Have | Slice 5 | Complete |
| Slice 7: class-level `rules` enforced | Should Have | Slice 5 | Complete |

## Definition of Done

- [ ] Slices 1–3 met (slice 4 recommended); `validate` enforces the constraint set the JSON-Schema writer and the class/slot cards describe.
- [ ] `cargo nextest run` green; `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo doc`.
- [ ] A conforming and a non-conforming checked-in fixture prove the exit-code contract end-to-end.
- [ ] README.md + CHANGELOG.md updated; [linkml-coverage.md](../linkml-coverage.md) notes instance-data validation.
