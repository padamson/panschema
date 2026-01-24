# [Feature Name] - Implementation Plan

**Feature:** [Short feature name]

**User Story:** [As a <role>, I want to <capability>, so that <benefit>]

**Related ADR (if applicable):** [Link to ADR if this feature involves major architectural decisions]

**Approach:** Vertical Slicing with Outside-In TDD

---

## Implementation Strategy

This implementation follows **vertical slicing** - each slice delivers end-to-end user value and can be tested/released independently.

*When developing this implementation plan, also consider the following documentation, and note any updates to documentation required by the user story implementation:*
- [Main README](../../README.md)
- [WHY](../../WHY.md)

---

## Vertical Slices

### Slice 1: [Walking Skeleton - Simplest Valuable Feature]

**Status:** [Not Started | In Progress | Completed]

**User Value:** [What can users do after this slice? One sentence description of end-to-end value.]

**Acceptance Criteria:**
- [ ] [Criterion 1 - e.g. "CLI accepts --input flag"]
- [ ] [Criterion 2 - e.g. "Output valid HTML to stdout"]
- [ ] [Criterion 3 - e.g. "Error on file not found"]

**Notes:**
- [High-level architectural decisions or trade-offs]
- [Out of scope items for this slice]

---

### Slice 2: [Build on Slice 1 - Add Next Valuable Feature]

**Status:** [Not Started | In Progress | Completed]

**User Value:** [What additional value does this slice provide?]

**Acceptance Criteria:**
- [ ] [Criterion 1]
- [ ] [Criterion 2]

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On |  Status |
|-------|----------|------------|--------|
| Slice 1 | Must Have | None | [Status] |
| Slice 2 | Must Have | Slice 1 | [Status] |

---

## Definition of Done

The feature is complete when ALL of the following are true:

- [ ] All acceptance criteria from user story are met
- [ ] All vertical slices marked as "Completed"
- [ ] All tests passing: `cargo nextest run`
- [ ] Library documentation complete with examples: `cargo doc`
- [ ] Code formatted: `cargo fmt --check`
- [ ] No clippy warnings: `cargo clippy -- -D warnings`
- [ ] README.md updated
- [ ] CHANGELOG.md updated
