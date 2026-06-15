# Feature: Graph Layout Selection

**Feature:** User-selectable 2D and 3D graph layout algorithms

**User Story:** As a documentation reader, I want to switch the schema graph between different layout algorithms (force-directed, hierarchical, circular, …) so that I can see the structure that best fits how I'm thinking about the schema right now — a class hierarchy as a tree, a property graph as a force layout, a small enum or cycle as a circle.

**Related ADR:** None yet (will need one once the algorithm set is fixed — likely "ADR: Graph layout algorithm set & default choice")

**Approach:** Vertical Slicing with Outside-In TDD. Each slice ships one additional layout algorithm end-to-end (Rust implementation + wasm binding + UI button), so the picker grows in usable form rather than landing as a single megacommit.

---

## Context

Panschema currently uses a single force-directed (Fruchterman-Reingold-derived) layout for the schema graph, in both 2D and 3D. This is the right default for "show me the shape of the connectivity," but it's not the right answer for every schema or every reader:

- **Class hierarchies** (`is_a` chains over BFO / CCO / ENVO, scimantic-style ontologies) are fundamentally trees. A reader scanning for "where does `Hypothesis` sit under `BFOContinuant`?" gets that instantly from a hierarchical layout and laboriously from a force layout.
- **Property cycles or small enums** (8-12 nodes in a circular relationship) read clearly when laid out as a circle with chords inside; force layouts fold them into an unrecognizable blob.
- **Wide-but-shallow taxonomies** (one root, many siblings) want a radial layout.
- **Large dense subgraphs** want stress majorization or ForceAtlas2 for cleaner separation than vanilla force-directed.

A picker also unblocks the layout-quality work that's currently entangled with the default: replacing the in-tree force code with d3-force-quality output requires either changing the default (risky — every existing user's graph will look different) or shipping the new algorithm alongside the old as a non-default choice (safe — opt-in until proven).

This feature spec captures the algorithm survey, the subset we'll implement, and the UX shape. The gating condition — a trustworthy force-directed default — was met by [Feature 02 slice 7 (viewport filling at all 3 scales)](02-core-ontology-documentation.md#slice-7-improve-force-directed-default-so-the-graph-fills-its-viewport). Slice 1 of this feature (the picker chrome over the existing force-directed implementation) is now safe to start.

---

## Literature survey

### 2D layout algorithms

#### Force-directed family

The dominant family. All variants treat nodes as physical bodies (charged particles, or particles in an energy field) and iterate to equilibrium. Differ in force shape, cooling schedule, and acceleration structures.

- **Eades' spring embedder (1984)** — the original. Logarithmic spring force + inverse-square repulsion. Pedagogically important; superseded by FR.
- **Fruchterman-Reingold (1991)** — `F_attr = d²/k` for connected pairs, `F_rep = -k²/d` for all pairs. Cools temperature linearly (max displacement per step shrinks each iteration). Simple, robust, ubiquitous. *(This is what panschema currently runs, in modified form.)*
- **Kamada-Kawai (1989)** — energy minimization on a global stress function: `E = Σᵢⱼ kᵢⱼ(|xᵢ - xⱼ| - lᵢⱼ)²`, where `lᵢⱼ` is the graph-theoretic shortest-path distance and `kᵢⱼ = 1/lᵢⱼ²`. Iterates by moving one node at a time to its local optimum. Slow (`O(N³)` to compute all-pairs shortest paths up front) but typically produces the prettiest medium-sized layouts. Sweet spot: ≤500 nodes.
- **GEM — Graph Embedder (Frick et al. 1995)** — per-node "temperature" that adapts based on whether the node is making "useful progress" (consistent direction) or "oscillating" (frustrated). Equivalent quality to FR at a fraction of the iterations.
- **ForceAtlas2 (Jacomy et al. 2014, used by Gephi)** — modern FR variant with three optional knobs that matter in practice: **linear attraction** (`F_attr = d` instead of `d²`, hubs settle in the middle naturally), **linlog mode** (`F_attr = log(d+1)`, emphasizes community structure), **node-size repulsion** (repulsion scaled by node degree, hubs push harder so they end up further apart). Strong out-of-the-box defaults; widely cited as the modern force-directed baseline.
- **OpenOrd (Martin et al. 2011)** — staged simulated-annealing variant for very large graphs (10⁴–10⁶ nodes). Cuts long edges in early phases, refines in later phases. Not relevant unless we get to schemas with thousands of classes.

All of these are `O(N²)` per iteration in their naive form, because repulsion is computed between every pair. The standard acceleration is the **Barnes-Hut quadtree** (`O(N log N)`), which approximates clusters of distant nodes by their center-of-mass. Mandatory above ~500 nodes.

#### Energy-minimization (non-force)

- **Stress majorization (Gansner, Koren, North 2005)** — same stress function as Kamada-Kawai, but solved by majorization (a convex optimization technique) rather than node-by-node gradient descent. Converges in `O(N²)` per iteration for ~30 iterations total — much faster than KK and with provable global convergence properties. The algorithm behind graphviz's `neato -Kstress`. The current best-in-class for "give me a high-quality static layout" on ≤2000 nodes.
- **PivotMDS / MDS (Brandes & Pich 2007)** — classical multidimensional scaling on the all-pairs graph-distance matrix, accelerated by projecting onto a small set of "pivot" nodes. Very fast (`O(N·k)` for k pivots ~ 50). Quality is "decent overview" — preserves the global shape better than force-directed but doesn't separate dense clusters as cleanly.

#### Constraint-based

- **Cola (Dwyer, Marriott et al. 2008+)** — extends stress majorization with hard and soft constraints. Examples: "nodes A, B, C must be on the same horizontal line," "node X must be inside region Y," "this subgraph must be enclosed in a rectangle." The right tool when the layout has to satisfy domain-specific structure (e.g. "all CCO entities aligned vertically"). Implemented in `cola.js` (web) and `libcola` (native). Heavier API surface than force-directed.

#### Hierarchical (DAG-specific)

- **Sugiyama framework (Sugiyama, Tagawa, Toda 1981)** — the canonical algorithm for directed acyclic graphs. Four phases: (1) **break cycles** by reversing the fewest edges, (2) **assign layers** by longest-path or network-simplex, (3) **reduce crossings** between adjacent layers (NP-hard; solved heuristically by barycenter or median methods), (4) **assign coordinates** to minimize edge bends. This is the algorithm behind graphviz's `dot`. *For class hierarchies (`is_a`/`subClassOf` chains), this is the gold standard.*

#### Circular / radial

- **Plain circular** — all nodes uniformly on a circle, edges drawn as straight chords inside. Works well for ≤30 nodes where the cyclic ordering doesn't matter. Variants order nodes to minimize edge crossings (Baur & Brandes 2005) but at ~30 nodes the simple variant is usually fine.
- **Radial / "ring tree" (Wills 1999, Stasko 2000)** — for trees: root at center, level-1 nodes on a ring at radius `r₁`, level-2 on a ring at `r₂ > r₁`, etc. Each subtree gets an angular slice proportional to its size. Compact, good for wide-shallow trees.
- **Concentric** — nodes grouped into rings by some attribute (degree, depth, type). Used for "core-periphery" visualization.

#### Spectral

- **Spectral layout (Hall 1970)** — assign coordinates from the eigenvectors of the graph Laplacian. Specifically, the 2nd and 3rd smallest eigenvectors give the `x` and `y` coordinates that minimize total squared edge length. Fast (`O(N · iter)` via Lanczos), and the layout reveals the graph's natural "modes" — communities, bottlenecks, etc. Less visually appealing than force-directed for small graphs but very informative for medium ones. Spectral is what underlies many modern non-linear embeddings (Laplacian Eigenmaps, t-SNE-of-graphs).

#### Algorithm comparison

| Algorithm | Time complexity | Best for | Weakness |
|---|---|---|---|
| Fruchterman-Reingold | O(N²) per iter, ~100 iters | General-purpose default, small-medium graphs | Local minima with crossings; slow convergence |
| Kamada-Kawai | O(N³) preprocess + O(N²) per iter | ≤500 nodes, when quality matters | Slow; one-node-at-a-time descent |
| ForceAtlas2 | O(N²) or O(N log N) with Barnes-Hut | Community-structured graphs, medium-large | Tuning the three modes is a learning curve |
| Stress majorization | O(N²) per iter, ~30 iters | High-quality static layouts ≤2000 nodes | Doesn't react to interactive dragging as smoothly as force-directed |
| PivotMDS | O(N·k) | Quick overview of large graphs | Doesn't separate dense clusters well |
| Sugiyama (dot) | O(V·E) | DAGs, class hierarchies | Only works for directed structure; cycles must be broken |
| Cola | O(N²) per iter | Layouts with domain constraints | Heavier API, complexity |
| Spectral | O(N·iter) | Community discovery, fast overview | Less polished visual; eigenvector sign ambiguity |
| Plain circular | O(N) | Cycles, small graphs ≤30 | Doesn't scale; crossings explode |
| Radial tree | O(N) | Wide-shallow trees | Trees only |

### 3D layout

3D extensions are mostly mechanical: add a `z` coordinate to forces / energy functions, and the algorithm carries over. Specific 3D-only approaches:

- **3D Fruchterman-Reingold / Kamada-Kawai / stress majorization** — straight extensions. Currently the entire active 3D layout literature for general graphs. *(panschema's WebGPU path uses 3D FR.)*
- **Hyperbolic space embedding** — map the graph to hyperbolic 2D / 3D space (where "circumference grows exponentially with radius"), then project back to Euclidean for display. The Poincaré disk model gives "fisheye" focus + context views — useful for very large hierarchies (Lamping & Rao 1996, "H3" by Munzner 1997 for 3D). Niche but well-known.
- **Spherical layout** — project onto the surface of a sphere. Edges drawn as great-circle arcs. Aesthetic for small fully-connected graphs; doesn't scale.
- **3D Sugiyama** — generalizes Sugiyama by adding depth as a free axis (so layers can be ribbons in 3D rather than rows in 2D). Limited adoption; the visual win over good 2D Sugiyama is unclear.

### Library landscape

| Library | Language | Algorithms | Notes |
|---|---|---|---|
| **graphviz** (`dot`, `neato`, `fdp`, `sfdp`, `twopi`, `circo`) | C | Sugiyama, stress, Frick, Barnes-Hut force, radial, circular | The canonical reference implementation. `dot` for DAGs, `neato`/`sfdp` for force, `twopi` for radial, `circo` for circular. |
| **d3-force** | JS | Force-directed with composable forces (link, manyBody, x, y, center, collide). Barnes-Hut. | The web standard. Quality is "good not great"; composable forces let you bias the layout for fill-the-viewport etc. |
| **3d-force-graph** | JS | 3D extension of d3-force using Three.js | The 3D web standard. |
| **cola.js / libcola** | JS / C++ | Stress majorization + constraints | Best free constraint-based layout. |
| **Gephi** | Java | ForceAtlas2, OpenOrd, Yifan Hu | Desktop-app world; ForceAtlas2 is their flagship. |
| **igraph** | C with Python/R/JS bindings | FR, KK, Sugiyama, circular, spectral, MDS, large-graph | Most algorithms in one place. |
| **OGDF** | C++ | Comprehensive (stress, planarization, Sugiyama, orthogonal, …) | Academic; reference quality. Heavy. |
| **netwulf**, **ipycytoscape**, **plotly** etc. | JS frontends | Mostly delegate to d3-force or cytoscape.js | Not new algorithm work. |

### Synthesis — what panschema should ship

Given the user's likely workflows (schema authors browsing their own ontologies; readers exploring an unfamiliar schema), the algorithm set with the highest information-per-implementation-effort is:

1. **Force-directed** (improved on what we have today) — the no-op-knowledge default. *Existing.*
2. **Hierarchical / Sugiyama** — for `is_a` / `subClassOf` chains. Highest user value for class-heavy schemas. *New.*
3. **Circular** — for cycles, small enums, "show everything equally." *New, trivial to implement.*
4. **Radial tree** — for wide-shallow taxonomies. *New, moderate complexity.*
5. **Stress majorization** — quality option for medium graphs. *New, moderate complexity.*

ForceAtlas2 / Cola / Spectral / PivotMDS are deferred — diminishing returns for the panschema use case.

For 3D: only **force-directed** initially (the existing 3D path). Hierarchical-3D and stress-3D can come later if the user-research signal is there; we don't have evidence yet that schema readers benefit from 3D layouts at all.

---

## Crate selection (decided)

Two maintained, MIT-licensed Rust crates carry most of this feature's algorithm load. A pre-implementation research pass surveyed the Rust graph-layout ecosystem (see commit history for the survey); the conclusions baked in here:

- **`egraph-rs`** ([github.com/likr/egraph-rs](https://github.com/likr/egraph-rs)) — workspace with `egraph-wasm` plus per-algorithm sub-crates (`petgraph-layout-sgd`, `-kamada-kawai`, `-mds`, `-stress-majorization`, `-overwrap-removal`, `-separation-constraints`, `-kernel-sgd`). Active in late 2025, real wasm-bindgen target, deps limited to `ndarray` + `petgraph` + `rand` (no rayon, no threads → wasm-safe). The only single source for Kamada-Kawai + Stress Majorization + SGD + MDS + overlap removal + constraint-based layout. Some sub-crates aren't published to crates.io and will need vendoring or git-deps.
- **`rust-sugiyama`** ([github.com/paddison/rust-sugiyama](https://github.com/paddison/rust-sugiyama) v0.4.0) — Sugiyama for layered/hierarchical layouts with real Barth-Mutzel-Jünger crossing minimization. petgraph-native (`StableDiGraph` input). Active 2025-09 release. No wasm CI on the repo; needs a single `cargo check --target wasm32-unknown-unknown` to confirm before depending on it.

Crates **rejected** during the research pass:
- `forceatlas2` — AGPL-3.0 (viral copyleft, blocker for an MIT-licensed CLI).
- `dagre-rs` — PolyForm Noncommercial (non-FOSS).
- `fdg-sim` — abandoned 2022-12, superseded by `fdg`.
- `fdg` 1.0 — clean and petgraph-native but only implements FR (Kamada-Kawai / ForceAtlas2 are README-only "planned"). Since `egraph-rs` already gives us KK + stress + SGD, `fdg` adds no algorithmic coverage we don't already have.
- `layout-rs` — emits SVG only, no public positions API.

Existing in-tree CPU force simulation (slice 7 work in [Feature 02](02-core-ontology-documentation.md#slice-7-improve-force-directed-default-so-the-graph-fills-its-viewport)) **stays** as the Force-Directed algorithm. It's already tuned for legible labels at all three viewport scales and the `forceX` / `forceY` aspect-bias is a property no off-the-shelf crate offers out of the box. Replacing it would discard work that's already shipped.

---

## Vertical Slices

### Slice 1: Picker chrome + `LayoutAlgorithm` enum (no algorithm changes)

**Status:** Completed (commit fa9d538)

**User Value:** The graph viz UI has a layout-picker control next to the existing 2D / 3D toggle. The picker is visible and functional but only has one option, "Force-directed," wired to the existing in-tree CPU simulation. Subsequent slices add options without re-designing the UI.

**Acceptance Criteria:**
- [ ] `LayoutAlgorithm` enum in `panschema-viz` with `ForceDirected` (the only variant that resolves to a real implementation at this slice) plus placeholder variants for the slices below (`Hierarchical`, `Stress`, `KamadaKawai`, `Sgd`, `Circular`, `RadialTree`). Constructing a `Visualization` with an un-implemented variant returns a clear error rather than panicking.
- [ ] Wasm `Visualization::new` takes a `layout: &str` parameter (or `u32` enum-tag, whichever round-trips cleanest through `wasm_bindgen`). Defaults to `"force-directed"` for backward compatibility with current callers.
- [ ] JS `readGraphLayout()` mirrors `readGraphAspect()` — reads a `--graph-layout` CSS custom property or `data-layout` attribute on `.graph-container`, defaults to `"force-directed"`.
- [ ] `panschema.toml` accepts `html_default_layout = "force-directed" | "hierarchical" | "stress" | "kamada-kawai" | "sgd" | "circular" | "radial-tree"` under each `[generate.<name>]` block. Validation rejects unknown values with an actionable error at manifest parse time.
- [ ] UI: layout picker rendered as a segmented control or dropdown in the graph chrome (placement TBD; the 2D/3D toggle is the model). For slice 1 the dropdown shows all algorithm names but only "Force-directed" is selectable — others are disabled with a "not yet implemented" tooltip.
- [ ] Picker choice persists to `localStorage` under a known key, alongside the existing label-prefs entry.
- [ ] Unit tests: enum round-trips through string parsing, manifest field parses + rejects bad values, picker localStorage round-trips.

**Notes:**
- This slice is plumbing — no new force-directed behavior. Multi-scale screenshots before/after should be byte-identical for the `force-directed` selection.
- The picker UI placement isn't bikeshedded here; the implementer picks whatever fits the existing graph-controls strip. The acceptance criterion is functional, not aesthetic.

---

### Slice 2: `egraph-rs` integration (dependency adoption + wasm smoke test)

**Status:** Completed

**User Value:** No user-visible change. This slice de-risks adopting `egraph-rs` for later slices by wiring it into `panschema-viz`, confirming wasm32 compilation, and proving the round-trip `panschema-viz GraphData → petgraph::Graph → egraph-rs algorithm → positions back into Visualization` works end-to-end on a single representative algorithm (Kamada-Kawai is the proposed pilot — it's the simplest non-force algorithm).

**Acceptance Criteria:**
- [x] `panschema-viz/Cargo.toml` adds dependencies on `petgraph-layout-kamada-kawai` and its required workspace siblings (`petgraph-drawing`, transitively `petgraph-algorithm-shortest-path`). Pulled via `git = "..."` pinned to `8e986826534774fe7beb9546154407927260e446`. *Scope note: only the KK sub-crate is added in this slice. `petgraph-layout-stress-majorization` and `petgraph-layout-sgd` land with slices 4 and 5 so each slice owns its dep churn; the wasm-build gate here proves the workspace integrates regardless.*
- [x] `cargo check --target wasm32-unknown-unknown -p panschema-viz` passes cleanly. CI's lint job gained a `cargo check --target wasm32-unknown-unknown -p panschema-viz` step as a cheap canary for wasm-incompat regressions in future dep bumps.
- [x] Internal helper `panschema_viz::layout::to_petgraph(&GraphData) -> (Graph<String, (), Undirected>, BTreeMap<String, NodeIndex>)` converts the wire format to petgraph; native unit tests cover node/edge counts and the unknown-endpoint drop rule.
- [x] Pilot `panschema_viz::layout::kamada_kawai(&GraphData, aspect_w, aspect_h) -> Vec<(f32, f32)>` runs `petgraph-layout-kamada-kawai` end-to-end and returns post-settle positions with an aspect-bias post-process. Native unit tests cover: `make_ring(15)` produces a non-degenerate bbox, `make_lopsided(20, 8)` (8 isolated singletons) doesn't panic and emits finite coordinates per node, the empty-graph case returns an empty Vec, and the aspect-bias scaling matches the analytic √(w/h) ratio.
- [x] The pilot is NOT yet wired into the picker — that lands in slice 3.

**Notes:**
- The pilot algorithm choice is debatable; Stress Majorization is also a reasonable first pick (it's `egraph-rs`'s most-cited algorithm). The criterion is "one algorithm working end-to-end through the new dep," not "the right algorithm shipped first."
- If `egraph-rs` sub-crates turn out to drag in something wasm-hostile that the research pass missed (`std::thread`, blocking I/O, etc.), this is where it surfaces. The slice has a clear failure mode: smoke test fails → either patch the sub-crate, vendor a stripped copy, or abandon `egraph-rs` and re-scope to `fdg`.
- The aspect-bias work from feature 02 slice 7 is per-tick force application that doesn't map onto `egraph-rs`'s deterministic optimizers. For `egraph-rs`-backed algorithms, the aspect bias becomes a post-process: scale `x` by `√(w/h)` and `y` by `√(h/w)` so the bbox aspect approximates `w:h` while preserving area. This compromise is acceptable for non-default algorithms.

---

### Slice 3: Kamada-Kawai algorithm (via `egraph-rs`)

**Status:** Completed

**User Value:** Selecting "Kamada-Kawai" runs the classical KK energy-minimization layout, which often produces visibly nicer node spacing for medium graphs (≤500 nodes) than force-directed at the cost of higher init latency. Best fit when convergence quality matters more than interactivity.

**Acceptance Criteria:**
- [x] `LayoutAlgorithm::KamadaKawai` resolves to a real implementation; `Visualization::new` calls the slice-2 pilot helper when the layout argument is `"kamada-kawai"` and freezes the simulation at the computed positions (`CpuSimulation::freeze_at`) so per-tick physics doesn't re-shape the layout.
- [x] Aspect-bias post-process applied to KK output (carried over from slice 2 — `√(w/h)` x-scale, `√(h/w)` y-scale).
- [x] Picker UI exposes "Kamada-Kawai (slower init)" as a selectable option with a hover tooltip explaining the trade-off. JS `IMPLEMENTED_LAYOUTS` accepts the new identifier so the localStorage round-trip works.
- [ ] Multi-scale screenshot harness produces a `target/graph-2d-{phone,laptop,4k}.png` for `LayoutAlgorithm::KamadaKawai`. **Deferred:** the multi-scale screenshot baselines for force-directed are already committed; adding KK baselines is a follow-on once we have UI consensus that the captured layouts are the intended reference.
- [x] Native unit tests on `egraph-rs`-derived positions confirm: no NaN/Inf in any coordinate, bbox is non-degenerate (≥ 100 world units on both axes for any reasonable test graph), positions stay within `MAX_RADIUS`-equivalent bounds. See `kamada_kawai_scaled_bbox_is_non_degenerate_and_within_world_bounds` in `panschema-viz/src/layout.rs`.
- [x] The freeze survives user interaction. Selecting a node (click-without-drag) leaves all node positions unchanged. Dragging a single node moves only that node — the rest of the static layout is preserved, not rearranged into force-directed equilibrium. Pinned by `dragging_one_node_in_a_frozen_simulation_leaves_other_nodes_untouched` in `panschema-viz/src/simulation.rs`. (The drag handler used to reheat unconditionally, undoing `freeze_at`'s halt for every interaction; the fix tracks `is_static_layout` on `Visualization` and skips the reheat for static layouts.)

**Notes:**
- KK convergence cost is `O(N³)` for the all-pairs shortest-path preprocess plus `O(N²)` per iteration. For schemas ≥ 500 classes the init latency becomes uncomfortable — surface a "switch to Force-directed" hint in the UI when the graph exceeds that threshold.
- KK output is scaled to a target world bbox via `layout::scale_to_world(positions, layout::WORLD_TARGET_DIMENSION)` so it fills the existing simulation's coordinate space (anchored to `CpuSimulation`'s `MAX_RADIUS = 800` safety net). This decoupling keeps the slice-2 helper aspect-bias-only and lets the visualization integrate any future static layout the same way.

---

### Slice 4: Stress Majorization algorithm (via `egraph-rs`)

**Status:** Completed

**User Value:** Selecting "Stress Majorization" runs a higher-quality (slower) layout that minimizes the squared deviation between Euclidean and graph-theoretic distances. Produces cleaner cluster separation and more uniform edge lengths than force-directed. The algorithm of choice when the schema has natural clusters that force-directed mashes together.

**Acceptance Criteria:**
- [x] `LayoutAlgorithm::Stress` resolves to a call into `petgraph-layout-stress-majorization` via the slice-2 helper layer (`panschema-viz/src/layout.rs::stress_majorization`). Picker integration: `Visualization::new` matches `LayoutAlgorithm::Stress` alongside KK / Hierarchical, calls `freeze_at` on the resulting positions, and inherits the slice-3 `is_static_layout` guard so node selection doesn't undo the static layout.
- [x] Aspect-bias post-process applied as in slice 3 (`√(w/h)` x-scale, `√(h/w)` y-scale). Pinned by `stress_majorization_aspect_bias_scales_coordinates` mirroring the KK test.
- [x] Picker UI exposes "Stress majorization" as a selectable option. Label: "Stress majorization (cluster separation)" in 2D mode, "(not implemented)" in 3D mode (stress is 2D-only via egraph-rs's `DrawingEuclidean2d`). `IMPLEMENTED_LAYOUTS` and `LAYOUT_MODES` in `graph_viz.html` updated accordingly; e2e test asserts the option is selectable in 2D and disabled in 3D.
- [ ] Multi-scale screenshot harness produces baseline PNGs and the comparison vs force-directed is reviewed. **Deferred** for the same reason as slices 3 and 6 — waiting on UI consensus before committing baselines.
- [ ] For graphs ≥ 500 classes, stress majorization gracefully degrades to a single-pass MDS via `petgraph-layout-mds` (the `egraph-rs` `pivot_mds` variant — `O(N·k)` for `k` pivots) so the wasm init doesn't hang the page. **Deferred** — no real-world panschema consumer hits the 500-class threshold yet (scimantic-schema is ~80 classes). When the first large-schema consumer surfaces, add the MDS dep + threshold check as a follow-up slice.
- [x] Native unit tests on the helper output confirm no NaN/Inf on connected inputs, bbox is non-degenerate after `scale_to_world`, aspect-bias scales per axis as advertised, and empty input returns an empty Vec. Five tests at `panschema-viz/src/layout.rs::tests::stress_majorization_*` cover the ring, connected-input bbox, empty graph, aspect bias, and the disconnected-components clamp-to-finite behavior.

**Notes:**
- Stress majorization is the algorithm behind graphviz's `neato -Kstress`. The output quality is the literature reference point for "what a good static layout looks like" on schemas in the 50-2000 node range.
- **Disconnected-components limitation:** stress majorization computes all-pairs graph-theoretic distances; any infinite entry (between unreachable pairs) propagates to NaN across the entire optimization, not just the disconnected nodes. The wrapper clamps NaN to `0.0` at the output edge so the picker integration always sees finite coordinates, but disconnected components stack at the origin. For schemas with isolated subgraphs, the picker UX should recommend force-directed instead. A connected-component fallback (run stress on the largest component, place others defensively) is reserved as a follow-up if a real schema hits this case.

---

### Slice 5: SGD algorithm (via `egraph-rs`)

**Status:** Completed

**User Value:** Selecting "SGD" runs the modern stochastic-gradient variant of stress majorization. Often the best quality-per-unit-time of any algorithm in `egraph-rs`'s lineup; converges in `O(N · iters)` instead of `O(N² · iters)` and produces visibly comparable output. The recommended default for medium-to-large graphs once the picker exists.

**Acceptance Criteria:**
- [x] `LayoutAlgorithm::Sgd` resolves to `petgraph-layout-sgd::FullSgd` via the slice-2 helper layer. `FullSgd` runs the all-pairs-shortest-paths variant; that's the right pick for schema-sized graphs (≤ a few hundred nodes), where the `O(N²)` distance preprocess is cheap compared with the user-visible quality win from globally-accurate distances.
- [x] Aspect-bias post-process applied as in slices 3 and 4 (`√(w/h)` x-scale, `√(h/w)` y-scale).
- [x] Picker UI exposes "SGD (fast quality)" as a selectable option in 2D mode; 3D mode greys it out with the standard `(not implemented)` label.
- [ ] Multi-scale screenshot harness baselines committed; output compared against stress majorization (slice 4) for quality and against force-directed (the existing default) for speed. **Deferred** — same reason as slices 3, 4, 6 (waiting on UI consensus before locking in reference baselines).
- [x] SGD is the new default layout. The JS picker falls back to `'sgd'` when neither localStorage nor the manifest's `html_default_layout` field has a value, and `HtmlWriter::{new, with_options}` default to `"sgd"`. The 3D mode (where SGD isn't supported) falls back to `'force-directed'` as before.
- [x] Disconnected components: same per-component shelf-packing pattern as slice 4. Stress's all-pairs-distance limitation also applies to SGD's distance matrix, so the helper splits the input into connected components, runs SGD on each, and shelf-packs the results into a rectangle whose aspect approximates the configured aspect.
- [x] Determinism: the internal RNG (`StdRng::seed_from_u64(42)`) is seeded with a constant so two calls on the same input produce byte-identical positions. Pinned by `sgd_is_deterministic_under_fixed_seed`. This is load-bearing for `panschema publish`'s idempotent-republish guarantee.
- [x] Native unit tests: ring layout, empty graph, determinism, disconnected-components shelf-packing without overlap.

**Notes:**
- SGD's stochastic nature would normally mean runs aren't bit-identical, but the seed-from-a-constant pattern makes the output deterministic — chosen so panschema publish's "same input → same output" guarantee survives this layout. If a future feature wants non-deterministic SGD (e.g. user-driven re-seeding), expose the seed as a knob rather than dropping determinism.
- The `petgraph-layout-sgd` dep pulls in `rand = "0.8"` (matching egraph-rs's version). Rand 0.9 is already in the lockfile via other deps; the two versions coexist via separate crate versions, no API surface conflict.

---

### Slice 6: Hierarchical algorithm (via `rust-sugiyama`)

**Status:** Completed

**User Value:** Selecting "Hierarchical" lays out the schema as top-to-bottom layered Sugiyama, with each `is_a` / `subClassOf` chain becoming a vertical descent and crossings between layers explicitly minimized via Barth-Mutzel-Jünger. Class hierarchies snap into legible form; non-tree edges (range, domain, mixin) draw as overlays in a contrasting style.

**Acceptance Criteria:**
- [x] `panschema-viz/Cargo.toml` adds `rust-sugiyama` (`= "0.4"`); CI's `cargo check --target wasm32-unknown-unknown` step passes against the dep — no upstream patch or vendoring was needed.
- [x] `LayoutAlgorithm::Hierarchical` resolves to a call into `rust-sugiyama` via `layout::hierarchical` (`panschema-viz/src/layout.rs:318`): (a) extracts hierarchy edges from `GraphData` via `is_hierarchy_edge`, (b) builds the `(u32, u32)` edge list rust-sugiyama's `from_edges` accepts (skipping the intermediate `petgraph::StableDiGraph` shape — the spec sketched it but rust-sugiyama's API takes the pair list directly), (c) runs sugiyama per disjoint hierarchy component, (d) maps the returned coordinates back to all nodes with orphan nodes (no hierarchy edges) falling back to a grid region below the layered layout, (e) applies the aspect-bias post-process (`√(w/h)` / `√(h/w)` scale).
- [x] Cycles in the input DAG are broken via `rust-sugiyama`'s built-in `greedy_feedback_arc_set` step. **Deferred:** the per-edge "which edges got reversed" surfacing via `wasm_bindgen` console warnings is intentionally not implemented — comment at `layout.rs:316` flags this as a follow-on diagnostic. Pinned indirectly by `hierarchical_handles_cycle_in_hierarchy_edges` at `layout.rs:1085`.
- [x] Picker UI exposes "Hierarchical" with a "best for class hierarchies" annotation; selectable from the layout dropdown in the rendered HTML graph chrome. **Deferred:** the optional "disable picker when ≤30% of edges are `is_a` / `subclass_of`" UX polish — the AC marks it Optional, low-value vs. effort.
- [ ] Multi-scale screenshot harness baselines a hierarchical layout for each of the three viewport sizes. The 80-node 4K case should visibly look like a layered tree, not a force-directed blob. **Deferred** for the same reason as slice 3's screenshot AC — waiting on UI consensus that captured layouts are the intended reference before committing baselines.
- [x] Native unit tests against fixtures with: a 3-layer balanced tree, a tree with cross-cutting non-`is_a` edges, a graph with a cycle (verify break+warn), an empty `is_a` relation (fall back gracefully). Eight tests at `panschema-viz/src/layout.rs:790-1085+` cover the four required fixtures plus disjoint components, no-hierarchy fallback, non-hierarchy-edge filtering, orphan grid placement, and aspect bias.
- [x] The freeze survives user interaction (same requirement as slice 3): clicking or single-node-dragging leaves other nodes in their static-layout positions, not rearranged. Shares the same `is_static_layout` guard as slice 3 in `Visualization::start_drag_at`; same regression test pins the contract.

**Notes:**
- This is the slice that justifies the "Hierarchical" prerequisite — `egraph-rs` doesn't include Sugiyama, and Sugiyama is the canonical algorithm for layered DAGs. There's no equivalent in the rest of the picker, so this slice is highest user-value for schema authors of class-heavy ontologies (the scimantic / BFO / CCO use case).

---

### Slice 7: Circular layout (in-tree)

**Status:** Not Started

**User Value:** Selecting "Circular" lays out every node uniformly on a circle (or ellipse matching the configured aspect), with edges drawn as straight chords inside. Best for small graphs (≤30 nodes), cycles, and "show everything as equal peers."

**Acceptance Criteria:**
- [ ] In-tree `panschema_viz::layout::circular(&GraphData, aspect_w, aspect_h) -> Vec<(f32, f32)>`. No new dependency; uniform angular placement plus the configured aspect's `x`/`y` scale factors.
- [ ] Node ordering minimizes crossings via Baur-Brandes 2-opt for ≤30 nodes; above that, fall back to input order (the swap cost dominates the quality gain).
- [ ] `LayoutAlgorithm::Circular` exposed in the picker; greyed-out / annotated when `N > 50`.
- [ ] Native unit tests: positions form a valid ellipse, 2-opt swap reduces crossings on a known-bad ordering.

**Notes:**
- ~100-200 LoC; the cheapest implementable slice in the feature. Could be done before slice 6 if the team wants a quick visible win.

---

### Slice 8: Radial tree (in-tree)

**Status:** Not Started

**User Value:** Selecting "Radial" places the root of the class hierarchy at the center and arranges the tree as concentric rings, with each subtree's angular wedge proportional to its size. Best for wide-shallow taxonomies with a single dominant root.

**Acceptance Criteria:**
- [ ] In-tree `panschema_viz::layout::radial_tree(&GraphData, aspect_w, aspect_h) -> Vec<(f32, f32)>`. Root detection: most-incoming-`is_a`-edges class, configurable via the manifest's `html_radial_root = "ClassName"` override.
- [ ] Wedge allocation: each subtree gets `angular_width(subtree) = 2π · subtree_leaf_count / total_leaf_count`. Each layer at `radius_k = k · layer_pitch`, layer pitch tuned so the configured aspect ratio matches the outermost-occupied ring's bbox.
- [ ] `LayoutAlgorithm::RadialTree` exposed in the picker; specifically recommended for schemas with a single root + ≥10 leaves.
- [ ] Native unit tests: leaf positions land on the outermost ring; wedge widths sum to `2π`; configurable root override works.

---

## Out of scope

- **ForceAtlas2 / OpenOrd / Cola** — diminishing returns for typical schema sizes. The research pass also flagged `forceatlas2` as AGPL (license-blocked) and Cola has no FOSS Rust port (closest are `egraph-rs`'s overlap-removal + separation-constraints sub-crates, which slot in here under stress majorization rather than as their own algorithm).
- **3D variants of stress / KK / SGD / hierarchical.** `egraph-rs` does support n-D drawing spaces (including spherical and hyperbolic — interesting for very large schemas via fisheye context+focus), and adding a `LayoutAlgorithm::Stress3D` is mechanically straightforward, but no user has asked for it and 3D layout quality is dominated by the WebGPU camera anyway. Add when a user use-case surfaces.
- **3D edge-crossing minimization.** Confirmed by the research pass: no Rust crate optimizes this objective, and the layout literature targets edge-length uniformity / angular resolution / cluster cohesion instead. The 3D path's quality metric is the projected-2D crossing count of the canonical orbit position — which is what the user actually sees on initial render — but exposing that in the picker is future work.
- **GPU-accelerated stress / Sugiyama.** `vibe-graph-layout-gpu` is the only wasm-targeted GPU layout crate today; ~170 downloads, single maintainer. Too early to depend on. Revisit when it matures or when graph sizes outgrow CPU.
- **Edge bundling / hierarchical edge bundling.** Orthogonal feature that helps with edge clutter regardless of node layout. `egraph-rs` ships FDEB; that's a future slice on top of this feature, not part of the picker itself.
- **Animated transitions between layouts.** Nice-to-have; defer until the picker has at least 3 working options to switch between.

---

### Slice 9: Auto-default to Hierarchical layout for `is_a`-heavy schemas

**Status:** ✅ Complete

**Priority:** Should Have

**User Value:** SGD is the right default for general schema topologies, but for schemas dominated by an `is_a` tree (taxonomies, BFO/CCO-grounded ontologies, scimantic-schema's claim spine) the Hierarchical layout shows the inheritance structure much more legibly. Authors of `is_a`-heavy schemas currently have to know to click the picker and select Hierarchical; first impressions go to SGD's force-like splatter. After this slice, panschema computes the schema's `is_a` density at render time and picks the appropriate default — Hierarchical when the tree dominates, SGD otherwise.

**Acceptance Criteria:**
- [x] `panschema-viz::layout::recommend_default_layout` computes an inheritance-density score — the fraction of edges whose `edge_type` is `SubclassOf` or `Mixin`. At `≥ 0.5` it returns `Hierarchical`; below, `SGD`. The threshold is a constant with a comment naming the tuning corpus (scimantic spine ≈0.7 vs. the mixed reference fixture ≈0.3). Exposed to JS via the `recommend_default_layout(json)` wasm export.
- [x] The not-pinned default is the `auto` sentinel (`html_writer` default; `panschema generate` emits it when `--input` has no manifest layout). With no `localStorage` choice and no pinned layout, the JS picker resolves `auto` via the wasm recommendation and selects the result.
- [x] An explicit `html_default_layout` in the manifest overrides the auto-detection (the picker's `data-default-layout` wins over `auto`).
- [x] A persisted `localStorage` choice overrides the auto-detection (checked first in `readGraphLayout`).
- [x] Unit tests: 8 `is_a` + 2 range → Hierarchical; 2 `is_a` + 8 range (and an edgeless graph) → SGD. E2E both directions: the mixed reference fixture auto-detects to SGD, and an `is_a`-heavy `taxonomy.ttl` fixture auto-detects to Hierarchical (proving the recommendation runs end-to-end, not a silent SGD fallback).
- [x] Manual test (scimantic dogfood): the `is_a`-heavy spine defaults to Hierarchical; the reference fixture defaults to SGD.

**Notes:**
- Source: friction `[2026-06-06] schema graph does not depict the is_a hierarchy legibly` (severity: annoyance). The note's "Routes to" suggests "consider defaulting [Hierarchical] for `is_a`-heavy schemas" — that's exactly this slice.
- The threshold is a heuristic, not a contract. If it consistently picks wrong for some schema shape, the manifest's `html_default_layout` override is the escape hatch.

---

### Slice 10: Compact multi-component packing

**Status:** ✅ Complete

**Priority:** Should Have

**User Value:** A schema with several disconnected components (e.g. scimantic's `Act` / `State` / `Claim` `is_a` trees plus isolated classes) rendered as a wide horizontal smear that wasted the vertical space and left each cluster too small to read. The SGD and Stress layouts shelf-pack components into an aspect-shaped rectangle *and then* applied a `√(w/h)` post-stretch — double-applying the aspect bias and over-stretching multi-component graphs horizontally. After this slice the clusters sit compactly and use the vertical space.

**Acceptance Criteria:**
- [x] In `sgd` and `stress_majorization`, the post-layout `√(w/h)` aspect stretch is applied only for a **single** component; with multiple components the shelf-packer has already biased the arrangement toward the aspect, so the redundant stretch is skipped.
- [x] Single-component graphs are unchanged (the stretch is their only aspect bias).
- [x] Existing per-component packing tests (disjoint bounding boxes) still pass.

**Notes:**
- Source: scimantic-schema dogfood — the three `is_a` trees smeared horizontally with empty vertical space, and clusters were too small to inspect (compounded the zoom/glyph legibility issues fixed in feature 04 slice 20).

---

### Slice 11: Variable edge length so force-directed clusters (LinLog / ForceAtlas2)

**Status:** 📋 Planned

**Priority:** Could Have

**User Value:** On a multi-cluster schema (scimantic's ~47 nodes: `Act` + its acts, the `Claim` spine, the `Method` tree, `State` + its standings), the Force-directed and Kamada-Kawai options spread nodes near-uniformly with heavy edge crossings and no visible clustering — every edge behaves as if it wants the same ideal length, so a dense group can't contract and separate groups can't push apart. Structure that should read at a glance doesn't. SGD (the current default) already reads better here, so this slice targets the FD/KK options specifically: let edge lengths vary so densely-connected groups tighten and sparse ones repel.

**Acceptance Criteria:**
- [ ] The Force-directed layout uses a **LinLog / ForceAtlas2-style** energy (logarithmic attraction + strong gravity), or weights the ideal link distance by degree/community, so dense groups contract and clusters separate — replacing the uniform Fruchterman-Reingold spring's single ideal length.
- [ ] A layout-quality check on a multi-cluster fixture: intra-cluster edges end up shorter than inter-cluster ones (or the edge-crossing count is bounded below the uniform-spring baseline).
- [ ] Determinism preserved (seeded RNG) so `panschema publish`'s idempotent-republish guarantee holds, matching the SGD contract.
- [ ] Kamada-Kawai is left as shortest-path spacing (inherently not cluster-tightening); the picker/docs steer users wanting a clustered look to the FD/LinLog mode rather than tuning KK.

**Notes:**
- Source: friction `[2026-06-14] force-directed & Kamada-Kawai don't cluster` (severity: annoyance). Prior art: Gephi ForceAtlas2 (LinLog mode + strong gravity), OpenOrd, d3-force with variable `linkDistance` + charge.
- The force simulation lives in `panschema-viz` (`simulation.rs` / `sim_common.rs`); this is an energy-model change to that sim, surfaced through the existing "Force-directed" picker option — no new picker entry.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: Picker chrome + enum | Must Have | Feature 02 slice 7 (✓) | ✅ Complete |
| Slice 2: `egraph-rs` integration + wasm smoke test | Must Have | Slice 1 | ✅ Complete |
| Slice 3: Kamada-Kawai | Should Have | Slice 2 | ✅ Complete |
| Slice 4: Stress Majorization | Should Have | Slice 2 | ✅ Complete |
| Slice 5: SGD | Should Have | Slice 2 | ✅ Complete |
| Slice 6: Hierarchical (Sugiyama) | Should Have | Slice 1 | ✅ Complete |
| Slice 7: Circular | Could Have | Slice 1 | Not Started |
| Slice 8: Radial tree | Could Have | Slice 1 | Not Started |
| Slice 9: Auto-default to Hierarchical for `is_a`-heavy schemas | Should Have | Slice 6 | ✅ Complete |
| Slice 10: Compact multi-component packing | Should Have | Slices 4, 5 | ✅ Complete |
| Slice 11: Variable edge length so FD clusters (LinLog / ForceAtlas2) | Could Have | Slice 2 | 📋 Planned |

**Prerequisite (✓ cleared):** Feature 02 [slice 7](02-core-ontology-documentation.md#slice-7-improve-force-directed-default-so-the-graph-fills-its-viewport) — the force-directed default fills the viewport with legible labels at all 3 scales. The picker can now expose the existing force-directed implementation as the "Force-directed" option without that option spreading a bad reputation across the others.

**Slice ordering:** Slice 1 is plumbing — no algorithmic change. Slice 2 is dep adoption + wasm smoke — also no UX change. Slices 3-5 can ship in any order (each is "wire one `egraph-rs` algorithm into the picker"). Slice 6 (Sugiyama) is the highest user value for class-heavy ontologies and should ship before slices 7-8 if effort tradeoffs need to be made.
