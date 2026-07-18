# Feature 32: JSON Schema writer

**Feature:** A new `json-schema` output format: `generate --format json-schema`
emits a [JSON Schema](https://json-schema.org/) (draft 2020-12) from the LinkML
schema — one `object` schema per class, with typed properties, required lists,
enums, `$ref`s between classes, and value constraints. It is the language-
agnostic structured-output contract: an LLM (via `rig`, Anthropic, or OpenAI
structured output) can produce instances conforming to it, and any JSON
validator can check instance data against it.

**User Story:** As someone building a graphRAG or extraction pipeline over a
LinkML ontology, I want a JSON Schema for my classes so an LLM's structured
output is constrained to valid instances — and so instance JSON validates
against the same schema — without hand-writing or maintaining a second schema.

**Related ADR:** [003 (LinkML as internal representation)](../adr/003-linkml-as-internal-representation.md),
[004 (reader/writer architecture)](../adr/004-reader-writer-architecture.md).
Sibling of the SHACL ([feature 17](17-class-validation-constructs.md)),
Postgres ([feature 24](24-postgres-ddl-writer.md)), and Rust
([feature 06](06-rust-codegen.md)) writers — another projection of the same IR.

**Why now:** it's the first pillar of the **LinkML+JSON instance program**
(JSON-Schema writer → LinkML instance reader → `panschema validate --data`)
that keeps the graphRAG demo entirely in LinkML and JSON — no OWL/TTL or
Rust-struct detour. The JSON Schema is what an LLM's structured output is
enforced against; the JSON it returns *is* a LinkML instance. It also unblocks
CuisineIQ (a build-time fidelity diff of LinkML components vs its frozen
OpenAPI contract), which has waited on this writer.

**Approach:** Vertical Slicing with Outside-In TDD. Each slice's output is
validated against an independent oracle — the `jsonschema` crate — for *both*
directions: the emitted document is itself a valid JSON Schema (checked against
the 2020-12 meta-schema), and representative instances validate as expected
(valid instances pass, malformed ones fail).

---

## Design decisions

- **Draft 2020-12**, using `$defs` for class definitions and `$ref`
  (`#/$defs/<Class>`) for class-valued slots. Current draft; what modern
  LLM structured-output APIs accept.
- **`additionalProperties: false`** on every class object. LLM structured
  output (and strict validation) wants closed objects; a stray property is a
  bug, not silently accepted.
- **One document, all classes in `$defs`.** A consumer targeting class `X`
  references `{"$ref": "#/$defs/X"}`. If the schema declares a `tree_root`
  class, the document's root also `$ref`s it (the natural entry point);
  otherwise the root is `$defs`-only.
- **Effective slots, not just direct.** A class's object carries every slot
  reachable via `is_a` / `mixins` / `slot_usage` — the same resolver the HTML,
  Rust, and Postgres writers use — so JSON Schema, Rust types, and SHACL all
  describe the same shape.
- **Range → type** mirrors the established LinkML built-in mapping (see the
  Rust/Postgres writers): `string`/`uri`/`curie`/… → `"string"`, `integer` →
  `"integer"`, `float`/`double`/`decimal` → `"number"`, `boolean` →
  `"boolean"`, `date`/`datetime`/`time` → `"string"` with the matching
  `format`. A class range → `$ref`; an enum range → the enum's `enum` list.
- **Additive + skip-clean.** A construct panschema can't express as JSON Schema
  is skipped with a diagnostic (the writer-projection warning pattern), never
  emitted broken.

## Vertical Slices

### Slice 1: Writer skeleton + scalar object schemas

**Status:** Complete

**Priority:** Must Have

**User Value:** `generate --format json-schema` emits a valid JSON Schema with
one closed `object` per class and its scalar slots as typed, required-aware
properties — the walking skeleton a validator and an LLM can already use for
scalar-only classes.

**Acceptance Criteria:**
- [x] A `JsonSchemaWriter` implements `Writer`, registered in `FormatRegistry` under `json-schema`; `generate --format json-schema` writes a `.json` file, and the format is documented in the CLI help + manifest `[generate.<schema>]`.
- [x] Each class becomes `#/$defs/<Class>`: an `object` with `properties` for its effective scalar slots (range → JSON Schema `type`/`format`), a `required` array from effective cardinality (`required` or `minimum_cardinality ≥ 1`), and `additionalProperties: false`. A multivalued scalar slot is an `array` of the scalar type.
- [x] The document roots at the `tree_root` class when the schema declares one (shipped once `tree_root` was modeled in [feature 33](33-linkml-instance-reader.md)); otherwise it is `$defs`-only. `$schema` is the 2020-12 dialect URI.
- [x] **Oracle:** the emitted document compiles in an independent JSON Schema validator (`jsonschema` dev-dep), and a valid scalar instance passes while one with a wrong-typed / missing-required / extra property fails (`accepts_valid_and_rejects_invalid_scalar_instances`, `emitted_document_compiles_as_a_valid_json_schema`).

### Slice 2: Enums, class `$ref`s, and value constraints

**Status:** Not Started

**Priority:** Must Have

**Depends on:** Slice 1.

**Acceptance Criteria:**
- [ ] An enum-range slot emits `{"enum": [<permissible values>]}`; a class-range slot emits `{"$ref": "#/$defs/<Class>"}` (array-wrapped when multivalued).
- [ ] Slot `pattern` → `pattern`; `minimum_value`/`maximum_value` → `minimum`/`maximum`; string length bounds if modeled → `minLength`/`maxLength`.
- [ ] **Oracle:** instances exercising an enum value, a nested class ref, a pattern, and a numeric bound validate as expected (in-range/valid pass; out-of-enum, pattern-mismatch, out-of-bound fail).

### Slice 3: Inheritance flattening + `any_of`

**Status:** Not Started

**Priority:** Should Have

**Depends on:** Slices 1–2.

**Acceptance Criteria:**
- [ ] Inherited/mixed-in slots appear on each class object (effective-slot flattening via the shared resolver), so a subclass instance validates against its own `$def` without the consumer chasing `is_a`.
- [ ] A polymorphic `any_of` range emits `oneOf` over the member `$ref`s/types.
- [ ] A construct with no JSON Schema projection is skipped with a diagnostic naming it, never emitted broken.

### Slice 4: LLM-structured-output ergonomics — deferred

**Status:** 📋 Deferred — build with the graphRAG demo

**Priority:** Could Have

**User Value:** A per-class / strict-subset mode tuned for LLM tool schemas
(e.g. a single class's schema inlined, no external `$ref`, the subset a given
provider's structured-output accepts), plus the `rig` `Extractor` wiring in the
demo app.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: skeleton + scalar objects | Must Have | Reader/Writer arch | Complete |
| Slice 2: enums, `$ref`, constraints | Must Have | Slice 1 | Not Started |
| Slice 3: inheritance + `any_of` | Should Have | Slices 1–2 | Not Started |
| Slice 4: LLM ergonomics + `rig` demo | Could Have | Slices 1–3 | 📋 Deferred |

## Definition of Done

- [ ] Slices 1–3 acceptance criteria met (slice 4 deferred)
- [ ] Every emitted document is meta-schema-valid and instance-validated via the `jsonschema` oracle
- [ ] `cargo nextest run` green; `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo doc`
- [ ] README.md + CHANGELOG.md updated; [linkml-coverage.md](../linkml-coverage.md) gains a JSON-Schema column/notes
