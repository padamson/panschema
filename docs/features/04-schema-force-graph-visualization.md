# Schema Force Graph Visualization - Implementation Plan

**Feature:** GPU-Accelerated 3D Force Graph Visualization

**User Story:** As a schema author, I want to explore my ontology as an interactive 3D force-directed graph, so that I can understand class relationships, property connections, and overall schema structure with intuitive rotation, zoom, and perspective—even for large ontologies.

**Related ADR (if applicable):** None yet (implementation approach to be determined through prototyping)

**Approach:** Vertical Slicing with Outside-In TDD

---

## Strategic Differentiation

WIDOCO and similar tools provide 2D D3.js-based visualization. panschema differentiates with:

1. **3D visualization** - Rotation, perspective, and depth make complex ontologies more navigable
2. **GPU acceleration** - WebGPU compute shaders enable smooth interaction with large ontologies (5,000+ nodes)
3. **Offline-first** - No CDN dependencies, works in air-gapped environments

---

## Architecture Decision (2026-01-31)

### Self-Contained in panschema

Originally considered contributing to gpui-d3rs, but decided to build directly in panschema because:

1. **gpui-d3rs targets desktop apps** via the gpui framework
2. **panschema needs browser-based visualization** via WebGPU
3. **No shared infrastructure** - the GPU force simulation doesn't depend on gpui

All visualization code lives in `src/gpu/` with a `gpu` feature flag.

### What We Need to Build

| Component | Complexity | Status |
|-----------|------------|--------|
| GPU Force Simulation | High | ✅ Complete (brute-force O(n²)) |
| 3D Graph Renderer | Medium | ✅ Complete |
| GraphWriter (JSON output) | Low | ✅ Complete |
| WebGPU Browser Target | Medium | ✅ Complete |
| Text/Label Rendering | Medium | Not Started |
| Node Selection & Dragging | Medium | Not Started |

---

## Architecture

### GPU Force Simulation Pipeline (Implemented)

```
┌─────────────────────────────────────────────────────────────┐
│                    CPU (Rust/WASM)                          │
├─────────────────────────────────────────────────────────────┤
│  Graph Data (nodes, edges)                                  │
│       │                                                     │
│       ▼                                                     │
│  Upload to GPU Buffers (GpuNode, GpuEdge)                   │
└───────┬─────────────────────────────────────────────────────┘
        │
        ▼
┌─────────────────────────────────────────────────────────────┐
│                 GPU Compute Shaders (WGSL)                  │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐   │
│  │ Link Force   │───▶│ Many-Body    │───▶│ Center Force │   │
│  │ (Springs)    │    │ (Brute O(n²))│    │ (Gravity)    │   │
│  └──────────────┘    └──────────────┘    └──────────────┘   │
│                                                 │           │
│                                                 ▼           │
│                                         ┌──────────────┐    │
│                                         │ Integrate    │    │
│                                         │ (Verlet)     │    │
│                                         └──────────────┘    │
│                                                │            │
└────────────────────────────────────────────────┼────────────┘
                                                 │
                                                 ▼
┌─────────────────────────────────────────────────────────────┐
│                 GPU Render Shaders (WGSL)                   │
│                      [Implemented in Slice 2]               │
├─────────────────────────────────────────────────────────────┤
│  • Instanced node spheres (icosphere mesh, Blinn-Phong)     │
│  • Edge lines (line primitive)                              │
│  • Camera3D (orbit/zoom/pan)                                │
│  • Depth buffer                                             │
└─────────────────────────────────────────────────────────────┘
```

### Technology Stack

| Layer | Technology | Notes |
|-------|------------|-------|
| GPU API | wgpu | Cross-platform, targets WebGPU in browsers |
| Shaders | WGSL | WebGPU Shading Language |
| Browser | WebGPU | Chrome 113+, Firefox 121+, Safari 18+ |
| Fallback | CPU simulation | For browsers without WebGPU (not yet implemented) |
| Build | wasm-pack | Rust → WASM compilation |

---

## Vertical Slices

### Slice 1: GPU Force Simulation Core

**Status:** ✅ Complete

**Location:** `src/gpu/`

**User Value:** High-performance force-directed graph layout for schema visualization.

**Acceptance Criteria:**
- [x] GPU buffer structures for nodes (position, velocity, fixed, charge, mass)
- [x] GPU buffer structures for edges (source, target, strength, distance)
- [x] Many-body force compute shader (brute-force O(n²))
- [x] Link force compute shader (spring constraints)
- [x] Center force compute shader (gravity toward center)
- [x] Velocity integration compute shader (with decay and clamping)
- [x] Alpha decay and convergence detection
- [x] Configurable parameters exported as constants

#### Implementation

| File | Purpose |
|------|---------|
| `src/gpu/mod.rs` | Module entry, public exports |
| `src/gpu/types.rs` | GPU buffer types, config, default constants |
| `src/gpu/shaders.rs` | WGSL compute shaders |
| `src/gpu/simulation.rs` | GpuSimulation orchestration |

**Feature flag:** `gpu` (optional, adds wgpu, bytemuck, pollster deps)

**Key design decisions:**
1. Sentinel value (`-1e9`) instead of NaN for fixed position detection (NaN unreliable in WGSL)
2. Brute-force O(n²) many-body instead of Barnes-Hut (sufficient for graphs < 5000 nodes)
3. Added center force for keeping graph centered
4. All parameters configurable via `GpuSimulationConfig` with exported `DEFAULT_*` constants

**Commands:**
```bash
cargo build --features gpu
cargo test --features gpu --lib
```

---

### Slice 2: 3D Graph Renderer

**Status:** ✅ Complete

**Location:** `src/gpu/` (extend existing module)

**User Value:** Force graph can be visualized with interactive 3D camera controls.

**Acceptance Criteria:**
- [x] Render instance types (`NodeInstance`, `EdgeInstance`, `CameraUniforms`)
- [x] Node rendering as instanced spheres (colored by type)
- [x] Edge rendering as lines
- [x] Camera3D with orbit, zoom, pan operations
- [x] Icosphere mesh generation
- [x] Off-screen rendering with `read_pixels()` for testing

#### Implementation

| File | Purpose |
|------|---------|
| `src/gpu/types.rs` | `NodeInstance`, `EdgeInstance`, `CameraUniforms`, `RenderConfig` |
| `src/gpu/camera.rs` | `Camera3D` with view/projection matrices, orbit/zoom/pan |
| `src/gpu/geometry.rs` | Icosphere mesh generation (level 2: 162 vertices) |
| `src/gpu/render_shaders.rs` | WGSL vertex/fragment shaders (Blinn-Phong lighting) |
| `src/gpu/renderer.rs` | `GpuRenderer` with instanced node/edge pipelines |

**Key design decisions:**
1. Separate `GpuRenderer` struct from `GpuSimulation` (single responsibility)
2. Shared `Arc<Device>` and `Arc<Queue>` between simulation and renderer
3. Inline matrix math (no new dependencies)
4. Icosphere level 2 balances visual quality and vertex count

**Commands:**
```bash
cargo build --features gpu
cargo test --features gpu --lib
```

---

### Slice 3: GraphWriter (Schema → Graph JSON)

**Status:** ✅ Complete

**Location:** `src/graph_writer.rs`

**User Value:** panschema can export schema structure as graph JSON for visualization.

**Acceptance Criteria:**
- [x] `GraphWriter` implements `Writer` trait
- [x] Outputs JSON format with graph topology (nodes and edges)
- [x] Node types: Class, Slot, Enum, Type (with distinct colors)
- [x] Edge types: SubclassOf, Mixin, Domain, Range, Inverse, TypeOf
- [x] Metadata: labels, descriptions, URIs
- [x] Options to include/exclude slots, enums, types
- [x] Registered in `FormatRegistry`

#### Implementation

| File | Purpose |
|------|---------|
| `src/graph_writer.rs` | GraphWriter, GraphData, GraphNode, GraphEdge, GraphOptions |
| `src/io.rs` | Register GraphWriter in FormatRegistry |
| `src/lib.rs` | Export graph_writer module |

**Commands:**
```bash
cargo build
cargo test --lib graph_writer
cargo run -- generate --input schema.yaml --output graph.json --format graph-json
```

---

### Slice 4: WebGPU HTML Integration (panschema)

**Status:** ✅ Complete

**User Value:** Users can view and interact with their schema as a 3D force graph in generated HTML documentation.

**Acceptance Criteria:**
- [x] HTML output includes embedded WASM + WebGPU visualization
- [x] Visualization initializes with schema data (embedded JSON)
- [x] Works offline (no external dependencies)
- [x] Loading indicator during WASM initialization
- [x] CPU fallback simulation for browsers without WebGPU
- [x] 2D Canvas rendering fallback when WebGPU unavailable
- [x] Browser support message for non-WebGPU browsers

#### Build Pipeline

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│ panschema       │────▶│ wasm-pack build │────▶│ graph-viz.wasm  │
│ src/gpu/        │     │ --target web    │     │ graph-viz.js    │
│ (force + render)│     │ --features gpu  │     │                 │
└─────────────────┘     └─────────────────┘     └─────────────────┘
                                                        │
                                                        ▼
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│ panschema       │────▶│ HTML template   │────▶│ output.html     │
│ SchemaGraph     │     │ + embedded WASM │     │ (self-contained)│
└─────────────────┘     └─────────────────┘     └─────────────────┘
```

#### Fallback Strategy

For browsers without WebGPU:
1. Detect WebGPU support at runtime
2. Fall back to CPU simulation (existing force module)
3. Render with 2D Canvas (still useful, just not 3D)
4. Show message: "For 3D visualization, use Chrome 113+, Firefox 121+, or Safari 18+"

---

### Slice 5: Node and Edge Labels

**Status:** ✅ Complete

**User Value:** Users can see labels on nodes and edges to understand what each element represents, with flexible controls to manage visual clutter.

**Acceptance Criteria:**

#### Text Rendering
- [x] Node labels positioned beside nodes (2D: Canvas text, 3D: HTML overlay)
- [x] Edge labels positioned at edge midpoint
- [x] Crisp text at any zoom level (HTML overlay for 3D, Canvas2D for 2D)
- [x] Labels hidden when node is behind camera (3D mode visibility culling)

#### Label Toggle Controls
- [x] Global toggle: All labels on/off (keyboard shortcut: `L`)
- [x] Node labels toggle: Show/hide all node labels (keyboard shortcut: `N`)
- [x] Edge labels toggle: Show/hide all edge labels (keyboard shortcut: `E`)
- [x] Hover reveal: Show label on hover even when labels are globally off
- [x] UI controls panel with toggle buttons for each mode
- [x] Persist label preferences in localStorage

#### Implementation

**Approach:** HTML overlay labels (simpler than SDF, crisp at any zoom)

For 3D mode, labels are rendered as HTML `<span>` elements positioned over the WebGPU canvas:
- WASM projects 3D node/edge positions to 2D screen coordinates
- JavaScript updates HTML element positions each frame
- devicePixelRatio handled for crisp rendering on HiDPI displays

For 2D mode, labels are rendered directly on the Canvas2D context with hover support in WASM.

**Files:**
| File | Purpose |
|------|---------|
| `panschema-viz/src/labels.rs` | LabelOptions state (all/node/edge toggles) |
| `panschema-viz/src/canvas2d.rs` | 2D Canvas label rendering, hover detection |
| `panschema-viz/src/lib.rs` | WASM bindings for label controls and hover |
| `panschema-viz/src/camera3d.rs` | 3D→2D projection for HTML overlay positioning |
| `panschema/templates/components/graph_viz.html` | HTML overlay, toggle buttons, localStorage |

**Key Design Decisions:**
1. HTML overlay for 3D labels (crisp text without SDF complexity)
2. Separate hover detection: WASM hit-testing for 2D, JavaScript proximity for 3D
3. Highlight style for hovered labels (blue background, white text)
4. devicePixelRatio conversion between canvas pixels and CSS pixels

---

### Slice 6: Interaction and Polish

**Status:** In Progress

**User Value:** Users can filter, search, focus on specific parts of the schema, and manually position nodes.

**Acceptance Criteria:**

#### Node Selection and Dragging
- [x] Hit testing: click detection on nodes (ray-cast in 3D, point-in-circle in 2D)
- [x] Click node to select (visual highlight, show info panel)
- [x] Drag node to reposition while simulation continues
- [x] Node becomes "fixed" during drag (velocity zeroed)
- [x] Release to let node rejoin simulation (or option to keep fixed)
- [x] Shift+click to toggle pin (desktop); long-press with haptic feedback (touch)
- [x] Visual feedback: cursor change, highlight on hover/select
- [ ] Touch support for mobile (tap to select, drag to move, long-press to pin)

#### Focus and Filtering
- [ ] Click node to "focus" - center camera, dim unconnected nodes _(dimming done; camera centering not implemented)_
- [ ] Filter by node type (show only classes, only properties, etc.) _(backend exists, no UI controls)_
- [ ] Search by label (highlights matching nodes)
- [ ] Show/hide edge types independently

#### UI and Details
- [x] Details panel on selection (label, description, connections)
- [x] Keyboard shortcuts:
  - `R` = reset camera
  - `F` = focus selected node
  - `Escape` = deselect
  - `Delete` = unfix selected node (let it rejoin simulation)
- [x] Selection persists across simulation ticks

---

### Slice 7: Barnes-Hut Optimization

**Status:** Not Started

**Priority:** Nice to Have (current brute-force handles ~5000 nodes)

**User Value:** Users with very large ontologies (10,000+ nodes) get smooth performance.

**Acceptance Criteria:**
- [ ] Octree spatial data structure for 3D Barnes-Hut
- [ ] GPU-compatible octree traversal in compute shader
- [ ] O(n log n) many-body force calculation
- [ ] Configurable theta parameter for accuracy vs speed tradeoff
- [ ] Performance benchmarks comparing brute-force vs Barnes-Hut

**Notes:**
- Only needed when brute-force O(n²) becomes a bottleneck
- Brute-force is simpler and sufficient for most real-world ontologies
- Can reference gpui-d3rs QuadTree or academic papers for octree design

---

### Slice 8: Class↔slot edges via `class.slots:` (inverse-of-`domain:`)

**Status:** Completed

**Priority:** Should Have

**User Value:** Every slot a class actually uses connects to that class in the rendered graph, regardless of whether the LinkML author wrote `slot.domain` on the slot side or just listed it in `class.slots:` on the class side. Per the LinkML metamodel, `domain_of` is the computed inverse of `domain:` — the graph should treat both as equivalent for class↔slot connectivity. Today the graph builder only honors `slot.domain`, so a schema where the host class lists the slot but the slot itself omits `domain:` shows the slot as an orphan node in the rendered viz, even though the class clearly uses it (and the HTML class-card already lists it correctly). Verified against scimantic-schema v0.2.0: at least 13 slots are currently orphan for this exact reason, including the multi-class `content` slot used by both `Evidence` and `Conclusion`.

**Acceptance Criteria:**
- [x] `GraphWriter` walks `schema.classes.<C>.slots:` lists during graph build and emits a class↔slot edge for each reference, in addition to (not instead of) the existing `slot.domain` traversal.
- [x] When a slot is referenced from N classes' `slots:` lists, the graph emits N distinct edges (one per class). Multi-class slots like scimantic's `content` (used by both `Evidence` and `Conclusion`) connect to both classes.
- [x] Idempotent: when a slot has `domain: X` AND `X.slots:` lists it, the result is a single `(slot:s, class:X)` edge — no duplicate emitted.
- [x] Edge type/label: reuse `EdgeType::Domain` (LinkML treats `domain` and `domain_of` as equivalent; a separate variant would split a semantically identical relation in two for no UX gain). Label stays `"domain"`.
- [x] Behavior gated by the existing `options.include_domain_edges` flag — class-side traversal is the same relation as slot-side traversal, not a separate toggle.
- [x] Range-edge behavior is unchanged: slots whose `range` is a primitive (`string`, `integer`, etc.) still produce no range node — this slice is strictly about class↔slot connectivity.
- [x] Unit tests: minimal class-side reference, idempotent both-sides, multi-class slot, non-existent slot reference (graceful skip, no panic).

**Notes:**
- One small helper in `GraphWriter`, called once after `add_slots` so the slot-side `domain` edges are already in `graph.edges` for the dedup seed. No new `EnumType` variants, no new option fields — pure additive logic.
- After this slice ships, scimantic-schema's deployed Pages graph at `/schema/v0.2.0/` gains edges for every previously-orphan slot, closing the cross-repo bug. Re-render needed downstream.

---

### Slice 9: Hover-driven ephemeral node and edge details

**Status:** Completed

**Priority:** Should Have

**User Value:** A schema author scanning the rendered graph can see what a node or edge represents *without* committing to a click. Hover surfaces an ephemeral mini-card (label, type, IRI, connection count for nodes; edge-type and endpoints for edges) anchored near the cursor; it disappears on hover-out. Faster than the slice-6 click-to-select flow for "I just want to identify this node and move on" — the dominant interaction pattern when reading an unfamiliar schema.

The current slice-6 click-to-select behavior is preserved for "I want to lock this view in" — clicking still pins the persistent details panel that already exists. Hover is the *additive* affordance, not a replacement.

**Acceptance Criteria:**
- [x] Hovering a node surfaces an ephemeral mini-card anchored near the cursor (offset so the cursor doesn't occlude the card). Card content: label, type (Class / Slot / Enum / Type) with an "(abstract)" suffix when applicable, schema-internal ID, IRI when the entity declares one, connection count, and the LinkML description as a wrapped block at the bottom.
- [x] Hovering an edge surfaces an ephemeral mini-card: edge type (`subclassOf`, `domain`, `range`, `mixin`, `inverseOf`, `typeOf`), source label, target label, rendered as a vertical triple `<source> ↓ <edge-type> ↓ <target>`.
- [x] Hover-out (cursor leaves the node/edge or leaves the canvas) hides the card immediately — no lingering or fade-in/out (snappy reading).
- [x] The card auto-positions to stay within the viewport (flip to the cursor's other side when near the right or bottom edge). `position: fixed` against the viewport so cursor-relative positioning works regardless of which ancestor is positioned.
- [x] Click-to-select (slice 6) still works and shows the persistent details panel. The hover card hides itself when the hovered node is already the currently click-selected node — the persistent panel below already shows the same content.
- [x] Hit testing: reuses the existing slice-6 hit-test path (`update_hover` for nodes; `edge_at` for edges). No new code on the Rust side; the hover card just consumes the existing hover state.
- [x] Mobile/touch: a `@media (pointer: coarse)` rule hides the card entirely on touch devices. Touch users still get the slice-6 tap-to-select flow unchanged.

**Notes:**
- The ephemeral card and the slice-6 persistent panel both consume the same `get_node_details` / `get_edge_details` JSON contract on the Rust side — DRYing the rendering logic so the card is a "preview" and the panel is the "pinned" view of the same data.
- The JSON-builder logic moved from the wasm-only `Visualization` methods into `pub(crate) build_*_details_json` free functions so the contract is unit-testable natively (the wasm `Visualization` itself needs an `HtmlCanvasElement` and can only be constructed in a browser).
- `SimNode` gained `description`, `uri`, and `is_abstract` fields — these existed on `GraphNode` but dropped on conversion before this slice. Carrying them through unlocks the hover-card content the user actually wants without changing the wire format.
- Hover state is cached by `kind:idx` so re-renders only happen when the hover target changes; mousemove events that don't change the target are O(1).

---

### Slice 10: Hover focus-mode highlight (node + 1-hop + 2-hop neighbors)

**Status:** Completed

**Priority:** Should Have

**User Value:** A schema author can see the *local subgraph* around any node by hovering — the hovered node, its 1-hop neighbors (directly-connected nodes), and its 2-hop neighbors (one more level out) snap to full opacity and slightly enlarged size, everything else dims to ~25% opacity. The "local context" jumps out without any clicks. Restores instantly on hover-out.

This is the most asked-for affordance in graph-exploration UIs (Gephi's "Ego network filter," Cytoscape's "focus-on-hover" mode, Cosmograph's `hovered-state-neighbors`). It transforms a 100-node tangle from "where is X" into "X and its neighborhood, clearly."

**Acceptance Criteria:**
- [x] Hovering a node activates focus mode: hovered node + 1-hop neighbors + 2-hop neighbors render at full opacity; all other nodes and edges dim. Edges between focused-set nodes render at full opacity; edges with one endpoint outside the focused set dim alongside the unfocused side. (Inherits the existing slice-6 dimming pass — the slice 10 work was extending the neighbor set to multi-hop and feeding it from hover instead of the F-key.)
- [x] Hover-out restores all nodes/edges to full opacity instantly. The `mouseleave` handler calls `clear_focus`; no fade animation — visual snap is the right read.
- [x] Focus mode is on by default but toggleable via a UI control. The "Focus on hover" button sits in the graph controls strip alongside Labels / Nodes / Edges; clicking toggles enabled state. The preference persists to localStorage under `panschema-focus-on-hover`.
- [x] Hovered-set highlight: the focal node renders with the existing slice-6 focused-node visual (brighter border / larger scale). The renderer already distinguishes focal from neighbors via `Option<usize>` vs `&HashSet<usize>`; we just fed it richer data.
- [x] Configurable hop depth: the JS-side `FOCUS_HOP_DEPTH = 2` constant and the Rust `focus_node(idx, max_hops)` parameter make this trivially adjustable. The schema-author default is 2 — reveals the local cluster without dragging in the whole graph. The F-key click-to-focus affordance uses the same depth for visual consistency.
- [x] Performance: the neighborhood set is computed once at `focus_node` time via BFS frontier expansion and cached on `InteractionState`. Per-frame access is an O(1) `HashSet::clone` instead of an O(E × hop_depth) re-walk; redundant calls (same hover target across mousemove ticks) are short-circuited in JS by caching `lastFocusedNodeIdx`.
- [x] Touch / `pointer: coarse`: focus mode is hover-driven, and hover events don't fire on touch — touch users still get the slice-6 tap-to-select flow unchanged. No extra wiring needed.
- [x] Composes with slice 9: when the cursor is over a node, both the ephemeral hover card (slice 9) and the focus dim (slice 10) activate. Two independent affordances driven by the same hover state.
- [x] Unit tests for BFS expansion: 0-hop (focal only), 1-hop (direct neighbors), 2-hop (neighbors-of-neighbors), overshoot (more hops than graph diameter), isolated node (empty neighborhood), clear-resets-everything.

**Notes:**
- BFS over the simulation's edge list instead of a precomputed adjacency map. Adjacency was the original plan; profiling on the scimantic graph (84 nodes, 149 edges) showed sub-millisecond BFS times, so the constant-factor win didn't justify the extra state. If a future schema hits sluggish focus, build the map in `CpuSimulation::from_graph_data` and cache.
- Dim opacity 0.25 from the existing slice-6 renderer pass — kept untouched. Sweet spot validated by the manual test on scimantic-schema; revisit if multi-scale screenshots show otherwise.

---

### Slice 11: Hover card surfaces resolved-schema context (slots, parents, mixins, permissible values)

**Status:** ✅ Complete

**Priority:** Should Have

**User Value:** Slice 9 surfaces what's already on the visualization wire format (label, type, IRI, description, abstract flag, raw connection count). This slice extends the hover card to answer the actual questions a schema author asks while reading the graph:

- For a **class** node: which slots can this class have? Which parents does it inherit from (`is_a` chain)? Which mixins does it mix in?
- For a **slot** node: what's its domain? What's its range? Is it required, multivalued?
- For an **enum** node: what are the permissible values?
- For an **edge**: a one-sentence LinkML semantic blurb keyed by edge type (e.g. `subclassOf`: "children inherit parent's slots and constraints"; `domain`: "this slot can appear on this class").

These are the questions whose answers currently require a click-to-pin, then scroll, then squint at the entity card. Surfacing them on hover would turn the graph from a static map into an *active* exploration surface — closer to what authoring tools like Protégé and WebProtégé do with their hover affordances.

**Acceptance Criteria:**
- [x] `GraphNode` (in `panschema-viz/src/graph_types.rs`) gains a `kind_metadata` field — an enum carrying the per-kind structured payload:
  - `Class { slots: Vec<String>, parents: Vec<String>, mixins: Vec<String> }`
  - `Slot { domain: Option<String>, range: Option<String>, required: bool, multivalued: bool }`
  - `Enum { permissible_values: Vec<String> }`
  - `Type` carries no extra payload (LinkML types are leaf primitives).
- [x] `GraphWriter` populates `kind_metadata` from the LinkML IR during graph build. Resolves `is_a` chains and `mixins:` lists by label (not by id), so the hover card can show "is_a: Premise" not "is_a: class:Premise".
- [x] `SimNode` carries the `kind_metadata` through unchanged (same propagation pattern as `description` / `uri` in slice 9).
- [x] `build_node_details_json` emits the structured payload under a `kindMetadata` JSON key. The JS card renderer dispatches per `type` to render the right rows.
- [x] Class hover shows up to 5 slot names with a "+N more" tail when overflow; full slot list still available on click-to-pin (slice-6 persistent panel). Same overflow pattern for parents and mixins.
- [x] Slot hover shows `domain`, `range`, and a small row of flags (`required`, `multivalued`) when set.
- [x] Enum hover shows permissible values inline (same overflow pattern as slot list).
- [x] Edge hover gains a one-sentence semantic blurb keyed by `EdgeType` — `SubclassOf`, `Mixin`, `Domain`, `Range`, `Inverse`, `TypeOf`. Hardcoded table in JS; ~6 short strings.
- [x] Native unit tests for `GraphWriter`'s metadata population: one fixture per kind covering the resolution rules (slots with overrides, `is_a` chains, multi-mixin classes, enums with 10+ permissible values).
- [x] `build_node_details_json` tests extended to cover each kind's structured payload.

**Notes:**
- The right home for the per-kind payload is an enum on `GraphNode`, not a bag of optional fields. Pattern-matching at the JSON-emit site keeps each kind's shape clean.
- Resolution: slot inheritance via `is_a` / mixins should be resolved by `GraphWriter` before emission — the visualization layer shouldn't traverse the LinkML IR. The hover card sees a flat list of slot names with the inheritance already applied.
- Edge blurb table is JS-side because it's display copy, not data. Keep it close to the renderer.
- Out of scope (defer to a later slice): clickable cross-references inside the hover card (clicking a parent class name in the hover card should jump-to that class — needs URL routing + viewport pan). Slice 11 ships read-only context; navigation is its own affordance.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: GPU Force Simulation | Must Have | None | ✅ Complete |
| Slice 2: 3D Graph Renderer | Must Have | Slice 1 | ✅ Complete |
| Slice 3: GraphWriter | Must Have | None | ✅ Complete |
| Slice 4: WebGPU HTML Integration | Must Have | Slices 1, 2, 3 | ✅ Complete |
| Slice 5: Node and Edge Labels | Should Have | Slice 4 | ✅ Complete |
| Slice 6: Interaction and Dragging | Should Have | Slice 4 | 🚧 In Progress |
| Slice 7: Barnes-Hut Optimization | Nice to Have | Slice 1 | Not Started |
| Slice 8: Class↔slot edges via `class.slots:` | Should Have | Slice 3 | ✅ Complete |
| Slice 9: Hover-driven ephemeral node and edge details | Should Have | Slice 6 | ✅ Complete |
| Slice 10: Hover focus-mode highlight (1-hop + 2-hop neighbors) | Should Have | Slice 6 | ✅ Complete |
| Slice 11: Hover card surfaces resolved-schema context | Should Have | Slice 9 | ✅ Complete |

---

## Definition of Done

The feature is complete when ALL of the following are true:

- [ ] All acceptance criteria from user story are met
- [ ] All vertical slices marked as "Completed"
- [ ] GPU simulation handles 5,000+ nodes smoothly (brute-force limit)
- [ ] Works in Chrome, Firefox, Safari with WebGPU
- [ ] Graceful fallback for browsers without WebGPU
- [ ] All tests passing: `cargo nextest run` and `cargo test --features gpu --lib`
- [ ] Library documentation complete with examples: `cargo doc`
- [ ] Code formatted: `cargo fmt --check`
- [ ] No clippy warnings: `cargo clippy -- -D warnings`
- [ ] README.md updated
- [ ] CHANGELOG.md updated

---

## Open Questions (Updated)

1. ~~**Octree vs Linear Barnes-Hut:**~~ **Resolved:** Started with brute-force O(n²). Sufficient for graphs up to ~5000 nodes. Barnes-Hut can be added later if needed for larger graphs.

2. ~~**Atomic float operations:**~~ **Resolved:** Avoided by having each thread write only to its own node's velocity (link force writes to both source and target, but race conditions are acceptable for force accumulation).

3. **Edge rendering:** Lines are simple but hard to see. Tubes look better but are expensive. Start with lines, upgrade if needed.

4. **WASM bundle size:** wgpu + wasm can be large. Target < 1MB gzipped. May need aggressive dead code elimination.

5. **Browser support timeline:** WebGPU is new. Track adoption and ensure fallback path works well.

6. ~~**Text rendering approach:**~~ **Resolved:** HTML overlay for 3D labels (simpler than SDF, crisp at any zoom). WASM projects positions, JavaScript updates HTML elements. Canvas2D text for 2D mode.

---

## References

- [GraphPU: Building a Large-scale 3D GPU Graph Visualization Tool](https://latentcat.com/en/blog/building-graphpu)
- [GraphWaGu: GPU Powered Large Scale Graph Layout](https://www.willusher.io/publications/graphwagu/)
- [wgpu WebGPU documentation](https://wgpu.rs/)
- [WebGPU spec and browser support](https://caniuse.com/webgpu)
- [D3-force algorithm reference](https://github.com/d3/d3-force)

---

## Implementation Log

### 2026-01-31: Slice 1 Complete (Architecture Change)

**Architecture Decision:**
- Originally prototyped in gpui-d3rs fork, but realized gpui-d3rs targets desktop apps (gpui)
- panschema needs browser-based WebGPU visualization
- Moved code directly into panschema with `gpu` feature flag

**Completed:**
- Created `src/gpu/` module in panschema with `gpu` feature flag
- Implemented GPU buffer types matching WGSL struct layouts (types.rs)
- Implemented 4 compute shaders: link_force, many_body_force (brute), center_force, integrate (shaders.rs)
- GpuSimulation orchestration with wgpu v24 (simulation.rs)
- Added configurable simulation parameters with exported default constants
- Fixed several issues:
  - `target` → `target_node` (WGSL reserved word)
  - NaN → sentinel value for fixed position detection
  - Empty buffer handling for zero nodes/edges
  - wgpu v24 API compatibility (Maintain::Wait, device descriptor)

**Test Coverage:**
- 5 GPU-specific tests in `src/gpu/simulation.rs`
- Run with: `cargo test --features gpu --lib`
- Pre-commit clippy runs with `--all-features` (catches GPU lint issues)

**Next:** Slice 2 (3D Graph Renderer)

### 2026-01-31: Slice 2 Complete (3D Graph Renderer)

- Implemented `GpuRenderer` with instanced sphere (nodes) and line (edges) rendering
- Added `Camera3D` with spherical coordinates and orbit/zoom/pan operations
- Added icosphere mesh generation for smooth node spheres
- Added `examples/university/` with sample LinkML schema
- 53 GPU tests, all passing

**Next:** Slice 3 (GraphWriter)

### 2026-01-31: Slice 3 Complete (GraphWriter)

- Implemented `GraphWriter` following Reader/Writer pattern
- Outputs graph topology JSON (nodes with IDs/types/colors, edges with source/target)
- No positions in JSON - computed at runtime by force simulation (Slice 4)
- Node types: Class (blue), Slot (green), Enum (purple), Type (orange)
- Edge types: SubclassOf, Mixin, Domain, Range, Inverse, TypeOf
- `GraphOptions` for filtering (include/exclude slots, enums, types)
- Registered in `FormatRegistry` with format ID `graph-json`
- 17 unit tests, all passing

**Next:** Slice 4 (WebGPU HTML Integration)

### 2026-01-31: Slice 4 Complete (WebGPU HTML Integration)

**Architecture:**
- Created `panschema-viz` workspace crate for WASM bindings
- Separate from main `panschema` crate to isolate WASM-specific dependencies
- Feature-gated WebGPU support: `#[cfg(all(feature = "webgpu", target_arch = "wasm32"))]`

**Completed:**
- `panschema-viz/` crate with WASM bindings (wasm-bindgen, wasm-pack)
- 2D CPU fallback: `CpuSimulation` + `Canvas2DRenderer`
- 3D WebGPU: `Simulation3D` + `WebGpuRenderer` (when webgpu feature enabled)
- Camera systems: `Camera` (2D pan/zoom) and `Camera3D` (orbit/zoom/pan)
- Graph JSON embedded in HTML output via `include_str!`
- WASM bundle embedded in HTML (offline-capable)
- Automatic fallback: WebGPU → 2D Canvas → static graph
- Loading spinner during WASM initialization
- Browser support message when falling back to 2D
- Sidebar "Schema Graph" link with node/edge count badge
- Smooth fit-to-bounds animation after simulation settles

**Files:**
| File | Purpose |
|------|---------|
| `panschema-viz/src/lib.rs` | WASM entry points, Visualization/Visualization3D |
| `panschema-viz/src/simulation.rs` | CPU 2D force simulation |
| `panschema-viz/src/simulation3d.rs` | CPU 3D force simulation (Fibonacci sphere) |
| `panschema-viz/src/canvas2d.rs` | 2D Canvas renderer with labels |
| `panschema-viz/src/webgpu.rs` | WebGPU 3D renderer (billboard nodes, lines) |
| `panschema-viz/src/camera.rs` | 2D camera with smooth animations |
| `panschema-viz/src/camera3d.rs` | 3D orbit camera with smooth animations |
| `panschema/templates/components/graph_viz.html` | Graph visualization component |
| `panschema/templates/components/sidebar.html` | Sidebar with graph link |

**Key Design Decisions:**
1. Billboard quads for 3D nodes (simpler than spheres, GPU-efficient)
2. Fibonacci sphere for initial 3D node distribution (even spacing)
3. Separate `is_3d()` method for reliable mode detection
4. 50-tick delay before fit-to-bounds (let simulation spread nodes)

**Test Coverage:**
- 187 tests passing (nextest)
- 195 tests passing with GPU feature
- 29 panschema-viz tests (camera, simulation)

**Next:** Slice 5 (Node and Edge Labels)

### 2026-02-01: Slice 5 Complete (Node and Edge Labels)

**Architecture Decision:**
- Chose HTML overlay for 3D labels instead of SDF font atlas
- Simpler implementation, crisp text at any zoom, no build-time font processing
- WASM projects 3D positions to screen coordinates, JavaScript positions HTML elements

**Completed:**
- `LabelOptions` struct with master toggle (all_labels) and category toggles (node_labels, edge_labels)
- Label toggle buttons in UI (All Labels, Nodes, Edges) with active state styling
- Keyboard shortcuts: `L` (all), `N` (nodes), `E` (edges)
- localStorage persistence of label preferences
- HTML overlay labels for 3D mode with visibility culling
- Canvas2D text labels for 2D mode
- Hover-to-reveal: show individual label on hover even when labels are toggled off
- 2D hover detection via WASM hit-testing (node_at, edge_at methods)
- 3D hover detection via JavaScript proximity check on projected positions
- Highlight styling for hovered labels (blue background, white text)
- devicePixelRatio handling for proper label alignment on HiDPI displays

**Files:**
| File | Purpose |
|------|---------|
| `panschema-viz/src/labels.rs` | LabelOptions state management |
| `panschema-viz/src/canvas2d.rs` | 2D label rendering with hover support |
| `panschema-viz/src/lib.rs` | WASM bindings for label/hover methods |
| `panschema-viz/src/camera3d.rs` | project_point(), project_to_screen() for 3D→2D |
| `panschema/templates/components/graph_viz.html` | HTML overlay, toggles, localStorage |

**Test Coverage:**
- 158 panschema tests passing
- 36 panschema-viz tests passing (includes camera3d projection tests)

**Next:** Slice 6 (Interaction and Dragging)
