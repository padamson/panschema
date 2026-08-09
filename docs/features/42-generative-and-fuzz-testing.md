# Feature 42: Generative and Fuzz Testing

**Feature:** Tests that supply their own inputs — properties checked over
generated schemas, and the readers hardened against arbitrary bytes.

**User Story:** As a maintainer of a tool that parses files other people
author and emits code, DDL, and RDF that other tools consume, I want the
test suite to try inputs I did not think of, so that the failure surfaces
here rather than in a consumer's build.

**Approach:** Vertical Slicing with Outside-In TDD.

---

## Why now, and what this is not

The suite has ~1200 example-based tests plus mutation testing per push.
Those answer two different questions well and a third not at all:

| Technique | Question it answers | Blind spot |
|---|---|---|
| Example tests | Does the behaviour I described hold? | Only the inputs I wrote |
| Mutation testing | Are my assertions strong enough to notice a change? | Only ever runs my existing inputs — it perturbs the *code* |
| **Generative / fuzz** | **Is there an input I never considered?** | No oracle beyond the properties you state |

Mutation testing cannot find an unconsidered input, by construction. That
is the gap this feature fills, and it is worth filling here specifically
because panschema is a **parser plus a family of serializers** — the shape
where generated inputs earn their keep.

**A naming trap worth recording.** In fuzzing, "mutation-based" describes a
fuzzer that mutates *seed inputs* (versus generation-based, which builds
inputs from a grammar). It is what `cargo-fuzz`/libFuzzer already does by
default. It is **not** a bridge between mutation testing and fuzzing, and
the collision with `cargo-mutants` is coincidental — the two perturb
different things. The real bridge is property-based testing, which is
slice 1.

**Deliberately not doing:** fuzzing the writers directly with arbitrary
bytes. Writers take the IR, not bytes; generated `SchemaDefinition` values
(slice 1) cover them better and shrink to a readable counterexample.

---

## Implementation Strategy

Order is cheapest-signal-first. Slice 1 runs inside `cargo nextest` in
milliseconds and needs no new toolchain, corpus, or CI job — so it lands
first and stays in the push gate. Slice 2 needs all three and therefore
lives outside the gate, on the same footing as full-codebase mutation runs.

---

## Vertical Slices

### Slice 1: Round-trip and determinism properties over generated schemas

**Status:** Complete

**Priority:** Should Have

**User Value:** A generated schema that panschema writes and reads back
means the same thing, and writing it twice produces the same bytes —
checked over hundreds of shapes per run instead of the handful anyone
thought to write down.

The two properties are chosen because each has a **real oracle**, which is
what separates a useful property from a tautology:

- **Round-trip:** `schema → TTL → OwlReader → schema` should preserve what
  the RDF layer models. The oracle is the input itself. This is the one
  that finds writer/reader disagreements, because unit tests on either side
  share the author's assumptions about the format; a round-trip does not.
- **Determinism:** the same schema rendered twice is byte-identical. Not a
  tautology — it is a live requirement (`verify`'s regenerate-and-diff
  gate, and migrations a runner checksums), and one format already fails
  it (see *Known exclusion*).

**Acceptance Criteria:**
- [x] A `SchemaDefinition` generator produces varied but *valid* schemas —
  classes, slots, enums, ranges that resolve, and the metadata the RDF
  layer carries — so a failure means a real defect rather than an input
  panschema never claimed to accept.
- [x] Round-tripping a generated schema through the OWL writer and reader
  preserves the constructs the RDF layer models, compared on a stated
  normal form rather than on raw struct equality.
- [x] Rendering a generated schema twice through each byte-stable writer
  produces identical bytes.
- [x] A failing case is reported **shrunk** to a minimal schema, so the
  counterexample is readable.
- [x] The properties run in the ordinary suite, in seconds, with no
  external toolchain.
- [x] Any construct the round-trip cannot preserve is named explicitly in
  the property (an allow-list with a reason), not silently normalized away
  — the exclusions are findings, and hiding them turns a real oracle into
  a tautology.

**Notes:**
- Generated values come from the IR types, so the generator is the place
  where "what is a valid schema?" gets written down. Expect that to
  surface disagreements between what the IR permits and what the writers
  assume.
- **Known exclusion: JSON-LD is not byte-stable** (July review finding 10,
  reconfirmed 2026-08-07). It is excluded from the determinism property
  with that reason recorded, not quietly skipped.
- Shrinking is the feature that makes this usable. A 40-class random
  counterexample is not actionable; a two-class one is.
- **First run, three findings** — the round-trip property failed before it
  ever went green, each time shrunk to a minimal schema:
  1. **Fixed:** the OWL reader stored every property at schema level with
     `domain` set but never gave the owning class the slot, so every
     resolver-driven writer projected empty classes from *any* OWL/TTL
     input — including the reference ontology. Masked everywhere because
     no writer test used a TTL input, and the HTML docs render slots as a
     slot-centric section with back-links, which looks complete.
  2. **Open, asserted in the property as a known asymmetry:** an enum
     emits as `owl:Class` + `owl:oneOf` and the reader has no rule to
     rebuild an `EnumDefinition`, so an enum returns as a class. The
     property pins the wrong behaviour so a change in either direction
     surfaces.
  3. **Open, excluded from generation with the reason written at the
     exclusion:** two classes declaring same-named attributes are
     class-local in LinkML but mint one property IRI in RDF — only one
     class keeps the slot on read-back, and two `rdfs:domain` triples mean
     *intersection* in OWL, so the emitted semantics are wrong before the
     reader is involved. Distinct per-class IRIs are an output-breaking
     change, so this is tracked rather than fixed here.

---

### Slice 3: Reader equivalence — one IR whichever reader produced it

**Status:** Complete

**Priority:** Should Have

**User Value:** The same schema expressed as LinkML YAML and as OWL/Turtle
produces the same effective IR, checked over generated schemas — the
"IR looks the same regardless of the reader" invariant tested directly
rather than inferred through a round-trip.

**Acceptance Criteria:**
- [x] A generated schema serialized to YAML and to TTL, read back through
  the respective readers, yields the same classes and effective slots on a
  stated normal form.
- [x] Divergences the formats genuinely cannot share are named exclusions
  with reasons, not silent normalizations.

**Notes:**
- The property went green on first run *because* it inherits slice 1's two
  exclusions (enums filtered on the TTL side; the generator avoids
  same-named attributes). Equivalence holds everywhere else.
- **Finding 3 gained an authoring-boundary guard** rather than waiting for
  the IRI fix: the shared load path now reports colliding slot definitions
  (same name defined at several sites → one RDF property IRI), and
  `--strict` fails on them. Verified against the three consumer schemas
  currently in authoring — all pass — and it caught a genuine collision in
  panschema's own `wine_catalog` fixture, which was converted to shared
  top-level slots. The per-declaring-class IRI scheme remains the real
  fix, tracked as an output-breaking change.

---

### Slice 2: Fuzz the readers against arbitrary bytes

**Status:** Complete (scaffolding + smoke campaign; long campaigns are the
on-demand job's business)

**Priority:** Could Have

**User Value:** A malformed or hostile schema file makes the reader fail
with an error, never a panic or a hang — including files fetched from a
`github:` dependency, which is the one input panschema takes that its
operator did not write.

**Acceptance Criteria:**
- [x] `OwlReader` and `YamlReader` each have a fuzz target taking arbitrary
  bytes (`fuzz/fuzz_targets/`), exercising the readers' path-based entry
  points rather than an internal parse function. **Scope is reader-level
  hardening only:** the `github:`-dependency threat actually enters through
  `import_resolve::load_schema_with_deps`, whose untrusted-input logic —
  the path-escape guard, import cycle detection, extension probing, and
  transitive re-reads — a single-`&[u8]` harness structurally cannot
  reach, since it needs a multi-file import graph. Fuzzing that layer
  needs a structured harness (generate an import *graph*, not a byte
  string) and is the follow-up slice to file if that surface grows.
- [x] Neither panics, aborts, nor hangs on any input the campaign reaches;
  a malformed file is a returned error. Held through the initial smoke
  campaign; every future campaign re-earns it.
- [x] A seed corpus is committed from the existing fixtures
  (`fuzz/seeds/<target>/`), so a campaign starts from real schema shapes
  rather than from noise. The *working* corpus a campaign grows
  (`fuzz/corpus/`, thousands of hash-named files) stays gitignored — the
  seeds are the repo's contract, the growth is machine state.
- [x] Any crash found is committed as a regression fixture and fixed with
  an ordinary test, so the suite keeps the finding without needing the
  fuzzer to rediscover it — exercised for real on this slice's first
  extended campaign (see below), and the reader-hardening tests that
  preceded it (smuggled nil, cyclic `rdf:rest`, XSD-alias collision) are
  the pattern.

**First real find (2026-08-08): the dependency Turtle parser panics on
malformed input.** A mutated seed hits `assertion failed: !txt.is_empty()`
inside sophia_turtle 0.10 — current latest; no upstream fix exists. The
consumer-facing fix is a `catch_unwind` boundary in `parse_ontology`
(a malformed file is a returned error whoever's code chokes), pinned by
the `turtle_parser_panic.ttl` regression fixture. **The fuzz target cannot
be shielded the same way:** cargo-fuzz builds with `panic=abort`, where
`catch_unwind` is a no-op — so owl_reader campaigns will keep re-finding
this class and ending early until the assertion is fixed upstream. A crash
artifact whose reproduction panics at sophia's `_generic_source.rs`
`!txt.is_empty()` assertion is this known bug, not a new finding. Filing
the upstream issue is the real fix's trigger.
- [x] Fuzzing runs on demand (`fuzz.yml`, `workflow_dispatch` with a
  configurable duration), never in the push gate — the same footing as
  full-codebase mutation runs. A schedule can be added if on-demand runs
  prove too easy to forget.

**Notes:**
- `cargo-fuzz` needs a nightly toolchain, which is why this is its own
  slice and its own job rather than something bolted to the existing CI.
- The consumer-facing motivation is concrete: `panschema add
  github:owner/repo@x.y.z` fetches and parses a file the operator did not
  author. Tar extraction on that path is already hardened (July review's
  "verified clean"); the *parsers* are not yet.
- Depends on nothing in slice 1, but ordered after it because slice 1 has
  the better cost-to-signal ratio.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: round-trip + determinism properties | Should Have | None | Complete |
| Slice 2: reader fuzz targets | Could Have | None | Complete |
| Slice 3: reader equivalence | Should Have | Slice 1 | Complete |

---

## Things to watch

- **A property with no oracle is a tautology.** "It doesn't panic" is worth
  little for a pure transform. Round-trip and determinism are in scope
  because each has something real to compare against.
- **Generator bias is invisible.** If the generator never emits a class
  with a mixin, the properties silently do not cover mixins. Coverage of
  the generator's own output is worth checking before trusting a green run.
- **Do not let the properties duplicate the example suite.** They exist to
  cover the input space between the examples, not to restate them.
- Per the repo's standing policy, heavy runs stay out of the per-push gate:
  full-codebase mutation testing is manual, and fuzzing takes the same
  footing.
