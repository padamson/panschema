# Feature 30: Cross-package schema imports & codegen composition

**Feature:** Develop a schema in one repository and consume it in application
repositories, in two composition modes — **inline** (the imported schema's
definitions are materialized into the app's generated output) and **crate**
(the schema becomes its own versioned Rust crate the app depends on) — with
exact-version pinning as the default, an explicit `update`, and an `outdated`
report. Builds directly on the shared load pipeline from
[feature 29](29-schema-load-pipeline-and-writer-consistency.md) and the
package manager (manifest / lockfile / source cache / `fetch`) from
[feature 05](05-schema-manager.md), and completes the cross-package half of
[feature 15](15-multi-file-schema-modularity.md)'s import story.

**User Story:** As someone developing a schema (e.g. `scimantic-schema`) that
several applications consume, I want each app to consume that schema across
the fetch/cache boundary — either
merged into the app's own generated types, or referenced as a shared,
versioned crate so both apps agree on one type — and I want every consumed
version pinned exactly and bumped deliberately, so a data-model change is
never a silent surprise.

**Related ADR:** [003 (LinkML as internal representation)](../adr/003-linkml-as-internal-representation.md),
[004 (Reader/Writer architecture)](../adr/004-reader-writer-architecture.md).

**Approach:** Vertical Slicing with Outside-In TDD. Inline (A1) is the
default, zero-config path and ships first; crate (A2) is a strict superset —
the same import resolver and IR, plus two new Rust-writer abilities. Every
slice is proven end-to-end by a self-contained two-package fixture in
panschema's own suite (no dependency on an external repo — see the project's
self-contained-tests rule).

## Why Now

- Feature 29 landed the one shared load path (`load_schema`) that read →
  resolve imports → schema-level diagnostics run through. Cross-package
  imports plug into exactly that seam; without it, each command would resolve
  packages differently.
- Consuming applications are blocked today. `[generate.<name>]` only emits
  `html`/`rust`, so an app can't get Postgres DDL from a fetched schema. And
  an `imports:` entry that names a fetched manifest dependency isn't resolved
  across the cache boundary — it's treated as a local path and fails.
- The apps layer: an application defines its own schema (e.g. app-local
  identity classes) that imports one or more shared functionality schemas and
  references their classes. That is the cross-package `imports:` merge.

## Composition modes (A1 vs A2)

Both modes share fetch → resolve imports → build IR. They differ only in how
the Rust writer treats an imported schema's types:

- **Inline (A1)** — materialize the imported types into the app's output. One
  generated module holds the app's own types plus the imported ones. Default,
  zero-config. Cost: each consuming app carries its own copy, so
  `app_a::Hypothesis` and `app_b::Hypothesis` are distinct Rust types.
- **Crate (A2)** — the schema is generated as its own versioned crate
  (`Cargo.toml` + `lib.rs`); a consuming app depends on that crate and its
  generated code emits `use scimantic_schema::Hypothesis;` instead of copying.
  One shared type, both apps agree, Cargo owns the app-side version pin.

The other writers (Postgres, HTML, SHACL, …) use merge (A1) semantics — one
combined DDL, one combined doc. A2 is a Rust-codegen concern.

## Vertical Slices

### Slice 1: Manifest `[generate]` writer coverage

**Status:** Complete

**Priority:** Must Have

**User Value:** An application can generate any output format — not just HTML
and Rust — from a fetched schema dependency, so a backend gets its Postgres
DDL (and TTL/SHACL/graph) from the same manifest that gets its Rust types.

**Acceptance Criteria:**
- [x] `[generate.<name>]` accepts `postgres`, `shacl`, `ttl`, `jsonld`, `rdfxml`, `ntriples`, and `graph-json` output paths in addition to `html`/`rust`, and `panschema generate` (no `--input`) emits each configured format for each schema.
- [x] A configured output whose writer errors fails the command naming the schema and format; an unconfigured format is simply not produced.

### Slice 2: Cross-package `imports:` across the cache boundary (inline / A1)

**Status:** Complete

**Priority:** Must Have

**Depends on:** Feature 29 Slice 1 (shared load path), Feature 05 (manifest /
lockfile / source cache / `fetch`).

**User Value:** An application schema that `imports:` a fetched dependency by
name loads that dependency's schema from the cache and merges it, so the app's
generated output includes both its own and the imported definitions.

**Acceptance Criteria:**
- [x] An `imports:` entry whose value matches a `[schemas]` dependency name resolves to that dependency's fetched schema in the source cache and merges into the importing schema (its classes/slots/enums/types/prefixes), reusing the feature-15 merge + collision rules.
- [x] Resolution precedence is deterministic and never shadows a real local import: local file → manifest-dependency name → builtin (`linkml:*`) → CURIE/URL.
- [x] An `imports:` entry that names neither a local file nor a declared `[schemas]` dependency produces a clear diagnostic naming the entry and pointing at `[schemas]` + `panschema fetch`, never a silent drop. (A *declared* dependency that isn't cached is fetched on demand rather than errored, so the actionable failure is the undeclared/misspelled entry.)
- [x] The imported schema's own-namespace CURIEs expand correctly on the read/merge path (through the one shared `expand_curie`), so cross-namespace references resolve.
- [x] A self-contained two-package fixture (a base schema + an app schema importing it by dependency name) generates and Rust-compiles, pinning the merge end to end.

### Slice 3: App layering — importing multiple schemas at once

**Status:** Not Started

**Priority:** Must Have

**Depends on:** Slice 2 (single cross-package import).

**User Value:** An application's own schema imports two or more shared schemas
together and generates output that merges all of them plus the app's own
local classes — the real app topology, where an app-local identity schema
composes several shared functionality domains.

**Acceptance Criteria:**
- [ ] An app schema that `imports:` two or more fetched dependencies loads and merges all of them together with its own definitions; every configured output format contains the classes, slots, and enums contributed by each imported schema and by the app itself.
- [ ] A class in the app schema can reference a class defined in any of the imported schemas (as a slot `range`, an `is_a` parent, or a mixin) and the reference resolves — no dangling-reference warning for a name a sibling import defines.
- [ ] When two imported schemas each import the same base schema at the same pinned version, that base's definitions appear once in the output, never duplicated.
- [ ] When two imported schemas import the same base schema at conflicting pinned versions, the command fails with a clear error naming both requiring schemas and both versions — it never silently picks one or merges across versions.
- [ ] A self-contained fixture with a diamond shape — an app importing two shared schemas that both import a common base — generates and Rust-compiles, pinning the multi-import merge (including the deduplicated shared base) end to end.

### Slice 4: Schema-as-crate emission (crate producer / A2)

**Status:** Not Started

**Priority:** Should Have

**User Value:** A schema repository can generate its schema as a standalone,
publishable Rust crate, so multiple applications share one versioned source of
truth for its types.

**Acceptance Criteria:**
- [ ] The Rust writer can emit a complete crate directory (`Cargo.toml` with a name/version derived from the schema, `src/lib.rs` with the public types, a deterministic module layout), configured via a `rust_crate` output in `[generate.<name>]`.
- [ ] The emitted crate compiles standalone and round-trips its types (extends the existing Rust-writer compile fixture to the crate shape).

### Slice 5: Extern references for imported schemas (crate consumer / A2)

**Status:** Not Started

**Priority:** Should Have

**Depends on:** Slice 2 (import resolution), Slice 4 (crate shape).

**User Value:** An application whose imported schema is provided as an external
crate references that crate's types instead of re-generating them, so the app
and the schema agree on one type and Cargo owns the version pin.

**Acceptance Criteria:**
- [ ] A per-import manifest mapping (e.g. `rust_extern.<dep> = "<crate_path>"`) makes the Rust writer emit `use <crate_path>::<Type>;` for that import's types rather than materializing them; absence of the mapping keeps the inline (A1) default.
- [ ] A two-package fixture where the app references the base schema as an external crate compiles, with the base type used by-reference (one type identity across the boundary).
- [ ] The same base schema consumed inline (A1) by one target and by-reference (A2) by another both work in one run.

### Slice 6: Dev-vs-release local override

**Status:** Not Started

**Priority:** Should Have

**User Value:** While co-developing a schema and an app in lockstep, the app
consumes the schema's local working copy; for CI/release it consumes the
pinned version — without hand-editing the manifest on each switch.

**Acceptance Criteria:**
- [ ] A git-ignorable override (e.g. `panschema.local.toml`, or a `[patch]`-style block) can redirect a pinned dependency to a local path; when present, the load/fetch path uses the local copy, and the base manifest + lockfile are untouched.
- [ ] With no override present, resolution uses the pinned version exactly as before (CI is unaffected by a teammate's local override).
- [ ] A diagnostic (not silent) indicates when an override is active, so a local build is never mistaken for a pinned one.

### Slice 7: Version hygiene — explicit `update` + `outdated`

**Status:** Not Started

**Priority:** Should Have

**User Value:** Bumping a pinned schema dependency is a deliberate, reviewable
act, and staleness is visible — without loosening pins.

**Acceptance Criteria:**
- [ ] `panschema update [<dep>] [--to <version>]` re-resolves the named dependency (or all) to a chosen or latest tag, rewrites the lockfile, and leaves the generated output to a subsequent `generate` (so the diff is reviewable).
- [ ] `panschema outdated` reports, read-only, each dependency's pinned version and any newer available tag; it changes nothing.
- [ ] Exact pinning remains the only resolution mode — no semver ranges are introduced (see "Tracked, deliberately not in this feature").

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|---|---|---|---|
| Slice 1: `[generate]` writer coverage | Must Have | Feature 05 | Complete |
| Slice 2: cross-package imports (inline / A1) | Must Have | Feature 29 S1, Feature 05 | Complete |
| Slice 3: app layering — multiple imports | Must Have | Slice 2 | Not Started |
| Slice 4: schema-as-crate (A2 producer) | Should Have | Rust writer | Not Started |
| Slice 5: extern references (A2 consumer) | Should Have | Slice 2, Slice 4 | Not Started |
| Slice 6: dev-vs-release local override | Should Have | Feature 05 | Not Started |
| Slice 7: `update` + `outdated` | Should Have | Feature 05 | Not Started |

Slices 1–3 are the near-term unblock (a layering app generates all formats
from its own schema plus one or more fetched dependencies, inline). Slices 4–5
deliver the shared-crate composition. Slices 6–7 are version-workflow
ergonomics.

## Definition of Done

- [ ] Slices 1–3 acceptance criteria met (the inline cross-package path — a
      layering app importing one or more fetched schemas — works end to end for
      self-contained fixtures); slices 4–7 as their value is confirmed by real
      consumers.
- [ ] Both composition modes provably coexist: one base schema consumed inline
      by one target and as an external crate by another, in a single run.
- [ ] Exact-version pinning is the default and only resolution mode.
- [ ] [linkml-coverage.md](../linkml-coverage.md) and README are updated for
      the manifest `[generate]` format coverage and the crate-emission mode.

## Tracked, deliberately not in this feature

- **Semver ranges / loose pinning.** A non-goal. A data model is a contract
  over persisted data and wire formats, not a library API — an "additive"
  schema change can still force a migration or break a consumer, so implicit
  range resolution is a footgun. Exact pins + an explicit `update` give the
  reproducibility and deliberate-bump properties without the risk. If a
  concrete need appears, it's a small addition on top of the pin machinery.
- **Automatic transitive version *unification*.** Slice 3 detects a
  same-base version conflict and errors (naming both requirers); what stays
  out of scope is a Cargo-style resolver that *auto-unifies* conflicting pins
  to a compatible version. That waits until the dependency graph is deep
  enough to warrant it.
- **OWL `owl:imports` following + import provenance in HTML.** That is
  [feature 15](15-multi-file-schema-modularity.md) Slice 4 (Could Have).
