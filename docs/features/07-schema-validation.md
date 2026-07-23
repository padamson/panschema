# Schema Validation - Implementation Plan

**Feature:** `panschema validate` subcommand for LinkML metaschema validation

**User Story:** As a schema author, I want `panschema validate schema.yaml` to check my LinkML schema against the LinkML metaschema and surface actionable diagnostics, so I catch structural mistakes before downstream consumers encounter them.

**Related ADR (if applicable):** None — extends the CLI surface defined in [feature 05](05-schema-manager.md) and the reader pipeline defined in [feature 03](03-reader-writer-architecture.md).

**Approach:** Vertical Slicing with Outside-In TDD

---

## Why Now

The CLI's tagline ("a universal CLI for schema conversion, documentation, validation, and comparison") promises validation, but no `validate` subcommand exists. The closest existing surface is `panschema verify`, which only checks lockfile checksum drift — it doesn't read the schema's content.

Surfaced by the scimantic-schema v0.2.0 dogfood: a schema author working in YAML benefits from a single command that says "this is a well-formed LinkML schema" or "this slot has a range that isn't a declared class / type / enum."

This is the smallest credible step toward the broader "comparison" surface promised in the tagline (which would require a `diff` subcommand and is out of scope for v0.4.0).

---

## Implementation Strategy

The LinkML metaschema is itself a LinkML YAML file (publicly maintained at `linkml/linkml-model`). Validation is structural: walk the input schema, check that every reference (class `is_a`, class `mixins`, slot `range`, slot `domain`, etc.) resolves to a declared entity, that required metaschema fields are present, and that value types match the metaschema's declared ranges.

For v0.4.0 we don't need to be a full LinkML metaschema implementation. The valuable subset is the "any reference is unresolvable" / "any required field is missing" class of error — the ones that already corrupt downstream codegen and produce confusing errors.

---

## Vertical Slices

### Slice 1: `panschema validate` CLI surface + unresolved-reference checks

**Status:** Not Started

**User Value:** Running `panschema validate schema.yaml` exits 0 on a well-formed schema and exits non-zero with a precise diagnostic when a reference doesn't resolve.

**Acceptance Criteria:**
- [ ] New `Validate { input: PathBuf }` variant on `Commands` (clap). Same input semantics as `generate --schema`: a single LinkML YAML or TTL file.
- [ ] Reader dispatch via `FormatRegistry` (consistent with `generate`).
- [ ] Walks the resolved schema and reports diagnostics for:
  - A class's `is_a` parent that isn't declared as a class.
  - A class's `mixins:` entry that isn't declared as a class.
  - A slot's `range:` that isn't a declared class, enum, type, or known primitive.
  - A class's `slots:` entry that isn't declared in `schema.slots`, `class.attributes`, or `class.slot_usage`.
  - An `any_of:` branch whose `range:` (or the slot's outer `range:` fallback) doesn't resolve.
- [ ] Diagnostics include the entity path (`Question.hasInput.any_of[1].range`) and a short message.
- [ ] Exit code 0 on no diagnostics, 1 on one or more diagnostics.
- [ ] Integration test against a fixture with each diagnostic kind.

**Notes:**
- The existing `rust_writer` slice 6.6 already does some of these checks inline in the generator (`// WARNING:` comments for unresolved global slot refs). `validate` is the explicit, fail-loud version of the same checks, applied at schema-load time rather than codegen time. Sharing helpers is in scope; introducing a new `Diagnostic` type from scratch is too.
- Don't try to be the LinkML metaschema validator in this slice. The metaschema covers ~60 classes and many constraints panschema doesn't yet emit anyway; chasing full conformance is its own feature.

---

### Slice 2: Metaschema-driven structural checks

**Status:** Not Started

**User Value:** `panschema validate` also catches structural mistakes the basic ref-check can't see (e.g. `permissible_values:` declared on a class instead of an enum, `range:` declared on a class definition, mistyped LinkML metaschema fields).

**Acceptance Criteria:**
- [ ] Vendor or pin the LinkML metaschema (or a subset) in `panschema/src/` and use it as the source of truth for "what fields are allowed where."
- [ ] Each diagnostic from slice 1 is upgraded with a metaschema source link or rule ID.
- [ ] New diagnostic kinds: "field X not allowed on Y," "field X has wrong type" (e.g. `multivalued: "true"` instead of `multivalued: true`).

**Notes:**
- This slice depends on a vendored or fetched LinkML metaschema. Decide between vendoring (deterministic, version-locked to a specific metaschema) vs fetching (always current, but introduces a network dependency).

---

### Slice 3 (optional): `panschema verify --strict` includes validation pass

**Status:** Not Started

**User Value:** A consumer's CI step that runs `panschema verify` (checksum drift) can also gate on schema validity in one call.

**Acceptance Criteria:**
- [ ] `verify --strict` runs validation against every manifested schema after checksum verification. Exit non-zero on either drift or diagnostic.

**Notes:**
- Avoid by default — drift and validity are separate concerns. Flag-gated additive behavior is the minimum-friction integration.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: CLI + ref-resolution checks | Must Have | Feature 03 | Not Started |
| Slice 2: Metaschema-driven checks | Should Have | Slice 1 | Not Started |
| Slice 3: `verify --strict` integration | Could Have | Slice 1 + Feature 05 | Not Started |

---

## Definition of Done

The feature is complete when ALL of the following are true:

- [ ] All slice 1 acceptance criteria met (slice 2 is "should have" for v0.4.0; slice 3 is optional).
- [ ] All tests passing: `cargo nextest run`
- [ ] Library documentation complete: `cargo doc`
- [ ] Code formatted + clippy clean: `cargo fmt --check` + `cargo clippy -- -D warnings`
- [ ] README.md updated with `panschema validate` example
- [ ] CHANGELOG.md updated
- [ ] scimantic-schema's v0.2.0 (and any subsequent schema with non-trivial mixins / external CURIEs) validates clean
