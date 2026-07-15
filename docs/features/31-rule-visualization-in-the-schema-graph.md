# Feature 31: Rule visualization in the schema graph

**Feature:** Make a class's conditional `rules` legible in the interactive
schema graph. A slot a rule governs carries a **marker glyph** with a
slot-scoped hover card ("present only when …"), and hovering it **highlights
the rule's participating nodes** — trigger slots, governed slots, the owning
class — with transient "when → then" connectors. Rules are shown
*node-centrically and on demand*, never as persistent edges.

**User Story:** As someone reading a schema's graph, I want to see which
fields a conditional rule governs and why, so I understand the schema's
conditional logic without leaving the graph or reading raw YAML.

**Related ADR:** [005 (graph-visualization conventions)](../adr/005-graph-visualization-conventions.md),
[003 (LinkML as internal representation)](../adr/003-linkml-as-internal-representation.md).
Builds on [feature 04](04-schema-force-graph-visualization.md) (the graph
viz), [feature 17](17-class-validation-constructs.md) (the rules IR + HTML
rendering), and the graph-JSON rule metadata already carried on class nodes.

**Approach:** Vertical Slicing, outside-in. The graph is enriched from the
exporter side first (self-contained, Rust-testable), then the viz consumes
it. Every viz slice is browser-dogfooded on a rendered graph — a wire-format
change can pass all Rust tests while the rendered graph is broken, so the
writer and viz sides are mirrored and hover-tested together.

## Why Now

- The graph JSON now carries each class's rules (title, description, rendered
  "when … then …" summary) in node metadata, but **the viz renders none of
  it** — the payload is inert.
- The graph **understates** rule-governed slots: a conditionally-required
  slot (e.g. present only when a status field takes certain values) draws as
  a plain `0..1` range edge, with no cue that a rule governs it. Dogfooding a
  consumer schema with verdict-conditional rules surfaced this.
- The relationship a rule expresses spans several slots and values, so it
  can't be an edge (see the design decision below) — the graph needs an
  on-demand reveal instead.

## Design decision: reveal on hover, never persistent edges

A rule is a class-scoped conditional — "when (precondition) then
(postcondition)" — where each side can be multiple slots, `any_of`
alternatives, and value checks. That is a small logical formula, not a binary
relation, so it cannot be carried by an edge without either lying (collapsing
`any_of` / multi-slot structure) or exploding the graph into a tangle.

Instead: the rule's participants are **highlighted transiently on hover** —
"ephemeral edges" that appear only while the reader is looking, with optional
faint connectors showing the when → then flow. The static graph stays
uncluttered; the relationship is discoverable; nothing is flattened. This is
the explicit alternative to new edge *types* — there are none.

## Vertical Slices

### Slice 1: Exporter — rule participant slots

**Status:** Complete

**Priority:** Must Have

**User Value:** The graph JSON carries, per rule, the slots it touches — split
into trigger and governed — so the viz can place governed-slot glyphs, build
slot-scoped cards, and highlight participants on hover. The data foundation
for every later slice.

**Acceptance Criteria:**
- [x] Each rule in a class node's metadata carries its **participant slots**, split into *trigger* (precondition) and *governed* (postcondition) slot names.
- [x] The viz places a governed-slot glyph by checking whether a slot appears in any rule's `governed_slots` — **derived from the per-rule data above, no separate per-slot flag** (avoids a redundant, class-ambiguous marker on globally-shared slot nodes).
- [x] Trigger/governed membership is computed from the same condition walk as the rendered summary (`crate::rules`), so a slot named through `any_of` or on either side is attributed correctly; deduplicated and sorted.
- [x] Output is additive and byte-stable for schemas with no rules (the fields skip serialization when empty), so the viz tolerates old and new payloads.

### Slice 2: Viz — governed-slot glyph and slot-scoped hover card

**Status:** Not Started

**Priority:** Should Have

**Depends on:** Slice 1.

**User Value:** A reader sees which fields are conditional and, on hover,
reads exactly the rule(s) governing *that* field — "present only when …" —
rather than the whole class's rule list.

**Acceptance Criteria:**
- [ ] A slot node the exporter marks as governed renders a distinct marker glyph, drawn by the same code as the rest of the graph so it scales and reads in grayscale.
- [ ] Hovering the glyph shows a card scoped to that slot: the rule(s) naming it, phrased slot-first (e.g. "`approved_by` — present only when (`verdict` = `approved`) or (`verdict` = `rejected`)"). A slot governed by several rules lists each.
- [ ] The card reuses the shared rule projection so it never drifts from the HTML card or the class-node hover.

### Slice 3: Viz — participant highlighting and ephemeral connectors

**Status:** Not Started

**Priority:** Should Have

**Depends on:** Slice 1, Slice 2.

**User Value:** Hovering a rule (its glyph, or its entry in the class card)
shows, on the graph itself, every field the rule connects — the conditional
relationship an edge can't carry.

**Acceptance Criteria:**
- [ ] Hovering a governed-slot glyph (or a rule entry in a card) highlights every node the rule touches — trigger slot(s), governed slot(s), and the owning class — with trigger and governed distinguished by accent.
- [ ] Optional transient connectors draw the when → then flow between the highlighted nodes and disappear when the hover ends; the persistent graph is unchanged.
- [ ] Highlighting is bidirectional and consistent: hovering the rule in the class card highlights the same nodes as hovering the governed slot's glyph.

### Slice 4: Rules section in the pinned card — pairs with pin-on-click

**Status:** Deferred — build with the pin-on-click popup redesign

**Priority:** When we need it

**User Value:** Once a hover card can be pinned, its longer content (a class's
full rule list, with the highlighting affordance) has a stable home.

**Acceptance Criteria:** *(to be written when the pin-on-click redesign is
scheduled; this feature supplies the data, that work supplies the pinned
surface).*

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|---|---|---|---|
| Slice 1: exporter participant slots | Must Have | graph-JSON rule metadata | Complete |
| Slice 2: governed-slot glyph + scoped card | Should Have | Slice 1 | Not Started |
| Slice 3: participant highlighting + connectors | Should Have | Slices 1–2 | Not Started |
| Slice 4: rules in the pinned card | When we need it | Slice 1, pin-on-click | Deferred |

Slice 1 is self-contained Rust (no wasm) and unblocks the rest. Slices 2–3
are the viz payoff and are browser-dogfooded. Slice 4 waits for the
pin-on-click popup redesign.

## Definition of Done

- [ ] Slices 1–3 acceptance criteria met (a rule-governed slot shows a glyph,
      a scoped card, and highlights its rule's participants on hover).
- [ ] The rendered graph is hover-tested on a schema with `any_of` /
      `value_presence` rules, not just asserted in Rust.
- [ ] [linkml-coverage.md](../linkml-coverage.md) notes the graph now
      surfaces rules; the graph legend explains the glyph.

## Tracked, deliberately not in this feature

- **Persistent rule edges / new edge types.** A non-goal — a rule's
  conditional, multi-slot, `any_of` structure can't be carried by an edge
  without loss. Transient highlight-on-hover replaces them.
- **The pin-on-click popup redesign itself.** A separate effort; this feature
  supplies the rule data and the highlighting behavior, and Slice 4 lands the
  pinned-card surface once that redesign exists.
- **Machine-readable rule projection** (SHACL `sh:or` / `sh:minCount` for
  `any_of` / `value_presence`). That is documentation, not visualization —
  tracked under [feature 17](17-class-validation-constructs.md).
