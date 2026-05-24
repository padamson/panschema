# Visualization Frictions

A running log of reader-side schema-graph visualization frictions
discovered during dogfood passes. Each entry is the input that
justifies an eventual feature spec or slice; the goal isn't to file
fixes in place but to accumulate enough structured friction data that
the right backlog falls out.

**Scope:** reader UX in `panschema-viz` — node/edge rendering, layout
quality, interaction (hover/click/focus), and any mode that affects
how a viewer perceives the schema's structure. *Not* authoring-side
lints (those go in `docs/authoring-frictions.md` per
[feature 10](features/10-authoring-experience.md)).

**Companion:** [feature 09 — Graph Layout Selection](features/09-graph-layout-selection.md).

## Format

Each friction is a short numbered entry:

- **Context:** which layout / mode / schema was active.
- **Tried:** what the reader was doing.
- **Expected:** what they thought would happen.
- **Got:** what actually happened.
- **Severity:** one of `annoyance` (works, but irritating), `impedes-comprehension` (forces extra effort to get the answer the reader wanted), `dealbreaker` (the visualization can't surface the answer at all).
- **Candidate fixes:** sketches of what might help. Not a commitment.

Frictions are ordered by date discovered, not severity. Severity
ranking is the input to the eventual feature-spec triage.

---

## F1 — Force-directed and Kamada-Kawai both produce hard-to-read blobs

**Context:** panschema-viz 2D mode, scimantic v0.2.0 (84 nodes / 81
edges), both `force-directed` and `kamada-kawai` layouts.

**Tried:** Switched between the two layouts looking for one that made
the schema's overall class hierarchy visible at a glance.

**Expected:** A view where `is_a` / `subClassOf` chains are obviously
layered (e.g. BFO-style hierarchies running top-to-bottom) and
property edges read as a secondary overlay.

**Got:** Both layouts produce a dense blob with many edge crossings.
Edge lengths look uniformly long regardless of relationship type. The
overall schema structure (what's a root class? what's a leaf? what
inherits from what?) is invisible.

**Severity:** `impedes-comprehension`

**Candidate fixes:**
- Promote feature 09 slice 6 (Sugiyama via `rust-sugiyama`) ahead of slices 4–5 (stress / SGD); Sugiyama is literally the algorithm class hierarchies want.
- Differentiate edge styles by `edge_type` so `is_a` reads as primary and properties as secondary even within a force layout.
- Re-tune `link_distance` (FD) and `WORLD_TARGET_DIMENSION` (KK) downward so the blob compresses; partial fix, won't address crossings.

---

## F2 — Node detail box pops in the corner, not next to the node

**Context:** Any layout, 2D mode.

**Tried:** Clicked a node to inspect its details (URI, class kind,
description, etc.).

**Expected:** A tooltip-style popup anchored to the clicked node, so
the reader's eye stays in the same region.

**Got:** The detail box appears in a fixed right-hand corner of the
graph viz. Worse: the click can re-trigger the simulation, the node
moves, and the reader has to relocate the now-disconnected node
visually before they can act on the detail.

**Severity:** `impedes-comprehension`

**Candidate fixes:**
- Render detail as a tooltip positioned next to the node (with a leader line if it has to be offset to avoid clipping the viewport).
- On hover, show; on click, pin/toggle so the reader can keep it open while exploring other nodes.
- Don't re-heat the simulation on click — clicks should be inspect-only by default.

---

## F3 — Edge labels can't be inspected

**Context:** Any layout, 2D mode.

**Tried:** Wanted to see what an edge represented (full label, source
and target, any annotations) for an edge whose rendered label was
truncated or ambiguous.

**Expected:** Hover or click the edge label and a detail box appears,
mirroring the node-inspection interaction.

**Got:** Edges don't respond to hover or click. The only context the
reader gets is the rendered short label.

**Severity:** `impedes-comprehension`

**Candidate fixes:**
- Make edges (or at least their labels) pointer-event targets with the same hover/click → detail-box interaction as nodes.

---

## F4 — Multiple detail boxes overlap

**Context:** Any layout, 2D mode, after F2/F3 are fixed and multiple
detail boxes can coexist.

**Tried:** Inspected several nodes/edges to compare them.

**Expected:** Detail boxes auto-arrange so they don't occlude each
other.

**Got:** Boxes stack on top of each other (or share a single fixed
slot, depending on UI), so the reader can only see one at a time.

**Severity:** `annoyance`

**Candidate fixes:**
- Layout-aware placement: push each new box to the nearest non-overlapping position; leader lines back to the anchor.
- Or: a stacked panel in a corner with anchored leader lines, accepting that the boxes aren't visually adjacent to their anchors.

---

## F5 — No focus mode for drilling into a node's neighborhood

**Context:** Any layout, 2D mode. The reader has identified a node
they care about and wants to study its local structure.

**Tried:** Clicked the node, tried to mentally trace its edges to
nearest and next-nearest neighbors.

**Expected:** A mode that, on click, re-arranges the view around the
selected node — centered, with nearest neighbors (nn) and next-nearest
neighbors (nnn) placed on equally-spaced spokes radiating outward.
Centered + nn + nnn nodes carry full visual focus; the rest of the
graph fades into the background but stays hoverable/clickable so the
reader can pivot.

**Got:** Clicking only highlights / selects in place. No spatial
reorganization. To trace nn/nnn the reader has to visually walk edges
through the blob.

**Severity:** `impedes-comprehension`

**Candidate fixes:**
- A toggleable "focus mode" on the graph chrome.
- When active: click → animate to a radial layout (centered node, nn ring at radius r₁, nnn ring at r₂).
- Background nodes desaturate / dim but keep pointer events; a second click on a background node re-pivots focus to it.

---

## F6 — Orphan nodes clutter the main graph

**Context:** Any layout, 2D mode. Schemas with disconnected
components (e.g. utility classes not yet wired into the main
hierarchy) put orphan nodes around the perimeter of the connected
cluster.

**Tried:** Wanted to study the connected portion of the schema
without the orphans contributing to visual noise.

**Expected:** A mode that sequesters orphan / disconnected nodes into
a matrix in a corner of the canvas, leaving the connected component
to use the rest of the viewport.

**Got:** Orphans live in the same world as the connected graph,
distributed around the perimeter by the centering force (or by
KK's initial placement for components KK can't reach). They aren't
useful where they sit and they consume viewport real estate the
connected cluster could use.

**Severity:** `annoyance`

**Candidate fixes:**
- "Sequester orphans" toggle: detect components of size 1 (or ≤ N), arrange them in a grid in a corner, exclude them from the main layout's bbox-fit computation.
- Optional escalation: sequester all components below a configurable size threshold, not just singletons.

---

## Triage

After every 5 frictions logged, re-examine the severity distribution
and group related entries. Frictions of `impedes-comprehension` or
higher get fed into the next feature-spec triage; `annoyance`-tagged
frictions cluster into a single QoL slice.

Don't fix in place — frictions exist to inform spec work, not to act
as a todo list. If a friction looks trivially fixable, log it anyway:
the value here is the structured backlog, not the urgency.
