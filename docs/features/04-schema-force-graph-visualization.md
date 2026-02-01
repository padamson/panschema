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

**Status:** Not Started

**User Value:** Users can filter, search, focus on specific parts of the schema, and manually position nodes.

**Acceptance Criteria:**

#### Node Selection and Dragging
- [ ] Hit testing: click detection on nodes (ray-cast in 3D, point-in-circle in 2D)
- [ ] Click node to select (visual highlight, show info panel)
- [ ] Drag node to reposition while simulation continues
- [ ] Node becomes "fixed" during drag (velocity zeroed)
- [ ] Release to let node rejoin simulation (or option to keep fixed)
- [ ] Double-click to toggle fixed/unfixed state
- [ ] Visual feedback: cursor change, highlight on hover/select
- [ ] Touch support for mobile (tap to select, drag to move)

#### Focus and Filtering
- [ ] Click node to "focus" - center camera, dim unconnected nodes
- [ ] Filter by node type (show only classes, only properties, etc.)
- [ ] Search by label (highlights matching nodes)
- [ ] Show/hide edge types independently

#### UI and Details
- [ ] Details panel on selection (label, description, connections)
- [ ] Keyboard shortcuts:
  - `R` = reset camera
  - `F` = focus selected node
  - `Escape` = deselect
  - `Delete` = unfix selected node (let it rejoin simulation)
- [ ] Selection persists across simulation ticks

#### Implementation Approach

**Hit Testing (3D):**
```rust
/// Ray-cast from camera through mouse position to find intersecting node
fn pick_node_3d(mouse_x: f32, mouse_y: f32, camera: &Camera3D, nodes: &[SimNode3D]) -> Option<usize> {
    let ray = camera.screen_to_ray(mouse_x, mouse_y);

    let mut closest: Option<(usize, f32)> = None;
    for (i, node) in nodes.iter().enumerate() {
        if let Some(t) = ray_sphere_intersect(&ray, node.position(), node.radius) {
            if closest.is_none() || t < closest.unwrap().1 {
                closest = Some((i, t));
            }
        }
    }
    closest.map(|(i, _)| i)
}
```

**Hit Testing (2D):**
```rust
/// Point-in-circle test for 2D canvas
fn pick_node_2d(mouse_x: f32, mouse_y: f32, camera: &Camera, nodes: &[SimNode]) -> Option<usize> {
    let world_pos = camera.screen_to_world(mouse_x, mouse_y);

    for (i, node) in nodes.iter().enumerate().rev() { // Back-to-front for z-order
        let dx = world_pos.x - node.x;
        let dy = world_pos.y - node.y;
        if dx * dx + dy * dy <= node.radius * node.radius {
            return Some(i);
        }
    }
    None
}
```

**Drag State Machine:**
```rust
enum DragState {
    None,
    Hovering(usize),           // Mouse over node
    Dragging { node: usize, offset: Vec3 }, // Actively dragging
}

struct InteractionState {
    drag: DragState,
    selected: Option<usize>,   // Currently selected node
    fixed_nodes: HashSet<usize>, // Nodes pinned by user
}
```

**Node Fixing During Drag:**
```rust
// In simulation tick:
for (i, node) in nodes.iter_mut().enumerate() {
    if interaction.is_dragging(i) {
        // Follow mouse, zero velocity
        node.x = drag_world_pos.x;
        node.y = drag_world_pos.y;
        node.z = drag_world_pos.z; // 3D only
        node.vx = 0.0;
        node.vy = 0.0;
        node.vz = 0.0;
    } else if interaction.is_fixed(i) {
        // User pinned this node
        node.vx = 0.0;
        node.vy = 0.0;
        node.vz = 0.0;
    }
}
```

**Files:**
| File | Purpose |
|------|---------|
| `panschema-viz/src/interaction.rs` | Hit testing, drag state, selection |
| `panschema-viz/src/lib.rs` | Wire up mouse events to interaction |
| `panschema/templates/components/graph_viz.html` | Mouse event handlers, cursor styles |

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

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: GPU Force Simulation | Must Have | None | ✅ Complete |
| Slice 2: 3D Graph Renderer | Must Have | Slice 1 | ✅ Complete |
| Slice 3: GraphWriter | Must Have | None | ✅ Complete |
| Slice 4: WebGPU HTML Integration | Must Have | Slices 1, 2, 3 | ✅ Complete |
| Slice 5: Node and Edge Labels | Should Have | Slice 4 | ✅ Complete |
| Slice 6: Interaction and Dragging | Should Have | Slice 4 | Not Started |
| Slice 7: Barnes-Hut Optimization | Nice to Have | Slice 1 | Not Started |

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
