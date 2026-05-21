# Bootstrap LinkML IR from the Metaschema - Implementation Plan

**Feature:** Generate `panschema/src/linkml/*.rs` from the LinkML metaschema YAML, replacing the hand-rolled IR.

**User Story:** As a panschema maintainer, I want the LinkML internal representation to be generated from the LinkML metaschema itself (using panschema's own `RustWriter`), so that every LinkML field — present and future — is faithfully represented by construction, and adding spec coverage stops being hand-edit-by-hand work.

**Related ADR (if applicable):** Extends [ADR-003: LinkML as Internal Representation](../adr/003-linkml-as-internal-representation.md) and [ADR-004: Reader/Writer Architecture](../adr/004-reader-writer-architecture.md).

**Approach:** Vertical Slicing with Outside-In TDD

---

## Why Now

After v0.3.0 ships, the natural next step is to close the gap between "panschema's IR" and "the LinkML metaschema." Today:

- `panschema/src/linkml.rs` is hand-rolled. We've grown it by adding fields ad hoc as features needed them (`mixins`, `slot_usage`, `prefixes`, …).
- `panschema/src/yaml_reader.rs` hand-maps every field from YAML into those types.
- Every new LinkML feature panschema wants to honor requires hand-editing both files.

Meanwhile, the LinkML metamodel is itself a LinkML YAML schema (see [`linkml/linkml-model/linkml_model/model/schema/meta.yaml`](https://github.com/linkml/linkml-model/blob/main/linkml_model/model/schema/meta.yaml), ~60 classes). And panschema's `RustWriter` (feature 06) is mature enough to emit a working Rust module from a real schema.

The bootstrap loop closes itself: run panschema's `RustWriter` over the LinkML metaschema → emit a `linkml/generated.rs` → swap in for the hand-rolled IR.

This collapses three problem classes at once:

1. **Faithful IR coverage:** every metaschema field maps to a Rust field, by construction. Zero drift between what panschema claims to model and what LinkML actually defines.
2. **RustWriter validation:** if the writer can codegen the metaschema cleanly (it's a fairly complex schema — recursive types, multiple inheritance, `slot_usage` overrides, etc.), then it can codegen *any* LinkML schema. The metaschema is the hardest test there is.
3. **Spec-driven evolution:** when LinkML's metaschema evolves, regenerate. No code review of hand-rolled field additions.

The cost: every downstream that imports `panschema::linkml::*` will face one round of API churn (field names align with the metaschema's lowerCamelCase / snake_case conventions; some optional vs required flags differ). After that, the API stops drifting from the spec.

---

## What This Does NOT Do

- **Replace `yaml_reader.rs`** entirely. The bootstrapped types are still deserializable via serde. The reader becomes much thinner (it's mostly `serde_yaml::from_str` against the generated struct), but it doesn't disappear — there's still LinkML's `imports:` resolution, identifier inference, and a few semantic checks to perform after raw deserialization.

- **Eliminate panschema-side modeling decisions.** The metaschema defines what a LinkML schema *is*. It doesn't tell us how to *render* one to OWL or HTML — those mapping rules still live in the writers. Bootstrap is an IR-shape concern, not a semantics concern.

- **Block on full LinkML metaschema coverage by the RustWriter.** Some metaschema features may not yet round-trip cleanly (e.g., `linkml:Anything` ranges, deeply pre-release LinkML constructs). Slice plan accepts subset coverage, with explicit known-gap documentation.

---

## Vertical Slices

### Slice 1: Vendor the metaschema + run RustWriter against it (manual)

**Status:** Not Started

**User Value:** A maintainer can run panschema's `RustWriter` over the LinkML metaschema YAML and see what falls out. The output is checked in for review but not yet wired into `panschema::linkml`.

**Acceptance Criteria:**
- [ ] Vendor a pinned snapshot of the LinkML metaschema YAML at `panschema/src/linkml/metaschema/meta.yaml` (or similar). Pin to a specific LinkML version (e.g., 1.7.0).
- [ ] `cargo run --release -- generate --input panschema/src/linkml/metaschema/meta.yaml --format rust --output panschema/src/linkml/generated.rs` produces a Rust module.
- [ ] The output module compiles as a standalone Rust file (`cargo check` against a scratch crate with `serde` + `chrono` deps).
- [ ] A `generated.rs.expected` snapshot is committed; `cargo test` regenerates and diffs to catch unintended drift in RustWriter output.

**Notes:**
- Expect this slice to surface RustWriter gaps. Each gap becomes a one-off issue or a slice in feature 06.
- If the metaschema's deserialization needs panschema features panschema doesn't yet emit (e.g., `linkml:Anything` ranges, recursive enums), document them as "out of scope for v0.4.0" and either skip the affected classes or vendor a trimmed metaschema variant.

---

### Slice 2: Swap `panschema::linkml::*` to use generated types

**Status:** Not Started

**User Value:** `panschema::linkml::ClassDefinition` (and friends) are now the generated types, not the hand-rolled ones. Every consumer that previously imported these gets the metaschema-derived shape.

**Acceptance Criteria:**
- [ ] Move `panschema/src/linkml.rs` → `panschema/src/linkml/handrolled.rs` (or delete it after migration). Make `panschema/src/linkml/mod.rs` re-export the generated module.
- [ ] Walk every internal `use crate::linkml::*` and migrate to the new field names + types. Common shifts: hand-rolled `class_uri: Option<String>` → metaschema's `class_uri` (likely same), but possibly different in optional/required flags and casing.
- [ ] `yaml_reader.rs` becomes a thin wrapper that calls `serde_yaml::from_str` against the generated `SchemaDefinition`. Any LinkML-specific post-processing (default ranges, identifier inference, prefix expansion) lives in a new `panschema/src/linkml/normalize.rs`.
- [ ] All existing reader/writer tests pass against the new types. Snapshot tests are updated to reflect any output shifts.

**Notes:**
- This is the API-churn slice. Expect 1–2 commits' worth of mechanical migration.
- Keep `Default` derives on every emitted type so test fixtures stay terse (currently `ClassDefinition::new("Foo")` style; with generated types, `ClassDefinition { name: "Foo".into(), ..Default::default() }`).
- The migration also exposes whether `RustWriter`'s slice 6.5 `Default` derive analysis is conservative-enough for the metaschema's classes. If it's not, refining slice 6.5 may be in scope here.

---

### Slice 3: Regeneration as a build step

**Status:** Not Started

**User Value:** A maintainer who pulls a newer LinkML metaschema version runs one command to regenerate the IR; CI catches RustWriter drift automatically.

**Acceptance Criteria:**
- [ ] `cargo xtask regen-linkml` (or a script in `panschema/scripts/`) regenerates `panschema/src/linkml/generated.rs` from the vendored metaschema YAML.
- [ ] CI compares the committed `generated.rs` against the live regenerated output and fails on drift (forces the maintainer to commit the regenerated file explicitly).
- [ ] CHANGELOG entry on every regeneration documents which LinkML metaschema version was used.

**Notes:**
- This is the operational closing-the-loop slice. Without it, the bootstrap erodes the moment LinkML changes.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: Vendor + dogfood RustWriter | Must Have | Feature 06 | Not Started |
| Slice 2: Swap IR to generated types | Must Have | Slice 1 | Not Started |
| Slice 3: Regeneration as build step | Should Have | Slice 2 | Not Started |

---

## Definition of Done

The feature is complete when ALL of the following are true:

- [ ] All slice 1 + 2 acceptance criteria met (slice 3 is "should have" for v0.4.0).
- [ ] `panschema::linkml::ClassDefinition` (and friends) are generated from the metaschema.
- [ ] All tests passing: `cargo nextest run`
- [ ] All writers (HTML, OWL/TTL, JSON-LD, RDF/XML, N-Triples, Rust, Graph) produce output for the test fixtures whose shape is identical (or documented-and-intentional-different) to the v0.3.0 baseline.
- [ ] No clippy warnings; `cargo fmt --check` clean.
- [ ] README + CHANGELOG updated.
- [ ] An ADR (probably ADR-005) documents the bootstrap loop, the migration moment, and the regeneration workflow.

---

## Strategic Significance

This is the *most aggressive* dogfood we can do: panschema generating panschema's own type definitions. If it works cleanly, panschema is by-construction conformant to the LinkML metaschema in a way no hand-rolled implementation could be.

If it doesn't work cleanly on the first try (which is the realistic outcome), every gap is a high-value test case for `RustWriter` (feature 06) — and panschema is the only Rust LinkML implementation with the infrastructure to find these gaps systematically.
