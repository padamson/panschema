//! Adaptive legend: which rows the notation key shows for a given graph.
//!
//! The legend explains symbols; a symbol the graph doesn't draw is noise
//! (and a schema with no enums shouldn't advertise the diamond). The spec
//! is computed from the *simulation* rather than the raw document because
//! rule participation (`in_rule`) is derived there once all nodes are
//! known — the same derivation that drives the amber rings.

use crate::graph_types::EdgeType;
use crate::simulation::CpuSimulation;

/// Which legend rows a graph needs. One flag per node row; edges keep
/// canonical order so the key reads the same across pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegendSpec {
    pub class: bool,
    pub abstract_class: bool,
    pub slot: bool,
    pub enum_: bool,
    pub type_: bool,
    pub individual: bool,
    pub external: bool,
    /// Edge kinds present, in canonical order.
    pub edges: Vec<EdgeType>,
    /// Crow's-foot terminators ride on range edges, so the section only
    /// applies when a range edge exists.
    pub cardinality: bool,
    /// Amber rule rings appear only when some node participates in a rule.
    pub rule_rings: bool,
    /// The blue selection ring is a component affordance (click to pin),
    /// shown whenever there is anything to select.
    pub selection_ring: bool,
}

impl LegendSpec {
    /// Every row the renderer can draw — the historical full key, used by
    /// the standalone entry point that has no graph to inspect.
    pub fn full() -> Self {
        Self {
            class: true,
            abstract_class: true,
            slot: true,
            enum_: true,
            type_: true,
            individual: true,
            external: true,
            edges: vec![
                EdgeType::SubclassOf,
                EdgeType::Mixin,
                EdgeType::Domain,
                EdgeType::Range,
                EdgeType::Inverse,
                EdgeType::TypeOf,
                EdgeType::Assertion,
            ],
            cardinality: true,
            rule_rings: true,
            selection_ring: true,
        }
    }
}

/// Compute the rows this graph actually needs.
pub fn legend_spec(sim: &CpuSimulation) -> LegendSpec {
    use crate::graph_types::{KindMetadata, NodeType};
    let mut spec = LegendSpec {
        class: false,
        abstract_class: false,
        slot: false,
        enum_: false,
        type_: false,
        individual: false,
        external: false,
        edges: Vec::new(),
        cardinality: false,
        rule_rings: false,
        selection_ring: !sim.nodes.is_empty(),
    };
    for node in &sim.nodes {
        if node.node_type == NodeType::External {
            spec.external = true;
            continue;
        }
        match &node.kind_metadata {
            Some(KindMetadata::Class { .. }) => {
                spec.class = true;
                if node.is_abstract {
                    spec.abstract_class = true;
                }
            }
            Some(KindMetadata::Slot { .. }) => spec.slot = true,
            Some(KindMetadata::Enum { .. }) => spec.enum_ = true,
            Some(KindMetadata::Individual { .. }) => spec.individual = true,
            None => spec.type_ = true,
        }
        if node.in_rule {
            spec.rule_rings = true;
        }
    }
    for kind in [
        EdgeType::SubclassOf,
        EdgeType::Mixin,
        EdgeType::Domain,
        EdgeType::Range,
        EdgeType::Inverse,
        EdgeType::TypeOf,
        EdgeType::Assertion,
    ] {
        if sim.edges.iter().any(|e| e.edge_type == kind) {
            spec.edges.push(kind);
        }
    }
    spec.cardinality = spec.edges.contains(&EdgeType::Range);
    spec
}

/// Layout metrics the legend's drawing and sizing share. Drawing walks a
/// `y` cursor with these; [`Canvas2DRenderer::legend_extent`] computes the
/// same walk arithmetically — one set of constants so they cannot drift.
pub(crate) mod legend_metrics {
    /// First baseline below the top edge.
    pub const TOP_Y: f64 = 18.0;
    /// Height of every row, header rows included.
    pub const ROW: f64 = 21.0;
    /// Extra space above each section header after the first.
    pub const SECTION_GAP: f64 = 6.0;
    /// Space under the last row, mirroring `TOP_Y`.
    pub const BOTTOM_PAD: f64 = 10.0;
    /// Fixed logical width; labels are the widest content and don't vary
    /// enough per spec to justify measuring text.
    pub const WIDTH: f64 = 240.0;
}

/// The node rows a spec asks for, in table order.
pub(crate) fn node_rows_for(
    spec: &LegendSpec,
) -> Vec<(
    crate::canvas2d::NodeRowKind,
    crate::canvas2d::NodeShape,
    [f32; 4],
    &'static str,
    bool,
)> {
    crate::canvas2d::node_legend_rows()
        .into_iter()
        .filter(|(kind, ..)| match kind {
            crate::canvas2d::NodeRowKind::Class => spec.class,
            crate::canvas2d::NodeRowKind::Slot => spec.slot,
            crate::canvas2d::NodeRowKind::Enum => spec.enum_,
            crate::canvas2d::NodeRowKind::Type => spec.type_,
            crate::canvas2d::NodeRowKind::Individual => spec.individual,
            crate::canvas2d::NodeRowKind::AbstractClass => spec.abstract_class,
            crate::canvas2d::NodeRowKind::External => spec.external,
        })
        .collect()
}

/// The edge rows a spec asks for, in table order.
pub(crate) fn edge_rows_for(spec: &LegendSpec) -> Vec<(EdgeType, &'static str)> {
    crate::canvas2d::edge_legend_rows()
        .into_iter()
        .filter(|(kind, _)| spec.edges.contains(kind))
        .collect()
}

/// The ring rows a spec asks for: the two amber rule rows when rules are
/// present, the selection row when there is anything to select.
pub(crate) fn ring_rows_for(
    spec: &LegendSpec,
) -> Vec<(
    &'static str,
    f64,
    crate::canvas2d::NodeShape,
    [f32; 4],
    &'static str,
)> {
    crate::canvas2d::ring_legend_rows()
        .into_iter()
        .filter(|(color, ..)| {
            if *color == crate::canvas2d::AMBER {
                spec.rule_rings
            } else {
                spec.selection_ring
            }
        })
        .collect()
}

/// The logical (CSS-px) size the adaptive key needs for `spec` — the
/// same walk the drawing makes, expressed as arithmetic over the shared
/// metrics, so a shell can size the panel to its rows instead of
/// reserving a fixed box with dead space under a short key.
pub fn legend_extent(spec: &LegendSpec) -> (f64, f64) {
    let node_rows = node_rows_for(spec).len();
    let edge_rows = edge_rows_for(spec).len();
    let ring_rows = ring_rows_for(spec).len();
    let card_rows = if spec.cardinality { 4 } else { 0 };

    let mut height = legend_metrics::TOP_Y;
    let mut section = |rows: usize| {
        if rows > 0 {
            height += legend_metrics::SECTION_GAP
                    + legend_metrics::ROW // header
                    + legend_metrics::ROW * rows as f64;
        }
    };
    section(node_rows);
    section(edge_rows);
    section(card_rows);
    section(ring_rows);
    // The first drawn section has no preceding gap; TOP_Y already
    // clears the top edge, and a bottom pad mirrors it.
    if height > legend_metrics::TOP_Y {
        height -= legend_metrics::SECTION_GAP;
    }
    height += legend_metrics::BOTTOM_PAD;
    (legend_metrics::WIDTH, height)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_types::GraphData;

    fn sim(json: &str) -> CpuSimulation {
        let data: GraphData = serde_json::from_str(json).expect("fixture parses");
        CpuSimulation::from_graph_data(&data)
    }

    #[test]
    fn a_schema_without_enums_gets_no_enum_row() {
        let s = sim(r#"{
            "schema_name": "t", "schema_title": "t", "format_version": "1.1",
            "nodes": [
                {"id": "c1", "label": "C1", "node_type": "class", "color": [0.1,0.2,0.3,1.0],
                 "kind_metadata": {"kind": "class", "slots": [], "parents": [], "mixins": [], "rules": []}},
                {"id": "s1", "label": "s1", "node_type": "slot", "color": [0.1,0.2,0.3,1.0],
                 "kind_metadata": {"kind": "slot", "domains": [], "required": false, "multivalued": false}}
            ],
            "edges": [{"source": "s1", "target": "c1", "edge_type": "domain"}]
        }"#);
        let spec = legend_spec(&s);
        assert!(spec.class && spec.slot, "present kinds get rows");
        assert!(
            !spec.enum_,
            "a schema with no enums must not advertise the diamond"
        );
        assert!(!spec.individual && !spec.external && !spec.type_);
        assert!(!spec.abstract_class, "no abstract class in the data");
        assert_eq!(spec.edges, vec![EdgeType::Domain]);
        assert!(!spec.cardinality, "no range edge, no crow's-feet section");
        assert!(!spec.rule_rings, "no rule participants, no amber-ring rows");
        assert!(spec.selection_ring, "there are nodes to select");
    }

    #[test]
    fn an_instance_graph_gets_individual_and_assertion_rows_only() {
        let s = sim(r#"{
            "schema_name": "t", "schema_title": "t", "format_version": "1.1", "graph_kind": "instance",
            "nodes": [
                {"id": "i1", "label": "A", "node_type": "individual", "color": [0.1,0.2,0.3,1.0],
                 "kind_metadata": {"kind": "individual", "types": ["C"], "literals": []}},
                {"id": "i2", "label": "B", "node_type": "individual", "color": [0.1,0.2,0.3,1.0],
                 "kind_metadata": {"kind": "individual", "types": ["C"], "literals": []}}
            ],
            "edges": [{"source": "i1", "target": "i2", "edge_type": "assertion", "label": "knows"}]
        }"#);
        let spec = legend_spec(&s);
        assert!(
            spec.individual,
            "individuals are the instance graph's node kind"
        );
        assert!(
            !spec.class && !spec.slot && !spec.enum_ && !spec.type_ && !spec.external,
            "no schema-kind rows on an instance graph"
        );
        assert_eq!(spec.edges, vec![EdgeType::Assertion]);
        assert!(!spec.cardinality, "assertions carry no crow's-feet");
    }

    #[test]
    fn range_edges_bring_the_cardinality_section() {
        let s = sim(r#"{
            "schema_name": "t", "schema_title": "t", "format_version": "1.1",
            "nodes": [
                {"id": "c1", "label": "C1", "node_type": "class", "color": [0.1,0.2,0.3,1.0],
                 "kind_metadata": {"kind": "class", "slots": [], "parents": [], "mixins": [], "rules": []}},
                {"id": "s1", "label": "s1", "node_type": "slot", "color": [0.1,0.2,0.3,1.0],
                 "kind_metadata": {"kind": "slot", "domains": [], "required": false, "multivalued": false}}
            ],
            "edges": [{"source": "s1", "target": "c1", "edge_type": "range"}]
        }"#);
        let spec = legend_spec(&s);
        assert!(spec.cardinality, "crow's-feet ride on range edges");
    }

    #[test]
    fn abstract_and_rule_flags_come_from_the_derived_nodes() {
        let s = sim(r#"{
            "schema_name": "t", "schema_title": "t", "format_version": "1.1",
            "nodes": [
                {"id": "c1", "label": "C1", "node_type": "class", "color": [0.1,0.2,0.3,1.0],
                 "is_abstract": true,
                 "kind_metadata": {"kind": "class", "slots": [], "parents": [], "mixins": [],
                   "rules": [{"title": "r", "description": null, "summary": "when x then y",
                              "participants": []}]}}
            ],
            "edges": []
        }"#);
        let spec = legend_spec(&s);
        assert!(spec.abstract_class, "abstract class present");
        assert!(
            spec.rule_rings,
            "a class declaring a rule participates in it, so the amber rings apply"
        );
    }

    /// The exactness contract: a row appears in the key if and only if the
    /// graph contains that kind — no more, no less. The expected sets are
    /// derived here independently, by walking the simulation directly,
    /// rather than repeating the spec's own logic.
    #[test]
    fn legend_rows_are_exactly_the_kinds_present_in_the_graph() {
        use crate::graph_types::{KindMetadata, NodeType};

        // A mixed graph: abstract class + slot + individual + external,
        // domain + assertion edges, a class rule — and deliberately NO
        // enum, NO type node, NO range edge.
        let s = sim(r#"{
            "schema_name": "t", "schema_title": "t", "format_version": "1.1",
            "nodes": [
                {"id": "c1", "label": "C1", "node_type": "class", "color": [0.1,0.2,0.3,1.0],
                 "is_abstract": true,
                 "kind_metadata": {"kind": "class", "slots": [], "parents": [], "mixins": [],
                   "rules": [{"title": "r", "description": null, "summary": "s", "participants": []}]}},
                {"id": "s1", "label": "s1", "node_type": "slot", "color": [0.1,0.2,0.3,1.0],
                 "kind_metadata": {"kind": "slot", "domains": [], "required": false, "multivalued": false}},
                {"id": "i1", "label": "A", "node_type": "individual", "color": [0.1,0.2,0.3,1.0],
                 "kind_metadata": {"kind": "individual", "types": ["C1"], "literals": []}},
                {"id": "x1", "label": "X", "node_type": "external", "color": [0.1,0.2,0.3,1.0]}
            ],
            "edges": [
                {"source": "s1", "target": "c1", "edge_type": "domain"},
                {"source": "i1", "target": "i1", "edge_type": "assertion", "label": "knows"}
            ]
        }"#);
        let spec = legend_spec(&s);

        // Expected node labels, walked independently off the sim nodes.
        let mut expected_nodes = std::collections::BTreeSet::new();
        for n in &s.nodes {
            if n.node_type == NodeType::External {
                expected_nodes.insert("External grounding");
                continue;
            }
            match &n.kind_metadata {
                Some(KindMetadata::Class { .. }) => {
                    expected_nodes.insert("Class");
                    if n.is_abstract {
                        expected_nodes.insert("Abstract class");
                    }
                }
                Some(KindMetadata::Slot { .. }) => {
                    expected_nodes.insert("Slot");
                }
                Some(KindMetadata::Enum { .. }) => {
                    expected_nodes.insert("Enum");
                }
                Some(KindMetadata::Individual { .. }) => {
                    expected_nodes.insert("Individual");
                }
                None => {
                    expected_nodes.insert("Type");
                }
            }
        }
        let shown_nodes: std::collections::BTreeSet<&str> =
            node_rows_for(&spec).iter().map(|r| r.3).collect();
        assert_eq!(
            shown_nodes, expected_nodes,
            "node rows must be exactly the kinds present — no more, no less"
        );

        // Expected edge kinds, walked independently off the sim edges.
        let expected_edges: std::collections::BTreeSet<EdgeType> =
            s.edges.iter().map(|e| e.edge_type).collect();
        let shown_edges: std::collections::BTreeSet<EdgeType> =
            edge_rows_for(&spec).iter().map(|r| r.0).collect();
        assert_eq!(
            shown_edges, expected_edges,
            "edge rows must be exactly the kinds present — no more, no less"
        );

        // Rule rings shown iff some node participates in a rule; the
        // fixture's class declares one.
        assert!(s.nodes.iter().any(|n| n.in_rule));
        let rings: Vec<&str> = ring_rows_for(&spec).iter().map(|r| r.4).collect();
        assert!(rings.contains(&"Slot in a rule") && rings.contains(&"Selected node"));

        // And the deliberate absences stay absent.
        assert!(!shown_nodes.contains("Enum") && !shown_nodes.contains("Type"));
        assert!(!spec.cardinality, "no range edge, no cardinality section");
    }

    /// The panel sizes to its rows: the extent is the drawing's walk done
    /// arithmetically, checked here against independent row counts.
    #[test]
    fn legend_extent_matches_the_row_count_arithmetic() {
        use super::legend_metrics as m;

        // Instance-graph key: Individual row, assertion edge, selection
        // ring — three sections, no cardinality.
        let s = sim(r#"{
            "schema_name": "t", "schema_title": "t", "format_version": "1.1",
            "nodes": [
                {"id": "i1", "label": "A", "node_type": "individual", "color": [0.1,0.2,0.3,1.0],
                 "kind_metadata": {"kind": "individual", "types": ["C"], "literals": []}}
            ],
            "edges": [{"source": "i1", "target": "i1", "edge_type": "assertion", "label": "knows"}]
        }"#);
        let spec = legend_spec(&s);
        let (w, h) = legend_extent(&spec);
        assert_eq!(w, m::WIDTH);
        // Sections: nodes(1 row) + edges(1) + rings(1), each with a header;
        // gaps before every section but the first; top and bottom padding.
        let expected = m::TOP_Y
            + 3.0 * (m::ROW + m::ROW) // 3 sections × (header + one row)
            + 2.0 * m::SECTION_GAP
            + m::BOTTOM_PAD;
        assert!(
            (h - expected).abs() < f64::EPSILON,
            "instance-key height: got {h}, expected {expected}"
        );

        // The full key is strictly taller than the three-row instance key.
        let (_, full_h) = legend_extent(&crate::legend::LegendSpec::full());
        assert!(
            full_h > h,
            "full key ({full_h}) must exceed a short key ({h})"
        );

        // An empty graph needs only the paddings.
        let empty = sim(
            r#"{"schema_name": "t", "schema_title": "t", "format_version": "1.1", "nodes": [], "edges": []}"#,
        );
        let (_, empty_h) = legend_extent(&legend_spec(&empty));
        assert!(
            (empty_h - (m::TOP_Y + m::BOTTOM_PAD)).abs() < f64::EPSILON,
            "empty key is just padding; got {empty_h}"
        );
    }

    #[test]
    fn the_full_spec_is_a_superset_of_any_computed_spec() {
        let full = LegendSpec::full();
        assert!(full.class && full.abstract_class && full.slot && full.enum_);
        assert!(full.type_ && full.individual && full.external);
        assert_eq!(full.edges.len(), 7, "every edge kind, canonical order");
        assert!(full.cardinality && full.rule_rings && full.selection_ring);
    }

    #[test]
    fn an_empty_graph_needs_no_rows_at_all() {
        let s = sim(
            r#"{"schema_name": "t", "schema_title": "t", "format_version": "1.1", "nodes": [], "edges": []}"#,
        );
        let spec = legend_spec(&s);
        assert!(!spec.selection_ring, "nothing to select");
        assert!(spec.edges.is_empty());
    }
}
