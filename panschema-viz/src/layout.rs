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
//! [`petgraph`] graphs consumed by `egraph-rs`-backed layout
//! algorithms ([`to_petgraph`]), plus the
//! Kamada-Kawai pilot helper ([`kamada_kawai`]) that proves the
//! integration end-to-end.

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
    /// Sugiyama-style layered layout for `is_a` / `subClassOf` DAGs.
    /// Planned implementation: `rust-sugiyama`.
    Hierarchical,
    /// Stress majorization. Planned implementation: `egraph-rs`.
    Stress,
    /// Kamada-Kawai energy minimization. Planned implementation:
    /// `egraph-rs`.
    KamadaKawai,
    /// Stochastic Gradient Descent. Planned implementation: `egraph-rs`.
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
            Self::ForceDirected | Self::KamadaKawai | Self::Hierarchical
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
/// undirected [`petgraph::Graph`] suitable for `egraph-rs`-backed
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

/// Run Kamada-Kawai energy-minimization via
/// `petgraph-layout-kamada-kawai` and return positions in the
/// original [`GraphData`] node order.
///
/// Applies an aspect-bias post-process so the rendered bounding box
/// approximates `aspect_w : aspect_h` while preserving area: `x` is
/// scaled by √(w/h), `y` by √(h/w). Disconnected components carry
/// the algorithm's native placement, which may overlap; cluster
/// separation for disconnected graphs is the caller's concern.
///
/// Empty input returns an empty `Vec`. Coordinates that the
/// algorithm leaves unset (e.g. nodes the algorithm couldn't place)
/// fall back to `(0.0, 0.0)`.
pub fn kamada_kawai(graph: &GraphData, aspect_w: f32, aspect_h: f32) -> Vec<(f32, f32)> {
    use petgraph_drawing::DrawingEuclidean2d;
    use petgraph_layout_kamada_kawai::KamadaKawai;

    if graph.nodes.is_empty() {
        return Vec::new();
    }

    let (pg, id_to_idx) = to_petgraph(graph);
    let mut drawing = DrawingEuclidean2d::<NodeIndex, f32>::initial_placement(&pg);
    let kk = KamadaKawai::new(&pg, |_| 1.0_f32);
    kk.run(&mut drawing);

    let sx = (aspect_w / aspect_h).sqrt();
    let sy = (aspect_h / aspect_w).sqrt();

    graph
        .nodes
        .iter()
        .map(|n| {
            let idx = id_to_idx[&n.id];
            let x = drawing.x(idx).unwrap_or(0.0);
            let y = drawing.y(idx).unwrap_or(0.0);
            (x * sx, y * sy)
        })
        .collect()
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
/// coordinate system (typically O(1) magnitudes from `egraph-rs` or
/// `petgraph_drawing`) lands inside the visualization's expected world
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
/// `aspect_w` and `aspect_h` bias the final bbox toward that aspect
/// via the same √(w/h), √(h/w) post-process used by [`kamada_kawai`].
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
            if node_idx < graph.nodes.len() {
                positions[node_idx] = ((x + x_offset) as f32, y as f32);
                placed[node_idx] = true;
            }
        }
        x_offset += width + COMPONENT_GAP;
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
                | LayoutAlgorithm::Hierarchical => {
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

    fn make_ring(n: usize) -> GraphData {
        let nodes = (0..n)
            .map(|i| GraphNode {
                id: format!("n{i}"),
                label: format!("N{i}"),
                node_type: NodeType::Class,
                color: [1.0, 0.0, 0.0, 1.0],
                description: None,
                uri: None,
                is_abstract: false,
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
                is_abstract: false,
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

    #[test]
    fn kamada_kawai_returns_position_per_node_on_ring() {
        let ring = make_ring(15);
        let positions = kamada_kawai(&ring, 1.0, 1.0);
        assert_eq!(positions.len(), 15);
        for (x, y) in &positions {
            assert!(x.is_finite(), "x must be finite, got {x}");
            assert!(y.is_finite(), "y must be finite, got {y}");
        }
        // A 15-node ring with KK should not collapse to a single
        // point: the bbox must have non-zero width and height.
        let xs: Vec<f32> = positions.iter().map(|p| p.0).collect();
        let ys: Vec<f32> = positions.iter().map(|p| p.1).collect();
        let w = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
            - xs.iter().cloned().fold(f32::INFINITY, f32::min);
        let h = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
            - ys.iter().cloned().fold(f32::INFINITY, f32::min);
        assert!(w > 0.1, "ring layout collapsed in x (width={w})");
        assert!(h > 0.1, "ring layout collapsed in y (height={h})");
    }

    #[test]
    fn kamada_kawai_returns_position_per_node_on_lopsided_graph() {
        // 20 connected nodes in a ring + 8 isolated singletons.
        // The pilot must not panic on disconnected components and
        // must emit exactly one position per input node.
        let graph = make_lopsided(20, 8);
        let positions = kamada_kawai(&graph, 1.0, 1.0);
        assert_eq!(positions.len(), 28);
        for (x, y) in &positions {
            assert!(x.is_finite(), "x must be finite on disconnected input");
            assert!(y.is_finite(), "y must be finite on disconnected input");
        }
    }

    #[test]
    fn kamada_kawai_on_empty_graph_returns_empty_vec() {
        let empty = GraphData {
            schema_name: "empty".into(),
            schema_title: None,
            format_version: "1.0".into(),
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        assert!(kamada_kawai(&empty, 1.0, 1.0).is_empty());
    }

    #[test]
    fn kamada_kawai_aspect_bias_scales_coordinates() {
        // Aspect bias is a deterministic per-coordinate scaling, so
        // for any aspect (w, h) the ratio of the biased position to
        // the square (1, 1) position must be exactly √(w/h) in x and
        // √(h/w) in y. The 4:2 case is the load-bearing one: it
        // distinguishes the `/` formula from any commutative
        // alternative (`*`, `+`) — √(4/2)=√2 ≠ √(4*2)=√8.
        let ring = make_ring(10);
        let square = kamada_kawai(&ring, 1.0, 1.0);
        for (aw, ah) in [(2.0_f32, 1.0), (4.0, 2.0), (1.0, 3.0)] {
            let biased = kamada_kawai(&ring, aw, ah);
            assert_eq!(biased.len(), square.len());
            let sx_expected = (aw / ah).sqrt();
            let sy_expected = (ah / aw).sqrt();
            for (i, ((sx, sy), (bx, by))) in square.iter().zip(biased.iter()).enumerate() {
                if sx.abs() > 0.01 {
                    let ratio = bx / sx;
                    assert!(
                        (ratio - sx_expected).abs() < 1e-4,
                        "aspect {aw}:{ah} node {i}: x ratio {ratio} != expected {sx_expected}"
                    );
                }
                if sy.abs() > 0.01 {
                    let ratio = by / sy;
                    assert!(
                        (ratio - sy_expected).abs() < 1e-4,
                        "aspect {aw}:{ah} node {i}: y ratio {ratio} != expected {sy_expected}"
                    );
                }
            }
        }
    }

    fn bbox(positions: &[(f32, f32)]) -> (f32, f32, f32, f32) {
        let (mut min_x, mut max_x) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut min_y, mut max_y) = (f32::INFINITY, f32::NEG_INFINITY);
        for &(x, y) in positions {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
        (min_x, max_x, min_y, max_y)
    }

    #[test]
    fn scale_to_world_targets_max_bbox_dimension() {
        // After scaling, the larger bbox dimension equals the target,
        // and the smaller dimension preserves the input aspect ratio.
        let mut positions = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 2.0), (0.0, 2.0)];
        scale_to_world(&mut positions, 600.0);
        let (min_x, max_x, min_y, max_y) = bbox(&positions);
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
        let (min_x, max_x, min_y, max_y) = bbox(&positions);
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
                is_abstract: false,
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
            nodes,
            edges,
        }
    }

    #[test]
    fn hierarchical_returns_position_per_node_on_balanced_tree() {
        // Sugiyama on a 3-layer binary tree (7 nodes) must place every
        // node and produce finite coordinates. The exact layout
        // depends on rust-sugiyama's internals (which heuristic, which
        // version) so we don't pin coordinates — just emit-per-node
        // and finiteness.
        let tree = make_balanced_tree(3);
        let positions = hierarchical(&tree, 1.0, 1.0);
        assert_eq!(positions.len(), 7);
        for (x, y) in &positions {
            assert!(x.is_finite() && y.is_finite(), "non-finite coordinate");
        }
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
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        assert!(hierarchical(&empty, 1.0, 1.0).is_empty());
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

    #[test]
    fn kamada_kawai_scaled_bbox_is_non_degenerate_and_within_world_bounds() {
        // Picker integration requires KK output to land inside the
        // in-tree CpuSimulation's world (its hard MAX_RADIUS = 800).
        // After `scale_to_world(..., WORLD_TARGET_DIMENSION = 600.0)`
        // the bbox larger dimension must be exactly 600 and every
        // node's distance from origin must stay under MAX_RADIUS.
        for graph in [make_ring(15), make_lopsided(20, 8), make_ring(30)] {
            let mut positions = kamada_kawai(&graph, 1.0, 1.0);
            scale_to_world(&mut positions, WORLD_TARGET_DIMENSION);
            for &(x, y) in &positions {
                assert!(x.is_finite() && y.is_finite(), "non-finite coordinate");
                let r = (x * x + y * y).sqrt();
                assert!(
                    r < 800.0,
                    "node at radius {r} exceeds simulation MAX_RADIUS"
                );
            }
            let (min_x, max_x, min_y, max_y) = bbox(&positions);
            let w = max_x - min_x;
            let h = max_y - min_y;
            assert!(w >= 100.0, "scaled bbox width {w} is degenerate (< 100)");
            assert!(h >= 100.0, "scaled bbox height {h} is degenerate (< 100)");
            assert!(w.max(h) - WORLD_TARGET_DIMENSION < 1e-2);
        }
    }
}
