# One Graph Component, Two Surfaces - Implementation Plan

**Feature:** Unify the schema-graph and instance-graph implementations

**User Story:** As a reader of published schema docs, I want the schema
graph and the instance graph to behave as one system — same controls,
same wording, same capabilities — so that what I learn on one section
holds one scroll later; and as a maintainer, I want one implementation
behind both, so a viz change lands everywhere at once.

**Related ADR (if applicable):** [ADR-009](../adr/009-instance-graph-publishing-and-addressing.md)
records the shared node-vocabulary contract and defers the *unified
canvas* (classes and individuals on one surface). This feature is the
smaller move ADR-009 gestures at: one **implementation**, still two
surfaces.

**Approach:** Vertical Slicing with Outside-In TDD

---

## Implementation Strategy

Where things stand today:

- **Engine: shared.** One `panschema-viz` WASM module draws both
  canvases with the ADR-009 vocabulary (a class's individuals wear its
  circle and color; an enum's used values its diamond). Each graph
  currently loads its own WASM instance.
- **Behavior: half-shared.** `components/graph_shell.html` defines
  `window.PanschemaGraphShell` (layout persistence, view buttons,
  hover-focus, refit, legend shell). The instance graph is built on it;
  the schema graph's 2,367-line template still carries its own older
  inline versions of the same behaviors and makes exactly one shell
  call.
- **Chrome: duplicated but guarded.** Two templates with mirrored
  `graph-*` / `instance-graph-*` markup and CSS. A parity test reads
  both toolbars from one rendered page and asserts the shared pieces
  are equal, so drift fails a test — the bridge until this feature
  lands. Layout-option wording exists in three hand-maintained copies
  (both templates' `<option title>` lists plus the schema template's JS
  title maps).

The slices below retire the duplication from the inside out: behavior
first (the shell the instance graph already proves out), then chrome,
then the engine's double-load. Each slice keeps both sections
pixel-working under the existing browser e2e suite.

*Documentation to revisit as slices land:* the graph sections of the
[Main README](../../README.md), ADR-009's amendment notes, and the
schema/instance sections of `docs/features/04` and `docs/features/33`
(if their control descriptions name either template).

---

## Vertical Slices

### Slice 1: The schema graph moves onto the shared shell

**Status:** In Progress

**User Value:** No visible change — the schema graph's controls behave
exactly as before — but every behavior the two graphs share (view
buttons, label toggles, hover focus, arrows, groundings, legend
toggle) now runs through `PanschemaGraphShell`, so a behavior fix or
feature lands on both graphs at once.

**Acceptance Criteria:**
- [ ] The schema graph's reset/zoom, label toggles, hover-focus,
      arrows, groundings, and legend are wired through the same shell
      functions the instance graph uses; the inline duplicates in the
      schema template are deleted. Layout *persistence* stays local:
      the schema picker carries 2D/3D-mode semantics (mode-filtered
      restore, a forced force-directed in 3D that must not overwrite
      the stored choice) that the shell's `layoutPref` deliberately
      lacks — it is mode logic, not duplication, and moves with the
      mode question in slice 3.
- [ ] Persisted preferences survive the migration: every localStorage
      key is unchanged, and the label prefs' one legacy JSON key seeds
      the per-toggle keys once.
- [ ] Every existing schema-graph browser test passes unchanged —
      hover focus, toggles, layout switching, legend, groundings.
- [ ] Schema-only controls (2D/3D mode, Groundings) keep working,
      composed beside the shell rather than forked from it.
- [ ] The schema template shrinks by at least the lines the shell
      already implements (measured in the commit, not estimated).

**Notes:**
- The shell was extracted when the instance graph was built and is
  proven there; this slice is repayment, not new design.
- Risk: the schema graph's keyboard shortcuts (L/N/E) and 3D-mode
  interactions are inline today; they move onto or beside the shell,
  never silently drop. Browser e2e is the gate for that.

---

### Slice 2: One chrome fragment, two inclusions

**Status:** Not Started

**User Value:** The toolbar, hint, separator, and layout picker exist
once, included by both sections with an id/class prefix and a
graph-kind flag from context; the parity test's job collapses from
"compare two copies" to "the fragment rendered twice".

**Acceptance Criteria:**
- [ ] A shared toolbar/picker fragment (the `rule_block_styles.html`
      include precedent) renders both graphs' chrome; the per-graph
      differences (schema-only buttons, the sgd noun, Hierarchical's
      qualifier, the 3D clause) are parameterized or branched in one
      place, stated in the fragment.
- [ ] Layout-option wording has one source shared by both `<option>`
      lists; the schema template's JS title maps read from or are
      generated with it, so the three copies become one.
- [ ] The two-sided parity test still passes and now also covers the
      controls that were one-sided (separator, hint visibility rule).
- [ ] The undefined `--spacing-sm` token cluster in the schema
      template is replaced with the live `--space-*` scale.

---

### Slice 3: One WASM load, and the 2D/3D question

**Status:** Not Started

**User Value:** The page loads the viz module once for both canvases
(smaller, faster pages), and the 2D/3D toggle either arrives on the
instance graph or its absence is a stated decision rather than drift.

**Acceptance Criteria:**
- [ ] Both graph sections initialize from a single WASM module
      instance; page weight drops accordingly and both canvases still
      paint (pixel-probed in e2e).
- [ ] The instance graph either gains the 2D/3D toggle (if the 3D
      path accepts an instance dataset without new engine work) or
      the feature doc records why it stays 2D-only, and the toolbar
      fragment expresses that as a parameter, not a fork.

**Notes:**
- 3D currently requires WebGPU and only the force-directed layout;
  whether an A-box in 3D is worth the wiring is a judgment call to
  make when the cost is visible — decide inside the slice, not here.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1 | Must Have | None | In Progress |
| Slice 2 | Must Have | Slice 1 | Not Started |
| Slice 3 | Nice to Have | Slice 2 | Not Started |

## Things to watch

- The parity test is the safety net for slices 1–2: it must stay red
  on real drift while the fragments move. If a slice makes it
  tautological (both sides render from one fragment), replace it with
  assertions on the fragment's two renderings rather than deleting it.
- The browser e2e suite is the gate for behavior moves; a template
  refactor that passes unit tests can still break hover/toggle/paint
  behavior only e2e sees.
- `graph_shell.html` grows API surface in slice 1; keep it a plain
  function bag — the moment it needs configuration objects per graph,
  prefer parameters at the call site so the shell stays readable.
