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
- [x] Generating a non-HTML format for a schema with non-empty `rules` warns that it isn't emitted there yet — `rules` is IR-modeled, so it no longer trips the feature-22 unmodeled-construct guard, but it also isn't projected by any writer but HTML until slice 4, and that gap must not be silent either. Generalized in [feature 23](23-cross-writer-construct-coverage-diagnostics.md) to a shared, format-aware mechanism (`classes_with_unprojected_constructs`) covering both `rules` and `unique_keys` (`cli_generate_non_html_warns_unprojected_constructs`).

**Notes:**
- Model the `slot_condition` fields panschema already surfaces elsewhere, plus `equals_string` / `equals_number` (the minimum needed for the User Story's motivating example — a precondition like "`status` = `actual`"); other exotic expression members are out of scope until a consumer needs them.
- The writer-projection warning (last AC) is a stopgap that [feature 23](23-cross-writer-construct-coverage-diagnostics.md) now owns and generalizes. It should stop reporting `rules` specifically once slice 4 lands, since `rules` would then be genuinely projected rather than merely warned-about.
- No graph-writer change is needed for the graph hover to show rules: the
  class-node hover clones the rendered HTML card's markup ("one source of
  truth"), so the Rules section appears in the hover automatically. The graph
  does *not* draw rules as a node/edge — they're not a binary relation.

---

### Slice 2: `unique_keys` (IR + card)

**Status:** Completed

**Priority:** Should Have

**User Value:** A class's uniqueness constraints show as a "Unique keys" row
listing each key's slot tuple.

**Acceptance Criteria:**
- [x] New `UniqueKey { unique_key_slots: Vec<String>, description: Option<String> }`; `ClassDefinition` gains `unique_keys: BTreeMap<String, UniqueKey>` (serde-default empty), auto-parsed (`class_definition_deserializes_unique_keys`).
- [x] The class card renders a "Unique keys" row per key, listing its slot tuple (`class_card_shows_unique_keys`).
- [x] Each referenced slot is checked against the class's effective slot set; an unresolved key slot is a diagnostic (`unresolved_unique_key_slots`, `cli_generate_warns_unresolved_unique_key_slot`).

**Notes:**
- This is documentation plus a structural check; enforcement against instance data is out of scope (the consuming application's job).
- Feature 07's `validate` surface (the AC's intended home for the structural check) isn't built yet, so the unresolved-key-slot check routes through the existing `generate`-time `eprintln!("warning: …")` path — the same stopgap [feature 23](23-cross-writer-construct-coverage-diagnostics.md)'s writer-projection warning uses. When feature 07 lands, it should call `unresolved_unique_key_slots` from the shared check path (and can gate it under `verify --strict`).
- Like `rules`, no graph-writer change is needed for the graph hover to show the Unique keys row: the class-node hover reuses the rendered HTML card. The graph draws no dedicated node/edge for `unique_keys`.

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

### Slice 4: SHACL projection of class constraints

**Status:** In Progress

**Priority:** Should Have — undeferred: three downstream consumers need a
machine-readable projection (nimbus's ch06 conditional rule; scidatica's
platform, which expects SHACL from panschema; scimantic-engine/t2t, which
load SHACL into oxigraph at runtime to validate triples before they hit
the store).

**User Value:** Emit `unique_keys` / `rules` / slot value-constraints as
SHACL shapes so they are machine-checkable by any SHACL engine, not just
visible in the docs.

**Design:** A standalone `ShaclWriter` (format id `shacl`, registered in
`FormatRegistry`), emitting a self-contained Turtle **shapes graph** — a
separate artifact from the OWL/TTL output, matching the `[generate.<name>]`
`shacl = "shapes.ttl"` shape consumers already expect (its manifest key is
wired separately, later). Reuses `rdf_serializers`' IRI derivation
(`expand_curie`, class/property IRIs, `map_linkml_to_xsd`) so every shape
targets the same class/property IRIs the OWL output declares. SHACL **Core**
only (no SHACL-AF) wherever expressible; the same `slot_conditions` →
predicate vocabulary [feature 24 slice 3](24-postgres-ddl-writer.md) maps to
SQL `CHECK` maps here to shape constraints.

**V&V:** generated shapes are loaded into `oxigraph` and shape triples
asserted via SPARQL (the [feature 27](27-rdf-owl-family-output-verification.md)
oracle). Full SHACL *validation* (a shape actually rejecting bad data)
needs a SHACL engine — no pure-Rust one exists yet, so that behavioral
tier is deferred, exactly as feature 28 slice 3 is for Postgres.

#### Slice 4a: `ShaclWriter` skeleton + base property shapes — walking skeleton

- [x] `ShaclWriter` implements `Writer` (`format_id() == "shacl"`), registered in `FormatRegistry::with_defaults` (`with_defaults_registers_shacl_writer`, `shacl_writer_format_id_is_shacl`).
- [x] One `sh:NodeShape` per class with `sh:targetClass <classIRI>`, and one `sh:property` shape per effective slot carrying: `sh:path <slotIRI>`; scalar range → `sh:datatype <xsd>`, single-valued class range → `sh:class <targetClassIRI>`; `required` → `sh:minCount 1`; `minimum_cardinality`/`maximum_cardinality` → `sh:minCount`/`sh:maxCount`; `pattern` → `sh:pattern`; `minimum_value`/`maximum_value` → `sh:minInclusive`/`sh:maxInclusive`. IRI derivation shared with the OWL graph via `rdf_serializers::{class_iri_string, slot_iri_string}` so shapes target the exact IRIs the OWL output declares (`every_class_gets_a_node_shape_targeting_its_iri`, `base_slot_constraints_project_to_property_shapes`, `a_scalar_slot_projects_to_sh_datatype`).
- [x] Output loads into `oxigraph` and the shape/target/constraint triples are SPARQL-assertable (the feature 27 oracle applied to the shapes graph).

#### Slice 4b: `rules` → conditional shapes

- [ ] Each rule with both pre/postconditions emits a shape encoding "if precondition then postcondition" in SHACL Core — `sh:or ( [ sh:not <preconditionShape> ] <postconditionShape> )`, the shape analogue of feature 24 slice 3's `NOT (pre) OR (post)` — with pre/post shapes built from the same `slot_conditions` field set. A rule not expressible this way is skipped with a diagnostic (reuse the `rules`-skip vocabulary from feature 24 slice 3).
- [ ] Stop reporting `rules` from [feature 23](23-cross-writer-construct-coverage-diagnostics.md)'s `classes_with_unprojected_constructs` for `format == "shacl"` — once `rules` is genuinely SHACL-projected, warning that it isn't would be a false signal.

#### Slice 4c: `unique_keys` → SPARQL constraint — optional

- [ ] SHACL Core has no native cross-instance uniqueness; a `unique_keys` tuple maps to a `sh:sparql` constraint (SHACL-AF). Build only if a consumer needs uniqueness machine-checked; otherwise `unique_keys` stays HTML/Postgres-only.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: `rules` | Must Have | None | Completed |
| Slice 2: `unique_keys` | Should Have | Feature 07 (shared check helper) | Completed |
| Slice 3: Boolean class expressions | Could Have | None | Not Started |
| Slice 4: SHACL/OWL projection | Could Have | Slices 1–2 | 📋 Deferred |

---

## Definition of Done

The feature is complete when ALL of the following are true:

- [x] Slices 1–2 acceptance criteria met (slice 3 optional; slice 4 deferred)
- [x] All tests passing: `cargo nextest run`
- [x] Library documentation complete: `cargo doc`
- [x] Code formatted + clippy clean: `cargo fmt --check` + `cargo clippy --all-targets --all-features -- -D warnings`
- [x] README.md updated — the "Loud about gaps" bullet already covers the diagnostics; individual modeled constructs aren't itemized in the feature list (consistent with `aliases`/`examples`/value-bounds)
- [x] CHANGELOG.md updated
- [x] [linkml-coverage.md](../linkml-coverage.md) ClassDefinition rows updated for the newly modeled constructs
