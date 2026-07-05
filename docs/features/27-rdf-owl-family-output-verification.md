# Feature 27: RDF/OWL writer family output verification & validation

**Feature:** Document the V&V already in place for the RDF-family writers
(`OwlWriter`/TTL, `JsonLdWriter`, `RdfXmlWriter`, `NTriplesWriter`, all
built on the shared `rdf_serializers::build_rdf_graph` core), and scope
an independent-oracle gap: nothing today checks the generated RDF graph
against anything other than panschema's own reader/serializers.

**User Story:** As a panschema maintainer, I want confidence that
generated RDF/OWL is graph-level sane and spec-conformant — not just
that four serializers agree with each other and my own reader parses
what my own writer wrote — so a structural mistake (a dangling blank
node, a malformed axiom) is caught by something that didn't share the
bug.

**Related ADR:** None.

**Approach:** Mostly retrospective; one real gap (no independent oracle)
scoped as a slice, with OWL-reasoner-level checking deferred until a
construct (e.g. feature 17 slice 4's `rules`→SHACL projection) actually
needs it — TDD spirit, not speculative.

---

## Current State — already in place

- **Serialization correctness by construction:** `sophia` (a real,
  standards-oriented RDF library) performs the actual serialization for
  every format in this family. Syntactic well-formedness for whichever
  format sophia targets is a property of using sophia correctly, not
  something panschema separately re-verifies from scratch.
- **Cross-format internal consistency:** `all_rdf_formats_produce_equivalent_content`
  writes the same schema through all four RDF writers and asserts they
  agree — the same triples, differently serialized. Strong signal that
  the shared `build_rdf_graph` core (not each format-specific serializer)
  is where the logic lives, so this test effectively pins that core.
- **Round-trip cross-check:** `owl_roundtrip_preserves_schema` writes TTL
  and reads it back through `OwlReader`, asserting equivalence to the
  original schema — "does my own reader accept what my own writer wrote
  and recover the same IR." Valuable, but it's an *internal* oracle
  (panschema checking panschema) — a shared blind spot in both reader and
  writer would pass silently.

## The gap

No **independent** tool ever loads the generated RDF and evaluates it —
nothing plays the role `rustc` plays for the Rust writer, or a real
browser plays for HTML. Sophia guarantees syntax; nothing today
evaluates graph-level sanity (e.g., "does every class actually have a
resolvable type triple") or OWL semantics.

---

## Vertical Slices

### Slice 1: Independent load-and-query oracle via `oxigraph`

**Status:** Completed

**Priority:** Should Have

**User Value:** Generated RDF is loaded into a real, independent triple
store and queried, catching graph-level mistakes the shared internal
core wouldn't self-report.

**Acceptance Criteria:**
- [x] Generated TTL loads cleanly into an `oxigraph` in-memory store with no parse errors (`oxigraph` is pure Rust — no JVM, consistent with panschema's own "no JVM" positioning, and test-only, never shipped) (`generated_ttl_loads_into_an_independent_triple_store`).
- [x] A handful of basic SPARQL sanity queries pass against representative fixtures — e.g. every class IRI has an `rdf:type owl:Class` triple, every declared `subClassOf`/`inverseOf`/mapping predicate resolves to a well-formed triple, no unexpected dangling blank nodes (`every_class_has_an_owl_class_type_triple_in_the_independent_store`).
- [x] The oracle itself is proven to have teeth — a test feeds it syntactically invalid Turtle and asserts the load actually fails (`oxigraph_rejects_malformed_turtle`), so the check can't be vacuously green.

**Notes:**
- Fast tier — in-memory, no Docker, runs on every test invocation.

---

### Slice 2: OWL-DL / SHACL consistency checking — deferred until a real construct needs it

**Status:** 📋 Deferred

**Priority:** Won't Have (until triggered)

**User Value:** Once panschema emits axioms sophisticated enough that
logical inconsistency becomes a real risk (e.g. [feature 17 slice
4](17-class-validation-constructs.md)'s eventual `rules`→SHACL/OWL
projection), a real reasoner confirms the emitted axioms are actually
consistent, not just syntactically present.

**Acceptance Criteria:**
- [ ] (when undeferred) A real OWL reasoner (HermiT, ELK, or similar) or SHACL validator checks generated axioms for consistency, invoked only in tests.

**Notes:**
- Likely needs a JVM-based tool (the mainstream OWL reasoners are Java) —
  acceptable for a test-only dependency never shipped to consumers, but
  worth a final look for a pure-Rust alternative before reaching for one.
  Deferred until feature 17 slice 4 (or another construct) actually
  produces axioms worth reasoning over — nothing today does.

---

### Slice 3: W3C RDF/Turtle test-suite conformance — low priority

**Status:** Not Started

**Priority:** Could Have

**User Value:** Pins confidence in the `sophia` dependency itself against
the canonical W3C test suite, rather than panschema's own logic.

**Acceptance Criteria:**
- [ ] (if pursued) A subset of the official W3C RDF/Turtle test-suite cases run against sophia's output for schemas panschema controls.

**Notes:**
- Lower priority than slices 1–2 — this mostly validates a dependency,
  not panschema's own writer logic.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: `oxigraph` load-and-query oracle | Should Have | None | Completed |
| Slice 2: OWL-DL/SHACL consistency checking | Won't Have (until triggered) | A real axiom-emitting construct | 📋 Deferred |
| Slice 3: W3C test-suite conformance | Could Have | None | Not Started |

---

## Definition of Done

- [x] Serialization-correctness-by-construction (sophia) and cross-format/round-trip internal checks exist and run in CI
- [x] Slice 1 acceptance criteria met
- [ ] Slice 2 only when its trigger condition (a real axiom-emitting construct) is met
- [x] All tests passing: `cargo nextest run --features dev`
- [x] CHANGELOG.md updated (slice 1)
