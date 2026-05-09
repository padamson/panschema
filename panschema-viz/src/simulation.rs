//! CPU-based force simulation for graph layout
//!
//! Implements a simple force-directed layout algorithm that runs in the browser.
//! Uses the same physics as the GPU version but runs on CPU for compatibility.

use crate::graph_types::{EdgeType, GraphData, GraphNode};
use crate::sim_common;

/// A node with position and velocity for simulation
#[derive(Debug, Clone)]
pub struct SimNode {
    /// Node ID (from GraphNode)
    pub id: String,
    /// Human-readable label for display
    pub label: String,
    /// Position in 2D space
    pub x: f32,
    pub y: f32,
    /// Velocity
    pub vx: f32,
    pub vy: f32,
    /// Color from GraphNode
    pub color: [f32; 4],
    /// Radius for rendering
    pub radius: f32,
    /// Connected-component id, set at construction time. Used by the
    /// many-body force to scale inter-component repulsion.
    pub(crate) component: usize,
}

impl SimNode {
    /// Create from GraphNode with random initial position
    pub fn from_graph_node(node: &GraphNode, index: usize, total: usize) -> Self {
        // Distribute nodes in a circle initially
        let angle = 2.0 * std::f32::consts::PI * (index as f32) / (total as f32);
        let radius = 100.0;

        Self {
            id: node.id.clone(),
            label: node.label.clone(),
            x: radius * angle.cos(),
            y: radius * angle.sin(),
            vx: 0.0,
            vy: 0.0,
            color: node.color,
            radius: 4.0,
            component: 0, // Filled in by from_graph_data after edges are known.
        }
    }
}

/// An edge for simulation (indices into node array)
#[derive(Debug, Clone)]
pub struct SimEdge {
    pub source: usize,
    pub target: usize,
    /// Human-readable label for display
    pub label: String,
}

impl SimEdge {
    /// Convert edge type to human-readable label
    fn format_edge_type(edge_type: EdgeType) -> String {
        match edge_type {
            EdgeType::SubclassOf => "subclassOf".to_string(),
            EdgeType::Mixin => "mixin".to_string(),
            EdgeType::Domain => "domain".to_string(),
            EdgeType::Range => "range".to_string(),
            EdgeType::Inverse => "inverseOf".to_string(),
            EdgeType::TypeOf => "typeOf".to_string(),
        }
    }
}

/// Configuration for the force simulation
#[derive(Debug, Clone)]
pub struct SimulationConfig {
    /// Repulsion strength (negative = repulsion)
    pub charge: f32,
    /// Link distance (rest length)
    pub link_distance: f32,
    /// Link strength
    pub link_strength: f32,
    /// Velocity decay (friction)
    pub velocity_decay: f32,
    /// Current alpha (simulation temperature)
    pub alpha: f32,
    /// Minimum alpha before stopping
    pub alpha_min: f32,
    /// Alpha decay rate
    pub alpha_decay: f32,
    /// Extra gap added to (r1 + r2) when resolving overlap
    pub collide_padding: f32,
    /// Fraction of overlap resolved per tick (0..1); lower values are smoother
    pub collide_strength: f32,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            charge: -60.0,
            link_distance: 50.0,
            link_strength: 1.0,
            velocity_decay: 0.6,
            alpha: 1.0,
            alpha_min: 0.001,
            alpha_decay: 1.0 - 0.001_f32.powf(1.0 / 300.0),
            collide_padding: 2.0,
            collide_strength: 0.7,
        }
    }
}

/// CPU force simulation
pub struct CpuSimulation {
    pub nodes: Vec<SimNode>,
    pub edges: Vec<SimEdge>,
    pub config: SimulationConfig,
    /// Mapping from node ID to index (for future interactivity)
    #[allow(dead_code)]
    node_id_to_index: std::collections::HashMap<String, usize>,
}

impl CpuSimulation {
    /// Create simulation from graph data
    pub fn from_graph_data(graph: &GraphData) -> Self {
        let total = graph.nodes.len();

        // Create nodes with initial positions
        let nodes: Vec<SimNode> = graph
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| SimNode::from_graph_node(n, i, total))
            .collect();

        // Build ID to index mapping
        let node_id_to_index: std::collections::HashMap<String, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.clone(), i))
            .collect();

        // Create edges with indices
        let edges: Vec<SimEdge> = graph
            .edges
            .iter()
            .filter_map(|e| {
                let source = node_id_to_index.get(&e.source)?;
                let target = node_id_to_index.get(&e.target)?;
                let label = e
                    .label
                    .clone()
                    .unwrap_or_else(|| SimEdge::format_edge_type(e.edge_type));
                Some(SimEdge {
                    source: *source,
                    target: *target,
                    label,
                })
            })
            .collect();

        // Compute connected components (undirected) so the many-body force can
        // apply stronger repulsion between disconnected pieces.
        let components =
            sim_common::compute_components(nodes.len(), edges.iter().map(|e| (e.source, e.target)));
        let mut nodes = nodes;
        for (i, node) in nodes.iter_mut().enumerate() {
            node.component = components[i];
        }

        // Components start in separate angular regions so they don't have to
        // traverse through each other to find equilibrium.
        layout_by_component_2d(&mut nodes, &components);

        Self {
            nodes,
            edges,
            config: SimulationConfig::default(),
            node_id_to_index,
        }
    }

    /// Check if simulation is still running
    pub fn is_running(&self) -> bool {
        self.config.alpha > self.config.alpha_min
    }

    /// Reheat the simulation to restart physics (e.g., when dragging starts)
    pub fn reheat(&mut self, alpha: f32) {
        self.config.alpha = alpha.clamp(self.config.alpha_min, 1.0);
    }

    /// Set a node's position (for dragging)
    pub fn set_node_position(&mut self, index: usize, x: f32, y: f32) {
        if let Some(node) = self.nodes.get_mut(index) {
            node.x = x;
            node.y = y;
            node.vx = 0.0;
            node.vy = 0.0;
        }
    }

    /// Get a node's world position (for focus calculations)
    #[allow(dead_code)] // Used in sub-slice 6.8 (focus mode)
    pub fn get_node_position(&self, index: usize) -> Option<(f32, f32)> {
        self.nodes.get(index).map(|n| (n.x, n.y))
    }

    /// Run one simulation tick
    pub fn tick(&mut self) {
        self.tick_with_fixed(&std::collections::HashSet::new());
    }

    /// Run one simulation tick, skipping velocity updates for fixed nodes
    pub fn tick_with_fixed(&mut self, fixed_nodes: &std::collections::HashSet<usize>) {
        if !self.is_running() {
            return;
        }

        let n = self.nodes.len();
        if n == 0 {
            return;
        }

        // Apply many-body force (repulsion between all nodes)
        self.apply_many_body_force();

        // Apply link force (springs between connected nodes)
        self.apply_link_force();

        // Velocity integration with a hard sphere clamp — bounds drift so
        // fit_to_bounds always frames a sensible region.
        const MAX_RADIUS: f32 = 200.0;
        const MAX_RADIUS_SQ: f32 = MAX_RADIUS * MAX_RADIUS;
        for (i, node) in self.nodes.iter_mut().enumerate() {
            if !fixed_nodes.contains(&i) {
                node.vx *= self.config.velocity_decay;
                node.vy *= self.config.velocity_decay;
                node.x += node.vx * self.config.alpha;
                node.y += node.vy * self.config.alpha;
                let dist_sq = node.x * node.x + node.y * node.y;
                if dist_sq > MAX_RADIUS_SQ {
                    let scale = MAX_RADIUS / dist_sq.sqrt();
                    node.x *= scale;
                    node.y *= scale;
                    // Reflect velocity inward: kill any outward component
                    node.vx = 0.0;
                    node.vy = 0.0;
                }
            } else {
                // Fixed nodes keep zero velocity
                node.vx = 0.0;
                node.vy = 0.0;
            }
        }

        // Resolve overlap via position correction (post-integration so it sees final positions)
        self.apply_collide_force(fixed_nodes);

        // Skip recenter when nodes are pinned: pins anchor the frame of reference.
        if fixed_nodes.is_empty() {
            self.recenter_centroid();
        }

        self.config.alpha *= 1.0 - self.config.alpha_decay;
    }

    /// Resolve node overlap via direct position correction.
    /// Any pair within (r1 + r2 + padding) is pushed apart by `strength` of the overlap.
    /// Fixed nodes don't move; the other absorbs the full correction.
    fn apply_collide_force(&mut self, fixed_nodes: &std::collections::HashSet<usize>) {
        let n = self.nodes.len();
        let padding = self.config.collide_padding;
        let strength = self.config.collide_strength;
        // Skip O(n²) HashSet lookups when nothing's pinned (the common case).
        let any_fixed = !fixed_nodes.is_empty();

        for i in 0..n {
            for j in (i + 1)..n {
                let (i_fixed, j_fixed) = if any_fixed {
                    (fixed_nodes.contains(&i), fixed_nodes.contains(&j))
                } else {
                    (false, false)
                };
                if i_fixed && j_fixed {
                    continue;
                }

                let dx = self.nodes[j].x - self.nodes[i].x;
                let dy = self.nodes[j].y - self.nodes[i].y;
                let r_sum = self.nodes[i].radius + self.nodes[j].radius + padding;
                let dist_sq = dx * dx + dy * dy;

                if dist_sq >= r_sum * r_sum || dist_sq <= 0.0 {
                    continue;
                }

                let dist = dist_sq.sqrt();
                let overlap = (r_sum - dist) * strength;
                let nx = dx / dist;
                let ny = dy / dist;

                let (i_share, j_share) = match (i_fixed, j_fixed) {
                    (true, false) => (0.0, 1.0),
                    (false, true) => (1.0, 0.0),
                    _ => (0.5, 0.5),
                };

                self.nodes[i].x -= nx * overlap * i_share;
                self.nodes[i].y -= ny * overlap * i_share;
                self.nodes[j].x += nx * overlap * j_share;
                self.nodes[j].y += ny * overlap * j_share;
            }
        }
    }

    /// Apply repulsion between all node pairs. Inter-component repulsion is
    /// scaled by `INTER_COMPONENT_SCALE` so disconnected pieces actively
    /// push apart instead of stalling at symmetric stationary points inside
    /// each other's neighborhoods.
    fn apply_many_body_force(&mut self) {
        const INTER_COMPONENT_SCALE: f32 = 5.0;
        let n = self.nodes.len();

        for i in 0..n {
            for j in (i + 1)..n {
                let dx = self.nodes[j].x - self.nodes[i].x;
                let dy = self.nodes[j].y - self.nodes[i].y;

                let dist_sq = dx * dx + dy * dy;
                let dist = dist_sq.sqrt().max(1.0);

                let scale = if self.nodes[i].component == self.nodes[j].component {
                    1.0
                } else {
                    INTER_COMPONENT_SCALE
                };
                let force = self.config.charge * scale / dist_sq;

                let fx = force * dx / dist;
                let fy = force * dy / dist;

                self.nodes[i].vx -= fx;
                self.nodes[i].vy -= fy;
                self.nodes[j].vx += fx;
                self.nodes[j].vy += fy;
            }
        }
    }

    /// Apply spring force between connected nodes
    fn apply_link_force(&mut self) {
        for edge in &self.edges {
            let (source, target) = (edge.source, edge.target);

            let dx = self.nodes[target].x - self.nodes[source].x;
            let dy = self.nodes[target].y - self.nodes[source].y;

            let dist = (dx * dx + dy * dy).sqrt().max(1.0);

            // Hooke's law: F = k * (x - x0)
            let stretch = dist - self.config.link_distance;
            let force = self.config.link_strength * stretch / dist;

            let fx = force * dx;
            let fy = force * dy;

            self.nodes[source].vx += fx;
            self.nodes[source].vy += fy;
            self.nodes[target].vx -= fx;
            self.nodes[target].vy -= fy;
        }
    }

    /// Translate the whole layout so its centroid sits at origin. Mirrors
    /// d3-force's `forceCenter` — components are free to settle wherever
    /// inter-component repulsion takes them, while the overall layout stays
    /// framed.
    fn recenter_centroid(&mut self) {
        let n = self.nodes.len();
        if n == 0 {
            return;
        }
        let mut sx = 0.0;
        let mut sy = 0.0;
        for node in &self.nodes {
            sx += node.x;
            sy += node.y;
        }
        let inv_n = 1.0 / n as f32;
        let cx = sx * inv_n;
        let cy = sy * inv_n;
        for node in &mut self.nodes {
            node.x -= cx;
            node.y -= cy;
        }
    }

    /// Run simulation to convergence (or max iterations)
    pub fn run_to_convergence(&mut self, max_iterations: usize) {
        for _ in 0..max_iterations {
            if !self.is_running() {
                break;
            }
            self.tick();
        }
    }
}

/// Place each connected component in its own angular region so disconnected
/// pieces don't start tangled. Single-component graphs keep their default
/// circular layout from `SimNode::from_graph_node`.
fn layout_by_component_2d(nodes: &mut [SimNode], components: &[usize]) {
    use std::f32::consts::PI;
    if nodes.is_empty() {
        return;
    }
    let num_components = components.iter().max().copied().unwrap_or(0) + 1;
    if num_components <= 1 {
        return;
    }

    let mut by_component: Vec<Vec<usize>> = vec![Vec::new(); num_components];
    for (i, &c) in components.iter().enumerate() {
        by_component[c].push(i);
    }

    let arc_per_component = 2.0 * PI / num_components as f32;
    let big_radius = 150.0;
    let small_radius = 50.0;

    for (component_id, members) in by_component.iter().enumerate() {
        let component_angle = component_id as f32 * arc_per_component;
        let cx = big_radius * component_angle.cos();
        let cy = big_radius * component_angle.sin();
        let m = members.len();
        for (intra_idx, &node_idx) in members.iter().enumerate() {
            let angle = if m <= 1 {
                0.0
            } else {
                2.0 * PI * intra_idx as f32 / m as f32
            };
            nodes[node_idx].x = cx + small_radius * angle.cos();
            nodes[node_idx].y = cy + small_radius * angle.sin();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph_types::{EdgeType, GraphEdge, GraphNode, NodeType};

    fn make_test_graph() -> GraphData {
        GraphData {
            schema_name: "test".to_string(),
            schema_title: None,
            format_version: "1.0".to_string(),
            nodes: vec![
                GraphNode {
                    id: "a".to_string(),
                    label: "A".to_string(),
                    node_type: NodeType::Class,
                    color: [1.0, 0.0, 0.0, 1.0],
                    description: None,
                    uri: None,
                    is_abstract: false,
                },
                GraphNode {
                    id: "b".to_string(),
                    label: "B".to_string(),
                    node_type: NodeType::Class,
                    color: [0.0, 1.0, 0.0, 1.0],
                    description: None,
                    uri: None,
                    is_abstract: false,
                },
            ],
            edges: vec![GraphEdge {
                source: "a".to_string(),
                target: "b".to_string(),
                edge_type: EdgeType::SubclassOf,
                label: None,
            }],
        }
    }

    #[test]
    fn creates_simulation_from_graph() {
        let graph = make_test_graph();
        let sim = CpuSimulation::from_graph_data(&graph);

        assert_eq!(sim.nodes.len(), 2);
        assert_eq!(sim.edges.len(), 1);
    }

    #[test]
    fn simulation_runs() {
        let graph = make_test_graph();
        let mut sim = CpuSimulation::from_graph_data(&graph);

        let initial_alpha = sim.config.alpha;
        sim.tick();

        assert!(sim.config.alpha < initial_alpha);
    }

    #[test]
    fn simulation_converges() {
        let graph = make_test_graph();
        let mut sim = CpuSimulation::from_graph_data(&graph);

        sim.run_to_convergence(500);

        assert!(!sim.is_running());
    }

    #[test]
    fn empty_graph_handles_gracefully() {
        let graph = GraphData {
            schema_name: "empty".to_string(),
            schema_title: None,
            format_version: "1.0".to_string(),
            nodes: vec![],
            edges: vec![],
        };
        let mut sim = CpuSimulation::from_graph_data(&graph);

        assert_eq!(sim.nodes.len(), 0);
        assert_eq!(sim.edges.len(), 0);

        // Should not panic when ticking empty graph
        sim.tick();
        sim.run_to_convergence(100);
    }

    #[test]
    fn single_node_graph() {
        let graph = GraphData {
            schema_name: "single".to_string(),
            schema_title: None,
            format_version: "1.0".to_string(),
            nodes: vec![GraphNode {
                id: "only".to_string(),
                label: "Only Node".to_string(),
                node_type: NodeType::Class,
                color: [1.0, 0.0, 0.0, 1.0],
                description: None,
                uri: None,
                is_abstract: false,
            }],
            edges: vec![],
        };
        let mut sim = CpuSimulation::from_graph_data(&graph);

        assert_eq!(sim.nodes.len(), 1);
        assert_eq!(sim.edges.len(), 0);

        // Record initial distance from origin
        let initial_dist =
            (sim.nodes[0].x * sim.nodes[0].x + sim.nodes[0].y * sim.nodes[0].y).sqrt();

        // Single node should converge (center force pulls to origin)
        sim.run_to_convergence(500);
        assert!(!sim.is_running());

        // Node should be closer to origin after simulation
        let final_dist = (sim.nodes[0].x * sim.nodes[0].x + sim.nodes[0].y * sim.nodes[0].y).sqrt();
        assert!(
            final_dist < initial_dist,
            "Center force should pull node towards origin"
        );
    }

    #[test]
    fn disconnected_nodes_repel() {
        let graph = GraphData {
            schema_name: "disconnected".to_string(),
            schema_title: None,
            format_version: "1.0".to_string(),
            nodes: vec![
                GraphNode {
                    id: "a".to_string(),
                    label: "A".to_string(),
                    node_type: NodeType::Class,
                    color: [1.0, 0.0, 0.0, 1.0],
                    description: None,
                    uri: None,
                    is_abstract: false,
                },
                GraphNode {
                    id: "b".to_string(),
                    label: "B".to_string(),
                    node_type: NodeType::Class,
                    color: [0.0, 1.0, 0.0, 1.0],
                    description: None,
                    uri: None,
                    is_abstract: false,
                },
            ],
            edges: vec![], // No edges - nodes should repel
        };
        let mut sim = CpuSimulation::from_graph_data(&graph);

        // Get initial distance
        let initial_dx = sim.nodes[1].x - sim.nodes[0].x;
        let initial_dy = sim.nodes[1].y - sim.nodes[0].y;
        let initial_dist = (initial_dx * initial_dx + initial_dy * initial_dy).sqrt();

        // Run simulation
        sim.run_to_convergence(500);

        // Nodes should still be apart (repulsion balanced by center force)
        let final_dx = sim.nodes[1].x - sim.nodes[0].x;
        let final_dy = sim.nodes[1].y - sim.nodes[0].y;
        let final_dist = (final_dx * final_dx + final_dy * final_dy).sqrt();

        // Collide force enforces r1 + r2 minimum independent of repulsion tuning.
        let min_separation = sim.nodes[0].radius + sim.nodes[1].radius;
        assert!(
            final_dist >= min_separation,
            "Nodes should not overlap: got dist={final_dist}, min={min_separation}"
        );
        // Position clamp + alpha decay keep them within bounded space.
        assert!(
            final_dist < initial_dist * 2.0,
            "Position clamp should limit spread"
        );
    }

    #[test]
    fn collide_force_prevents_overlap() {
        // Two nodes placed inside each other should be pushed apart by the collide force.
        let graph = GraphData {
            schema_name: "overlap".to_string(),
            schema_title: None,
            format_version: "1.0".to_string(),
            nodes: vec![
                GraphNode {
                    id: "a".to_string(),
                    label: "A".to_string(),
                    node_type: NodeType::Class,
                    color: [1.0, 0.0, 0.0, 1.0],
                    description: None,
                    uri: None,
                    is_abstract: false,
                },
                GraphNode {
                    id: "b".to_string(),
                    label: "B".to_string(),
                    node_type: NodeType::Class,
                    color: [0.0, 1.0, 0.0, 1.0],
                    description: None,
                    uri: None,
                    is_abstract: false,
                },
            ],
            edges: vec![],
        };
        let mut sim = CpuSimulation::from_graph_data(&graph);

        // Force overlap.
        sim.nodes[0].x = 0.0;
        sim.nodes[0].y = 0.0;
        sim.nodes[1].x = 1.0;
        sim.nodes[1].y = 0.0;

        let r_sum = sim.nodes[0].radius + sim.nodes[1].radius;

        // strength=0.7 per tick → overlap shrinks geometrically
        for _ in 0..50 {
            sim.tick();
        }

        let dx = sim.nodes[1].x - sim.nodes[0].x;
        let dy = sim.nodes[1].y - sim.nodes[0].y;
        let dist = (dx * dx + dy * dy).sqrt();

        assert!(
            dist >= r_sum,
            "Collide force should resolve overlap: dist={dist}, r_sum={r_sum}"
        );
    }

    #[test]
    fn collide_force_skips_two_fixed_nodes() {
        // If both nodes are fixed, neither should move even when overlapping.
        let graph = GraphData {
            schema_name: "two_fixed".to_string(),
            schema_title: None,
            format_version: "1.0".to_string(),
            nodes: vec![
                GraphNode {
                    id: "a".to_string(),
                    label: "A".to_string(),
                    node_type: NodeType::Class,
                    color: [1.0, 0.0, 0.0, 1.0],
                    description: None,
                    uri: None,
                    is_abstract: false,
                },
                GraphNode {
                    id: "b".to_string(),
                    label: "B".to_string(),
                    node_type: NodeType::Class,
                    color: [0.0, 1.0, 0.0, 1.0],
                    description: None,
                    uri: None,
                    is_abstract: false,
                },
            ],
            edges: vec![],
        };
        let mut sim = CpuSimulation::from_graph_data(&graph);

        sim.nodes[0].x = 0.0;
        sim.nodes[0].y = 0.0;
        sim.nodes[1].x = 1.0;
        sim.nodes[1].y = 0.0;

        let mut fixed = std::collections::HashSet::new();
        fixed.insert(0);
        fixed.insert(1);

        sim.tick_with_fixed(&fixed);

        assert_eq!(sim.nodes[0].x, 0.0);
        assert_eq!(sim.nodes[1].x, 1.0);
    }

    #[test]
    fn edge_labels_formatted_correctly() {
        let graph = GraphData {
            schema_name: "test".to_string(),
            schema_title: None,
            format_version: "1.0".to_string(),
            nodes: vec![
                GraphNode {
                    id: "a".to_string(),
                    label: "A".to_string(),
                    node_type: NodeType::Class,
                    color: [1.0, 0.0, 0.0, 1.0],
                    description: None,
                    uri: None,
                    is_abstract: false,
                },
                GraphNode {
                    id: "b".to_string(),
                    label: "B".to_string(),
                    node_type: NodeType::Class,
                    color: [0.0, 1.0, 0.0, 1.0],
                    description: None,
                    uri: None,
                    is_abstract: false,
                },
            ],
            edges: vec![
                GraphEdge {
                    source: "a".to_string(),
                    target: "b".to_string(),
                    edge_type: EdgeType::SubclassOf,
                    label: None, // Should use formatted edge type
                },
                GraphEdge {
                    source: "b".to_string(),
                    target: "a".to_string(),
                    edge_type: EdgeType::Range,
                    label: Some("custom label".to_string()), // Should use custom label
                },
            ],
        };
        let sim = CpuSimulation::from_graph_data(&graph);

        assert_eq!(sim.edges[0].label, "subclassOf");
        assert_eq!(sim.edges[1].label, "custom label");
    }

    #[test]
    fn missing_edge_target_filtered() {
        let graph = GraphData {
            schema_name: "test".to_string(),
            schema_title: None,
            format_version: "1.0".to_string(),
            nodes: vec![GraphNode {
                id: "a".to_string(),
                label: "A".to_string(),
                node_type: NodeType::Class,
                color: [1.0, 0.0, 0.0, 1.0],
                description: None,
                uri: None,
                is_abstract: false,
            }],
            edges: vec![GraphEdge {
                source: "a".to_string(),
                target: "nonexistent".to_string(), // Missing target
                edge_type: EdgeType::SubclassOf,
                label: None,
            }],
        };
        let sim = CpuSimulation::from_graph_data(&graph);

        // Edge with missing target should be filtered out
        assert_eq!(sim.nodes.len(), 1);
        assert_eq!(sim.edges.len(), 0);
    }
}
