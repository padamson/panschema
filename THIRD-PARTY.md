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
  The vendored files are byte-identical to upstream `8e986826534774fe7beb9546154407927260e446`
  apart from imports, so the fork's deltas never touched them; re-sync
  against upstream, not the fork.
- **License:** MIT, Copyright (c) 2018 Yosuke Onoue (full text below)
- **Vendored at:** `panschema-viz/src/layout/algo/`

### Why it is vendored

egraph-rs publishes to PyPI, not crates.io, so its Rust crates were only ever
reachable as a git dependency. panschema additionally needed a patch upstream
had not taken, which meant carrying a fork and rebasing it against upstream
churn. What the graph actually uses is a small, finished slice: three layout
algorithms over a 2D Euclidean drawing, sharing one all-pairs Dijkstra. Owning
that slice costs less than owning the fork. The aim is to leave no
git-sourced crate in the dependency graph at all: `cargo vet` models registry
dependencies only, so anything pinned by git is invisible to it, and
`deny.toml` needs an `allow-git` entry to permit it. Vendoring proceeds one
algorithm at a time, so until the last one lands some egraph-rs crates are
still pinned.

### What was taken, and what was not

| Upstream path | Vendored as | Kept |
|---|---|---|
| `crates/drawing/src/drawing.rs` | `algo/drawing.rs` | the `Drawing` trait |
| `crates/drawing/src/drawing/drawing_euclidean_2d.rs` | `algo/drawing_euclidean_2d.rs` | all |
| `crates/drawing/src/metric.rs` | `algo/metric.rs` | `Delta`, `Metric`, `MetricCartesian` |
| `crates/drawing/src/metric/metric_euclidean_2d.rs` | `algo/metric_euclidean_2d.rs` | all |
| `crates/drawing/src/lib.rs` | `algo/mod.rs` | the `DrawingIndex` / `DrawingValue` traits |
| `crates/algorithm/shortest-path/src/distance_matrix.rs` | `algo/distance_matrix.rs` | `DistanceMatrix`, `FullDistanceMatrix`, `IndexIterator` |
| `crates/algorithm/shortest-path/src/dijkstra.rs` | `algo/dijkstra.rs` | `dijkstra_with_distance_matrix`, `all_sources_dijkstra` |
| `crates/layout/kamada-kawai/src/lib.rs` | `algo/kamada_kawai.rs` | all |

Not vendored: the N-dimensional, spherical, hyperbolic and torus drawing
spaces and their metrics; `SubDistanceMatrix` and the single- and
multi-source Dijkstra wrappers built on it; the BFS, Warshall-Floyd and
weighted-edge-length shortest-path implementations.

Apart from import paths rewritten for the flattened module (and their
re-ordering by this repository's rustfmt) and the omissions above, the files
are unmodified copies. Keeping them diffable against the
revision they came from is deliberate: it is what makes a later re-sync or an
upstream bug-fix cheap to apply, and it is why the module carries an
`allow(dead_code)` rather than being trimmed to exactly what is called.

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
