# Third-party code vendored into this repository

Code copied into this tree rather than depended on. Dependencies resolved
from crates.io are not listed here — `cargo deny` and `cargo vet` cover
those, and neither tool sees vendored source, which is why this file exists.

## egraph-rs — graph-layout numerics

- **Project:** https://github.com/likr/egraph-rs
- **Taken from:** https://github.com/padamson/egraph-rs at
  `9b38f5d17992b20c1ea0466c1e8805f014ad3b88` — a fork carrying petgraph-0.8
  and rand-0.10 ports. That revision is a fork merge commit and is not
  reachable from any upstream branch, which is why the fork is named here.
  The fork's deltas touch none of the vendored files — `git diff
  8e98682 9b38f5d` over them is empty — so re-sync against upstream
  `8e986826534774fe7beb9546154407927260e446`, not the fork, and carry
  forward the deliberate changes listed below.
- **License:** MIT, Copyright (c) 2018 Yosuke Onoue (full text below)
- **Vendored at:** `panschema-viz/src/layout/algo/`

### Why it is vendored

egraph-rs publishes to PyPI, not crates.io, so its Rust crates were only ever
reachable as a git dependency. panschema additionally needed a patch upstream
had not taken, which meant carrying a fork and rebasing it against upstream
churn. What the graph actually uses is a small, finished slice: three layout
algorithms over a 2D Euclidean drawing, sharing one all-pairs Dijkstra. Owning
that slice costs less than owning the fork, and it leaves no git-sourced
crate in the dependency graph: `cargo vet` models registry dependencies only,
so a git pin was invisible to it, and `deny.toml` needed an `allow-git` entry
to permit one. Vendoring SGD also took `rand` and `getrandom` out of the wasm
bundle; see the SGD shuffle below.

### What was taken, and what was not

| Upstream path | Vendored as | Kept |
|---|---|---|
| `crates/drawing/src/drawing.rs` | `algo/drawing.rs` | the `Drawing` trait |
| `crates/drawing/src/drawing/drawing_euclidean_2d.rs` | `algo/drawing_euclidean_2d.rs` | `DrawingEuclidean2d` |
| `crates/drawing/src/metric.rs` | `algo/metric.rs` | `Delta`, `Metric` |
| `crates/drawing/src/metric/metric_euclidean_2d.rs` | `algo/metric_euclidean_2d.rs` | `DeltaEuclidean2d`, `MetricEuclidean2d` |
| `crates/drawing/src/lib.rs` | `algo/mod.rs` | the `DrawingIndex` / `DrawingValue` traits |
| `crates/algorithm/shortest-path/src/distance_matrix.rs` | `algo/distance_matrix.rs` | `DistanceMatrix`, `FullDistanceMatrix` |
| `crates/algorithm/shortest-path/src/dijkstra.rs` | `algo/dijkstra.rs` | `dijkstra_with_distance_matrix`, `all_sources_dijkstra` |
| `crates/layout/kamada-kawai/src/lib.rs` | `algo/kamada_kawai.rs` | all |
| `crates/layout/stress-majorization/src/lib.rs` | `algo/stress_majorization.rs` | all |
| `crates/layout/sgd/src/sgd.rs` | `algo/sgd.rs` | `Sgd`, shuffle changed (below) |
| `crates/layout/sgd/src/full_sgd.rs` | `algo/full_sgd.rs` | `FullSgd` |
| `crates/layout/sgd/src/scheduler.rs` | `algo/scheduler.rs` | the `Scheduler` trait |
| `crates/layout/sgd/src/scheduler/scheduler_exponential.rs` | `algo/scheduler_exponential.rs` | all |

Not vendored: the N-dimensional, spherical, hyperbolic and torus drawing
spaces and their metrics; `SubDistanceMatrix` and the single- and
multi-source Dijkstra wrappers built on it; the BFS, Warshall-Floyd and
weighted-edge-length shortest-path implementations; the sparse SGD variant;
and the constant, linear, quadratic and reciprocal SGD schedulers.

Within the items that were kept, the members nothing in panschema uses are
removed: `DistanceMatrix::get`, `set`, `row_indices` and `col_indices`, with the
`IndexIterator` the last two return, and `FullDistanceMatrix`'s private
`index`; `Drawing::is_empty`, `dimension`, `node_id` and `index`;
`DrawingEuclidean2d::set_x`, `set_y`, `centralize`, `clamp_region`,
`initial_placement_with_bfs_order` and `edge_segments`; the `MetricCartesian`
trait and its impl for `MetricEuclidean2d`; `Sgd::node_pairs`,
`update_distance` and `update_weight`; `FullSgd`'s `Default` impl; and the
`Clone` derive on `DrawingEuclidean2d`. With them gone the module no longer
needs `allow(dead_code)`, so the compiler's dead-code lint covers it again —
for functions, methods and types. It cannot see trait impls or derives, so
those were checked by hand; the two small value types keep their ordinary
derives (`Copy`, `Debug`, `Default`) whether or not anything uses them.

Apart from those removals, the code is unmodified except for the deliberate
changes below, which a re-sync should carry forward:

- **An attribution header** is prepended to every vendored file.
- **`algo/mod.rs` is this repository's own module root**, not a copy: only
  `DrawingIndex` and `DrawingValue` come from upstream's
  `crates/drawing/src/lib.rs`. The `Shuffle` trait beside them is panschema's.
  Don't diff the rest of it against upstream.
- **Import paths**, rewritten for the flattened module, and re-ordered by this
  repository's rustfmt.
- **Doc examples** in `kamada_kawai.rs` and `stress_majorization.rs` are marked
  `ignore`, prefixed with a paragraph saying why, and their imports repointed.
  The module is private, so rustdoc would compile each example as an external
  crate that cannot reach it.
- **SGD's shuffle is generic over `Shuffle` instead of `rand::Rng`.** Upstream's
  `Sgd::shuffle<R: Rng>` becomes `Sgd::shuffle<R: Shuffle>`, and its body calls
  the trait. `Shuffle` is defined in `algo/mod.rs`, so the vendored file still
  reaches nothing outside `algo/`. panschema supplies the `SplitMix64` in
  `panschema-viz/src/layout/rng.rs`, which is this repository's code, not
  vendored. The shuffle is still Fisher–Yates, but the generator's sequence
  differs, so an SGD layout's coordinates differ from upstream's for the same
  seed. It stays deterministic from run to run.
- **The node-id vectors are no longer stored.** `DrawingEuclidean2d` and
  `FullDistanceMatrix` each kept a `Vec` of node identifiers that, once the
  removals above, was read only for its length. `Drawing::len` now counts the
  coordinates and `DistanceMatrix::shape` reports the matrix's own dimensions,
  which is the same number.
- **One upstream test assertion is corrected**, in `stress_majorization.rs`.
  `test_stress_majorization_parameters` asserted the default epsilon
  `== 1e-4`, but the constructor builds it as `(1e-4).into()` through
  `From<f32>`, so under the test's `f64` it is `9.999999747378752e-5`. It
  fails the same way in upstream's own checkout at the vendored revision;
  it never ran here before because it lived inside a dependency. The layout
  itself is unaffected, since panschema instantiates with `f32`.

A re-sync against upstream therefore has to account for two kinds of
difference: the deliberate changes just listed, which carry forward, and the
removed members listed before them, which a diff will show as deletions and
which should stay deleted unless panschema starts using them.

### License

```
The MIT License (MIT)

Copyright (c) 2018 Yosuke Onoue

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
