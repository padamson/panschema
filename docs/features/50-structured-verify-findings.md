# Feature 50: Structured verify findings

**Feature:** Every finding `verify` reports carries a kind a program can match on.

**User Story:** As a consumer of `panschema::validate`, I want each violation to say what kind of finding it is and to carry its data as fields, so that I can act on a finding without parsing the sentence written for a person.

**Related ADR:** [ADR-008](../adr/008-instance-data-reader-architecture.md) places `validate_instances` as the format-agnostic core over the instance model. This feature changes the shape of what it returns, not where it sits. Builds on [Feature 34](34-validate-instance-data.md).

**Approach:** Vertical Slicing with Outside-In TDD

---

## Why

`Violation` is a record name and a sentence. The sentence is the only thing that says which of the validator's checks fired, so anything that wants to treat a dangling reference differently from a cardinality miss, or count findings by kind, or emit them as JSON, has to grep prose. Several of the checks are handed a structured finding by the model crate, one that already carries the slot, the target, or the key kind as fields, and flatten it into that sentence, throwing away structure the model crate did the work to keep. The validator's own tests match phrases in the sentence for the same reason, and break on copy edits.

---

## Vertical Slices

### Slice 1: A kind on every violation

**Status:** Completed

**User Value:** A caller of `validate_instances` can match a finding's kind and read its data as fields; `verify` prints exactly what it printed before.

**Acceptance Criteria:**
- [x] Every finding `validate_instances` and `validate_instance_data` return carries a kind a caller can match on, with the finding's data (class, slot, value, bounds, names, candidates) as fields rather than words inside a sentence.
- [x] A finding that originates in the model crate (a dangling reference, an undeclared field, an expansion gap, an unusable collection entry) carries the model's own type, so nothing the model recorded is lost at this boundary.
- [x] The line `verify` prints for each finding is unchanged: the sentence a kind renders to is the sentence the finding produced before.
- [x] The validator's tests assert a finding's kind and fields; the rendered sentence is asserted only where the wording itself is the claim.

**Notes:**
- `Violation { record, kind }` replaces `Violation { record, detail }`. The sentence stays available as `detail()`, and `Display` is unchanged, so `verify` needs no change. Reading the old field is the one break for library consumers; `Eq` is dropped because numeric bounds are `f64`.
- The kind enum is `#[non_exhaustive]`: a new check adds a variant without a breaking release.
- A value the reader could not fit to its slot arrives as the reader's phrase for its shape (`an object`, `a number`), which the finding carries as given. Typing that shape is the instance model's change to make, and the follow-on for it.
- Out of scope here, and the natural follow-on: `verify --format json`, which this feature makes a rendering choice rather than a parsing project.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1 | Must Have | None | Completed |

---

## Definition of Done

The feature is complete when ALL of the following are true:

- [x] All acceptance criteria from user story are met
- [x] All vertical slices marked as "Completed"
- [x] All tests passing: `cargo nextest run`
- [x] Library documentation complete with examples: `cargo doc`
- [x] Code formatted: `cargo fmt --check`
- [x] No clippy warnings: `cargo clippy -- -D warnings`
- [x] README.md updated (no user-facing change; nothing to update)
- [x] CHANGELOG.md updated
