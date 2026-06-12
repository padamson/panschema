# LinkML IR Resolver Services - Implementation Plan

**Feature:** Shared resolver helpers on `SchemaDefinition` that walk inheritance, mixins, `slot_usage`, and prefix mappings — consumed by every writer instead of being reimplemented per-writer.

**User Story:** As a panschema maintainer, I want slot resolution (`is_a` + mixins + `slot_usage` merge-overlay), curie expansion, and effective cardinality to live in one place behind a stable API, so that the HTML writer, Rust writer, graph writer, and future writers (SHACL, SQL) share one correctness story instead of three diverging walkers.

**Related ADR (if applicable):** Extends [ADR-003: LinkML as Internal Representation](../adr/003-linkml-as-internal-representation.md) — codifies that the IR exposes resolution services, not just raw fields.

**Approach:** Vertical Slicing with Outside-In TDD

---

## Why Now

Three writers now ship slot-resolution walkers, none shared:

1. [rust_writer.rs](../../panschema/src/rust_writer.rs) — `resolve_slots` walks `is_a` + mixins + `slot_usage`, applies merge-overlay, returns the effective slot table for a class. Used by struct emission and trait derivation. Shipped in feature 06 slice 6.3.
2. [html_writer.rs](../../panschema/src/html_writer.rs) — the class-card slot-usage refinement renderer (feature 02 slice 5) walks the same constructs to render refined slots inline with inherited ones.
3. [graph_writer.rs](../../panschema/src/graph_writer.rs) — slice 11's `resolve_class_slots` walks `is_a` + mixins + `attributes` + `slots`, but **ignores `slot_usage` entirely**. The hover card shows inherited slot names with their unrefined cardinality / range.

Three walkers, three correctness bugs to find independently. Slice 11 already ships with a known blind spot (`slot_usage`). The Rust writer's `resolve_slots` is the most complete of the three; extracting it into `panschema::linkml::resolve` and having the other two delegate is the lowest-risk path to a single resolver.

Curie expansion has the same shape: `SchemaDefinition.prefixes` is a `BTreeMap<String, String>`, but no `expand_curie(curie) -> Option<String>` helper exists, so the HTML class card surfaces `prov:Entity` raw and the graph hover card surfaces whatever `class_uri` happens to hold (sometimes a curie, sometimes already-expanded). One helper, consumed everywhere.

This work is orthogonal to feature 08 (bootstrap the IR from the metaschema). Whether the IR is hand-rolled or generated, the *resolver services* live on top of it. Feature 08 changes how the IR fields come to exist; this feature changes how callers consume them.

---

## Architecture Overview

```
SchemaDefinition (raw IR fields: classes, slots, prefixes, ...)
                │
                ▼
       panschema::linkml::resolve  ◄── new module
       ┌────────────────────────────┐
       │ resolve_effective_slots    │  walks is_a + mixins + slot_usage
       │ expand_curie               │  prefix lookup + fallback
       │ effective_cardinality      │  required/multivalued/min/max overlay
       │ slot_provenance            │  "Person.name came from Named (mixin)"
       └────────────────────────────┘
                │
   ┌────────────┼────────────┬─────────────┐
   ▼            ▼            ▼             ▼
HtmlWriter  RustWriter  GraphWriter  future (SHACL, SQL)
```

The resolver is a sibling module to `linkml.rs`, not part of `SchemaDefinition` itself, so writers borrow `&schema` and call free functions or methods on a `Resolver<'a>` view. This keeps `SchemaDefinition` a pure data type and lets the resolver carry caches (e.g. for transitive ancestor sets) without polluting the IR.

---

## Vertical Slices

### Slice 12.1: Extract `resolve_effective_slots` into shared module

**Status:** ✅ Complete

**Priority:** Must Have

**User Value:** One resolver for slot inheritance. Rust writer, HTML writer, and graph writer all call the same code; `slot_usage` refinements light up everywhere at once.

**Acceptance Criteria:**
- [x] New module `panschema/src/linkml_resolve.rs` (sibling to `linkml.rs`; no folder migration required for the simple lift).
- [x] `pub fn resolve_effective_slots(class: &ClassDefinition, schema: &SchemaDefinition) -> BTreeMap<String, SlotDefinition>` lifted from `rust_writer::resolve_slots`. Signature and return type preserved verbatim — the `ResolvedSlot` wrapper that carries provenance metadata lands in slice 12.4 once consumers actually need it. The point of 12.1 is to make the lift pure.
- [x] Behaviour preserved exactly: walks `is_a` chain + mixins + own `attributes` + own `slots:` refs, applies `slot_usage` as merge-overlay (only fields the override actually sets).
- [x] Visited-set cycle guard preserved from `rust_writer::resolve_slots` (the `_walk` helper threading `BTreeSet<String>` of class names currently on the recursion stack). `resolve_slots_walk` and `merge_slot_override` move with it.
- [x] `rust_writer` calls `linkml_resolve::resolve_effective_slots` at the two existing call sites; the private `resolve_slots` / `resolve_slots_walk` / `merge_slot_override` go away. (Implemented as a `pub(crate) use` re-export so the call sites and their tests remain untouched — only the source-of-truth definition moved.)
- [x] All 16 existing rust_writer unit tests (`compute_class_roles`, `resolve_slots` inheritance + mixin + slot_usage merge cases, `is_descendant_of`, etc., per feature 06 slice 6.3) still pass without modification — they continue to test the same `resolve_slots` function, just imported from a different module.
- [x] One new test in `linkml_resolve::tests` pins the public-surface contract directly (a single fixture covering is_a + mixin + slot_usage in one shot) so a future change to the public API trips a test in the *resolver's* own module, not just in rust_writer's.

**Notes:**
- This is a pure refactor + module-promote. Behaviour change is zero. The point is to make the *next* slot-related change (slice 12 of feature 04, slice 6.11 of feature 06, slice 10 of feature 02) trivially small.
- Don't try to "improve" the resolver while lifting it. Lift first, change second. `ResolvedSlot` wrapper + provenance metadata is deliberately deferred to slice 12.4 so the lift can ship with byte-identical rust_writer output.
- The merge-overlay quirk on bool fields (only `true` overrides flow through; documented in `merge_slot_override`) is preserved verbatim. Distinguishing "absent" from "explicit false" would need a hand-rolled IR refactor to `Option<bool>` — out of scope for the extract step.

---

### Slice 12.2: `expand_curie` on `SchemaDefinition`

**Status:** ✅ Complete

**Priority:** Should Have

**User Value:** Every consumer that displays `class_uri` / `slot_uri` / `meaning` can show a stable, expanded IRI regardless of whether the source schema wrote it as a curie or a full URI. The graph hover card's "IRI:" row stops being inconsistent across nodes.

**Acceptance Criteria:**
- [x] `pub fn expand_curie(schema: &SchemaDefinition, value: &str) -> Option<String>` in `linkml_resolve`. Returns `Some(full_iri)` when `value` matches the `prefix:rest` shape and `prefix` is in `schema.prefixes`, or when `value` is bare and `schema.default_prefix` resolves to a known prefix. Returns `None` for inputs that don't look like a curie. Returns `Some(value.to_string())` pass-through when `value` already starts with `http://`, `https://`, or `urn:`.
- [x] Unit tests covering: standard prefix expansion (`prov:Entity` → `http://www.w3.org/ns/prov#Entity`); default-prefix fallback when no `:` in input; URL pass-through (http://, https://, urn:); unknown prefix returns `None`; bare name with no `default_prefix` returns `None`; empty input returns `None`. 6 tests total.
- [x] No consumer changes in this slice — RDF migration and HTML consumption are separate slices.

**Notes:**
- LinkML curies use `:` as separator; this is unambiguous because LinkML class/slot names disallow `:`.
- Don't try to also implement reverse contraction (full IRI → curie). Slice 12.4 covers that if a writer needs it.

---

### Slice 12.3: `effective_cardinality` overlay

**Status:** ✅ Complete

**Priority:** Should Have

**User Value:** Cardinality displayed in the HTML class card, graph hover card, and codegen comments comes from one place. Adding `minimum_cardinality` / `maximum_cardinality` support to one consumer doesn't require finding and patching three call sites.

**Acceptance Criteria:**
- [x] `pub fn effective_cardinality(slot: &SlotDefinition) -> Cardinality` where `Cardinality { required: bool, multivalued: bool, min: Option<u32>, max: Option<u32> }`.
- [x] Precedence (highest wins): explicit `minimum_cardinality`/`maximum_cardinality` from `slot_usage` overlay → same fields on the inherited slot → `required` / `multivalued` flags. The function takes a `ResolvedSlot` (post-resolution) so the overlay logic lives in slice 12.1, not here.
- [x] Tests covering: explicit `min: 0, max: 1` produces `required=false, multivalued=false`; `min: 1, max: None` produces `required=true, multivalued=true`; `slot_usage` setting only `required: true` preserves inherited `multivalued`.

**Notes:**
- Effective cardinality is a *view* over a `ResolvedSlot`, not new state. Keeping it as a pure function lets writers compute it on the fly without caching.
- The graph hover card's "Flags:" row (slice 11) becomes "Cardinality:" once this lands and surfaces `min..max`.

---

### Slice 12.4: Slot provenance — "where did this come from?"

**Status:** ✅ Complete

**Priority:** Nice to Have

**User Value:** Consumers can say "Person.name (inherited from Named via mixin)" instead of a flat slot list. Authors building intuition for inheritance get the answer without manually walking the class hierarchy.

**Acceptance Criteria:**
- [x] `ResolvedSlot` (introduced here — 12.1 deliberately shipped without the wrapper) carries a typed `Provenance` enum: `Direct`, `Inherited { from: String, via: InheritancePath }`, `Refined { from: String, by_slot_usage: bool }`. `InheritancePath` distinguishes `IsA(chain: Vec<String>)` from `Mixin(via: String)`. Exposed via `resolve_effective_slots_with_provenance`; the plain `resolve_effective_slots` delegates to the same walk, so the two can't diverge.
- [x] `rust_writer` doc-comments on flattened fields gain an "Inherited from `<class>`" line.
- [x] HTML class card surfaces a small "from `<class>`" tag on inherited slots (consumed in feature 02 slice 10).
- [x] Graph hover card "Slots:" row tags inherited entries with a `(from <origin>)` suffix (consumed in feature 04 slice 13).
- [x] Unit tests covering diamond inheritance (A → B → D, A → C → D) — provenance for D.name follows the first-found path; tests pin the `is_a` chain as that path (processed first; mixins never overwrite).

**Notes:**
- Provenance is purely additive — `ResolvedSlot` consumers that don't care can ignore the metadata.
- Diamond inheritance and `slot_usage` chains can produce ambiguous provenance (e.g. inherited from both `B` and `C` via different paths). The resolver picks the first-found path deterministically; the API surfaces that path, not the ambiguity. A future slice could expose the full lattice if a consumer needs it.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| 12.1: Extract `resolve_effective_slots` | Must Have | feature 06 slice 6.3 (the resolver to lift) | ✅ Complete |
| 12.2: `expand_curie` | Should Have | None | ✅ Complete |
| 12.3: `effective_cardinality` | Should Have | 12.1 | ✅ Complete |
| 12.4: Slot provenance | Nice to Have | 12.1 | ✅ Complete |

---

## Out of Scope (deferred past this feature)

- **Reverse curie contraction** (full IRI → curie). Use case is "compact display when the schema's prefixes match" — file as a follow-up if a consumer asks. The grammar is awkward (which prefix wins when two cover the same IRI?) and isn't blocking any current writer.
- **Caching transitive ancestor sets across writer calls.** Today every writer walks fresh. If profiling shows the resolver is a hot path for large schemas, add an opt-in `Resolver` struct with caches; until then, free functions are simpler.
- **Reference-target validation.** If a slot's `range` points at a class that doesn't exist in `schema.classes`, the resolver silently returns the raw string. A future "schema lint" feature should report these — out of scope here.
- **`pattern` regex compilation.** The IR carries the raw regex string; consumers compile it themselves if they need to. The resolver doesn't validate or compile.
