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

## Slice 2: Numeric value bounds

**Status:** 📋 Planned

**Priority:** Should Have

**User Value:** A numeric slot with `minimum_value` / `maximum_value` (e.g.
`strength`, `confidenceLevel` bounded `0.0..1.0`) should show the bound on its
card and emit it as an OWL datatype restriction, so the range constraint is
both visible and reasoner-checkable.

**Acceptance Criteria:**
- [ ] `SlotDefinition` gains `minimum_value` / `maximum_value` (`Option<f64>`), auto-parsed from YAML.
- [ ] The slot card shows the bounds (reusing a card row / badge), distinct from the `min..max` *cardinality* badge.
- [ ] The RDF serializers emit an `owl:withRestrictions` datatype restriction on the property's range — a blank `rdfs:Datatype` with `owl:onDatatype` and an `owl:withRestrictions` list of `xsd:minInclusive` / `xsd:maxInclusive` facets.
- [ ] Tests cover the IR parse, the card render, and the RDF facet emission.

**Notes:**
- Source: friction roadmap Gap #4 (ch07 cluster 2). Under feature 07's validation family conceptually, but the work here is *rendering/emission*, not validation. Rust codegen is deferrable.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: OWL relationship characteristics | Should Have | None | ✅ Complete |
| Slice 2: Numeric value bounds | Should Have | None | 📋 Planned |
