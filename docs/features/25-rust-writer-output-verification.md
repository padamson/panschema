# Feature 25: Rust writer output verification & validation

**Feature:** Document the real-compiler V&V already in place for
`RustWriter`'s output, and scope what (if anything) to add — the
reference case for "validate against the actual target language, not
just your own expectations of it."

**User Story:** As a panschema maintainer, I want confidence that
generated Rust always compiles and behaves like real Rust — not just
that it matches a string I hand-wrote as the expected test output — so a
writer regression is caught before it reaches a consumer's `cargo build`.

**Related ADR:** None. Builds on [feature 20](20-dogfood-schema-regression-fixtures.md),
which extends this same harness to real, released dogfood schemas.

**Approach:** Mostly retrospective — this writer already has the
strongest V&V of any in panschema. Document it so the other writer V&V
docs ([26](26-html-graph-viz-output-verification.md),
[27](27-rdf-owl-family-output-verification.md),
[28](28-postgres-ddl-writer-output-verification.md)) have a concrete
target to match, and scope any real remaining gaps as slices.

---

## Why This Doc

Reading LinkML's actual `relmodel_transformer.py` source (while scoping
[feature 24](24-postgres-ddl-writer.md)) surfaced two real issues in the
Postgres writer that hand-written unit tests alone hadn't caught — both
were about whether the *target language's own rules* were satisfied, not
whether the Rust code matched its author's expectations. The Rust writer
already avoids this trap: `rustc` itself is the oracle, not this
project's own assumptions about what valid Rust looks like. This doc
exists to name that pattern explicitly so it can be replicated (or
consciously not replicated, where the cost doesn't fit) for every writer.

---

## Current State — already in place

Two tiers, cheap-fast and thorough-slow, exactly the shape every other
writer's V&V doc in this set aims for:

- **Fast, every test run:** `syn::parse_file` on generated output
  (dev-dependency `syn`, `features = ["full"]`). Catches syntax errors
  with no compile cost. Documented in `Cargo.toml` as deliberately
  *not* catching semantic errors (unresolved types, missing imports) —
  that's the next tier's job.
- **Thorough, real compiler:** `codegen_fixture_compiles_and_round_trips_in_downstream_crate`
  and the `scimantic_*_can_be_constructed_in_downstream_crate` tests
  actually `cargo build` the generated code in a scratch downstream
  crate. `rustc` is a genuine external oracle — it validates syntax
  *and* types, catching exactly the class of error `syn` can't.
- **Real-schema regression** ([feature 20](20-dogfood-schema-regression-fixtures.md)):
  every vendored release of the real dogfood schemas is parsed (`syn`)
  on every run; the latest release per schema is additionally
  `cargo build`-compiled. Extends the same two-tier harness to schemas
  panschema doesn't control the shape of.
- Generated code is explicitly excluded from `cargo fmt` / `clippy` on
  the *consumer* side (`#![cfg_attr(rustfmt, rustfmt_skip)]`,
  `#[allow(clippy::all)]`) — deliberate, and orthogonal to correctness:
  the writer's own test suite still runs `cargo fmt`/`clippy` against
  panschema's *own* source, just not against what it generates for
  others.

No unmet Must-Have exists today — this doc's slices are genuinely
optional, added only if a real gap surfaces (TDD spirit: build the next
tier when a regression demonstrates the current tiers missed it, not
speculatively).

---

## Vertical Slices

### Slice 1: Widen dogfood full-compile coverage — as compile time allows

**Status:** Not Started

**Priority:** Could Have

**User Value:** More than just the latest release of each dogfood schema
gets real-compiler coverage, catching a regression that only affects an
older schema shape.

**Acceptance Criteria:**
- [ ] If total CI time budget allows, widen feature 20's "latest release only" `cargo build` policy to more (or all) vendored versions; log whichever policy is chosen so it isn't a silent cap (per feature 20's own note).

---

### Slice 2: Synthetic edge-case generator — only if hand-written fixtures prove insufficient

**Status:** Not Started

**Priority:** Could Have

**User Value:** Edge cases hand-written fixtures don't happen to cover
(deep `is_a` chains, exotic keyword collisions, unusual `any_of` nesting)
get exercised automatically.

**Acceptance Criteria:**
- [ ] (when triggered by a real missed-edge-case bug) A generator producing varied synthetic schemas, fed through the writer + `syn`/compile check.

**Notes:**
- Explicitly not built speculatively — only stand this up after a real
  bug demonstrates hand-written coverage has a gap generation would have
  caught.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: Widen dogfood compile coverage | Could Have | Feature 20 | Not Started |
| Slice 2: Synthetic edge-case generator | Could Have | A demonstrated gap | Not Started |

---

## Definition of Done

This doc's own bar is already met by existing infrastructure — it exists
to document that, not to drive new required work:

- [x] Fast-tier (`syn`) and thorough-tier (real `cargo build`) both exist and run in CI
- [x] Real-schema regression coverage exists (feature 20)
- [ ] Any slice above, only if and when its trigger condition is met
