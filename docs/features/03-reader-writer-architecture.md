# Feature: Reader/Writer Architecture

**Feature:** Refactor to LinkML IR with Reader/Writer Pattern

**User Story:** As a schema developer, I want panschema to use a modular architecture internally, so that future format support can be added without breaking existing functionality.

**Related ADRs:**
- [ADR-003: LinkML as Internal Representation](../adr/003-linkml-as-internal-representation.md)
- [ADR-004: Reader/Writer Architecture](../adr/004-reader-writer-architecture.md)

**Approach:** Vertical Slicing with Outside-In TDD

---

## Implementation Strategy

This feature refactors the existing rontodoc codebase to the new reader/writer architecture while **preserving identical user-facing behavior**. The key constraint:

```bash
# Before (v0.1.0)
rontodoc generate --input reference.ttl --output docs/

# After (v0.2.0) - same output, new architecture
panschema doc reference.ttl --output docs/
```

All existing E2E tests must continue passing throughout the refactor.

*Documentation updates required:*
- [Main README](../../README.md) - Update CLI name and commands
- [CHANGELOG](../../CHANGELOG.md) - Document architectural changes
- [WHY](../../WHY.md) - Update vision for panschema

---

## Vertical Slices

### Slice 1: LinkML IR Core Structs

**Status:** Completed

**User Value:** Foundation for all subsequent slices. No user-visible change, but enables the architecture.

**Acceptance Criteria:**
- [x] `SchemaDefinition` struct with name, id, prefixes, description
- [x] `ClassDefinition` struct with name, description, attributes, is_a, mixins
- [x] `SlotDefinition` struct with name, description, range, required, multivalued
- [x] `EnumDefinition` struct with name, permissible_values
- [x] `TypeDefinition` struct with name, typeof, uri
- [x] All structs derive `serde::Serialize`, `serde::Deserialize`, `Debug`, `Clone`, `PartialEq`
- [x] Unit tests for struct construction and serialization (21 tests)
- [x] Structs align with LinkML specification (MinimalSubset)

**Notes:**
- Reference: [LinkML Specification](https://linkml.io/linkml-model/latest/docs/specification/)
- Implemented in `src/linkml.rs` with full serde YAML support
- Annotations field added to all structs for format-specific metadata preservation
- `Prefix` and `PermissibleValue` helper structs also implemented

---

### Slice 2: Reader/Writer Traits

**Status:** Completed

**User Value:** Establishes extensibility pattern for future formats.

**Acceptance Criteria:**
- [x] `Reader` trait defined with `read()` and `supported_extensions()` methods
- [x] `Writer` trait defined with `write()` and `format_id()` methods
- [x] Format dispatcher (`FormatRegistry`) that selects reader/writer based on file extension
- [x] Error types for unsupported formats (`IoError` enum)
- [x] Unit tests for trait contracts (8 tests)

**Notes:**
- Implemented in `src/io.rs`
- Traits are object-safe for dynamic dispatch via `Box<dyn Reader>` and `Box<dyn Writer>`
- `FormatRegistry` provides `reader_for_path()`, `reader_for_extension()`, `writer_for_format()`
- Case-insensitive extension matching

---

### Slice 3: OwlReader Implementation

**Status:** Completed

**User Value:** Existing TTL files work with new architecture.

**Acceptance Criteria:**
- [x] `OwlReader` implements `Reader` trait
- [x] Existing TTL parser refactored as internal implementation detail
- [x] Mapping layer converts `OntologyMetadata` → `SchemaDefinition`
- [x] Classes map to `ClassDefinition` with hierarchy preserved
- [x] Properties map to `SlotDefinition` with domain/range
- [x] Individuals map to class instances (stored in annotations or separate field)
- [x] OWL-specific metadata preserved in annotations (e.g., `panschema:source_format`)
- [x] Integration tests: parse reference.ttl → valid SchemaDefinition (14 tests)

**Notes:**
- Implemented in `src/owl_reader.rs` with all OWL parsing logic consolidated
- Parses OWL to internal `OntologyMetadata`, then maps to LinkML IR
- Individuals stored in annotations as `panschema:individuals`, `panschema:individual:{id}`, etc.
- XSD datatypes mapped to LinkML built-in types (string, integer, float, boolean, date, etc.)
- Property type (ObjectProperty/DatatypeProperty) preserved in `panschema:owl_property_type` annotation
- 23 tests (parsing + mapping)

---

### Slice 4: HtmlWriter Implementation

**Status:** Completed

**User Value:** Documentation generation works from LinkML IR.

**Acceptance Criteria:**
- [x] `HtmlWriter` implements `Writer` trait
- [x] Existing renderer refactored to accept `SchemaDefinition`
- [x] Template data structs derived from IR (not OWL types)
- [x] Class cards render from `ClassDefinition`
- [x] Property cards render from `SlotDefinition`
- [x] Individual cards render from IR representation
- [x] All existing snapshot tests pass with identical output
- [x] E2E tests pass: TTL → OwlReader → IR → HtmlWriter → HTML (7 tests)

**Notes:**
- Implemented in `src/html_writer.rs`
- Old `renderer.rs` and `parser.rs` removed - pipeline now uses Reader/Writer exclusively
- Template data structs (`EntityRef`, `ClassData`, `PropertyData`, `IndividualData`) built from SchemaDefinition
- Individual labels and property values retrieved from annotations
- Main pipeline updated in `main.rs` and `server.rs` to use `OwlReader` + `HtmlWriter`

---

### Slice 5: CLI Rename and Integration

**Status:** Completed

**User Value:** Users install and run `panschema` with familiar commands.

**Acceptance Criteria:**
- [x] Crate renamed from `rontodoc` to `panschema` in Cargo.toml
- [x] Binary name is `panschema`
- [x] `panschema generate` command (kept for v0.2.0, `doc` planned for future)
- [x] `panschema serve` command for dev server (unchanged behavior)
- [x] Help text updated for panschema branding
- [x] README updated with new CLI examples and vision
- [x] GitHub repo renamed to `panschema`
- [x] WHY.md updated with pandoc-like vision

**Notes:**
- Kept `generate` command for v0.2.0 to minimize breaking changes
- `doc` alias planned for future release
- GitHub repo renamed, old URLs redirect automatically

---

### Slice 6: Release v0.2.0

**Status:** Not Started

**User Value:** Users can install panschema from crates.io.

**Acceptance Criteria:**
- [x] All tests passing (unit, integration, E2E)
- [x] `cargo fmt --check` passes
- [x] `cargo clippy --all-targets --all-features -- -D warnings` passes
- [x] CHANGELOG.md updated for v0.2.0
- [x] Version bumped to 0.2.0
- [x] Tag v0.2.0 triggers release workflow
- [x] `cargo install panschema` works from crates.io
- [x] Generated documentation identical to v0.1.0 for same input

**Notes:**
- Publishing rontodoc v0.1.1 that prints deprecation notice pointing to panschema v0.2.0

---

### Slice 7: RDF emitter correctness (v0.3.0 dogfood follow-up)

**Status:** Not Started

**User Value:** `panschema generate --format ttl|jsonld|ntriples|rdfxml` produces RDF that is actually valid against the spec for that format and faithfully reflects the LinkML schema's multiple-inheritance semantics. Downstream consumers (reasoners, triple stores, OWL editors) can ingest the output without manual post-processing.

**Context:** Surfaced by the scimantic-schema v0.2.0 dogfood (BFO/CCO re-grounding, where most class IRIs are CURIEs against external prefixes). The pre-existing serializers emitted `<cco:ont00000005>` rather than the expanded absolute IRI, which is invalid N-Triples and produces wrong-IRI semantics in TTL / JSON-LD / RDF/XML. The same dogfood revealed that mixins were never emitted as `rdfs:subClassOf` even though LinkML treats mixins as multiple-inheritance.

**Acceptance Criteria:**
- [ ] All CURIE-shaped IRIs (`prefix:local`) in the schema — `class_uri`, `slot_uri`, slot ranges, etc. — are expanded against `schema.prefixes` before emission. The TTL output uses `<https://...>` form OR compact `prefix:local` paired with a `@prefix` declaration at the top, not the current `<prefix:local>` invalid form.
- [ ] TTL output declares `@prefix` lines at the top for every prefix referenced in the body.
- [ ] JSON-LD output includes a top-level `@context` mapping every prefix referenced in the body. `@id` values are either expanded absolute IRIs or compact forms resolvable against the context.
- [ ] N-Triples output uses absolute IRIs everywhere (no compact forms; `<...>` always contains a fully-qualified URL).
- [ ] RDF/XML output declares `xmlns:` for every prefix used and uses absolute IRIs in `rdf:about` / `rdf:resource`.
- [ ] For every class with `mixins:`, the writer emits one `rdfs:subClassOf` triple per mixin in addition to the existing `is_a` triple. The mixin target IRI follows the same CURIE-expansion rules.
- [ ] `panschema generate --format ntriples` against scimantic@0.2.0 (or any schema with non-`scimantic:` `class_uri` values) round-trips through a strict parser (e.g. `rdflib` / `rapper`) with zero relative IRIs.
- [ ] Snapshot tests in `panschema/src/rdf_serializers.rs` extended against a fixture that exercises: a class with a CURIE `class_uri`, a class with mixins, and a slot with a CURIE `slot_uri`.

**Notes:**
- The expansion logic should live in a shared helper (e.g. `expand_curie(name: &str, schema: &SchemaDefinition) -> String`) so all four serializers reuse it. Unknown prefixes (declared in `prefixes:` but unresolved against any known namespace) get passed through unchanged with a `// WARNING` comment in test output mode; in normal runs they emit a `tracing::warn!` and pass through unchanged so the writer is robust to incomplete schemas.
- `default_prefix:` is honored: a bare class name without a colon-CURIE in `class_uri` uses `<default_prefix><default_separator><name>`. This matches LinkML's `class_uri` resolution rules.
- The mixin emission is a one-line addition next to the `is_a` emission; the bigger change is the CURIE expansion.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: LinkML IR Structs | Must Have | None | Completed |
| Slice 2: Reader/Writer Traits | Must Have | Slice 1 | Completed |
| Slice 3: OwlReader | Must Have | Slice 2 | Completed |
| Slice 4: HtmlWriter | Must Have | Slice 3 | Completed |
| Slice 5: CLI Rename | Must Have | Slice 4 | Completed |
| Slice 6: Release | Must Have | Slice 5 | Completed |
| Slice 7: RDF emitter correctness | Must Have (v0.3.0) | Slice 4 | Not Started |
