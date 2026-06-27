# Feature 19: Schema-encoded slot defaults — `ifabsent`

**Feature:** Model LinkML `ifabsent` on a slot and honor it in Rust codegen (and
the slot card), so a slot's default value lives in the schema instead of as an
untracked convention in each consumer.

**User Story:** As a schema author, I want `ifabsent` on a slot — e.g.
`ifabsent: "ItemStatus(planned)"` — to generate a field that defaults to that
value, so the default is part of the schema (the single source of truth) rather
than a comment each downstream consumer has to re-encode by hand.

**Related ADR (if applicable):** None — adds a metaslot to the IR
([linkml.rs](../../panschema/src/linkml.rs)) and extends the Rust codegen
([feature 06](06-rust-codegen.md)). `ifabsent` is in the not-modeled
`SlotDefinition` long tail in [linkml-coverage.md](../linkml-coverage.md).

**Approach:** Vertical Slicing with Outside-In TDD.

---

## Why Now

Surfaced dogfooding panschema in **slp**: a placed item's `status` should
default to `planned` when omitted (most items are planned, so the field is left
out to keep the JSON clean). LinkML says exactly that with `ifabsent: "ItemStatus(planned)"`,
but panschema drops it — the generated field is a bare `Option<ItemStatus>` and
the default survives only as a convention in slp's take-off code. That breaks
"the schema is the single source of truth."

`ifabsent` has a small, enumerable set of value forms. The codegen's job is to
parse the form, map it to a Rust default, and attach it with
`#[serde(default = "…")]` so an absent field deserializes to the schema-declared
default. Start with the forms slp needs (enum + scalars); defer the long tail.

---

## Vertical Slices

### Slice 1: `ifabsent` in the IR + enum-valued default in Rust codegen

**Status:** Complete

**Priority:** Should Have

**User Value:** A non-multivalued slot with `ifabsent: "<Enum>(<value>)"`
generates a field that deserializes to that enum variant when absent — the slp
`status` → `planned` case.

**Acceptance Criteria:**
- [x] `SlotDefinition` gains `ifabsent: Option<String>`, `#[serde(default, skip_serializing_if = "Option::is_none")]`, auto-parsed from YAML (`slot_definition_deserializes_ifabsent`).
- [x] For a non-multivalued slot whose `range` is an enum and whose `ifabsent` parses as `<EnumName>(<permissible_value>)`, the Rust writer emits the field as the bare enum type (not `Option`) with `#[serde(default = "<fn>")]`, and a generated default fn returning the matching variant (resolved via `variant_ident_for`) (`render_class_emits_ifabsent_enum_default`).
- [x] An `ifabsent` whose enum/variant doesn't resolve falls back to the existing `Option<T>` rendering with a `// WARNING:` comment (consistent with the writer's other unresolved-reference handling), rather than emitting a broken default.

**Notes:**
- A defaulted slot always has a value, so the non-`Option` field is the faithful shape. This interacts with the required/optional logic — keep the change localized to "non-multivalued + resolvable `ifabsent`."
- Also accept the bare permissible-value form if LinkML emits it without the enum prefix; resolve against the slot's `range` enum.

---

### Slice 2: Scalar `ifabsent` forms

**Status:** Complete

**Priority:** Should Have

**User Value:** Slots with scalar defaults — `int(0)`, `string("x")`,
`float(1.5)`, `true`/`false` — generate fields that default to those literals.

**Acceptance Criteria:**
- [x] The codegen parses the scalar `ifabsent` forms (`int(...)`, `string(...)`, `float(...)`/`double(...)`, boolean) and emits a non-`Option` field with a `#[serde(default = "<fn>")]` returning the literal (`render_class_emits_ifabsent_scalar_defaults`).
- [x] String defaults are escaped correctly in the generated literal; numeric forms map to the field's Rust numeric type (`i64`/`f64`, whole-number floats suffixed to type as `f64`).

---

### Slice 3: Show the default on the slot card

**Status:** Complete

**Priority:** Should Have

**User Value:** A slot's default renders in the HTML docs (a "Default" row), so a
reader sees the declared default without reading the schema source.

**Acceptance Criteria:**
- [x] The slot card shows a "Default" row with the `ifabsent` value (rendered readably — `planned`, `0`, `"x"`) when set, nothing when unset (`slot_card_shows_default`).

**Notes:**
- HTML-only; this is the doc-completeness half of the same metaslot, independent of the codegen slices.

---

### Slice 4: Long-tail `ifabsent` forms — deferred

**Status:** 📋 Deferred

**Priority:** Could Have

**User Value:** Honor the rare `ifabsent` forms — `class_curie`, `slot_curie`,
`date`/`datetime`, and a *non-empty* default list for a multivalued slot.

**Why deferred:** Slices 1–2 (enum + scalar) cover the dogfood need and the
overwhelming majority of real `ifabsent` uses. What's left is genuinely rare.
Static `date`/`datetime` defaults are uncommon (LinkML defaults are static
literals — there is no `now()`), and `class_curie`/`slot_curie` defaults are
meta-modeling territory. The common multivalued case — *default to empty* —
needs no `ifabsent` at all: a multivalued slot already generates a `Vec<T>` that
`#[serde(default)]` fills with an empty vec on absence, so only a *non-empty*
default list falls here. Pick any of these up when a consumer actually needs it.

**Acceptance Criteria:**
- [ ] (when undeferred) The deferred forms parse and emit correct Rust defaults, with tests per form.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: IR + enum default (codegen) | Should Have | Feature 06 | Complete |
| Slice 2: Scalar defaults (codegen) | Should Have | Slice 1 | Complete |
| Slice 3: Default on slot card (HTML) | Should Have | Slice 1 (IR field) | Complete |
| Slice 4: Long-tail forms (rare) | Could Have | Slice 1 | 📋 Deferred |

---

## Definition of Done

The feature is complete when ALL of the following are true:

- [x] Slices 1–3 acceptance criteria met (slice 4 deferred)
- [x] All tests passing: `cargo nextest run`
- [x] Generated code with an `ifabsent` slot compiles in a downstream crate (the slp `status` → `planned` case round-trips)
- [x] Library documentation complete: `cargo doc`
- [x] Code formatted + clippy clean: `cargo fmt --check` + `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] README.md updated
- [x] CHANGELOG.md updated
- [x] [linkml-coverage.md](../linkml-coverage.md) `SlotDefinition` row updated — `ifabsent` moves out of the not-modeled long tail
