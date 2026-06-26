# Feature 16: Lifecycle & editorial metadata — deprecated, aliases, see_also, examples

**Feature:** Model and render the common-metadata editorial metaslots LinkML
defines on every element — `deprecated`, `aliases`, `see_also`, `examples`,
`comments`, `notes` — IR → HTML doc body → RDF where a standard predicate
exists.

**User Story:** As a maintainer of a living vocabulary, I want to mark an
element `deprecated`, give it `aliases`, `see_also` links, and worked
`examples`, and have them render in the docs (and emit to RDF where meaningful),
so consumers see lifecycle and usage guidance instead of those fields being
silently dropped.

**Related ADR (if applicable):** None — additive fields on the
common-metadata-bearing structs in [linkml.rs](../../panschema/src/linkml.rs),
rendered through the existing card paths from
[feature 02](02-core-ontology-documentation.md).

**Approach:** Vertical Slicing with Outside-In TDD. Each metaslot is a small,
serde-default field on the structs that carry LinkML's `common_metadata` mixin
(`SchemaDefinition`, `ClassDefinition`, `SlotDefinition`, `EnumDefinition`,
`TypeDefinition`), auto-parsed by the serde-derived reader and surfaced via the
existing card-row / badge path and the RDF predicate-emission path from
[feature 14](14-slot-constraints.md). Low individual cost, high collective
coverage; sliced by rendering treatment, highest lifecycle value first.

---

## Why Now

[linkml-coverage.md](../linkml-coverage.md) flags the editorial/provenance long
tail as "the biggest doc-completeness gap" (common-metadata row; priority gap
5). For a *living* vocabulary the two that bite first are `deprecated` (sunset a
service type without deleting it) and `examples` (show what a valid value looks
like) — both currently parsed-and-dropped by serde with no error.

---

## Vertical Slices

### Slice 1: `deprecated` (lifecycle)

**Status:** Complete

**Priority:** Should Have

**User Value:** An element marked `deprecated:` shows a "Deprecated" badge plus
the deprecation note on its card — a sunset service type / slot reads as sunset
instead of looking current.

**Acceptance Criteria:**
- [x] The five common-metadata structs gain `deprecated: Option<String>`, auto-parsed from YAML (`class_definition_deserializes_deprecated`).
- [x] Each card renders a "Deprecated" badge plus the note text when set, and nothing when unset (`class_card_shows_deprecated_badge`), reusing the existing badge path.
- [x] RDF emits `owl:deprecated true` on the element IRI when set (`build_rdf_graph_emits_owl_deprecated`).

**Notes:**
- Graph rendering of the badge is cheap/optional and deferred; the card badge is the high-value surface.

---

### Slice 2: `aliases` + `see_also`

**Status:** Not Started

**Priority:** Should Have

**User Value:** Alternative names and related-resource links render on the card
and round-trip to RDF, so synonyms and cross-references are discoverable.

**Acceptance Criteria:**
- [ ] Structs gain `aliases: Vec<String>` and `see_also: Vec<String>` (URIorCURIE), serde-default empty.
- [ ] The card shows an "Aliases" row (comma-joined) and a "See also" row of links — CURIE-expanded, reusing the existing mapping-link path (`class_card_shows_aliases_and_see_also`).
- [ ] RDF emits `skos:altLabel` per alias and `rdfs:seeAlso` per `see_also` entry.

---

### Slice 3: `examples`

**Status:** Not Started

**Priority:** Should Have

**User Value:** Worked examples render as a card section, so a reader sees what a
valid value looks like.

**Acceptance Criteria:**
- [ ] New `Example { value: String, description: Option<String> }`; structs gain `examples: Vec<Example>`, auto-parsed (LinkML `examples` is a list of structured `example` objects).
- [ ] The card renders an "Examples" section listing each value with its optional description (`slot_card_shows_examples`).

**Notes:**
- RDF has no standard predicate for `examples`; RDF emission is out of scope.

---

### Slice 4: `comments` + `notes` (editorial text)

**Status:** Not Started

**Priority:** Could Have

**User Value:** Editorial `comments` / `notes` render as text rows for
documentation completeness.

**Acceptance Criteria:**
- [ ] Structs gain `comments: Vec<String>` and `notes: Vec<String>`, serde-default empty.
- [ ] The card renders them as text rows visually distinct from `description` (`class_card_shows_comments_and_notes`).

**Notes:**
- `todos` is author-facing; deferred (could later surface behind a verbose/author mode).
- `in_subset` belongs to the separate subsets gap ([linkml-coverage.md](../linkml-coverage.md) priority gap 8), not here.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: `deprecated` | Should Have | None | Complete |
| Slice 2: `aliases` + `see_also` | Should Have | None | Not Started |
| Slice 3: `examples` | Should Have | None | Not Started |
| Slice 4: `comments` + `notes` | Could Have | None | Not Started |

---

## Definition of Done

The feature is complete when ALL of the following are true:

- [ ] Slices 1–3 acceptance criteria met (slice 4 optional)
- [ ] All tests passing: `cargo nextest run`
- [ ] Library documentation complete: `cargo doc`
- [ ] Code formatted + clippy clean: `cargo fmt --check` + `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] README.md updated
- [ ] CHANGELOG.md updated
- [ ] [linkml-coverage.md](../linkml-coverage.md) common-metadata rows updated for the newly modeled metaslots
