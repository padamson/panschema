# Feature 15: Multi-file schema modularity — imports resolution

**Feature:** Resolve LinkML `imports:` (and OWL `owl:imports`) at load time so a
schema split across files merges into one resolved IR before any writer runs —
closing the "tracked but never resolved" gap on `SchemaDefinition.imports`.

**User Story:** As a schema author maintaining a growing vocabulary, I want to
split it across files via `imports:` and have panschema merge the imported
schemas into one resolved `SchemaDefinition` before generating docs / graph /
RDF / Rust, so I can modularize a large ontology (core + networking + storage +
compute) without losing a single unified output.

**Related ADR (if applicable):** None — extends the reader pipeline
([feature 03](03-reader-writer-architecture.md)); the merge is a load-time
resolution service that sits beside the resolver in
[feature 12](12-linkml-ir-resolver-services.md).

**Approach:** Vertical Slicing with Outside-In TDD.

---

## Why Now

`SchemaDefinition.imports: Vec<String>` already deserializes, but nothing
follows it — every render column for `imports` is `○` in
[linkml-coverage.md](../linkml-coverage.md) ("tracked, never resolved or
rendered"; priority gap 7). Today the only way to render a multi-file schema as
one document is to physically concatenate it; the manifest
([feature 05](05-schema-manager.md)) emits *separate, independent* docs per
schema and never merges. A serious infra/services vocabulary outgrows a single
file (core + per-domain modules), so import resolution is the prerequisite for
modular authoring with a unified rendered output.

Resolution is a load-time concern, deliberately upstream of every writer: a
Reader yields the root `SchemaDefinition`, a single merge pass folds the
imports in, and every writer consumes the already-merged schema unchanged. This
mirrors the [feature 12](12-linkml-ir-resolver-services.md) "single source of
truth" philosophy — one merger, not one per writer.

---

## Vertical Slices

### Slice 1: Single-level local import merge (walking skeleton)

**Status:** Complete

**Priority:** Must Have

**User Value:** A root schema with `imports: [common]` renders classes, slots,
enums, types, and prefixes from both files in one output.

**Acceptance Criteria:**
- [x] New resolution entry point (`resolve_imports(schema, root_path, &registry)`) invoked in the `generate` path after read, before writer dispatch.
- [x] Each `imports:` entry naming a local file (resolved relative to the importing file; `.yaml`/`.yml`/`.ttl`, extension optional) is loaded via `FormatRegistry` and merged into the root.
- [x] Merge unions `classes`, `slots`, `enums`, `types`, and `prefixes`; the importing (root) schema wins on a name collision (the collision is recorded for slice 2 diagnostics).
- [x] A self-import / import cycle is detected and reported as a clear error, not an infinite loop (`resolve_imports_detects_cycle`).
- [x] Each merged element records its origin file (provenance) for later diagnostics and rendering.
- [x] Integration test: root + one imported file → generated HTML contains a class defined only in the import (`generate_merges_single_import`).

**Notes:**
- Out of scope here: transitive imports, CURIE/remote imports, and builtin `linkml:*` imports (slices 2–3).
- No writer changes — the merged schema is shaped exactly like a single-file schema.

---

### Slice 2: Transitive imports + collision diagnostics

**Status:** Not Started

**Priority:** Must Have

**User Value:** Imports of imports resolve, and a name defined incompatibly in
two files surfaces a clear diagnostic instead of a silent last-writer-wins.

**Acceptance Criteria:**
- [ ] Imports resolve transitively; each file is loaded at most once — diamond imports de-duplicated by canonical path (`resolve_imports_dedupes_diamond`).
- [ ] A name collision across files where the two definitions differ is reported with both source files and the entity path; byte-identical re-definitions are silently unified.
- [ ] Cycle detection holds across the full transitive graph, not just direct self-imports.

---

### Slice 3: Builtin + CURIE/remote imports

**Status:** Not Started

**Priority:** Should Have

**User Value:** Real-world LinkML schemas that `imports: linkml:types` (and
CURIE/URL imports) load without spurious file-not-found errors.

**Acceptance Criteria:**
- [ ] `imports:` entries resolving to LinkML builtins (`linkml:types`, `linkml:meta`, …) are recognized and treated as no-ops — their primitive types are already known to the writers — rather than failing (`resolve_imports_ignores_linkml_builtins`).
- [ ] CURIE / URL imports resolve via prefix expansion plus the source cache (reusing the [feature 13](13-upstream-label-cache.md) fetch/cache path); offline mode degrades to a diagnostic, not a hang.

**Notes:**
- Mapping the full `linkml:meta` metamodel is out of scope; only the builtin *type* set must be honored so primitive ranges keep resolving.

---

### Slice 4: OWL `owl:imports` + import provenance in HTML

**Status:** Not Started

**Priority:** Could Have

**User Value:** OWL/TTL inputs follow `owl:imports`, and the docs show which
source file each element came from.

**Acceptance Criteria:**
- [ ] `owl_reader` follows `owl:imports` IRIs, feeding the same merge pass as the LinkML side (`owl_reader_follows_owl_imports`).
- [ ] HTML renders an "Imported from" indicator (source file / namespace) on elements that originated in an import, sourced from slice-1 provenance.

**Notes:**
- The manifest still lists one entry per generated doc; `imports` modularizes a *single* schema's sources — it does not replace the manifest's multi-schema story.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: Single-level local merge | Must Have | Feature 03 | Complete |
| Slice 2: Transitive + collisions | Must Have | Slice 1 | Not Started |
| Slice 3: Builtin + CURIE/remote | Should Have | Slice 1 (+ Feature 13) | Not Started |
| Slice 4: `owl:imports` + provenance | Could Have | Slice 1 | Not Started |

---

## Definition of Done

The feature is complete when ALL of the following are true:

- [ ] Slices 1–2 acceptance criteria met (slice 3 Should Have; slice 4 optional)
- [ ] All tests passing: `cargo nextest run`
- [ ] Library documentation complete: `cargo doc`
- [ ] Code formatted + clippy clean: `cargo fmt --check` + `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] README.md updated with a multi-file `imports` example
- [ ] CHANGELOG.md updated
- [ ] [linkml-coverage.md](../linkml-coverage.md) `imports` row updated (render columns move off `○`)
