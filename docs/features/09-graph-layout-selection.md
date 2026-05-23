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

This feature spec captures the algorithm survey, the subset we'll implement, and the UX shape. It is **deferred until after Feature 02's force-directed default is solid** — a picker is only useful if every option in it is trustworthy.

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

## Vertical Slices

### Slice 1: Layout selection infrastructure + Force-directed (default)

**Status:** Not Started

**User Value:** The graph viz UI has a layout-picker control (chip / segmented control / dropdown) next to the existing 2D/3D toggle. The picker is visible and functional but only has one option, "Force-directed," producing the same output as today. Selecting it is a no-op; the picker exists so subsequent slices can add options without re-designing the UI.

**Acceptance Criteria:**
- [ ] `LayoutAlgorithm` enum in `panschema-viz`: `ForceDirected` (default), with placeholder variants for the layouts in later slices (`Hierarchical`, `Circular`, `RadialTree`, `Stress`). Non-default variants return an `unimplemented` error for now.
- [ ] `Visualization::new` and `create_visualization_3d` accept a layout argument (string or enum int). Currently only "force-directed" is accepted.
- [ ] JS `readGraphLayout()` mirrors `readGraphAspect()` — reads a `data-layout` attribute on `.graph-container` or a `--graph-layout` custom property, defaulting to "force-directed."
- [ ] `panschema.toml` accepts `html_default_layout = "force-directed"` (and "hierarchical" etc. once those exist) under `[generate.<name>]`.
- [ ] UI: layout picker rendered as a segmented control in the bottom-right corner of the graph viz (or wherever fits the existing chrome). For slice 1 it shows only one option.
- [ ] Persistence: chosen layout saved to localStorage like the label prefs.

---

### Slice 2: Hierarchical / Sugiyama (2D only)

**Status:** Not Started

**User Value:** Selecting "Hierarchical" lays out the schema as a top-to-bottom tree of `is_a` / `subClassOf` relationships, with parallel siblings on the same horizontal layer. Class hierarchies snap into legible form; non-tree edges (range, domain, mixin) are drawn as overlays.

**Acceptance Criteria:**
- [ ] Sugiyama implementation: cycle-breaking (greedy edge reversal), layer assignment (longest-path), crossing reduction (barycenter heuristic, multiple sweeps), x-coordinate assignment (Brandes-Köpf or simpler median).
- [ ] Tested against a fixture with: a simple tree (3 layers), a tree with cross-cutting non-`is_a` edges, a graph with a cycle (must break + warn), and a graph with mixed-direction edges.
- [ ] Output respects the configured aspect ratio (Sugiyama produces a layered grid; the layer pitch and intra-layer node spacing are tuned to match the target aspect).
- [ ] UI shows "Hierarchical" as a second picker option when the schema is amenable (heuristic: ≥30% of edges are `is_a` / `subclass_of`); shows it but greyed out / annotated otherwise.

**Notes:**
- Library prior art: there are Rust crates (`layout-rs`, `grid-graph`) but quality is uneven. We'll likely implement Sugiyama in-tree.
- Sugiyama is the most code in this feature. Consider sub-slicing: 2.1 = cycle-break + layer assignment + naive coords; 2.2 = barycenter crossing reduction; 2.3 = polished x-coords.

---

### Slice 3: Circular layout

**Status:** Not Started

**User Value:** Selecting "Circular" lays out every node uniformly on a circle, with edges drawn as straight chords inside. Best for small graphs (≤30 nodes), cycles, and "show everything as equal peers."

**Acceptance Criteria:**
- [ ] Nodes placed uniformly on a circle of radius matching the configured aspect (an ellipse for `aspect_w ≠ aspect_h`).
- [ ] Order around the circle minimizes edge crossings — start with the input order, then apply Baur-Brandes 2-opt swaps for ≤30 nodes (above 30, swaps become expensive and the layout isn't ideal anyway).
- [ ] UI shows "Circular" as a third option; greyed-out / annotated when N > 50.

**Notes:**
- Cheap to implement (~100 LoC). High visual quality for the right input.

---

### Slice 4: Radial tree

**Status:** Not Started

**User Value:** Selecting "Radial" places the root of the class hierarchy at the center and lays out the tree as concentric rings, with each subtree allocated an angular slice proportional to its size. Best for wide-shallow taxonomies and when the user wants to see one "anchor" node clearly.

**Acceptance Criteria:**
- [ ] Root detection: most-incoming-`is_a`-edges class, with a configurable override via manifest.
- [ ] Wedge allocation: each subtree gets `angular_width(subtree) = 2π · subtree_leaf_count / total_leaf_count`.
- [ ] Each layer at `radius_k = k · layer_pitch`, with layer pitch tuned so the configured aspect ratio matches the bbox of the outermost-occupied ring.
- [ ] UI shows "Radial" as a fourth option; specifically recommended for schemas with a single root + ≥10 leaves.

---

### Slice 5: Stress majorization

**Status:** Not Started

**User Value:** Selecting "Stress" runs a higher-quality (slower) layout algorithm that minimizes the squared deviation between Euclidean and graph-theoretic distances. Produces cleaner cluster separation and more uniform edge lengths than force-directed at the cost of ~2× the init time.

**Acceptance Criteria:**
- [ ] All-pairs shortest path computed via BFS (`O(V · (V+E))`).
- [ ] Stress function `Σᵢⱼ wᵢⱼ (|xᵢ - xⱼ| - dᵢⱼ)²` with `wᵢⱼ = 1/dᵢⱼ²` and `dᵢⱼ` the graph-distance.
- [ ] Majorization iteration: ~30 sweeps of weighted Jacobi.
- [ ] Tested against fixtures showing measurably better edge-length-uniformity than force-directed on the same graph.
- [ ] UI shows "Stress" with a "slower" annotation.

**Notes:**
- Reasonable Rust references exist (`stress_majorization` is a small dependency, ~500 LoC; or implement in-tree).
- For schemas ≥ 500 classes the all-pairs-BFS becomes expensive; warn / fallback to PivotMDS preprocessing.

---

## Out of scope

- **ForceAtlas2 / OpenOrd / Cola** — diminishing returns for typical schema sizes. Revisit if we get a schema where the chosen subset doesn't work.
- **3D variants of hierarchical / stress** — defer until we have user-research evidence that 3D non-force layouts help.
- **Edge bundling / hierarchical edge bundling** — orthogonal feature that helps with edge clutter regardless of node layout. Worth its own future feature.
- **Animated transitions between layouts** — nice-to-have; defer until the picker has at least 3 options to switch between.
- **GPU-accelerated stress / Sugiyama** — defer; CPU is fine for the schema sizes we target.
- **Per-edge type filtering tied to layout** ("show only `is_a` edges in Hierarchical mode"). Probably the right call but it's a UI feature, not a layout feature.

---

## Slice Priority and Dependencies

| Slice | Priority | Depends On | Status |
|-------|----------|------------|--------|
| Slice 1: Picker infrastructure + force-directed | Must Have | Feature 02 force-directed solid | Not Started |
| Slice 2: Hierarchical | Should Have | Slice 1 | Not Started |
| Slice 3: Circular | Should Have | Slice 1 | Not Started |
| Slice 4: Radial tree | Could Have | Slice 1 | Not Started |
| Slice 5: Stress majorization | Could Have | Slice 1 | Not Started |

**Prerequisite:** Feature 02 force-directed default must produce visually-trustworthy layouts before the picker is exposed to users. A picker over algorithms-that-look-bad will just spread the bad reputation across all of them.
