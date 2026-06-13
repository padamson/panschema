# ADR-005: Graph Visualization Conventions

## Status

Accepted (2026-06-13)

## Context

panschema renders a schema as an interactive force-directed graph
(feature 04). The graph encodes two things that today are under-used
visual channels:

- **Node kinds** (`panschema-viz` `NodeType`): `Class`, `Slot`,
  `Enum`, `Type`. Abstract classes are a sub-state of `Class`.
- **Edge kinds** (`EdgeType`): `SubclassOf` (`is_a`), `Mixin`,
  `Domain` (slot→class), `Range` (slot→class/type/enum), `Inverse`,
  `TypeOf` (individual→class).

**Current rendering.** Nodes are filled circles colored by kind
(class = blue, slot = green, enum = purple, type = orange); abstract
classes only differ by reduced alpha. Every edge is the *same* 1.5px
gray line; its kind is legible only by reading the text label, and
its direction is not shown at all. Slice 15 (directed-edge
arrowheads) surfaced the question this ADR answers: before adding a
single uniform arrowhead, what is the *whole* convention for drawing
nodes and edges of different kinds?

**Why conventions matter here.** panschema is an ontology/LinkML
tool; its users come from a world with established visual languages,
and matching them buys instant legibility instead of forcing readers
to learn a bespoke notation. Two traditions are directly relevant:

- **UML class diagrams** — the near-universal notation for the
  *structural* relationships. Generalization (`is_a` / `subClassOf`)
  is a solid line with a **hollow triangle** head pointing at the
  parent; realization/mixin-like relations are **dashed** with a
  hollow head; plain associations use open/line arrows. The hollow
  triangle for "is-a" is the single most recognized glyph in the
  space.
- **VOWL** (Visual Notation for OWL, implemented by WebVOWL) — the
  closest thing to a standard for *ontology graphs* specifically. It
  encodes node kind by **shape** (classes = circles, datatypes =
  rectangles), not color alone, and gives each edge role a distinct
  directed style plus a legend.

**Accessibility.** Color alone is not an accessible encoding (~8% of
men have a color-vision deficiency). Shape (for nodes) and line-style
+ arrowhead-shape (for edges) carry the distinction redundantly with
color, so the graph is readable in grayscale.

**Renderer constraints.** There are two renderers: the 2D HTML canvas
(`canvas2d.rs`, the default, present on every machine) and the 3D
WebGPU path (`simulation3d.rs`/`webgpu.rs`, opt-in, GPU-dependent).
Canvas trivially supports hollow/filled polygons, dashed strokes, and
arbitrary node shapes; the 3D path renders all nodes as one icosphere
mesh and edges as plain lines, so richer glyphs there are
shader/instancing work.

## Decision

Adopt a **UML-grounded, VOWL-informed, accessibility-first** notation,
with the **2D canvas as the reference renderer** and the 3D path
carrying a reduced form. Color is always *reinforcing*, never the
sole signal.

### Node conventions

Shape is the primary (accessible) channel; color is retained and
unchanged as reinforcement.

| Node kind | Shape | Rationale |
|---|---|---|
| Class | Filled **circle** | Current; VOWL class glyph. |
| Class (abstract) | Circle with a **dashed outline** + lighter fill | Replaces the alpha-only cue; "abstract" reads structurally, matching the HTML card's `abstract` badge. |
| Type (datatype/primitive) | **Rectangle** | VOWL datatype glyph. |
| Enum | **Diamond** | A controlled vocabulary — distinct from both class and datatype. |
| Slot | Rounded **pill** | panschema graphs slots as nodes (VOWL models properties as edge labels); the pill keeps them visibly a different category. |

Colors stay as defined in `graph_types::colors`.

### Edge conventions

Each edge kind is distinguished **redundantly** by line style +
arrowhead glyph + color, so any single channel suffices.

| Edge kind | Line | Head (at target) | Reads as |
|---|---|---|---|
| `SubclassOf` (`is_a`) | solid | **hollow triangle** → parent | UML generalization |
| `Mixin` | **dashed** | hollow triangle | UML realization analog |
| `Domain` (slot→class) | solid | **filled** arrow | "defined on" |
| `Range` (slot→target) | solid | filled arrow | "points at" |
| `Inverse` | dashed | filled arrows at **both** ends | symmetric, no single direction |
| `TypeOf` (individual→class) | solid | filled arrow | "is a kind of" |

- Arrowheads sit on the **target node's perimeter** (stepped back by
  the node's rendered radius) and scale with that radius so they stay
  legible at any zoom without dominating short edges.
- Per-kind **color** comes from a small muted palette, keyed to the
  same edge-kind vocabulary the hover card already uses
  (`EDGE_KIND_BLURBS`). Structural edges (`is_a`, `mixin`) share a
  neutral hue; `domain`/`range`/`inverse`/`type_of` get distinct
  tints. The palette stays muted so a dense graph doesn't turn gaudy.

### Edge cardinality (crow's-foot on `range` edges)

A slot's multiplicity toward its range is shown with **ER crow's-foot
notation** at the **target end of the `range` edge** (slot → target).
The values come from the resolver's `effective_cardinality`
(slice 12.3), so the graph and the hover/class-card agree:

| LinkML cardinality | Crow's-foot (target end) |
|---|---|
| `1..1` (required, single) | mandatory-one — bar+bar `‖` |
| `0..1` (optional, single) | optional-one — circle+bar `○│` |
| `1..*` (required, multivalued) | mandatory-many — bar+foot `│⟨` |
| `0..*` (optional, multivalued) | optional-many — circle+foot `○⟨` |
| explicit `min..max` (not one of the above) | small `min..max` text label |

- Crow's-foot is the only place classic ER cardinality has a clean
  home in this graph: panschema **reifies slots as nodes**, so there
  is no `Class —relationship— Class` line to annotate, but the
  `range` edge *is* "the slot relates to N of this target," which is
  exactly the slot's cardinality.
- **Caveat (documented):** because a slot node is shared across every
  class that uses it, the edge shows the slot's **global** cardinality
  (its `SlotDefinition` flags/bounds). Per-class `slot_usage`
  refinements are not reflected at the edge — they remain in the hover
  card / class card (slice 14), which are per-class views.
- On a `range` edge the crow's-foot **replaces** the filled
  arrowhead and is the edge's terminator: it sits at the target rim,
  so direction still reads (the slot is the source), and a single
  glyph carries both "points here" and multiplicity rather than
  stacking two terminators. Consequently the "Arrows" toggle does not
  affect `range` edges — their crow's-foot is always shown (it's
  cardinality, not a direction decoration). Other edge kinds keep
  their arrow/triangle heads, which the toggle still hides.
- The crow's-foot glyphs get a legend entry alongside the edge-kind
  glyphs.

### Controls & discoverability

- An **"Arrows"** toggle hides all heads (direction off) for dense
  graphs; line style + color still distinguish kinds. Default on.
  Preference persists in `localStorage` (consistent with the
  Labels / Focus-on-hover toggles).
- A small **legend** maps glyph → meaning so the notation is
  learnable in place (VOWL/WebVOWL ship one). Collapsible; off by
  default on narrow viewports.

### 3D (reduced form)

The WebGPU renderer adopts the **per-kind edge color** and a single
**cone head** for direction. Hollow-vs-filled heads, dashed lines, and
per-kind node shapes are **deferred** in 3D (shader/instancing cost);
2D remains the notation's reference. This mirrors how slice 15 already
treated the 3D arrowhead as optional.

### Out of scope

- An **ER-projection graph mode** that collapses slot nodes into
  `Class —slot[0..*]→ Class` edges (à la LinkML's `gen-erdiagram`
  Mermaid output). It is the natural home for *fully* native
  cardinality, but it is a separate graph mode and a much larger
  change; decide it on its own when a consumer needs it. The
  crow's-foot decision above gives the *current* (slot-as-node)
  projection meaningful cardinality without it.
- Exact byte-for-byte VOWL conformance — we borrow its principles
  (shape encoding, legend, directedness), not its full color spec, so
  the notation fits panschema's existing palette and LinkML's
  vocabulary (`is_a`, `mixin`, `slot_usage`) rather than OWL's.

## Consequences

**Positive**

- Readers fluent in UML/ontology tooling parse the graph immediately;
  `is_a` vs `mixin` vs `domain` is visible without reading labels.
- Grayscale-legible: shape + line-style + head-shape are redundant
  with color.
- One documented convention governs every current and future
  node/edge styling change, so slices don't each re-litigate it.

**Negative / costs**

- Meaningfully more rendering code in `canvas2d.rs` (polygon glyphs,
  dashed strokes, per-kind dispatch) and new node-shape drawing.
- The 2D/3D notation gap is now explicit: 3D is intentionally a
  reduced form until the shader work is justified.
- A legend is new UI surface to design and keep in sync with the
  palette.

## Implementation staging

The conventions land across several slices (woven into feature 04);
each is independently shippable and dogfoodable:

1. **Edge kinds in 2D** (reshapes slice 15): line style + arrowhead
   glyph + per-kind color + the Arrows toggle. The bulk of the edge
   convention; closes the "undirected lines" friction.
2. **Edge cardinality in 2D**: crow's-foot glyphs on `range` edges
   from `effective_cardinality`.
3. **Node shapes in 2D**: circle / rectangle / diamond / pill by
   kind, plus the abstract dashed-outline treatment.
4. **Graph legend**: collapsible glyph→meaning key (edge kinds +
   crow's-foot cardinality + node shapes).
5. **3D reduced form**: per-kind edge color + cone heads.

Slices 2–5 are new feature-04 slices added when this ADR is accepted.
