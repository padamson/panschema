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

**Status:** Complete (39b882f, pushed)

**User Value:** No visible change — the schema graph's controls behave
exactly as before — but every behavior the two graphs share (view
buttons, label toggles, hover focus, arrows, groundings, legend
toggle) now runs through `PanschemaGraphShell`, so a behavior fix or
feature lands on both graphs at once.

**Acceptance Criteria:**
- [x] The schema graph's reset/zoom, label toggles, hover-focus,
      arrows, groundings, and legend are wired through the same shell
      functions the instance graph uses; the inline duplicates in the
      schema template are deleted. Layout *persistence* stays local:
      the schema picker carries 2D/3D-mode semantics (mode-filtered
      restore, a forced force-directed in 3D that must not overwrite
      the stored choice) that the shell's `layoutPref` deliberately
      lacks — it is mode logic, not duplication, and moves with the
      mode question in slice 3.
- [x] Persisted preferences survive the migration: every localStorage
      key is unchanged, and the label prefs' one legacy JSON key seeds
      the per-toggle keys once.
- [x] Every existing schema-graph browser test passes unchanged —
      hover focus, toggles, layout switching, legend, groundings.
- [x] Schema-only controls (2D/3D mode, Groundings) keep working,
      composed beside the shell rather than forked from it.
- [x] The schema template shrinks by at least the lines the shell
      already implements (measured in the commit, not estimated).

**Notes:**
- The shell was extracted when the instance graph was built and is
  proven there; this slice is repayment, not new design.
- Risk: the schema graph's keyboard shortcuts (L/N/E) and 3D-mode
  interactions are inline today; they move onto or beside the shell,
  never silently drop. Browser e2e is the gate for that.

---

### Slice 2: One chrome fragment, two inclusions

**Status:** Complete (22654d8, pushed)

**Decided 2026-09-04 (rendered-page review):** the schema graph's look
is canonical — active toggles are a **solid fill**, and the toolbar is
an **overlay on the canvas**. The instance graph adopts both. The
shared CSS rides in `graph_shell.html` (already included wherever any
graph renders), using `graph-*` class names; IDs stay per-graph
(`graph-*` / `instance-graph-*`) for per-page uniqueness.

**User Value:** The toolbar, hint, separator, and layout picker exist
once, included by both sections with an id/class prefix and a
graph-kind flag from context; the parity test's job collapses from
"compare two copies" to "the fragment rendered twice".

Two concrete divergences a review of the rendered pages (2026-09-04)
confirmed, both from the duplicated CSS/markup this slice collapses:

- **Active-toggle styling differs.** The schema graph's
  `.graph-toggle.active` is a solid fill (`background:
  --color-primary`, white text); the instance graph's
  `.instance-graph-toggle.active` is only a border tint
  (`border-color: --color-accent`, no fill). Same "on" state, two
  visual languages — an active button reads as a filled chip on one
  graph and an outlined one on the other.
- **Toolbar arrangement differs.** The schema graph splits its
  controls into two strips (2D/3D + Layout above the canvas, view
  buttons + toggles below it); the instance graph puts everything in
  one strip above the canvas.

**Acceptance Criteria:**
- [x] A shared toolbar *markup* fragment: one Askama macro
      (`graph_toolbar.html`) renders both graphs' button strip. The
      per-graph differences are macro arguments — the Arrows and Legend
      tooltips (the schema's name T-box edge types the A-box lacks) —
      plus two flags for the schema-only Groundings control and 3D pan
      hint. The keyboard-shortcut gap is closed the same way: L/N/E/R
      wiring is one shell function (`wireLabelKeys`) both graphs call,
      so the `(L/N/E)` hints are honest on each without a second
      implementation. The layout `<option>` list stays per-template
      (its wording diverges more; see the deferred JS-map item).
- [x] One shared stylesheet fragment defines the toolbar, button, and
      toggle styles for both graphs, so the active-toggle look is
      identical (one `.active` rule, not `.graph-toggle.active` vs
      `.instance-graph-toggle.active`) and a style change lands on
      both. The `graph-*` / `instance-graph-*` CSS blocks that mirror
      each other collapse to one prefix-agnostic rule set.
- [x] The two toolbars share one arrangement: the same controls sit
      in the same place on both graphs (a reader moving between the
      sections finds the reset, the toggles, and the layout picker
      where they were), with the schema-only 2D/3D + Groundings
      controls slotted into that shared arrangement rather than
      forcing a different strip layout.
- [x] The graph-agnostic layout-option tooltips are shared verbatim
      between the two `<option>` lists (the parity test asserts it).
      Folding the schema template's *dynamic* JS title maps
      (`LAYOUT_TITLES_2D/3D`, `LAYOUT_LABELS_2D/3D`) into that one
      source is deferred to slice 3: those maps exist for 2D/3D
      mode-switching (the 3D variants have no `<option>` equivalent),
      so they settle with the 2D/3D question, not before it.
- [x] The parity test still passes and now asserts the shared
      active-toggle style renders identically (one `.graph-toggle.active`
      rule, the per-template `.instance-graph-toggle.active` gone) and
      that both graphs render the shared control overlay.
- [x] The undefined `--spacing-sm` token cluster in the schema
      template is replaced with the live `--space-*` scale (all of
      `--spacing-xs/sm/md/lg` converted).

---

### Slice 3: One WASM load

**Status:** Complete (4220ed5, pushed)

**Decided 2026-09-04:** the 2D/3D question is settled by removal — the
3D renderer is gone (see
[ADR-005's amendment](../adr/005-graph-visualization-conventions.md#amendment-2026-09-04-the-3d-renderer-is-removed)),
so there is no mode to unify: the mode-dependent layout wording and
picker filtering left with it. What remained of this slice was the
double module load.

**User Value:** A page carrying both graphs fetches the visualization
module once instead of twice, and repeat visits serve it from the
browser cache — it re-downloads only when the binary that published
the page changed.

**Acceptance Criteria:**
- [x] A page carrying both graphs fetches the wasm module exactly
      once (browser-asserted from the page's own resource timeline),
      and both canvases still paint.
- [x] A schema-only page and a data-only page each still render.
- [x] The asset URLs are stable across page views and change exactly
      when the published bundle's content does (a writer test pins the
      content stamp and the absence of per-view timestamps), so HTTP
      caching applies across visits.

**Notes:**
- Mechanism: one memoized loader in the graph shell; asset URLs carry
  a content stamp of the embedded bundle rather than a per-view
  timestamp, and the wasm fetch starts before the JS import so the
  two downloads overlap.
- Accepted trade: both graphs now share one wasm instance, so a
  panic that poisons it (a steady-state panic, not a caught
  constructor failure) can take down both canvases where separate
  instances confined it to one. The viz crate holds no mutable
  statics, so cross-talk short of a panic is not expected; revisit if
  malformed-dataset panics ever surface in the field.

---

### Slice 4: One hover card

**Status:** Complete

**User Value:** Hovering a node shows the same card, built by the same
code, on both graphs — so the card's layout, pinning, and dragging
behave identically and a fix lands once.

The instance graph already used `PanschemaGraphShell.makeHoverCard`;
the schema graph carried a separate, richer hover implementation. Its
richness turned out to be mostly one trick — reusing the entity's
already-rendered doc card — plus edge hover, a pinned mode with a close
button and drag handle, and viewport-relative placement. The shell's
card grew those, and the schema template's own hover functions and
card styles went.

**Acceptance Criteria:**
- [x] The schema graph's hover card renders through the shared shell
      helper, extended (in the shell, once) to carry the richer rows
      the schema card needs. The schema template's own hover
      *behavior* — placement, pin, close, drag, node-over-edge
      dispatch, and the selected node's hover suppression — is
      deleted; what stays per-graph is *content*, passed in as
      callbacks (the doc-card reuse and compact rows, the edge
      triple).
- [x] Every schema hover behavior a browser test covers still passes:
      the compact card, the pinned/draggable full card, edge-type and
      triple detail. (Edge hover and the full-card mode had no browser
      test before this slice; both gained one first, green against the
      old implementation, so the migration was measured against them.)
- [x] The `.graph-hover-*` CSS collapses into the shared card style
      the instance graph uses, with the schema-only rows added there.

**Notes:**
- The card's *content* stays per-graph, passed in as callbacks
  (`renderNode`, `renderEdge`); the card's *behavior* — placement,
  pin, close, drag, the full-mode widening — is the shell's. The
  markup is one Askama macro rendered under each graph's id prefix,
  the same shape as the toolbar.
- Placement unified on the schema graph's model: `position: fixed`
  with viewport-edge flipping. The instance card was canvas-relative
  and clamped inside the canvas, which clips on a small graph.
- The instance graph's pinned card gained the close button and drag
  handle for free; before, its pinned card could only be dismissed by
  clicking empty canvas or pressing Escape. It also takes the shared
  card's type size and spacing (a point smaller than its old card,
  with the key column still aligned).
- "A pinned card ignores hover" was implemented in each template; it
  is the shell's now (`update`, `unpin`), and a browser test on each
  graph pins it. Closing the card locks nothing: the node stays
  selected and hovers like any other (the schema graph used to
  suppress the selected node's hover card — dogfooding read that as
  a bug, and it went). `unpin()` closes whatever is up, pinned or
  not, so a dataset swap resets the card outright and a card left up
  across a keyboard switch cannot show the old dataset's node under
  the new one's index.
- Touch: coarse pointers have no hover, so only the card a tap pins
  shows, on both graphs. The schema graph previously hid its card on
  touch outright — tap-to-select worked but nothing appeared — and
  the instance graph showed its card only on pages without the schema
  graph, whose rule reached across once the class was shared.
- Measured: the schema template is 1,201 lines (2,238 at the start
  of the feature), the instance template 457; the shell holds the
  25 card rules both graphs share.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1 | Must Have | None | Complete (pushed) |
| Slice 2 | Must Have | Slice 1 | Complete (pushed) |
| Slice 4 | Should Have | Slice 2 | Complete |
| Slice 3 | Nice to Have | Slice 2 | Complete (pushed) |

Measured duplication surface (2026-09-04, post-slice-1): the schema
template is 2,238 lines to the instance template's 521. Ten CSS class
stems are defined in both under mirrored `graph-*` / `instance-graph-*`
prefixes (btn, container, controls-separator, help, hover-card,
layout-label, layout-select, legend, toggle) — slice 2's target. The
schema-only `.graph-hover-*` family (~15 classes) and the inline hover
functions are slice 4's. Slices 2 and 4 together account for most of
the remaining gap; slice 3 is small by comparison.

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
