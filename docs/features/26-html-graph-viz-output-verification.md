# Feature 26: HTML + graph visualization output verification & validation

**Feature:** Document the real-browser V&V already in place for
`HtmlWriter` (and the embedded graph visualization it emits), and scope
gaps — chiefly, spec-level HTML validity, which "renders correctly in
Chromium" does not prove (browsers are forgiving parsers).

**User Story:** As a panschema maintainer, I want confidence that
generated HTML/JS/WASM actually works in a real browser and is valid
markup, not just that Askama rendered the template I expected, so a
writer regression that breaks the live page is caught before release.

**Related ADR:** None.

**Approach:** Mostly retrospective, like [feature 25](25-rust-writer-output-verification.md);
scope real gaps as slices, added only when demand or a missed bug
justifies them.

---

## Current State — already in place

- **Fast tier:** the writer's own unit tests assert on rendered HTML
  strings (Askama template output) — the "does my code produce the
  markup I expect" tier every writer has.
- **Thorough tier, real browser:** `tests/e2e.rs` drives real Chromium
  via `playwright-rs`, loading the generated page and interacting with
  it — hovering nodes, checking labels render, exercising the graph
  visualization's WASM/WebGPU-or-2D-canvas path. A real browser engine
  is the oracle: it parses the HTML, executes the JS, runs the WASM, and
  the test asserts on what actually renders/responds — not on the
  template output text.
- **Manual iteration harness:** `e2e_2d_graph_screenshots` runs the 2D
  layout at three viewport scales (phone/laptop/4K) against synthetic
  ontologies, dumping screenshots + pixel-bbox/label-count stats.
  `#[ignore]`d — explicitly a developer feedback loop for tuning layout
  parameters, not a regression check, and does not run in CI at all
  (confirmed: no `--include-ignored` in any workflow).
- `GraphWriter` (`graph-json` format) has no independent test harness of
  its own — its correctness is currently validated *indirectly*, via the
  HTML/viz E2E tests that consume the same graph-JSON shape the
  standalone writer emits.

## Gaps

- **No independent HTML-spec validator.** Chromium is very forgiving —
  it silently repairs malformed markup rather than rejecting it, so
  "renders and behaves correctly in Chromium" is not proof of spec-valid
  HTML. Nothing today would catch, say, invalid nesting or an
  unescaped attribute that happens not to visibly break rendering.
- **The multi-viewport screenshot harness is local-only.** It never runs
  automatily anywhere, so a real visual regression across viewport sizes
  could land unnoticed unless someone runs it by hand.
- `graph-json` output has no schema/contract test of its own if it's
  ever consumed by something other than panschema's own HTML/viz layer.

---

## Vertical Slices

### Slice 1: HTML5 spec validation

**Status:** Not Started

**Priority:** Should Have — cheap and closes the biggest real gap
(Chromium's leniency masking spec violations).

**User Value:** Generated HTML is checked against the actual HTML5
grammar, not just "a browser tolerated it."

**Acceptance Criteria:**
- [ ] Generated HTML is parsed by a real HTML5-conformance parser (e.g. `html5ever`, the pure-Rust engine Servo/Firefox use) and any parse errors are surfaced as test failures.
- [ ] Runs as a fast, every-test-run check — no browser needed for this tier.

**Notes:**
- Prefer a Rust-native parser over shelling out to a JVM/Python
  validator (e.g. `html5validator`) — keeps this tier dependency-light
  and consistent with panschema's "single binary, no JVM" posture, even
  though this dependency is test-only and never shipped.

---

### Slice 2: Promote the multi-viewport visual check into CI — optional

**Status:** Not Started

**Priority:** Could Have

**User Value:** A visual regression across viewport sizes gets *some*
periodic automated signal instead of relying on someone remembering to
run the harness by hand.

**Acceptance Criteria:**
- [ ] (if adopted) A scheduled or `workflow_dispatch` CI job runs `e2e_2d_graph_screenshots --ignored` and uploads the screenshots as artifacts (or diffs them against a committed baseline) — mirroring the mutation-testing full/diff tiering (fast per-push, thorough on schedule/manual).

**Notes:**
- Screenshot-diffing has real flakiness risk (font rendering, GPU
  differences across CI runners) — worth a spike before committing to
  pixel-diff assertions; artifact-upload-for-human-review is the safer
  first step.

---

### Slice 3: `graph-json` standalone contract test — only if a second consumer appears

**Status:** Not Started

**Priority:** Could Have

**User Value:** `graph-json` output is validated on its own terms if
something other than panschema's own HTML/viz ever consumes it.

**Acceptance Criteria:**
- [ ] (when triggered) A schema/contract test independent of the HTML/viz E2E harness.

**Notes:**
- Not built speculatively — today's indirect coverage (via HTML/viz E2E)
  is proportionate to today's single consumer.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: HTML5 spec validation | Should Have | None | Not Started |
| Slice 2: Promote visual check to CI | Could Have | None | Not Started |
| Slice 3: `graph-json` contract test | Could Have | A second consumer | Not Started |

---

## Definition of Done

- [x] Real-browser E2E tier exists and runs in CI (`playwright-rs`)
- [ ] Slice 1 acceptance criteria met
- [ ] Slices 2–3 only if their trigger condition is met
- [ ] All tests passing: `cargo nextest run --features dev`
- [ ] CHANGELOG.md updated (slice 1 only — infra-only slices 2–3 may not warrant an entry, per the CHANGELOG consolidation convention)
