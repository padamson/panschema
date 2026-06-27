# Feature 20: Dogfood schema regression fixtures + release monitoring

**Feature:** Vendor every released version of the real dogfood schemas as
checked-in fixtures, render + compile each in panschema's own test suite, and
run a weekly action that opens an issue when a new release hasn't been vendored
yet.

**User Story:** As a panschema maintainer, I want every released version of my
real schemas compiled by panschema's own tests, so a codegen regression that
would break a real schema is caught in panschema CI — self-contained, no network
at test time — and I'm told when a new release needs vendoring.

**Related ADR (if applicable):** None — builds on the self-contained Rust
compile harness (the `codegen.yaml` fixture + scratch-crate `cargo build` in
`panschema/tests/rust_writer.rs`). The vendored scimantic snapshot from that work
is the first entry.

**Approach:** Vertical Slicing with Outside-In TDD.

---

## Why Now

Real schemas are the strongest integration signal panschema has — the
reserved-keyword and `ifabsent` bugs were both real-schema usage that the
synthetic unit tests missed. Released (tagged) versions are immutable, so
vendoring them is deterministic, drifts never, and needs no network at test
time. They double as regression coverage: panschema must keep rendering every
schema it ever supported.

`main` is deliberately **out of scope** — a vendored snapshot of a moving branch
is stale the moment it lands, and the live "does panschema `main` handle the
schema's `main`" signal belongs in each schema repo's own CI (build against
panschema `main`), not in a frozen fixture here.

The dogfood schemas: `scimantic-schema`, `scidatica-schema`, and
`nimbus-schema`. nimbus has no releases yet — it joins automatically once it
does.

---

## Vertical Slices

### Slice 1: Vendoring layout + script + render regression test

**Status:** Not Started

**Priority:** Should Have

**User Value:** Every vendored release renders to syntactically valid Rust in
panschema's tests, with no network — so a render regression against a real
schema fails CI.

**Acceptance Criteria:**
- [ ] Layout `panschema/tests/fixtures/dogfood/<repo>/<version>.yaml`, each file headed by a comment naming the source repo + tag and the date vendored.
- [ ] `scripts/vendor-dogfood-schemas.sh <repo> [<version>|all]` fetches the schema YAML at the given release tag(s) and writes it into the layout (idempotent; run by hand, committed explicitly).
- [ ] A parametrized test walks `tests/fixtures/dogfood/**` and asserts each vendored schema reads and renders to Rust that `syn::parse_file` accepts.
- [ ] Seeded with the scimantic release(s) already vendored (moved under `dogfood/scimantic-schema/`).

**Notes:**
- The `syn`-parse check is fast, so it runs for every vendored version. Full `cargo build` is the next slice.

---

### Slice 2: Compile coverage per schema

**Status:** Not Started

**Priority:** Should Have

**User Value:** The latest release of each schema is `cargo build`-compiled (not
just parsed), so a regression that produces parseable-but-uncompilable Rust is
caught against real schemas.

**Acceptance Criteria:**
- [ ] The latest vendored release per schema is compiled via the existing scratch-crate `cargo build` harness; older releases stay at the `syn`-parse check (keeps CI time bounded — note the policy in the test).
- [ ] A compile failure names the offending schema + version.

**Notes:**
- If total compile time stays small, widen to compile all vendored versions; `log`/comment whichever policy is chosen so it isn't a silent cap.

---

### Slice 3: Weekly release-monitor action

**Status:** Not Started

**Priority:** Should Have

**User Value:** When a dogfood schema cuts a release that isn't vendored yet, an
issue is opened listing what's missing — the prompt to vendor it (and to do any
panschema work a new construct needs first).

**Acceptance Criteria:**
- [ ] `.github/workflows/dogfood-release-monitor.yml` runs on a weekly schedule (and `workflow_dispatch`), lists each schema's releases via the GitHub API, and compares against the vendored fixtures.
- [ ] For any released version with no `dogfood/<repo>/<version>.yaml`, it opens (or updates) a single tracking issue listing the missing `repo@version`s — no duplicate issues on re-run.
- [ ] Opens nothing when everything is vendored.

**Notes:**
- Issue, not auto-PR: a new release can use a construct panschema doesn't support yet, so a human vendors it (running the slice-1 script) after any needed panschema work.

---

### Slice 4: RDF / HTML smoke per vendored schema — optional

**Status:** Not Started

**Priority:** Could Have

**User Value:** Each vendored schema also renders HTML and RDF without panicking,
extending real-schema coverage past codegen.

**Acceptance Criteria:**
- [ ] The parametrized test also runs the HTML and RDF writers over each vendored schema and asserts they produce output without error.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: Layout + script + render test | Should Have | self-contained compile harness | Not Started |
| Slice 2: Compile coverage | Should Have | Slice 1 | Not Started |
| Slice 3: Weekly release monitor | Should Have | Slice 1 | Not Started |
| Slice 4: RDF / HTML smoke | Could Have | Slice 1 | Not Started |

---

## Definition of Done

The feature is complete when ALL of the following are true:

- [ ] Slices 1–3 acceptance criteria met (slice 4 optional)
- [ ] Every current release of `scimantic-schema` and `scidatica-schema` is vendored and green (nimbus when it has releases)
- [ ] The default `cargo nextest run` needs no network
- [ ] All tests passing: `cargo nextest run`
- [ ] Code formatted + clippy clean: `cargo fmt --check` + `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] README.md / CONTRIBUTING note on how to vendor a new release (run the script, commit)
- [ ] CHANGELOG.md updated
