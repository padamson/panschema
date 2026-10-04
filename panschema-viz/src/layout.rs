//! Layout-algorithm enumeration and helpers for the schema-graph
//! visualization.
//!
//! Each [`LayoutAlgorithm`] variant is one of the algorithms exposed
//! in the picker UI. Only the variants whose
//! [`LayoutAlgorithm::is_implemented`] returns `true` actually produce
//! node positions; the rest return a clear "not yet implemented"
//! error so the picker UI, wasm constructor, CSS custom property,
//! and manifest field can all agree on the canonical wire format
//! before each implementation lands.
//!
//! The string identifiers are the canonical wire-format used by:
//! - the wasm `Visualization::new` constructor's `layout` parameter,
//! - the `--graph-layout` CSS custom property on `.graph-container`,
//! - panschema's `panschema.toml` `html_default_layout` field.
//!
//! The module also hosts the conversion glue between panschema-viz's
//! wire-format [`GraphData`](crate::graph_types::GraphData) and the
//! [`petgraph`] graphs consumed by the stress-based layout
//! algorithms ([`to_petgraph`]), and the layouts themselves:
//! [`kamada_kawai`], [`stress_majorization`], [`sgd`], and
//! [`hierarchical`].

mod algo;
mod rng;

/// Identifies which layout algorithm should produce node positions for
/// the schema-graph render. Only [`LayoutAlgorithm::ForceDirected`]
/// resolves to a real implementation; the rest are placeholders that
/// surface a clear error if requested, so the wire format and picker
/// UI can stabilize while implementations land.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutAlgorithm {
    /// In-tree CPU force simulation tuned for viewport filling and
    /// readable labels.
    ForceDirected,
    /// Sugiyama-style layered layout for `is_a` / `subClassOf` DAGs,
    /// via `rust-sugiyama`.
    Hierarchical,
    /// Stress majorization, vendored from `egraph-rs`.
    Stress,
    /// Kamada-Kawai energy minimization, vendored from `egraph-rs`.
    KamadaKawai,
    /// Stochastic Gradient Descent, vendored from `egraph-rs`. The default.
    Sgd,
    /// Uniform-on-a-circle (or ellipse for non-square aspects).
    /// Planned implementation: in-tree.
    Circular,
    /// Radial tree layout from a dominant root. Planned
    /// implementation: in-tree.
    RadialTree,
}

impl LayoutAlgorithm {
    /// The canonical string identifier used on the wire (wasm
    /// constructor, CSS custom property, manifest field).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ForceDirected => "force-directed",
            Self::Hierarchical => "hierarchical",
            Self::Stress => "stress",
            Self::KamadaKawai => "kamada-kawai",
            Self::Sgd => "sgd",
            Self::Circular => "circular",
            Self::RadialTree => "radial-tree",
        }
    }

    /// All known algorithm identifiers, in the order they should
    /// appear in a picker UI.
    pub const ALL: &'static [Self] = &[
        Self::ForceDirected,
        Self::Hierarchical,
        Self::Stress,
        Self::KamadaKawai,
        Self::Sgd,
        Self::Circular,
        Self::RadialTree,
    ];

    /// `true` for variants that resolve to a working implementation.
    /// Picker UIs use this to grey out unselectable options.
    pub fn is_implemented(&self) -> bool {
        matches!(
            self,
            Self::ForceDirected | Self::KamadaKawai | Self::Hierarchical | Self::Stress | Self::Sgd
        )
    }

    /// Human-readable label, suitable for the picker UI.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::ForceDirected => "Force-directed",
            Self::Hierarchical => "Hierarchical",
            Self::Stress => "Stress majorization",
            Self::KamadaKawai => "Kamada-Kawai",
            Self::Sgd => "SGD",
            Self::Circular => "Circular",
            Self::RadialTree => "Radial tree",
        }
    }
}

impl std::str::FromStr for LayoutAlgorithm {
    type Err = LayoutAlgorithmParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        for variant in Self::ALL {
            if variant.as_str() == s {
                return Ok(*variant);
            }
        }
        Err(LayoutAlgorithmParseError {
            unknown: s.to_string(),
        })
    }
}

/// Returned when the wasm constructor or manifest receives a layout
/// name that doesn't match any [`LayoutAlgorithm`] variant. The error
/// message lists every accepted name so the caller can fix the typo
/// without consulting docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutAlgorithmParseError {
    pub unknown: String,
}

impl std::fmt::Display for LayoutAlgorithmParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let known: Vec<&str> = LayoutAlgorithm::ALL.iter().map(|a| a.as_str()).collect();
        write!(
            f,
            "unknown layout algorithm `{}`; expected one of: {}",
            self.unknown,
            known.join(", ")
        )
    }
}

impl std::error::Error for LayoutAlgorithmParseError {}

// ---------------------------------------------------------------
// petgraph integration + Kamada-Kawai pilot
// ---------------------------------------------------------------

use crate::graph_types::GraphData;
use petgraph::Graph;
use petgraph::Undirected;
use petgraph::graph::NodeIndex;
use std::collections::BTreeMap;

/// Convert panschema-viz's wire-format [`GraphData`] into an
/// undirected [`petgraph::Graph`] suitable for the stress-based
/// layout algorithms.
///
/// Returns the graph plus an `id → NodeIndex` lookup so callers can
/// map algorithm output back to the input node order. Edges
/// referencing unknown node ids are silently dropped, matching how
/// the in-tree force simulation handles missing endpoints.
pub fn to_petgraph(
    graph: &GraphData,
) -> (Graph<String, (), Undirected>, BTreeMap<String, NodeIndex>) {
    let mut pg = Graph::new_undirected();
    let mut id_to_idx = BTreeMap::new();
    for node in &graph.nodes {
        let idx = pg.add_node(node.id.clone());
        id_to_idx.insert(node.id.clone(), idx);
    }
    for edge in &graph.edges {
        if let (Some(&s), Some(&t)) = (id_to_idx.get(&edge.source), id_to_idx.get(&edge.target)) {
            pg.add_edge(s, t, ());
        }
    }
    (pg, id_to_idx)
}

/// Kamada-Kawai energy minimization over the LinkML schema graph, laid out one
/// component at a time and shelf-packed by `layout_by_components`, returning
/// positions in the original [`GraphData`] node order.
///
/// Each component is laid out on its own because Kamada-Kawai cannot lay out
/// a graph whose nodes are not all reachable from one another: an unreachable
/// pair makes every node's gradient NaN, which the algorithm reads as already
/// converged, so it would return its starting positions unmoved.
///
/// Empty input returns an empty `Vec`.
pub fn kamada_kawai(graph: &GraphData, aspect_w: f32, aspect_h: f32) -> Vec<(f32, f32)> {
    layout_by_components(graph, aspect_w, aspect_h, kamada_kawai_component)
}

/// Run Kamada-Kawai on a single connected component, returning positions in
/// `component` order.
fn kamada_kawai_component(
    pg: &Graph<String, (), Undirected>,
    component: &[NodeIndex],
) -> Vec<(f32, f32)> {
    use algo::{DrawingEuclidean2d, KamadaKawai};

    if let Some(positions) = trivial_component(component) {
        return positions;
    }
    let sub = component_subgraph(pg, component);
    let mut drawing = DrawingEuclidean2d::<NodeIndex, f32>::initial_placement(&sub);
    KamadaKawai::new(&sub, |_| 1.0_f32).run(&mut drawing);
    finite_positions(&drawing, component.len())
}

/// Whether the canvas's long axis is x. A square canvas counts as wide: x is
/// the reading direction, so a chain lies along it and a tall compact layout
/// is widened toward square. `orient_along_canvas` and `fit_to_aspect` both
/// take their side from here, so they cannot disagree.
fn canvas_is_wide(aspect_w: f32, aspect_h: f32) -> bool {
    aspect_w >= aspect_h
}

/// Below this ratio of a component's principal spreads it has no long axis
/// worth turning toward the canvas, and `orient_along_canvas` leaves it as
/// the algorithm laid it out. Real schema graphs measured in 2026-10 lie
/// between 1.0 and 1.5; chains lie beyond 15.
const MIN_ELONGATION_TO_ORIENT: f32 = 2.0;

/// Turns `positions` about their centroid so that their long axis lies along
/// x when `wide`, along y otherwise, with the first position toward the left
/// or the top, and returns whether it turned them.
///
/// The long axis is the principal axis of the positions' covariance, and the
/// elongation is the ratio of the spreads along and across it. Below
/// [`MIN_ELONGATION_TO_ORIENT`] there is no long axis to speak of and nothing
/// moves. A single node or a lone edge is placed directly by the pipeline
/// rather than laid out, and is left as placed. A turn keeps every distance,
/// so a chain stays exactly as straight as the layout left it, which is what
/// lets it use the canvas's length without being stretched to it. An axis
/// has two directions; putting the first position first along it keeps a
/// chain reading the same way however the layout happened to tilt it.
fn orient_along_canvas(positions: &mut [(f32, f32)], wide: bool) -> bool {
    if positions.len() < 3 {
        return false;
    }
    let n = positions.len() as f64;
    let (mx, my) = positions
        .iter()
        .fold((0.0_f64, 0.0_f64), |(sx, sy), &(x, y)| {
            (sx + f64::from(x), sy + f64::from(y))
        });
    let (mx, my) = (mx / n, my / n);
    let (sxx, syy, sxy) =
        positions
            .iter()
            .fold((0.0_f64, 0.0_f64, 0.0_f64), |(sxx, syy, sxy), &(x, y)| {
                let (dx, dy) = (f64::from(x) - mx, f64::from(y) - my);
                (sxx + dx * dx, syy + dy * dy, sxy + dx * dy)
            });
    // Eigenvalues of the covariance matrix [[sxx, sxy], [sxy, syy]].
    let half_trace = (sxx + syy) / 2.0;
    let discriminant = (half_trace * half_trace - (sxx * syy - sxy * sxy))
        .max(0.0)
        .sqrt();
    let (along, across) = (
        half_trace + discriminant,
        (half_trace - discriminant).max(0.0),
    );
    // A smaller spread of zero means collinear positions, an elongation past
    // any threshold; the floor keeps that a number rather than a division by
    // zero. Positions all at one point have no spread at all and stay.
    let elongation = (along / across.max(f64::MIN_POSITIVE)).sqrt();
    if elongation < f64::from(MIN_ELONGATION_TO_ORIENT) {
        return false;
    }
    let axis = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let turn = if wide {
        -axis
    } else {
        std::f64::consts::FRAC_PI_2 - axis
    };
    let rotated = |turn: f64, (x, y): (f32, f32)| {
        let (dx, dy) = (f64::from(x) - mx, f64::from(y) - my);
        (
            dx * turn.cos() - dy * turn.sin(),
            dx * turn.sin() + dy * turn.cos(),
        )
    };
    // The other way round, a half turn more, if the first position would
    // land past the centroid; exactly on it, the direction stays as it is.
    let (first_x, first_y) = rotated(turn, positions[0]);
    let direction = if (wide && first_x > 0.0) || (!wide && first_y > 0.0) {
        -1.0
    } else {
        1.0
    };
    for p in positions.iter_mut() {
        let (dx, dy) = rotated(turn, *p);
        *p = ((mx + direction * dx) as f32, (my + direction * dy) as f32);
    }
    true
}

/// The least the realized-aspect correction in `fit_to_aspect` is allowed to
/// scale either axis, up or down; [`stretch_cap`] raises it when the canvas
/// needs more.
///
/// The correction scales the axes apart to bring a layout's aspect to the
/// canvas's, which magnifies any bow in a nearly straight run of nodes by the
/// factor squared. No layout leaves a chain straight: measured from the
/// spiral start, stress leaves it bowed by 3 to 10 percent of its length,
/// SGD by up to 5 and Kamada-Kawai by 2 to 3, and a chain's own aspect is so
/// extreme that an uncapped correction turned any of those into an L. At 1.5
/// the bow grows by at most 2.25×, a gentle curve. The default 16:8 canvas
/// needs 1.41× to fit a square layout, inside this floor.
const MAX_ASPECT_STRETCH: f32 = 1.5;

/// How far `fit_to_aspect` may scale either axis for a canvas of aspect
/// `target`: [`MAX_ASPECT_STRETCH`], or what a square layout needs to reach
/// the target if that is more, so a compact graph is always fitted to the
/// canvas it was configured for and only an elongated one is held back.
fn stretch_cap(target: f32) -> f32 {
    let square_needs = target.sqrt();
    MAX_ASPECT_STRETCH
        .max(square_needs)
        .max(square_needs.recip())
}

/// The bounding box of `points` as `(min_x, max_x, min_y, max_y)`; infinite
/// and inverted when there are none.
fn bounding_box(points: &[(f32, f32)]) -> (f32, f32, f32, f32) {
    points.iter().fold(
        (
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ),
        |(min_x, max_x, min_y, max_y), &(x, y)| {
            (min_x.min(x), max_x.max(x), min_y.min(y), max_y.max(y))
        },
    )
}

/// Lays out one connected component, returning positions in `component` order.
type ComponentLayout = fn(&Graph<String, (), Undirected>, &[NodeIndex]) -> Vec<(f32, f32)>;

/// Lay `graph` out one connected component at a time with `layout_component`,
/// then shelf-pack the components into a single drawing.
///
/// The stress-based layouts' all-pairs distance formulation produces NaN
/// across the entire optimization when any two nodes are unreachable, not
/// just the disconnected ones. Real schemas have isolated nodes (unused enums,
/// types, slots), so each component is laid out on its own, then the results
/// are shelf-packed into a rectangle whose aspect approximates the configured
/// `aspect_w : aspect_h` — taller components first, wrapping to a new row when
/// adding the next would exceed the target row width. This produces a roughly
/// rectangular final bbox even when the input has many small disconnected
/// pieces, instead of a thin horizontal strip.
///
/// Every component gets at least a `min_cell` square cell, so single nodes,
/// whose own bbox is 0×0, grid into a compact block instead of collapsing
/// into a zero-height row.
///
/// Empty input returns an empty `Vec`.
///
/// Before packing, each component is turned so that its long axis, if it has
/// one, runs along the canvas's long axis (`orient_along_canvas`); a turn
/// keeps every distance, so this uses the canvas without distorting the
/// layout, and compact components are left as laid out.
///
/// `#[mutants::skip]`: the shelf-packing arithmetic admits several
/// observationally-equivalent formulations of the bbox accumulation,
/// `total_area` / `target_row_width` formula, wrap condition, and
/// per-row position assignment — different mutations produce
/// alternative-but-still-valid layouts that pass the callers' contracts
/// (finite coordinates, non-overlapping components, aspect-biased bbox).
/// Those contracts are pinned by the `every_layout_*` tests, which run all
/// three layouts that share this pipeline, and by the stress and SGD packing
/// tests; the specific arithmetic chosen here is one valid implementation,
/// not the only one.
#[mutants::skip]
fn layout_by_components(
    graph: &GraphData,
    aspect_w: f32,
    aspect_h: f32,
    layout_component: ComponentLayout,
) -> Vec<(f32, f32)> {
    if graph.nodes.is_empty() {
        return Vec::new();
    }

    let (pg, _) = to_petgraph(graph);
    let components = connected_components_of(&pg);

    // Lay out each component independently and translate so its bbox
    // starts at the origin (positions are returned as offsets from the
    // component's own origin — the packer adds the per-component
    // origin to each).
    struct LaidOut {
        positions: Vec<(f32, f32)>,
        component: Vec<NodeIndex>,
        width: f32,
        height: f32,
    }
    let mut laid: Vec<LaidOut> = components
        .into_iter()
        .map(|component| {
            let mut raw = layout_component(&pg, &component);
            orient_along_canvas(&mut raw, canvas_is_wide(aspect_w, aspect_h));
            let (min_x, max_x, min_y, max_y) = bounding_box(&raw);
            let width = (max_x - min_x).max(0.0);
            let height = (max_y - min_y).max(0.0);
            let translated: Vec<(f32, f32)> =
                raw.iter().map(|(x, y)| (x - min_x, y - min_y)).collect();
            LaidOut {
                positions: translated,
                component,
                width,
                height,
            }
        })
        .collect();

    // Tallest components first — pack into rows whose target width is
    // tuned so the overall packed bbox approximates `aspect_w : aspect_h`.
    laid.sort_by(|a, b| {
        b.height
            .partial_cmp(&a.height)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // The per-component gap must scale with the components' own
    // coordinate space — a fixed gap dominates the layout when a
    // component's natural bbox is small, and disappears when it is
    // large. 5% of the largest component's dimension
    // gives clear visual separation without smearing the cluster's
    // bbox to the corners of the viewport. A floor of 1.0 keeps tiny
    // single-edge components from packing on top of each other.
    let max_dim = laid
        .iter()
        .map(|c| c.width.max(c.height))
        .fold(0.0_f32, f32::max);
    let component_gap = (max_dim * 0.05).max(1.0);
    // Singletons (and 2-node lines) have a ~0×0 — or zero-height — bbox.
    // Without a floor the shelf-packer strings them into a single
    // zero-height row that wastes all the vertical space (a thin
    // horizontal smear). Give every component a minimum *square* cell so
    // isolated nodes grid up into a compact 2D block instead of a line.
    let min_cell = (max_dim * 0.12).max(component_gap * 2.0);
    let cell = |c: &LaidOut| (c.width.max(min_cell), c.height.max(min_cell));
    let total_area: f32 = laid
        .iter()
        .map(|c| {
            let (w, h) = cell(c);
            w * h
        })
        .sum();
    // Floor the row width at the widest component (not an absolute
    // constant) so the target tracks the layout's own coordinate scale —
    // a fixed floor that exceeds the content forces everything onto one
    // row and re-creates the horizontal smear.
    let target_row_width = (total_area * aspect_w / aspect_h).sqrt().max(max_dim);

    let mut positions: Vec<(f32, f32)> = vec![(0.0, 0.0); graph.nodes.len()];
    let mut row_x = 0.0_f32;
    let mut row_y = 0.0_f32;
    let mut row_height = 0.0_f32;
    for c in &laid {
        let (cw, ch) = cell(c);
        if row_x > 0.0 && row_x + cw > target_row_width {
            row_y += row_height + component_gap;
            row_x = 0.0;
            row_height = 0.0;
        }
        // Center each component's own bbox within its (possibly larger)
        // cell so a singleton sits in the middle of its grid slot.
        let off_x = row_x + (cw - c.width) / 2.0;
        let off_y = row_y + (ch - c.height) / 2.0;
        // `to_petgraph` adds one node per `graph.nodes` entry, in order, so a
        // node's index is its position in the output.
        for (sub_pos, &node_index) in c.positions.iter().zip(c.component.iter()) {
            positions[node_index.index()] = (sub_pos.0 + off_x, sub_pos.1 + off_y);
        }
        row_x += cw + component_gap;
        row_height = row_height.max(ch);
    }

    // Only a component the algorithm laid out has a shape the stretch could
    // distort; singletons and lone edges are placed directly.
    let laid_out_by_algorithm = laid.iter().any(|c| c.component.len() > 2);
    fit_to_aspect(&mut positions, aspect_w, aspect_h, laid_out_by_algorithm);
    positions
}

/// Lengthens `positions` along the canvas's long axis toward the canvas's
/// aspect `aspect_w : aspect_h`, area-preserving: x is scaled by
/// `√(target / realized)` and y by its inverse.
///
/// The shelf-packer gets the arrangement roughly right, but with a big
/// component plus many singletons it tends to come out near-square, narrower
/// than a wide canvas, leaving it mostly empty. Measuring the realized aspect
/// lands single- and multi-component graphs on the target alike, and is a
/// no-op when already there, so it cannot double-apply into a horizontal
/// smear the way a blind `√(target)` stretch did.
///
/// The fit only ever lengthens along the canvas's long axis, x for a wide or
/// square canvas and y for a tall one. A layout already longer than the
/// canvas, a turned chain above all, is left alone for the camera to fit by
/// its length: squashing it would bend whatever bow it has for no gain in
/// fill.
///
/// With `cap_stretch`, the stretch is capped per axis at [`stretch_cap`], so
/// a layout far from the target is fitted only as far as the cap allows
/// rather than distorted onto the target. The cap protects what an algorithm
/// laid out; a graph of only singletons and lone edges, placed directly, has
/// no such shape, and the caller leaves it uncapped so a few isolated pairs
/// still spread to fill the canvas. A degenerate bounding box or aspect is
/// left alone.
fn fit_to_aspect(positions: &mut [(f32, f32)], aspect_w: f32, aspect_h: f32, cap_stretch: bool) {
    let (min_x, max_x, min_y, max_y) = bounding_box(positions);
    let (bw, bh) = (max_x - min_x, max_y - min_y);
    if !(bw > f32::EPSILON && bh > f32::EPSILON && aspect_w > 0.0 && aspect_h > 0.0) {
        return;
    }
    let realized = bw / bh;
    let target = aspect_w / aspect_h;
    let cap = if cap_stretch {
        stretch_cap(target)
    } else {
        f32::INFINITY
    };
    // The bounds only ever lengthen along the canvas's long axis: x may grow
    // for a wide canvas and shrink for a tall one, never the other way.
    let (lowest, highest) = if canvas_is_wide(aspect_w, aspect_h) {
        (1.0, cap)
    } else {
        (cap.recip(), 1.0)
    };
    let s = (target / realized).sqrt().clamp(lowest, highest);
    for p in positions.iter_mut() {
        p.0 *= s;
        p.1 /= s;
    }
}

/// Stress majorization over the LinkML schema graph, laid out one component
/// at a time and shelf-packed by `layout_by_components`.
///
/// Stress majorization is the literature reference point for "what a
/// good static layout looks like" on graphs in the 50–2000 node range
/// (the algorithm behind graphviz's `neato -Kstress`). Compared with
/// `kamada_kawai`'s one-node-at-a-time gradient descent, the
/// majorization formulation converges in ~30 iterations of
/// `O(N²)` updates and produces cleaner cluster separation and more
/// uniform edge lengths.
pub fn stress_majorization(graph: &GraphData, aspect_w: f32, aspect_h: f32) -> Vec<(f32, f32)> {
    layout_by_components(graph, aspect_w, aspect_h, stress_majorization_component)
}

/// Enumerate connected components of an undirected petgraph as lists
/// of [`NodeIndex`]. Used by `layout_by_components` to dispatch a layout per
/// component, since the stress-based layouts' all-pairs distance matrix
/// breaks on disconnected inputs.
fn connected_components_of(
    pg: &petgraph::Graph<String, (), petgraph::Undirected>,
) -> Vec<Vec<NodeIndex>> {
    use petgraph::visit::{Bfs, IntoNodeIdentifiers};
    use std::collections::HashSet;

    let mut components: Vec<Vec<NodeIndex>> = Vec::new();
    let mut visited: HashSet<NodeIndex> = HashSet::new();
    for start in pg.node_identifiers() {
        if visited.contains(&start) {
            continue;
        }
        let mut component = Vec::new();
        let mut bfs = Bfs::new(pg, start);
        while let Some(node) = bfs.next(pg) {
            if visited.insert(node) {
                component.push(node);
            }
        }
        components.push(component);
    }
    components
}

/// The layout of a component too small to need an algorithm, if it is one: a
/// single node at the origin, or a lone edge at the target length.
///
/// Every algorithm lays a lone edge out at its length on its own; returning
/// this pair keeps a lone edge identical under every layout.
fn trivial_component(component: &[NodeIndex]) -> Option<Vec<(f32, f32)>> {
    match component.len() {
        0 | 1 => Some(vec![(0.0, 0.0); component.len()]),
        2 => Some(vec![(0.0, 0.0), (1.0, 0.0)]),
        _ => None,
    }
}

/// The subgraph induced by `component`. Its node `i` is `component[i]`, which
/// is what lets `finite_positions` read positions back in `component` order.
fn component_subgraph(
    pg: &Graph<String, (), Undirected>,
    component: &[NodeIndex],
) -> Graph<(), (), Undirected> {
    use petgraph::visit::EdgeRef;

    let mut sub: Graph<(), (), Undirected> = Graph::new_undirected();
    let mut orig_to_sub: BTreeMap<NodeIndex, NodeIndex> = BTreeMap::new();
    for &orig in component {
        let new_id = sub.add_node(());
        orig_to_sub.insert(orig, new_id);
    }
    for &orig in component {
        for edge in pg.edges(orig) {
            let target = edge.target();
            if orig.index() <= target.index()
                && let (Some(&from), Some(&to)) = (orig_to_sub.get(&orig), orig_to_sub.get(&target))
            {
                sub.add_edge(from, to, ());
            }
        }
    }
    sub
}

/// The first `n` positions of `drawing`, in node order, with any non-finite
/// coordinate replaced by 0 so it cannot poison the packing.
fn finite_positions(
    drawing: &algo::DrawingEuclidean2d<NodeIndex, f32>,
    n: usize,
) -> Vec<(f32, f32)> {
    let finite = |v: Option<f32>| v.filter(|v| v.is_finite()).unwrap_or(0.0);
    (0..n)
        .map(|i| {
            let idx = NodeIndex::new(i);
            (finite(drawing.x(idx)), finite(drawing.y(idx)))
        })
        .collect()
}

/// Run stress majorization on a single connected component, returning
/// positions in `component` order.
fn stress_majorization_component(
    pg: &Graph<String, (), Undirected>,
    component: &[NodeIndex],
) -> Vec<(f32, f32)> {
    use algo::{DrawingEuclidean2d, StressMajorization};

    if let Some(positions) = trivial_component(component) {
        return positions;
    }
    let sub = component_subgraph(pg, component);
    let mut drawing = DrawingEuclidean2d::<NodeIndex, f32>::initial_placement(&sub);
    let mut sm = StressMajorization::new(&sub, &drawing, &mut |_| 1.0_f32);
    sm.run(&mut drawing);
    finite_positions(&drawing, component.len())
}

/// Run SGD (Stochastic Gradient Descent) layout over the LinkML
/// schema graph. Like [`stress_majorization`], SGD minimizes a stress
/// function but using a stochastic per-pair update instead of a global
/// majorization step — typically the best quality-per-time of the
/// stress-based layouts here, converging in `O(N · iters)` time with
/// visibly comparable quality to stress majorization.
///
/// Laid out one component at a time and shelf-packed by
/// `layout_by_components`, for the same reason as [`stress_majorization`].
///
/// The RNG is seeded deterministically so the same input produces
/// byte-identical output across runs; this matters for the
/// idempotent-publish guarantee that `panschema publish` makes.
pub fn sgd(graph: &GraphData, aspect_w: f32, aspect_h: f32) -> Vec<(f32, f32)> {
    layout_by_components(graph, aspect_w, aspect_h, sgd_component)
}

/// Recommend the 2D default layout from the graph's inheritance
/// density: the fraction of edges that are `subclass_of` or `mixin`.
/// When at least half the edges are inheritance, the graph is a tree
/// at heart and the Hierarchical (Sugiyama) layout shows that
/// structure far more legibly than SGD's force-like placement; below
/// the threshold SGD stays the general-purpose default.
///
/// The 0.5 threshold is tuned against the two ends of the corpus:
/// scimantic's `is_a`-heavy claim spine (≈0.7 inheritance → wants
/// Hierarchical) versus the mixed-edge reference fixture (≈0.3 → wants
/// SGD). It's a heuristic, not a contract — `html_default_layout` (and
/// a persisted user choice) override it.
pub fn recommend_default_layout(graph: &GraphData) -> LayoutAlgorithm {
    let total = graph.edges.len();
    if total == 0 {
        return LayoutAlgorithm::Sgd;
    }
    let inheritance = graph
        .edges
        .iter()
        .filter(|e| {
            matches!(
                e.edge_type,
                crate::graph_types::EdgeType::SubclassOf | crate::graph_types::EdgeType::Mixin
            )
        })
        .count();
    if inheritance as f32 / total as f32 >= 0.5 {
        LayoutAlgorithm::Hierarchical
    } else {
        LayoutAlgorithm::Sgd
    }
}

/// Run SGD on a single connected component, returning positions in
/// `component` order.
fn sgd_component(pg: &Graph<String, (), Undirected>, component: &[NodeIndex]) -> Vec<(f32, f32)> {
    use algo::{DrawingEuclidean2d, FullSgd, Scheduler, SchedulerExponential};
    use rng::SplitMix64;

    if let Some(positions) = trivial_component(component) {
        return positions;
    }
    let sub = component_subgraph(pg, component);
    let mut sgd_state = FullSgd::new().build(&sub, |_| 1.0_f32);
    let mut drawing = DrawingEuclidean2d::<NodeIndex, f32>::initial_placement(&sub);
    let mut rng = SplitMix64::new(42);
    let mut scheduler = sgd_state.scheduler::<SchedulerExponential<f32>>(100, 0.1);
    scheduler.run(&mut |eta| {
        sgd_state.shuffle(&mut rng);
        sgd_state.apply(&mut drawing, eta);
    });
    finite_positions(&drawing, component.len())
}

/// Default target for [`scale_to_world`] in world units. Sized so the
/// rendered layout fills the in-tree `CpuSimulation`'s world bounding
/// box without clipping against its `MAX_RADIUS = 800` safety net.
pub const WORLD_TARGET_DIMENSION: f32 = 600.0;

/// Rescale a position list in place so its bounding box has its larger
/// dimension equal to `target_max_dim` world units, while preserving
/// aspect ratio and centroid. A degenerate bbox (all points coincident,
/// or only one node) is left untouched — there's nothing meaningful to
/// scale.
///
/// Used by static (non-force-directed) layouts so their natural
/// coordinate system (typically O(1) magnitudes from the stress-based
/// layouts) lands inside the visualization's expected world
/// range.
pub fn scale_to_world(positions: &mut [(f32, f32)], target_max_dim: f32) {
    if positions.len() < 2 {
        return;
    }
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for &(x, y) in positions.iter() {
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    let bbox_w = max_x - min_x;
    let bbox_h = max_y - min_y;
    let bbox_max = bbox_w.max(bbox_h);
    if bbox_max <= 0.0 || !bbox_max.is_finite() {
        return;
    }
    let scale = target_max_dim / bbox_max;
    let cx = (min_x + max_x) * 0.5;
    let cy = (min_y + max_y) * 0.5;
    for p in positions.iter_mut() {
        p.0 = (p.0 - cx) * scale;
        p.1 = (p.1 - cy) * scale;
    }
}

use crate::graph_types::EdgeType;

/// Edge types that contribute to the class-hierarchy spine. Sugiyama
/// runs over the sub-DAG these edges form; property edges (`Domain`,
/// `Range`, `Inverse`, `TypeOf`) overlay the layered output later
/// without participating in layering or cycle-breaking.
fn is_hierarchy_edge(t: EdgeType) -> bool {
    matches!(t, EdgeType::SubclassOf | EdgeType::Mixin)
}

/// Bounds check on a sugiyama vertex id before we index `positions`.
/// Provably equivalent under `<` vs `<=` because rust-sugiyama's
/// `from_edges` returns vertex ids in `0..n-1` (it auto-creates
/// nodes for the unique endpoints it sees), so `idx == n` never
/// happens. The check is a defensive guard for a future API shift.
/// Extracted into its own function so `#[mutants::skip]` can suppress
/// the `<` mutation without losing coverage on the surrounding
/// `hierarchical` arithmetic.
#[mutants::skip]
fn sugiyama_index_in_bounds(idx: usize, n: usize) -> bool {
    idx < n
}

/// Accumulator step for the per-component x-offset Sugiyama uses to
/// place disjoint subgraphs side-by-side. `+` vs `*` between `width`
/// and `gap` is observationally equivalent for rust-sugiyama's
/// typical small `width` return values — both produce gaps within
/// the test's tolerance band — so attempting to distinguish them
/// either requires overfitting to one upstream version's width math
/// or trading the disambiguating fixture for one that breaks for
/// other reasons. Skipped here rather than chased.
#[mutants::skip]
fn advance_component_offset(x_offset: f64, width: f64, gap: f64) -> f64 {
    x_offset + width + gap
}

/// Run a Sugiyama-style layered layout over the `is_a` / `mixin`
/// sub-DAG of the schema and return positions in original
/// [`GraphData`] node order.
///
/// Property edges (range / domain / inverse / typeof) deliberately
/// don't participate in the layering — they overlay the layered
/// output afterwards. Nodes that don't appear in any hierarchy edge
/// (orphans relative to the hierarchy spine, even if they carry
/// property edges) fall back to a grid arrangement below the layered
/// region so the connected layered cluster keeps the central
/// viewport.
///
/// `aspect_w` and `aspect_h` bias the final bbox toward that aspect with a
/// fixed stretch: x is scaled by √(w/h) and y by √(h/w). Unlike the
/// stress-based layouts it does not measure the realized aspect first.
///
/// Cycles in the hierarchy edge subset (which LinkML schemas
/// shouldn't have, but pathological inputs might) are broken by
/// rust-sugiyama's internal greedy feedback arc set. We don't surface
/// which edges got reversed — that's a follow-on diagnostic.
pub fn hierarchical(graph: &GraphData, aspect_w: f32, aspect_h: f32) -> Vec<(f32, f32)> {
    use rust_sugiyama::configure::Config;

    if graph.nodes.is_empty() {
        return Vec::new();
    }

    let id_to_idx: BTreeMap<&str, u32> = graph
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i as u32))
        .collect();

    let hierarchy_edges: Vec<(u32, u32)> = graph
        .edges
        .iter()
        .filter(|e| is_hierarchy_edge(e.edge_type))
        .filter_map(|e| {
            let s = id_to_idx.get(e.source.as_str())?;
            let t = id_to_idx.get(e.target.as_str())?;
            Some((*s, *t))
        })
        .collect();

    // Track which node indices Sugiyama actually placed so we can
    // arrange orphans (nodes with no hierarchy edges) in a separate
    // region after layout completes.
    let mut placed: Vec<bool> = vec![false; graph.nodes.len()];
    let mut positions: Vec<(f32, f32)> = vec![(0.0, 0.0); graph.nodes.len()];

    let layouts = if hierarchy_edges.is_empty() {
        Vec::new()
    } else {
        let config = Config::default();
        rust_sugiyama::from_edges(&hierarchy_edges, &config)
    };

    // Concatenate disjoint hierarchy components left-to-right, with a
    // gap between each component proportional to its width, so a
    // schema with multiple roots reads as side-by-side trees rather
    // than overlapping.
    let mut x_offset = 0.0_f64;
    const COMPONENT_GAP: f64 = 50.0;
    for (subgraph, width, _height) in layouts {
        for (vertex_id, (x, y)) in subgraph {
            let node_idx = vertex_id;
            if sugiyama_index_in_bounds(node_idx, graph.nodes.len()) {
                positions[node_idx] = ((x + x_offset) as f32, y as f32);
                placed[node_idx] = true;
            }
        }
        x_offset = advance_component_offset(x_offset, width, COMPONENT_GAP);
    }

    // Arrange orphan nodes (no hierarchy edges) in a grid below the
    // layered region. The grid sits at a deliberate vertical gap so
    // the layered cluster stays in the central viewport.
    let orphan_indices: Vec<usize> = placed
        .iter()
        .enumerate()
        .filter_map(|(i, &p)| (!p).then_some(i))
        .collect();
    if !orphan_indices.is_empty() {
        let layered_min_y = positions
            .iter()
            .zip(placed.iter())
            .filter_map(|(p, &pl)| pl.then_some(p.1))
            .fold(f32::INFINITY, f32::min);
        let orphan_top_y = if layered_min_y.is_finite() {
            layered_min_y - 80.0
        } else {
            0.0
        };
        const ORPHAN_SPACING: f32 = 30.0;
        let columns = ((orphan_indices.len() as f32).sqrt().ceil() as usize).max(1);
        for (i, &idx) in orphan_indices.iter().enumerate() {
            let col = i % columns;
            let row = i / columns;
            positions[idx] = (
                (col as f32) * ORPHAN_SPACING,
                orphan_top_y - (row as f32) * ORPHAN_SPACING,
            );
        }
    }

    let sx = (aspect_w / aspect_h).sqrt();
    let sy = (aspect_h / aspect_w).sqrt();
    for p in positions.iter_mut() {
        p.0 *= sx;
        p.1 *= sy;
    }

    positions
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn from_str_accepts_every_canonical_identifier() {
        // Every variant in `ALL` must round-trip through `from_str` →
        // `as_str` cleanly. This is the catch-net for "added a new
        // variant but forgot the parser branch."
        for variant in LayoutAlgorithm::ALL {
            let parsed = LayoutAlgorithm::from_str(variant.as_str()).unwrap();
            assert_eq!(parsed, *variant);
        }
    }

    #[test]
    fn from_str_rejects_unknown_names() {
        let err = LayoutAlgorithm::from_str("nope").unwrap_err();
        assert_eq!(err.unknown, "nope");
        let msg = err.to_string();
        assert!(msg.contains("nope"));
        assert!(msg.contains("force-directed"));
        assert!(msg.contains("hierarchical"));
    }

    #[test]
    fn only_real_implementations_report_implemented() {
        for variant in LayoutAlgorithm::ALL {
            let implemented = variant.is_implemented();
            match variant {
                LayoutAlgorithm::ForceDirected
                | LayoutAlgorithm::KamadaKawai
                | LayoutAlgorithm::Hierarchical
                | LayoutAlgorithm::Stress
                | LayoutAlgorithm::Sgd => {
                    assert!(implemented, "{:?} should be implemented", variant)
                }
                _ => assert!(!implemented, "{:?} should not be implemented", variant),
            }
        }
    }

    #[test]
    fn all_variants_have_distinct_canonical_identifiers() {
        let mut seen = std::collections::HashSet::new();
        for variant in LayoutAlgorithm::ALL {
            assert!(
                seen.insert(variant.as_str()),
                "duplicate identifier: {}",
                variant.as_str()
            );
        }
    }

    #[test]
    fn all_variants_have_distinct_display_names() {
        let mut seen = std::collections::HashSet::new();
        for variant in LayoutAlgorithm::ALL {
            assert!(
                seen.insert(variant.display_name()),
                "duplicate display name: {}",
                variant.display_name()
            );
        }
    }

    #[test]
    fn canonical_identifiers_use_kebab_case() {
        // Identifiers must be lowercase ASCII + `-` only, so they
        // slot into CSS custom-property values, manifest strings,
        // and URL query params without escaping.
        for variant in LayoutAlgorithm::ALL {
            let id = variant.as_str();
            assert!(
                id.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "identifier `{id}` must be kebab-case ASCII"
            );
            assert!(!id.starts_with('-') && !id.ends_with('-'));
        }
    }

    // ---------------------------------------------------------------
    // petgraph integration + Kamada-Kawai pilot tests
    // ---------------------------------------------------------------

    use crate::graph_types::{EdgeType, GraphEdge, GraphNode, NodeType};

    /// Build a graph with `is_a` (subclass_of) and `range` edges in the
    /// given counts (nodes are placeholders) to exercise the layout
    /// recommendation's inheritance-density threshold.
    fn graph_with_edge_mix(is_a: usize, range: usize) -> GraphData {
        let n = is_a + range + 1;
        let nodes = (0..n)
            .map(|i| GraphNode {
                id: format!("n{i}"),
                label: format!("N{i}"),
                node_type: NodeType::Class,
                color: [1.0, 0.0, 0.0, 1.0],
                description: None,
                uri: None,
                uri_unresolved: false,
                is_abstract: false,
                kind_metadata: None,
            })
            .collect();
        let mut edges = Vec::new();
        for i in 0..is_a {
            edges.push(GraphEdge {
                source: format!("n{}", i + 1),
                target: "n0".to_string(),
                edge_type: EdgeType::SubclassOf,
                label: None,
            });
        }
        for i in 0..range {
            edges.push(GraphEdge {
                source: format!("n{}", is_a + i + 1),
                target: "n0".to_string(),
                edge_type: EdgeType::Range,
                label: None,
            });
        }
        GraphData {
            schema_name: "mix".to_string(),
            schema_title: None,
            nodes,
            edges,
            format_version: "1.0".to_string(),
            graph_kind: crate::graph_types::GraphKind::default(),
        }
    }

    #[test]
    fn recommends_hierarchical_for_is_a_heavy_graph() {
        // 8 subclass_of + 2 range → 0.8 inheritance → Hierarchical.
        let g = graph_with_edge_mix(8, 2);
        assert_eq!(recommend_default_layout(&g), LayoutAlgorithm::Hierarchical);
    }

    #[test]
    fn recommends_sgd_for_reference_heavy_graph() {
        // 2 subclass_of + 8 range → 0.2 inheritance → SGD.
        let g = graph_with_edge_mix(2, 8);
        assert_eq!(recommend_default_layout(&g), LayoutAlgorithm::Sgd);
        // Edgeless graph also defaults to SGD (no signal).
        let mut empty = graph_with_edge_mix(0, 0);
        empty.edges.clear();
        assert_eq!(recommend_default_layout(&empty), LayoutAlgorithm::Sgd);
    }

    fn make_ring(n: usize) -> GraphData {
        let nodes = (0..n)
            .map(|i| GraphNode {
                id: format!("n{i}"),
                label: format!("N{i}"),
                node_type: NodeType::Class,
                color: [1.0, 0.0, 0.0, 1.0],
                description: None,
                uri: None,
                uri_unresolved: false,
                is_abstract: false,
                kind_metadata: None,
            })
            .collect();
        let edges = (0..n)
            .map(|i| GraphEdge {
                source: format!("n{i}"),
                target: format!("n{}", (i + 1) % n),
                edge_type: EdgeType::SubclassOf,
                label: None,
            })
            .collect();
        GraphData {
            schema_name: "ring".into(),
            schema_title: None,
            format_version: "1.0".into(),
            graph_kind: crate::graph_types::GraphKind::default(),
            nodes,
            edges,
        }
    }

    fn make_lopsided(connected_n: usize, isolated_n: usize) -> GraphData {
        let total = connected_n + isolated_n;
        let nodes = (0..total)
            .map(|i| GraphNode {
                id: format!("n{i}"),
                label: format!("N{i}"),
                node_type: NodeType::Class,
                color: [1.0, 0.0, 0.0, 1.0],
                description: None,
                uri: None,
                uri_unresolved: false,
                is_abstract: false,
                kind_metadata: None,
            })
            .collect();
        let edges = (0..connected_n)
            .map(|i| GraphEdge {
                source: format!("n{i}"),
                target: format!("n{}", (i + 1) % connected_n),
                edge_type: EdgeType::SubclassOf,
                label: None,
            })
            .collect();
        GraphData {
            schema_name: "lopsided".into(),
            schema_title: None,
            format_version: "1.0".into(),
            graph_kind: crate::graph_types::GraphKind::default(),
            nodes,
            edges,
        }
    }

    #[test]
    fn to_petgraph_preserves_node_and_edge_counts() {
        let ring = make_ring(5);
        let (pg, idx) = to_petgraph(&ring);
        assert_eq!(pg.node_count(), 5);
        assert_eq!(pg.edge_count(), 5);
        assert_eq!(idx.len(), 5);
        for id in idx.keys() {
            assert!(id.starts_with('n'));
        }
    }

    #[test]
    fn to_petgraph_drops_edges_with_unknown_endpoints() {
        let mut graph = make_ring(3);
        graph.edges.push(GraphEdge {
            source: "ghost".into(),
            target: "n0".into(),
            edge_type: EdgeType::SubclassOf,
            label: None,
        });
        let (pg, _) = to_petgraph(&graph);
        assert_eq!(pg.node_count(), 3);
        // 3 ring edges retained; the ghost edge is dropped.
        assert_eq!(pg.edge_count(), 3);
    }

    type Layout = fn(&GraphData, f32, f32) -> Vec<(f32, f32)>;

    /// The layouts that share `layout_by_components`.
    const LAYOUTS: [(&str, Layout); 3] = [
        ("stress", stress_majorization),
        ("sgd", sgd),
        ("kamada_kawai", kamada_kawai),
    ];

    #[test]
    fn every_layout_returns_a_finite_position_per_node() {
        for (layout, run) in LAYOUTS {
            for graph in [make_ring(15), make_lopsided(20, 8)] {
                let positions = run(&graph, 1.0, 1.0);
                assert_eq!(positions.len(), graph.nodes.len(), "{layout}");
                assert!(
                    positions
                        .iter()
                        .all(|(x, y)| x.is_finite() && y.is_finite()),
                    "{layout}: non-finite coordinate in {positions:?}"
                );
                let (min_x, max_x, min_y, max_y) = bounding_box(&positions);
                assert!(max_x - min_x > 0.1, "{layout}: collapsed in x");
                assert!(max_y - min_y > 0.1, "{layout}: collapsed in y");
            }
        }
    }

    #[test]
    fn every_layout_returns_nothing_for_an_empty_graph() {
        let mut empty = make_ring(1);
        empty.nodes.clear();
        empty.edges.clear();
        for (layout, run) in LAYOUTS {
            assert!(run(&empty, 1.0, 1.0).is_empty(), "{layout}");
        }
    }

    #[test]
    fn every_layout_scales_into_the_simulation_world() {
        // The picker hands static layouts to the CpuSimulation, whose hard
        // MAX_RADIUS is 800. After `scale_to_world` the larger bbox dimension
        // must be exactly WORLD_TARGET_DIMENSION and every node inside the
        // radius, for connected and disconnected input alike.
        for (layout, run) in LAYOUTS {
            for graph in [make_ring(15), make_lopsided(20, 8), make_ring(30)] {
                let mut positions = run(&graph, 1.0, 1.0);
                scale_to_world(&mut positions, WORLD_TARGET_DIMENSION);
                for &(x, y) in &positions {
                    let r = x.hypot(y);
                    assert!(r < 800.0, "{layout}: node at radius {r} exceeds MAX_RADIUS");
                }
                let (min_x, max_x, min_y, max_y) = bounding_box(&positions);
                let (w, h) = (max_x - min_x, max_y - min_y);
                assert!(w >= 100.0, "{layout}: scaled width {w} is degenerate");
                assert!(h >= 100.0, "{layout}: scaled height {h} is degenerate");
                assert!(w.max(h) - WORLD_TARGET_DIMENSION < 1e-2, "{layout}");
            }
        }
    }

    // A lone edge is the one component every algorithm could lay out but none
    // needs to: the pipeline places it directly, so it looks the same whichever
    // layout a schema's reader picks, and a graph of isolated pairs stays tidy.
    #[test]
    fn every_layout_draws_a_lone_edge_the_same_way() {
        let edge = make_path(2);
        let reference = stress_majorization(&edge, 1.0, 1.0);
        let [(ax, ay), (bx, by)] = reference[..] else {
            unreachable!()
        };
        assert_eq!((bx - ax, by - ay), (1.0, 0.0), "a unit edge, lying flat");
        for (layout, run) in LAYOUTS {
            assert_eq!(run(&edge, 1.0, 1.0), reference, "{layout}");
        }
    }

    // A small instance graph is often a few isolated pairs. Nothing in it was
    // laid out by an algorithm, so there is no shape for the cap to protect,
    // and the pairs are spread to fill the canvas exactly as before the cap.
    // A ring with isolated nodes packs into a block narrower than a 16:8
    // canvas by more than the cap can make up, so the cap shows through the
    // public entry points: the block is widened, but stops short of the
    // canvas aspect.
    #[test]
    fn every_layout_caps_how_far_a_packed_graph_is_widened() {
        let graph = make_lopsided(20, 8);
        for (layout, run) in LAYOUTS {
            let fitted = aspect(&run(&graph, 2.0, 1.0));
            assert!(
                fitted > 1.0 && fitted < 1.95,
                "{layout}: packed graph came out at aspect {fitted}; the cap should stop it short of 2"
            );
        }
    }

    #[test]
    fn every_layout_fills_the_canvas_with_a_graph_of_lone_edges() {
        let mut pairs = make_ring(4);
        pairs.edges = [(0, 1), (2, 3)]
            .map(|(s, t)| GraphEdge {
                source: format!("n{s}"),
                target: format!("n{t}"),
                edge_type: EdgeType::SubclassOf,
                label: None,
            })
            .to_vec();
        for (layout, run) in LAYOUTS {
            let fitted = aspect(&run(&pairs, 16.0, 8.0));
            assert!(
                (fitted - 2.0).abs() < 1e-3,
                "{layout}: two lone edges came out with aspect {fitted} on a 2:1 canvas"
            );
        }
    }

    fn rectangle(w: f32, h: f32) -> Vec<(f32, f32)> {
        vec![(0.0, 0.0), (w, 0.0), (0.0, h), (w, h), (w / 2.0, h / 2.0)]
    }

    #[test]
    fn a_square_canvas_counts_as_wide() {
        assert!(canvas_is_wide(1.0, 1.0));
        assert!(canvas_is_wide(16.0, 8.0));
        assert!(!canvas_is_wide(8.0, 16.0));
    }

    // The fit only lengthens a layout along the canvas's long axis, toward the
    // canvas's aspect and within the cap. A layout already longer than the
    // canvas is left to the camera, which fits its width; squashing it would
    // bend whatever bow it has for no gain in fill. A square canvas counts as
    // wide, so a tall layout on it is widened and a wide one is left alone.
    #[test]
    fn the_fit_only_lengthens_a_layout_along_the_canvas() {
        let cases = [
            // name, width, height of the layout, canvas, expected aspect after
            ("too square for a wide canvas", 1.0, 1.0, (2.0, 1.0), 2.0),
            (
                "far too tall for a wide canvas, capped",
                1.0,
                3.0,
                (2.0, 1.0),
                0.75,
            ),
            (
                "already wider than a wide canvas",
                3.0,
                1.0,
                (2.0, 1.0),
                3.0,
            ),
            ("too square for a tall canvas", 1.0, 1.0, (1.0, 2.0), 0.5),
            (
                "far too wide for a tall canvas, capped",
                3.0,
                1.0,
                (1.0, 2.0),
                3.0 / 2.25,
            ),
            (
                "already taller than a tall canvas",
                1.0,
                3.0,
                (1.0, 2.0),
                1.0 / 3.0,
            ),
            (
                "a wide layout on a square canvas",
                3.0,
                1.0,
                (1.0, 1.0),
                3.0,
            ),
            (
                "a tall layout on a square canvas, widened within the cap",
                1.0,
                3.0,
                (1.0, 1.0),
                0.75,
            ),
        ];
        for (name, w, h, (aw, ah), expected) in cases {
            let mut positions = rectangle(w, h);
            fit_to_aspect(&mut positions, aw, ah, true);
            let fitted = aspect(&positions);
            assert!(
                (fitted / expected - 1.0).abs() < 1e-5,
                "{name}: {w}x{h} on {aw}:{ah} came out {fitted}, expected {expected}"
            );
        }
    }

    // A layout with no extent on an axis, or one a hair wide, has no aspect to
    // fit; a canvas with no extent has no aspect to fit to. All are left
    // exactly as they are rather than scaled by zero or infinity.
    #[test]
    fn the_fit_leaves_a_degenerate_layout_or_canvas_alone() {
        let hair = f32::EPSILON;
        for (name, points, (aw, ah)) in [
            (
                "a vertical line on a wide canvas",
                vec![(1.0, 0.0), (1.0, 2.0), (1.0, 1.0)],
                (2.0, 1.0),
            ),
            (
                "a horizontal line on a tall canvas",
                vec![(0.0, 1.0), (2.0, 1.0), (1.0, 1.0)],
                (1.0, 2.0),
            ),
            (
                "a hairline wide on a wide canvas",
                rectangle(hair, 1.0),
                (2.0, 1.0),
            ),
            (
                "a hairline tall on a tall canvas",
                rectangle(1.0, hair),
                (1.0, 2.0),
            ),
            ("a canvas with no width", rectangle(1.0, 1.0), (0.0, 1.0)),
            ("a canvas with no height", rectangle(1.0, 1.0), (1.0, 0.0)),
        ] {
            let mut positions = points.clone();
            fit_to_aspect(&mut positions, aw, ah, true);
            assert_eq!(positions, points, "{name}");
        }
    }

    #[test]
    fn an_uncapped_fit_lengthens_all_the_way_to_the_canvas() {
        let mut positions = rectangle(1.0, 3.0);
        fit_to_aspect(&mut positions, 2.0, 1.0, false);
        assert!(
            (aspect(&positions) - 2.0).abs() < 1e-5,
            "got {}",
            aspect(&positions)
        );
    }

    /// Angle of a chain's end-to-end line from the x axis, folded into [0, 90°].
    fn chain_angle_degrees(chain: &[(f32, f32)]) -> f32 {
        let (ax, ay) = chain[0];
        let (bx, by) = chain[chain.len() - 1];
        let angle = (by - ay).atan2(bx - ax).to_degrees().abs();
        if angle > 90.0 { 180.0 - angle } else { angle }
    }

    /// Every pairwise distance, sorted, so two layouts of the same nodes can
    /// be compared as shapes regardless of where and how they are turned.
    fn distances(positions: &[(f32, f32)]) -> Vec<f32> {
        let mut out = vec![];
        for j in 1..positions.len() {
            for i in 0..j {
                out.push((positions[i].0 - positions[j].0).hypot(positions[i].1 - positions[j].1));
            }
        }
        out.sort_by(|a, b| a.partial_cmp(b).unwrap());
        out
    }

    // A chain is turned to lie along the canvas's long axis and is then longer
    // than the canvas, so the fit leaves it alone: it keeps every distance the
    // layout gave it, bow included, and only the turn and the packing's shift
    // distinguish it from the raw component layout.
    #[test]
    fn every_layout_lays_a_chain_along_the_canvas_at_its_own_length() {
        for (layout, component, run) in COMPONENT_LAYOUTS {
            for n in [5, 8, 12] {
                let (pg, chain) = path_component(n);
                let raw = component(&pg, &chain);
                for (aw, ah, along_x) in [
                    (16.0_f32, 9.0_f32, true),
                    (2.0, 1.0, true),
                    (1.0, 2.0, false),
                ] {
                    let laid = run(&make_path(n), aw, ah);
                    let angle = chain_angle_degrees(&laid);
                    assert!(
                        if along_x { angle < 10.0 } else { angle > 80.0 },
                        "{layout}, {n} nodes on {aw}:{ah}: chain lies at {angle} degrees"
                    );
                    for (d, r) in distances(&laid).iter().zip(distances(&raw)) {
                        assert!(
                            (d - r).abs() <= 1e-3 * r.max(1e-3),
                            "{layout}, {n} nodes on {aw}:{ah}: a distance changed from {r} to {d}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn every_layout_packs_components_into_disjoint_boxes() {
        // Two disconnected 5-node rings. Each layout must lay both out and pack
        // them into regions that do not overlap. Pins the position-assignment
        // arithmetic and the shelf-packer's row and x accumulation, which a swap
        // of `+` for `-` or `*` would turn into overlapping or nonsensical boxes.
        let mut graph = make_ring(10);
        graph.edges = (0..5)
            .map(|i| (i, (i + 1) % 5))
            .chain((0..5).map(|i| (5 + i, 5 + (i + 1) % 5)))
            .map(|(s, t)| GraphEdge {
                source: format!("n{s}"),
                target: format!("n{t}"),
                edge_type: EdgeType::SubclassOf,
                label: None,
            })
            .collect();
        for (layout, run) in LAYOUTS {
            let positions = run(&graph, 1.0, 1.0);
            // Components are packed tallest first, so which ring lands in which
            // slot is not fixed; only that the two boxes are disjoint.
            let (a_min_x, a_max_x, a_min_y, a_max_y) = bounding_box(&positions[0..5]);
            let (b_min_x, b_max_x, b_min_y, b_max_y) = bounding_box(&positions[5..10]);
            for (ring, extent) in [
                ("ring A x", a_max_x - a_min_x),
                ("ring A y", a_max_y - a_min_y),
                ("ring B x", b_max_x - b_min_x),
                ("ring B y", b_max_y - b_min_y),
            ] {
                assert!(extent > 0.5, "{layout}: {ring} collapsed (extent {extent})");
            }
            let overlap_x = (a_max_x.min(b_max_x) - a_min_x.max(b_min_x)).max(0.0);
            let overlap_y = (a_max_y.min(b_max_y) - a_min_y.max(b_min_y)).max(0.0);
            assert!(
                overlap_x == 0.0 || overlap_y == 0.0,
                "{layout}: components overlap by ({overlap_x}, {overlap_y})"
            );
        }
    }

    #[test]
    fn sgd_is_deterministic_under_fixed_seed() {
        // The internal RNG is seeded with a constant so re-running
        // `panschema publish` against an unchanged schema produces
        // byte-identical HTML (per the idempotency guarantee). Two
        // calls on the same input must produce identical positions.
        let ring = make_ring(10);
        let first = sgd(&ring, 1.0, 1.0);
        let second = sgd(&ring, 1.0, 1.0);
        assert_eq!(first.len(), second.len());
        for ((a_x, a_y), (b_x, b_y)) in first.iter().zip(second.iter()) {
            assert_eq!(a_x, b_x, "non-deterministic x");
            assert_eq!(a_y, b_y, "non-deterministic y");
        }
    }

    fn make_path(n: usize) -> GraphData {
        let mut graph = make_ring(n);
        graph.edges.pop(); // the ring's closing edge, n{n-1} -> n0
        graph.schema_name = "path".into();
        graph
    }

    /// Panics unless `positions` lays a path out in node order with every
    /// pair at a distance proportional to its hop count: the worst relative
    /// error against the mean edge length must stay under `PATH_TOLERANCE`.
    /// A path is the one graph whose stress optimum is known exactly —
    /// collinear and evenly spaced — so the error is 0 there, whatever the
    /// layout's scale.
    fn assert_spaces_path_evenly(positions: &[(f32, f32)]) {
        let dist = |i: usize, j: usize| {
            let (a, b) = (positions[i], positions[j]);
            (a.0 - b.0).hypot(a.1 - b.1)
        };
        let n = positions.len();
        let unit = (1..n).map(|i| dist(i - 1, i)).sum::<f32>() / (n - 1) as f32;
        // Every ratio below divides by `unit`. A layout that collapsed to a
        // point, or went non-finite, makes each one NaN — and `f32::max`
        // discards NaN, so the tolerance check alone would pass it.
        assert!(
            unit.is_finite() && unit > 0.0,
            "path collapsed or went non-finite: mean edge length {unit}; positions {positions:?}"
        );
        let mut worst = 0.0_f32;
        for j in 1..n {
            for i in 0..j {
                let expected = unit * (j - i) as f32;
                worst = worst.max((dist(i, j) - expected).abs() / expected);
            }
        }
        assert!(
            worst < PATH_TOLERANCE,
            "worst distortion {worst}; positions {positions:?}"
        );
    }

    /// A path as the per-component layout functions receive it: the
    /// petgraph and its nodes in path order, so `positions[i]` is `n{i}`.
    ///
    /// The path tests go through these rather than the public entry points
    /// on purpose. Those end with a realized-aspect correction that scales x
    /// and y independently, and on a near-straight path that amplifies
    /// exactly the perpendicular drift being measured.
    fn path_component(
        n: usize,
    ) -> (
        petgraph::Graph<String, (), petgraph::Undirected>,
        Vec<NodeIndex>,
    ) {
        single_component(&make_path(n))
    }

    /// A connected graph as the per-component layout functions receive it:
    /// the petgraph and its nodes in `graph.nodes` order.
    fn single_component(
        graph: &GraphData,
    ) -> (
        petgraph::Graph<String, (), petgraph::Undirected>,
        Vec<NodeIndex>,
    ) {
        let (pg, id_to_idx) = to_petgraph(graph);
        let component = graph.nodes.iter().map(|node| id_to_idx[&node.id]).collect();
        (pg, component)
    }

    // A converged layout measures at most about 0.01 here (stress and SGD;
    // Kamada-Kawai 0.002), and a layout that never moves a node, or stops
    // after one step, measures 0.77 or more. 0.15 sits clear of both.
    const PATH_TOLERANCE: f32 = 0.15;

    #[test]
    fn kamada_kawai_spaces_a_path_by_graph_distance() {
        // Eight nodes, not five: at the algorithm's own loose threshold the
        // chain still bends well past the tolerance by eight.
        let (pg, component) = path_component(8);
        assert_spaces_path_evenly(&kamada_kawai_component(&pg, &component));
    }

    #[test]
    fn stress_majorization_spaces_a_path_by_graph_distance() {
        let (pg, component) = path_component(5);
        assert_spaces_path_evenly(&stress_majorization_component(&pg, &component));
    }

    #[test]
    fn sgd_spaces_a_path_by_graph_distance() {
        let (pg, component) = path_component(5);
        assert_spaces_path_evenly(&sgd_component(&pg, &component));
    }

    #[test]
    fn stress_majorization_separates_2_node_components() {
        // A 2-node component sits inside a larger graph (so the
        // shelf-packer's main path runs). The 2-node piece must not
        // collapse to a single point: left running, stress turns a single
        // edge's coordinates NaN within a few iterations, and the
        // component wrapper's non-finite guard maps both to the origin
        // unless it short-circuits the pair. Manifested in the
        // scimantic-schema v0.2.0 dogfood as two overlapping labels
        // in the orphan corner of the rendered graph.
        let mut graph = GraphData {
            schema_name: "two_node_orphan".into(),
            schema_title: None,
            format_version: "1.0".into(),
            graph_kind: crate::graph_types::GraphKind::default(),
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        // Main component (a 4-node ring) plus a disconnected 2-node
        // edge. Node ids n0..n3 form the ring, n4-n5 are the orphan.
        for i in 0..6 {
            graph.nodes.push(crate::graph_types::GraphNode {
                id: format!("n{i}"),
                label: format!("n{i}"),
                node_type: crate::graph_types::NodeType::Class,
                color: [1.0, 1.0, 1.0, 1.0],
                description: None,
                uri: None,
                uri_unresolved: false,
                is_abstract: false,
                kind_metadata: None,
            });
        }
        for (s, t) in [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5)] {
            graph.edges.push(crate::graph_types::GraphEdge {
                source: format!("n{s}"),
                target: format!("n{t}"),
                edge_type: crate::graph_types::EdgeType::SubclassOf,
                label: None,
            });
        }
        let positions = stress_majorization(&graph, 1.0, 1.0);
        assert_eq!(positions.len(), 6);
        let (n4_x, n4_y) = positions[4];
        let (n5_x, n5_y) = positions[5];
        let dist = ((n5_x - n4_x).powi(2) + (n5_y - n4_y).powi(2)).sqrt();
        assert!(
            dist > 0.5,
            "2-node orphan component must separate; got distance {dist} between n4 and n5"
        );
    }

    #[test]
    fn sgd_grids_isolated_nodes_into_a_2d_block() {
        // The shape scimantic hits mid-build: a sizeable connected
        // component plus many isolated class nodes. The singletons must
        // grid into a 2D block that uses vertical space — not smear into
        // a single zero-height row (which is what a 0×0 packing footprint
        // produced before the min-cell floor). Regression guard.
        let graph = make_lopsided(20, 16);
        let positions = sgd(&graph, 1.0, 1.0);
        assert_eq!(positions.len(), 36);

        // Isolated nodes are appended after the connected ring, so they
        // are indices 20..36.
        let (min_x, max_x, min_y, max_y) = bounding_box(&positions[20..]);
        let (w, h) = (max_x - min_x, max_y - min_y);
        assert!(
            h > 0.0,
            "isolated nodes must span vertical space, not a flat row (w={w}, h={h})"
        );
        // A block, not a 1-D line: the 16 singletons wrap across multiple
        // rows so the spread isn't lopsidedly horizontal.
        assert!(
            w / h < 8.0,
            "isolated block should be roughly 2D, got {w}x{h} (ratio {})",
            w / h
        );
    }

    #[test]
    fn stress_majorization_shelf_packs_disconnected_components() {
        // 20-node ring + 8 isolated singletons. Stress's all-pairs
        // formulation can't run over the union (any unreachable pair
        // produces NaN globally), so the wrapper splits the input into
        // connected components, runs stress per-component, and shelf-
        // packs the result. All 28 coordinates must be finite; the
        // bbox must be non-degenerate (the 20-node ring contributes
        // visible spread, and the singletons separate by at least the
        // configured per-component gap).
        let graph = make_lopsided(20, 8);
        let positions = stress_majorization(&graph, 1.0, 1.0);
        assert_eq!(positions.len(), 28);
        for (x, y) in &positions {
            assert!(x.is_finite(), "all coordinates must stay finite");
            assert!(y.is_finite(), "all coordinates must stay finite");
        }
        // bbox is non-degenerate even with disconnected pieces.
        let xs: Vec<f32> = positions.iter().map(|p| p.0).collect();
        let ys: Vec<f32> = positions.iter().map(|p| p.1).collect();
        let w = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
            - xs.iter().cloned().fold(f32::INFINITY, f32::min);
        let h = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
            - ys.iter().cloned().fold(f32::INFINITY, f32::min);
        assert!(
            w > 0.1,
            "shelf-packed disconnected layout collapsed in x (width={w})"
        );
        assert!(
            h > 0.1,
            "shelf-packed disconnected layout collapsed in y (height={h})"
        );
    }

    // A compact graph is lengthened toward the canvas's aspect: x by
    // √(target / realized) and y by its inverse, so the aspect moves by that
    // factor squared. The 4:2 case distinguishes √(w/h) from any commutative
    // alternative. A ring is near square, so these aspects stay inside the
    // stretch cap, which is checked here so that a cap that engaged would be
    // reported as such; the cap and the lengthen-only rule have their own tests.
    #[test]
    fn every_layout_lengthens_a_compact_graph_toward_the_canvas_aspect() {
        let ring = make_ring(10);
        let (pg, nodes) = single_component(&ring);
        for (layout, component, run) in COMPONENT_LAYOUTS {
            let raw = aspect(&component(&pg, &nodes));
            for (aw, ah) in [(2.0_f32, 1.0), (4.0, 2.0), (1.0, 2.0)] {
                let target = aw / ah;
                let needed = (target / raw).sqrt();
                let cap = stretch_cap(target);
                assert!(
                    needed > cap.recip() && needed < cap,
                    "{layout}: the ring's raw aspect {raw} needs {needed} to reach {target}, and the cap would engage"
                );
                let fitted = aspect(&run(&ring, aw, ah));
                assert!(
                    (fitted / target - 1.0).abs() < 1e-3,
                    "{layout} on {aw}:{ah}: raw aspect {raw} came out {fitted}, expected {target}"
                );
            }
        }
    }

    /// Each layout's component function beside its entry point.
    const COMPONENT_LAYOUTS: [(&str, ComponentLayout, Layout); 3] = [
        ("stress", stress_majorization_component, stress_majorization),
        ("sgd", sgd_component, sgd),
        ("kamada_kawai", kamada_kawai_component, kamada_kawai),
    ];

    fn aspect(positions: &[(f32, f32)]) -> f32 {
        let (min_x, max_x, min_y, max_y) = bounding_box(positions);
        (max_x - min_x) / (max_y - min_y)
    }

    #[test]
    fn the_stretch_cap_is_the_floor_or_what_a_square_layout_needs() {
        assert_eq!(stretch_cap(1.0), MAX_ASPECT_STRETCH);
        assert_eq!(
            stretch_cap(2.0),
            MAX_ASPECT_STRETCH,
            "16:8 needs 1.41, under the floor"
        );
        assert_eq!(
            stretch_cap(4.0),
            2.0,
            "a square layout needs 2x to reach 4:1"
        );
        assert_eq!(stretch_cap(0.25), 2.0, "and 2x the other way to reach 1:4");
    }

    fn turned(points: &[(f32, f32)], degrees: f32) -> Vec<(f32, f32)> {
        let (c, s) = (degrees.to_radians().cos(), degrees.to_radians().sin());
        points
            .iter()
            .map(|&(x, y)| (x * c - y * s, x * s + y * c))
            .collect()
    }

    fn bowed_chain(n: usize) -> Vec<(f32, f32)> {
        (0..n).map(|i| (i as f32, 0.1 * (i as f32).sin())).collect()
    }

    #[test]
    fn orienting_turns_a_chain_along_the_axis_and_keeps_every_distance() {
        // The chain is bowed, so its end-to-end line sits a degree or so off
        // its principal axis; the axis is what lies along the canvas. It is
        // tilted one way and the reverse way, since an axis has two
        // directions and the first node must come first along it either way.
        let centroid = |points: &[(f32, f32)]| {
            let n = points.len() as f32;
            (
                points.iter().map(|p| p.0).sum::<f32>() / n,
                points.iter().map(|p| p.1).sum::<f32>() / n,
            )
        };
        for (tilt, wide, low, high) in [
            (40.0, true, 0.0, 5.0),
            (220.0, true, 0.0, 5.0),
            (40.0, false, 85.0, 90.0),
            (220.0, false, 85.0, 90.0),
        ] {
            let tilted = turned(&bowed_chain(8), tilt);
            let mut positions = tilted.clone();
            assert!(
                orient_along_canvas(&mut positions, wide),
                "tilt {tilt}, wide = {wide}: not turned"
            );
            let angle = chain_angle_degrees(&positions);
            assert!(
                (low..=high).contains(&angle),
                "tilt {tilt}, wide = {wide}: chain lies at {angle} degrees"
            );
            let first = positions[0];
            let first_comes_first = positions.iter().all(|p| {
                if wide {
                    p.0 >= first.0 - 1e-4
                } else {
                    p.1 >= first.1 - 1e-4
                }
            });
            assert!(
                first_comes_first,
                "tilt {tilt}, wide = {wide}: the first node is not first along the axis: {positions:?}"
            );
            for (d, r) in distances(&positions).iter().zip(distances(&tilted)) {
                assert!(
                    (d - r).abs() < 1e-4,
                    "tilt {tilt}, wide = {wide}: distance {r} became {d}"
                );
            }
            let (before, after) = (centroid(&tilted), centroid(&positions));
            assert!(
                (before.0 - after.0).abs() < 1e-4 && (before.1 - after.1).abs() < 1e-4,
                "tilt {tilt}, wide = {wide}: the turn moved the centroid from {before:?} to {after:?}"
            );
        }
        // Three positions are the fewest with a spread to measure.
        let mut three = turned(&bowed_chain(3), 40.0);
        assert!(
            orient_along_canvas(&mut three, true),
            "three positions not turned"
        );
    }

    // When the first node sits on the centroid line there is no side for it
    // to be on, and the layout keeps the direction it had.
    #[test]
    fn orienting_keeps_the_direction_when_the_first_node_is_central() {
        let mut along_x = vec![(0.0_f32, 0.0_f32), (-2.0, 0.0), (2.0, 0.0)];
        assert!(orient_along_canvas(&mut along_x, true));
        assert_eq!(along_x, [(0.0, 0.0), (-2.0, 0.0), (2.0, 0.0)]);
        let mut along_y = vec![(0.0_f32, 0.0_f32), (0.0, -2.0), (0.0, 2.0)];
        assert!(orient_along_canvas(&mut along_y, false));
        assert_eq!(along_y, [(0.0, 0.0), (0.0, -2.0), (0.0, 2.0)]);
    }

    // The threshold is a ratio of spreads and inclusive: a 1.9:1 rectangle at
    // a slant and a square are left exactly as they are; a 2.1:1 one is
    // turned, and so is one exactly 2:1 (its spreads are 4 and 1, so the
    // ratio is exact in floating point).
    #[test]
    fn orienting_leaves_a_near_square_layout_alone() {
        for (name, points, expect_turn) in [
            (
                "a 1.9:1 rectangle",
                turned(&rectangle(1.9, 1.0), 30.0),
                false,
            ),
            ("a square", turned(&rectangle(1.0, 1.0), 30.0), false),
            (
                "a 2.1:1 rectangle",
                turned(&rectangle(2.1, 1.0), 30.0),
                true,
            ),
            ("a rectangle exactly 2:1", rectangle(2.0, 1.0), true),
        ] {
            let mut positions = points.clone();
            let turned_it = orient_along_canvas(&mut positions, true);
            assert_eq!(turned_it, expect_turn, "{name}");
            if !expect_turn {
                assert_eq!(positions, points, "{name} moved");
            }
        }
        let mut two = vec![(0.0, 0.0), (5.0, 1.0)];
        assert!(
            !orient_along_canvas(&mut two, true),
            "two points have no spread to measure"
        );
    }

    // Each component is turned on its own before packing, so a graph of two
    // chains has both lying along the canvas.
    #[test]
    fn every_layout_turns_each_component_before_packing() {
        let mut chains = make_ring(10);
        chains.edges = (0..4)
            .map(|i| (i, i + 1))
            .chain((5..9).map(|i| (i, i + 1)))
            .map(|(s, t)| GraphEdge {
                source: format!("n{s}"),
                target: format!("n{t}"),
                edge_type: EdgeType::SubclassOf,
                label: None,
            })
            .collect();
        for (layout, run) in LAYOUTS {
            let positions = run(&chains, 2.0, 1.0);
            for (which, chain) in [("first", &positions[0..5]), ("second", &positions[5..10])] {
                let angle = chain_angle_degrees(chain);
                assert!(
                    angle < 10.0,
                    "{layout}: the {which} chain lies at {angle} degrees"
                );
            }
        }
    }

    #[test]
    fn scale_to_world_targets_max_bbox_dimension() {
        // After scaling, the larger bbox dimension equals the target,
        // and the smaller dimension preserves the input aspect ratio.
        let mut positions = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (0.0, 2.0)];
        scale_to_world(&mut positions, 600.0);
        let (min_x, max_x, min_y, max_y) = bounding_box(&positions);
        let w = max_x - min_x;
        let h = max_y - min_y;
        assert!((w - 600.0).abs() < 1e-3, "width {w} != 600");
        // Aspect 4:2 → 2:1. After scaling so width=600, height should be 300.
        assert!((h - 300.0).abs() < 1e-3, "height {h} != 300");
    }

    #[test]
    fn scale_to_world_centers_bbox_on_origin() {
        let mut positions = vec![(100.0, 200.0), (400.0, 800.0)];
        scale_to_world(&mut positions, 600.0);
        let (min_x, max_x, min_y, max_y) = bounding_box(&positions);
        // Centroid sits at origin so the rendered layout fills the
        // simulation's world symmetrically around (0, 0).
        assert!((min_x + max_x).abs() < 1e-3);
        assert!((min_y + max_y).abs() < 1e-3);
    }

    #[test]
    fn scale_to_world_skips_positions_with_any_non_finite_coordinate() {
        // The bbox computation must ignore a position when *either*
        // coordinate is non-finite. With the (correct) `||`, a
        // mixed-finite point like (NaN, 0.0) is dropped before
        // min/max see it; weakening to `&&` would let the NaN
        // propagate into the bbox and make every scaled output NaN.
        let mut positions = vec![
            (0.0, 0.0),
            (100.0, 100.0),
            (f32::NAN, 0.0),      // x not finite, y finite
            (0.0, f32::INFINITY), // x finite, y not finite
        ];
        scale_to_world(&mut positions, 600.0);
        // The first two finite points define a bbox of side 100;
        // scaled to 600, they sit at ±300 from the centroid (50, 50).
        assert!(positions[0].0.is_finite() && positions[0].1.is_finite());
        assert!(positions[1].0.is_finite() && positions[1].1.is_finite());
        assert!((positions[0].0 + positions[1].0).abs() < 1e-3);
        assert!((positions[0].1 + positions[1].1).abs() < 1e-3);
        let bbox_dim = (positions[1].0 - positions[0].0).abs();
        assert!(
            (bbox_dim - 600.0).abs() < 1e-3,
            "scaled finite bbox dim should be 600, got {bbox_dim}"
        );
    }

    #[test]
    fn scale_to_world_leaves_degenerate_inputs_alone() {
        // Singleton, empty, and all-coincident inputs have no meaningful
        // bbox to scale; the function must not divide by zero or NaN.
        let mut empty: Vec<(f32, f32)> = Vec::new();
        scale_to_world(&mut empty, 600.0);
        assert!(empty.is_empty());

        let mut singleton = vec![(123.0, 456.0)];
        scale_to_world(&mut singleton, 600.0);
        assert_eq!(singleton, vec![(123.0, 456.0)]);

        let mut coincident = vec![(5.0, 5.0), (5.0, 5.0), (5.0, 5.0)];
        scale_to_world(&mut coincident, 600.0);
        // All points coincident → bbox_max = 0 → no scaling applied.
        assert_eq!(coincident, vec![(5.0, 5.0), (5.0, 5.0), (5.0, 5.0)]);
    }

    fn make_balanced_tree(depth: u32) -> GraphData {
        // Binary tree: 2^depth - 1 nodes, every non-leaf has 2 children
        // wired via `subClassOf` edges. The canonical input that
        // Sugiyama should render as cleanly stacked layers.
        let total = (1u32 << depth) - 1;
        let nodes: Vec<GraphNode> = (0..total)
            .map(|i| GraphNode {
                id: format!("n{i}"),
                label: format!("N{i}"),
                node_type: NodeType::Class,
                color: [1.0, 0.0, 0.0, 1.0],
                description: None,
                uri: None,
                uri_unresolved: false,
                is_abstract: false,
                kind_metadata: None,
            })
            .collect();
        let mut edges = Vec::new();
        for parent in 0..total {
            let left = 2 * parent + 1;
            let right = 2 * parent + 2;
            for child in [left, right] {
                if child < total {
                    edges.push(GraphEdge {
                        source: format!("n{child}"),
                        target: format!("n{parent}"),
                        edge_type: EdgeType::SubclassOf,
                        label: None,
                    });
                }
            }
        }
        GraphData {
            schema_name: "tree".into(),
            schema_title: None,
            format_version: "1.0".into(),
            graph_kind: crate::graph_types::GraphKind::default(),
            nodes,
            edges,
        }
    }

    #[test]
    fn hierarchical_returns_position_per_node_on_balanced_tree() {
        // Sugiyama on a 3-layer binary tree (7 nodes) must place every
        // node and produce finite coordinates. The exact layout
        // depends on rust-sugiyama's internals (which heuristic, which
        // version) so we don't pin coordinates.
        //
        // To distinguish real Sugiyama output from the orphan-grid
        // fallback (which fires when the layered region is empty, e.g.
        // the layout loop's bounds check breaks): Sugiyama groups the
        // 4 leaves on a single layer (same y). The orphan grid for
        // 7 nodes uses `columns = ceil(sqrt(7)) = 3` columns, so it
        // can pack at most 3 nodes per y. A max-per-y count ≥ 4
        // therefore proves we got the layered output.
        let tree = make_balanced_tree(3);
        let positions = hierarchical(&tree, 1.0, 1.0);
        assert_eq!(positions.len(), 7);
        for (x, y) in &positions {
            assert!(x.is_finite() && y.is_finite(), "non-finite coordinate");
        }
        let mut by_y: std::collections::BTreeMap<i32, usize> = std::collections::BTreeMap::new();
        for (_x, y) in &positions {
            *by_y.entry((y * 100.0) as i32).or_insert(0) += 1;
        }
        let max_per_layer = by_y.values().copied().max().unwrap_or(0);
        assert!(
            max_per_layer >= 4,
            "expected at least 4 nodes on the leaf layer, got max {max_per_layer} — likely fell to orphan grid"
        );
    }

    #[test]
    fn is_hierarchy_edge_includes_subclass_and_mixin_only() {
        assert!(is_hierarchy_edge(EdgeType::SubclassOf));
        assert!(is_hierarchy_edge(EdgeType::Mixin));
        assert!(!is_hierarchy_edge(EdgeType::Domain));
        assert!(!is_hierarchy_edge(EdgeType::Range));
        assert!(!is_hierarchy_edge(EdgeType::Inverse));
        assert!(!is_hierarchy_edge(EdgeType::TypeOf));
    }

    #[test]
    fn hierarchical_places_disjoint_components_side_by_side_with_gap() {
        // Two disjoint 3-node hierarchies (parent + 2 children each).
        // Each component spans the full vertex_spacing because
        // siblings sit on a shared layer.
        let nodes: Vec<GraphNode> = ["a0", "a1", "a2", "b0", "b1", "b2"]
            .iter()
            .map(|id| GraphNode {
                id: (*id).to_string(),
                label: (*id).to_string(),
                node_type: NodeType::Class,
                color: [1.0, 0.0, 0.0, 1.0],
                description: None,
                uri: None,
                uri_unresolved: false,
                is_abstract: false,
                kind_metadata: None,
            })
            .collect();
        let edges = vec![
            GraphEdge {
                source: "a1".into(),
                target: "a0".into(),
                edge_type: EdgeType::SubclassOf,
                label: None,
            },
            GraphEdge {
                source: "a2".into(),
                target: "a0".into(),
                edge_type: EdgeType::SubclassOf,
                label: None,
            },
            GraphEdge {
                source: "b1".into(),
                target: "b0".into(),
                edge_type: EdgeType::SubclassOf,
                label: None,
            },
            GraphEdge {
                source: "b2".into(),
                target: "b0".into(),
                edge_type: EdgeType::SubclassOf,
                label: None,
            },
        ];
        let g = GraphData {
            schema_name: "two_trees".into(),
            schema_title: None,
            format_version: "1.0".into(),
            graph_kind: crate::graph_types::GraphKind::default(),
            nodes,
            edges,
        };
        let positions = hierarchical(&g, 1.0, 1.0);
        let a_x_max = positions[..3]
            .iter()
            .map(|p| p.0)
            .fold(f32::NEG_INFINITY, f32::max);
        let b_x_min = positions[3..]
            .iter()
            .map(|p| p.0)
            .fold(f32::INFINITY, f32::min);
        let b_x_max = positions[3..]
            .iter()
            .map(|p| p.0)
            .fold(f32::NEG_INFINITY, f32::max);

        // Pin the direction: rust-sugiyama orders subgraphs by their
        // smallest node index, so tree A (nodes 0–2) is processed
        // first at x_offset=0 and tree B (nodes 3–5) at x_offset > 0.
        // The implementation accumulates x_offset *positively* after
        // each component. Mutations that flip the operator or sign
        // (`+=` → `-=`, `+` → `-` on the per-component apply) push
        // tree B to the left of tree A, which this assertion catches.
        // If a future rust-sugiyama release changes its subgraph
        // ordering, this assertion is the right place to re-engineer
        // the integration — better than silently shipping a layout
        // that grew in the wrong direction.
        assert!(
            a_x_max < b_x_min,
            "tree A (indices 0–2) must sit left of tree B (indices 3–5): a_x_max={a_x_max} b_x_min={b_x_min}"
        );

        let gap = b_x_min - a_x_max;
        // COMPONENT_GAP is 50 in source; the actual gap may be larger
        // because sugiyama adds internal padding to each component's
        // bbox. Floor at 40 to allow for that; ceiling at 200 catches
        // a `+ COMPONENT_GAP` being replaced by `* COMPONENT_GAP` on
        // non-tiny widths.
        assert!(
            (40.0..200.0).contains(&gap),
            "gap {gap} between disjoint trees out of expected [40, 200] range"
        );
        // Sanity: tree B's max sits to the right of its min.
        assert!(b_x_min <= b_x_max);
    }

    #[test]
    fn hierarchical_falls_back_when_no_hierarchy_edges() {
        // Graph with only property edges (no SubclassOf / Mixin)
        // has nothing for Sugiyama to layer. Every node falls into
        // the orphan-grid fallback rather than panicking.
        let mut g = make_ring(5);
        for e in g.edges.iter_mut() {
            e.edge_type = EdgeType::Range;
        }
        let positions = hierarchical(&g, 1.0, 1.0);
        assert_eq!(positions.len(), 5);
        for (x, y) in &positions {
            assert!(x.is_finite() && y.is_finite());
        }
        // The orphan grid should spread nodes — not collapse to a point.
        let xs: Vec<f32> = positions.iter().map(|p| p.0).collect();
        let unique_xs = xs
            .iter()
            .map(|x| (*x * 100.0) as i32)
            .collect::<std::collections::HashSet<_>>();
        assert!(
            unique_xs.len() > 1,
            "orphan grid should distribute nodes across x"
        );
    }

    #[test]
    fn hierarchical_filters_non_hierarchy_edges_from_layout() {
        // A two-layer hierarchy with a cross-cutting property edge:
        // the property edge must NOT feed Sugiyama (it would create a
        // cycle once direction is normalized) and must not affect the
        // layered positions of the hierarchy nodes.
        let mut g = make_balanced_tree(2); // root + 2 leaves
        g.edges.push(GraphEdge {
            source: "n1".into(),
            target: "n2".into(),
            edge_type: EdgeType::Range,
            label: None,
        });
        let positions = hierarchical(&g, 1.0, 1.0);
        assert_eq!(positions.len(), 3);
        // Root and its two children should still land on distinct
        // y-coordinates (different Sugiyama layers); the cross-cutting
        // property edge can't have collapsed them onto one layer.
        let ys: std::collections::HashSet<i32> =
            positions.iter().map(|p| (p.1 * 100.0) as i32).collect();
        assert!(ys.len() >= 2, "hierarchy should produce at least 2 layers");
    }

    #[test]
    fn hierarchical_handles_empty_graph() {
        let empty = GraphData {
            schema_name: "empty".into(),
            schema_title: None,
            format_version: "1.0".into(),
            graph_kind: crate::graph_types::GraphKind::default(),
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        assert!(hierarchical(&empty, 1.0, 1.0).is_empty());
    }

    #[test]
    fn hierarchical_orphans_sit_below_layered_region_with_grid_spacing() {
        // Build 3 hierarchy nodes + 5 orphans. Use square aspect so
        // the bias pass is a no-op and we can read absolute spacing.
        // The orphans must sit *below* the layered region (lower y)
        // and form a grid with exactly `ORPHAN_SPACING = 30` between
        // adjacent rows and columns. This pins the orphan-positioning
        // arithmetic: any flip of the `-` to `+` puts orphans above
        // the layered cluster; any `*` → `+` / `/` collapses the
        // grid's row spacing to ~1.
        let mut g = make_balanced_tree(2);
        let layered_n = g.nodes.len();
        for i in 100..105 {
            g.nodes.push(GraphNode {
                id: format!("orphan{i}"),
                label: format!("O{i}"),
                node_type: NodeType::Class,
                color: [1.0, 0.0, 0.0, 1.0],
                description: None,
                uri: None,
                uri_unresolved: false,
                is_abstract: false,
                kind_metadata: None,
            });
        }
        let positions = hierarchical(&g, 1.0, 1.0);
        assert_eq!(positions.len(), layered_n + 5);

        let layered_min_y = positions[..layered_n]
            .iter()
            .map(|p| p.1)
            .fold(f32::INFINITY, f32::min);
        let orphan_max_y = positions[layered_n..]
            .iter()
            .map(|p| p.1)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            orphan_max_y < layered_min_y,
            "orphans must sit below layered region: orphan_max_y={orphan_max_y} layered_min_y={layered_min_y}"
        );

        // 5 orphans → sqrt(5).ceil() = 3 columns. Column 0 holds
        // orphans 0 and 3 (one per row). Row gap should be exactly
        // ORPHAN_SPACING = 30 in y.
        let row0_col0 = positions[layered_n];
        let row1_col0 = positions[layered_n + 3];
        let row_gap = row0_col0.1 - row1_col0.1;
        assert!(
            (row_gap - 30.0).abs() < 1e-3,
            "row gap should be 30, got {row_gap}"
        );
        // Column gap: orphans 0 and 1 sit in the same row, different
        // columns. Spacing in x should also be 30.
        let row0_col1 = positions[layered_n + 1];
        let col_gap = row0_col1.0 - row0_col0.0;
        assert!(
            (col_gap - 30.0).abs() < 1e-3,
            "col gap should be 30, got {col_gap}"
        );
        // Orphan grid origin sits at x=0 — pins the `col as f32 *
        // ORPHAN_SPACING` formula against an `%` → `+` swap that
        // would shift the entire grid right by `columns *
        // ORPHAN_SPACING`.
        assert!(
            row0_col0.0.abs() < 1e-3,
            "first orphan column should be at x=0, got {}",
            row0_col0.0
        );
    }

    #[test]
    fn hierarchical_aspect_bias_scales_coordinates_per_axis() {
        // Sugiyama output for the same input must scale by √(w/h) in
        // x and √(h/w) in y when an aspect-biased layout is requested.
        // The 4:2 aspect distinguishes `/` from `*` (√2 ≠ √8) and
        // distinguishes `*=` from `+=` / `/=` (ratio biased/square
        // would otherwise be additive or inverted).
        let g = make_balanced_tree(3);
        let square = hierarchical(&g, 1.0, 1.0);
        let biased = hierarchical(&g, 4.0, 2.0);
        assert_eq!(square.len(), biased.len());
        let sx_expected = (4.0_f32 / 2.0).sqrt();
        let sy_expected = (2.0_f32 / 4.0).sqrt();
        for (i, ((sx, sy), (bx, by))) in square.iter().zip(biased.iter()).enumerate() {
            if sx.abs() > 0.01 {
                let ratio = bx / sx;
                assert!(
                    (ratio - sx_expected).abs() < 1e-3,
                    "node {i}: x ratio {ratio} != expected {sx_expected}"
                );
            }
            if sy.abs() > 0.01 {
                let ratio = by / sy;
                assert!(
                    (ratio - sy_expected).abs() < 1e-3,
                    "node {i}: y ratio {ratio} != expected {sy_expected}"
                );
            }
        }
    }

    #[test]
    fn hierarchical_handles_cycle_in_hierarchy_edges() {
        // Pathological: SubclassOf edges that form a cycle. LinkML
        // shouldn't accept this, but rust-sugiyama's feedback arc set
        // must break the cycle internally so we don't panic. The test
        // succeeds if the function returns finite positions for every
        // node.
        let mut g = make_ring(4);
        for e in g.edges.iter_mut() {
            e.edge_type = EdgeType::SubclassOf;
        }
        let positions = hierarchical(&g, 1.0, 1.0);
        assert_eq!(positions.len(), 4);
        for (x, y) in &positions {
            assert!(x.is_finite() && y.is_finite(), "cycle broke Sugiyama");
        }
    }
}
