# Feature 14: Slot constraints — OWL characteristics + value bounds

**Feature:** Render a slot's OWL relationship characteristics and value
bounds — IR → HTML card badges → RDF axioms.

**User Story:** As a schema author grounding relations in OWL semantics, I
want `transitive` / `symmetric` / etc. on a slot, and `minimum_value` /
`maximum_value` on a numeric slot, to show on the slot card *and* emit the
corresponding OWL/RDF axioms, so the constraints I declare are visible to a
reader and checkable by a reasoner — not silently dropped.

**Approach:** Two small slices that reuse the existing characteristic-badge /
slot-card-row paths and the RDF property-emission path — no new abstractions,
localized extensions. Surfaced by the scimantic-schema dogfood (ch07 cluster 3
claim relations need property characteristics; cluster 2 `strength` /
`confidenceLevel` need `0.0..1.0` value bounds).

---

## Slice 1: OWL relationship characteristics

**Status:** ✅ Complete

**Priority:** Should Have

**User Value:** A slot declared `symmetric` / `asymmetric` / `reflexive` /
`irreflexive` / `transitive` (LinkML's relationship metaslots) previously
parsed and vanished — invisible on the card and absent from the RDF, so the
reasoning semantics the author declared were lost. Now each shows as a badge
and emits its `owl:<Name>Property` type axiom — the semantic payoff: a
reasoner can use `owl:TransitiveProperty` to infer the transitive closure.

**Acceptance Criteria:**
- [x] `SlotDefinition` gains five `#[serde(default, skip_serializing_if = "is_false")]` bools — `symmetric`, `asymmetric`, `reflexive`, `irreflexive`, `transitive` — auto-parsed from LinkML YAML by the serde-derived reader.
- [x] The slot card renders a characteristic badge per set flag (reusing the existing `Required` / `Identifier` badge path), and nothing for unset flags (`slot_card_shows_owl_characteristic_badges`).
- [x] The RDF serializers emit `<prop> rdf:type owl:<Name>Property` for each set flag, alongside the existing `owl:ObjectProperty` type (`build_rdf_graph_emits_owl_characteristic_axioms_for_slots`). Dogfood-verified: a `transitive: true, symmetric: true` slot emits both axioms in the generated TTL and both badges in the HTML.

**Notes:**
- Source: friction roadmap Gap #6 (claim relations, ch07 cluster 3). Graph rendering of characteristics is cheap/optional and deferred; Rust codegen is n/a.
- OWL technically scopes these characteristics to object properties; panschema emits whatever the author declared and leaves OWL-profile validation to a downstream reasoner / `linkml-validate`.

---

## Slice 2: Numeric value bounds (IR + card)

**Status:** ✅ Complete

**Priority:** Should Have

**User Value:** A numeric slot with `minimum_value` / `maximum_value` (e.g.
`strength`, `confidenceLevel` bounded `0.0..1.0`) shows the bound on its card,
so the constraint the author declared is visible while reading the docs
instead of being silently dropped.

**Acceptance Criteria:**
- [x] `SlotDefinition` gains `minimum_value` / `maximum_value` (`Option<f64>`), auto-parsed from YAML (`slot_definition_deserializes_value_bounds`).
- [x] The slot card shows the bounds as `≥ {min}` / `≤ {max}` badges — whole numbers without a trailing `.0` — read distinctly from the `min..max` *cardinality* badge (`slot_card_shows_value_bound_badges`).

**Notes:**
- Source: friction roadmap Gap #4 (ch07 cluster 2). Rust codegen is deferrable.

---

## Slice 2b: Value bounds as an OWL datatype restriction (RDF) — deferred

**Status:** 📋 Deferred

**Priority:** Could Have

**User Value:** Emit the bounds as an `owl:withRestrictions` datatype
restriction on the property's range — a blank `rdfs:Datatype` with
`owl:onDatatype` and an `owl:withRestrictions` list of `xsd:minInclusive` /
`xsd:maxInclusive` facets — so the constraint is reasoner-checkable, not just
visible.

**Why deferred:** The OWL facet is a T-box reasoner constraint. scimantic's
real-study dogfood validates instances with `linkml-validate` (against the
LinkML schema, not the OWL projection) and queries instance RDF with SPARQL,
so the facet isn't on its critical path. The RDF construction is also the
involved part (sophia blank nodes + an `rdf:List` + explicit `xsd:decimal`
literals — patterns not yet used in the serializers). Picked up when a
reasoning consumer actually needs it.

**Acceptance Criteria:**
- [ ] RDF serializers emit the `owl:withRestrictions` datatype restriction described above, with a test pinning the `xsd:minInclusive` / `xsd:maxInclusive` facet triples.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: OWL relationship characteristics | Should Have | None | ✅ Complete |
| Slice 2: Numeric value bounds (IR + card) | Should Have | None | ✅ Complete |
| Slice 2b: Value bounds OWL restriction (RDF) | Could Have | Slice 2 | 📋 Deferred |
