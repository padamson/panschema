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
downstream API consumers that need an OpenAPI/JSON-Schema contract generated
from the same LinkML the Rust types come from — so a data model backing both
Rust services and typed API clients isn't authored twice and left to drift.

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

**Status:** Complete

**Priority:** Must Have

**Depends on:** Slice 1.

**Acceptance Criteria:**
- [x] An enum-range slot emits `{"enum": [<permissible values>]}` (BTreeMap key order); a class-range slot emits `{"$ref": "#/$defs/<Class>"}`, array-wrapped when multivalued.
- [x] Slot `pattern` → `pattern`; `minimum_value`/`maximum_value` → `minimum`/`maximum`, applied to the scalar base. (String length bounds aren't modeled in the IR, so `minLength`/`maxLength` are out of scope until they are.)
- [x] **Oracle:** the enriched document compiles, and instances exercising an enum value, a nested class ref, a pattern, and a numeric bound validate as expected — out-of-enum, pattern-mismatch, out-of-bound, and scalar-where-class-ref-declared all fail (`enum_class_and_constraints_project`, `rich_instances_validate_as_expected`).

### Slice 3: Inheritance flattening + `any_of`

**Status:** Complete

**Priority:** Should Have

**Depends on:** Slices 1–2.

**Acceptance Criteria:**
- [x] Inherited/mixed-in slots appear on each class object (effective-slot flattening via the shared `resolve_effective_slots_with_provenance` — the same resolver the Rust/Postgres/SHACL writers use), so a subclass instance validates against its own `$def` without chasing `is_a` (`inherited_slots_flatten_onto_the_subclass`).
- [x] A polymorphic `any_of` range emits `anyOf` over each branch's value schema (`any_of_range_projects_to_anyof`, oracle-checked: either branch validates, neither fails). Uses `anyOf` rather than `oneOf` — LinkML `any_of` means "satisfies at least one", which is JSON Schema `anyOf`.

### Slice 3b: Custom-`types:` resolution

**Status:** Complete

**Priority:** Should Have

**Depends on:** Slice 3.

**Acceptance Criteria:**
- [x] A range naming a schema `types:` entry resolves to its base scalar by following the `typeof` chain (or the type's `uri` datatype), carrying the type's `pattern`, instead of the permissive `true` fallback (`custom_types_resolve_to_base_scalar_and_facets`, oracle-checked). A `typeof` cycle terminates at `string`; an unrecognized datatype falls back to `string`.
- [x] Surfaced and fixed a parser bug in the process: `typeof` was read under a non-standard `typeof_` key, so a real schema's type inheritance was silently dropped everywhere (docs/graph/JSON-Schema). Now read from the correct `typeof` key.

A writer-projection diagnostic for a range that genuinely can't project (an
unresolvable custom type) is a possible later refinement; today such a range is
the permissive `true` fallback, which never wrongly rejects an instance.

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
| Slice 2: enums, `$ref`, constraints | Must Have | Slice 1 | Complete |
| Slice 3: inheritance + `any_of` | Should Have | Slices 1–2 | Complete |
| Slice 3b: custom-`types:` resolution | Should Have | Slice 3 | Complete |
| Slice 4: LLM ergonomics + `rig` demo | Could Have | Slices 1–3 | 📋 Deferred |

## Definition of Done

- [ ] Slices 1–3 acceptance criteria met (slice 4 deferred)
- [ ] Every emitted document is meta-schema-valid and instance-validated via the `jsonschema` oracle
- [ ] `cargo nextest run` green; `cargo fmt --check`; `cargo clippy --all-targets --all-features -- -D warnings`; `cargo doc`
- [ ] README.md + CHANGELOG.md updated; [linkml-coverage.md](../linkml-coverage.md) gains a JSON-Schema column/notes
