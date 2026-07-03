# Feature 17: Class-level validation constructs — unique_keys, rules, boolean expressions

**Feature:** Model and surface LinkML's class-level constraint constructs —
`unique_keys`, `rules`, and boolean class expressions (`all_of` /
`exactly_one_of` / `none_of`) — IR → HTML constraint section, with the RDF/SHACL
projection deferred.

**User Story:** As a schema author expressing data-integrity rules ("an AWS
offering must declare a region"; "name is unique per environment"), I want
`unique_keys` and `rules` on a class modeled and documented, so the constraints
I declare are visible to readers (and available to a future validator) instead
of being silently dropped.

**Related ADR (if applicable):** None — complements
[feature 07](07-schema-validation.md). Feature 07 checks a schema is
*structurally* well-formed (references resolve); this feature models and renders
the *constraint constructs themselves*. Together they set up a future
data-level validation / SHACL path.

**Approach:** Vertical Slicing with Outside-In TDD. The most involved of the
documentation gaps — `rules` is structured (pre/postconditions over
`slot_conditions`) and boolean expressions nest. Documentation-first: model the
constructs and render them human-readably on the class card. RDF/SHACL emission
is deferred — panschema has no SHACL writer, and in the project's division of
labor instance-data enforcement lives in the consuming application, not the doc
generator. Start with the simplest high-value construct.

---

## Why Now

[linkml-coverage.md](../linkml-coverage.md) names `rules` + `unique_keys` as
"the high-value validation gaps" (ClassDefinition row; priority gap 4). They are
parsed-and-dropped today. Making the declared constraints *visible in the docs*
is valuable on its own and is the prerequisite for any later machine-checkable
projection.

---

## Vertical Slices

### Slice 1: `rules` (IR + card)

**Status:** Completed

**Priority:** Must Have — the conditional-requirement half of the User
Story (e.g. "an actual deployment must name its environment and
provider"); built before `unique_keys` because it's the construct a real
consumer is currently blocked on.

**User Value:** A class's conditional rules render as a readable "Rules" section
(e.g. "if `provider` = AWS then `region` is required").

**Acceptance Criteria:**
- [x] New IR for `ClassRule` with `preconditions` / `postconditions` (each an anonymous class expression carrying `slot_conditions`), plus optional `title` / `description`; `ClassDefinition` gains `rules: Vec<ClassRule>`.
- [x] The serde-derived reader parses the nested `slot_conditions` map — slot name → the constraint subset panschema already renders (`range` / `required` / cardinality / value bounds / `pattern`), plus `equals_string` / `equals_number` (LinkML's slot-condition equality checks — needed for a precondition like "`status` = `actual`"; not otherwise expressible by the renders-elsewhere subset) (`class_definition_deserializes_rules`).
- [x] The class card renders each rule as its description plus a human-readable "when … then …" rendering of its pre/postconditions, e.g. "when `status` = `actual`, then `region` is required" (`class_card_shows_rules`).
- [x] Generating a non-HTML (RDF) format for a schema with non-empty `rules` warns that they aren't emitted to RDF/OWL yet — `rules` is IR-modeled, so it no longer trips the feature-22 unmodeled-construct guard, but it also isn't RDF-projected until slice 4, and that gap must not be silent either (`cli_generate_rdf_warns_rules_not_emitted`, `classes_with_rules_unsupported_in_rdf_*`).

**Notes:**
- Model the `slot_condition` fields panschema already surfaces elsewhere, plus `equals_string` / `equals_number` (the minimum needed for the User Story's motivating example — a precondition like "`status` = `actual`"); other exotic expression members are out of scope until a consumer needs them.
- The RDF-gap warning (last AC) is a stopgap, not a general mechanism — it is
  specific to `rules`. It should be removed once slice 4 lands, since `rules`
  would then be genuinely RDF-projected rather than merely warned-about.

---

### Slice 2: `unique_keys` (IR + card)

**Status:** Not Started

**Priority:** Should Have

**User Value:** A class's uniqueness constraints show as a "Unique keys" row
listing each key's slot tuple.

**Acceptance Criteria:**
- [ ] New `UniqueKey { unique_key_slots: Vec<String>, description: Option<String> }`; `ClassDefinition` gains `unique_keys: BTreeMap<String, UniqueKey>` (serde-default empty), auto-parsed (`class_definition_deserializes_unique_keys`).
- [ ] The class card renders a "Unique keys" row per key, listing its slot tuple (`class_card_shows_unique_keys`).
- [ ] Each referenced slot is checked against the class's effective slot set; an unresolved key slot is a diagnostic routed through the [feature 07](07-schema-validation.md) check path (shared helper).

**Notes:**
- This is documentation plus a structural check; enforcement against instance data is out of scope (the consuming application's job).

---

### Slice 3: Boolean class expressions (`all_of` / `exactly_one_of` / `none_of`)

**Status:** Not Started

**Priority:** Could Have

**User Value:** Class-level boolean combinations render on the card, mirroring
the existing slot-level `any_of`.

**Acceptance Criteria:**
- [ ] `ClassDefinition` gains `all_of` / `exactly_one_of` / `none_of` (lists of anonymous class expressions); slot `any_of` already exists, this models the class-level set.
- [ ] The card renders each as a labeled list of member expressions, reusing the slot `any_of` rendering idiom for consistency (`class_card_shows_boolean_expressions`).

---

### Slice 4: SHACL / OWL projection of class constraints — deferred

**Status:** 📋 Deferred

**Priority:** Could Have

**User Value:** Emit `unique_keys` / `rules` as SHACL shapes (or OWL
restrictions) so they are machine-checkable, not just visible.

**Why deferred:** panschema has no SHACL writer, and instance-data validation is
the consuming application's responsibility in the current architecture — the doc
generator documents constraints, it does not enforce them against an A-box.
Picked up when a validation consumer needs a machine-readable projection; pairs
naturally with a `panschema validate --data` surface extending
[feature 07](07-schema-validation.md).

**Acceptance Criteria:**
- [ ] (when undeferred) Emit a SHACL `sh:NodeShape` per class with property-shape constraints mirroring `unique_keys` and the renderable rule subset, with tests pinning the shape triples.
- [ ] (when undeferred, for `rules` specifically) Remove slice 1's interim `classes_with_rules_unsupported_in_rdf` warning — once `rules` is genuinely RDF-projected, warning that it isn't would itself be a false signal.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: `rules` | Must Have | None | Completed |
| Slice 2: `unique_keys` | Should Have | Feature 07 (shared check helper) | Not Started |
| Slice 3: Boolean class expressions | Could Have | None | Not Started |
| Slice 4: SHACL/OWL projection | Could Have | Slices 1–2 | 📋 Deferred |

---

## Definition of Done

The feature is complete when ALL of the following are true:

- [ ] Slices 1–2 acceptance criteria met (slice 3 optional; slice 4 deferred)
- [ ] All tests passing: `cargo nextest run`
- [ ] Library documentation complete: `cargo doc`
- [ ] Code formatted + clippy clean: `cargo fmt --check` + `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] README.md updated
- [ ] CHANGELOG.md updated
- [ ] [linkml-coverage.md](../linkml-coverage.md) ClassDefinition rows updated for the newly modeled constructs
