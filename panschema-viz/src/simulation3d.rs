//! 3D Force simulation for graph layout
//!
//! Extends the 2D simulation with z-axis for 3D visualization.

use crate::graph_types::{EdgeType, GraphData, GraphNode};
use crate::sim_common;

/// A node with 3D position and velocity for simulation
#[derive(Debug, Clone)]
pub struct SimNode3D {
    /// Node ID (from GraphNode)
    pub id: String,
    /// Human-readable label for display
    pub label: String,
    /// Position in 3D space
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Velocity
    pub vx: f32,
    pub vy: f32,
    pub vz: f32,
    /// Color from GraphNode (RGBA)
    pub color: [f32; 4],
    /// Radius for rendering
    pub radius: f32,
    /// Connected-component id, set at construction time. Used by the
    /// many-body force to scale inter-component repulsion.
    pub(crate) component: usize,
}

impl SimNode3D {
    /// Create from GraphNode with initial position on a sphere
    pub fn from_graph_node(node: &GraphNode, index: usize, total: usize) -> Self {
        // Distribute nodes on a sphere using Fibonacci lattice
        let golden_ratio = (1.0 + 5.0_f32.sqrt()) / 2.0;
        let i = index as f32;
        let n = total as f32;

        // Fibonacci sphere
        let theta = 2.0 * std::f32::consts::PI * i / golden_ratio;
        let phi = (1.0 - 2.0 * (i + 0.5) / n).acos();

        let radius = 100.0;
        let x = radius * phi.sin() * theta.cos();
        let y = radius * phi.sin() * theta.sin();
        let z = radius * phi.cos();

        Self {
            id: node.id.clone(),
            label: node.label.clone(),
            x,
            y,
            z,
            vx: 0.0,
            vy: 0.0,
            vz: 0.0,
            color: node.color,
            radius: 6.0,
            component: 0, // Filled in by from_graph_data after edges are known.
        }
    }
}

/// An edge for simulation (indices into node array)
#[derive(Debug, Clone)]
pub struct SimEdge3D {
    pub source: usize,
    pub target: usize,
    /// Human-readable label for display
    pub label: String,
}

impl SimEdge3D {
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

/// Configuration for the 3D force simulation
#[derive(Debug, Clone)]
pub struct SimulationConfig3D {
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

impl Default for SimulationConfig3D {
    fn default() -> Self {
        Self {
            charge: -100.0,
            link_distance: 60.0,
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

/// 3D CPU force simulation
pub struct Simulation3D {
    pub nodes: Vec<SimNode3D>,
    pub edges: Vec<SimEdge3D>,
    pub config: SimulationConfig3D,
    /// Mapping from node ID to index
    #[allow(dead_code)]
    node_id_to_index: std::collections::HashMap<String, usize>,
}

impl Simulation3D {
    /// Create simulation from graph data
    pub fn from_graph_data(graph: &GraphData) -> Self {
        let total = graph.nodes.len();

        // Create nodes with initial positions
        let nodes: Vec<SimNode3D> = graph
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| SimNode3D::from_graph_node(n, i, total))
            .collect();

        // Build ID to index mapping
        let node_id_to_index: std::collections::HashMap<String, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id.clone(), i))
            .collect();

        // Create edges with indices
        let edges: Vec<SimEdge3D> = graph
            .edges
            .iter()
            .filter_map(|e| {
                let source = node_id_to_index.get(&e.source)?;
                let target = node_id_to_index.get(&e.target)?;
                let label = e
                    .label
                    .clone()
                    .unwrap_or_else(|| SimEdge3D::format_edge_type(e.edge_type));
                Some(SimEdge3D {
                    source: *source,
                    target: *target,
                    label,
                })
            })
            .collect();

        let components =
            sim_common::compute_components(nodes.len(), edges.iter().map(|e| (e.source, e.target)));
        let mut nodes = nodes;
        for (i, node) in nodes.iter_mut().enumerate() {
            node.component = components[i];
        }
        layout_by_component_3d(&mut nodes, &components);

        Self {
            nodes,
            edges,
            config: SimulationConfig3D::default(),
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

    /// Run one simulation tick
    pub fn tick(&mut self) {
        self.tick_with_fixed(&std::collections::HashSet::new());
    }

    /// Apply repulsion between all node pairs (3D version of the 2D
    /// many-body force; see `simulation::apply_many_body_force`).
    fn apply_many_body_force(&mut self) {
        const INTER_COMPONENT_SCALE: f32 = 5.0;
        let n = self.nodes.len();

        for i in 0..n {
            for j in (i + 1)..n {
                let dx = self.nodes[j].x - self.nodes[i].x;
                let dy = self.nodes[j].y - self.nodes[i].y;
                let dz = self.nodes[j].z - self.nodes[i].z;

                let dist_sq = dx * dx + dy * dy + dz * dz;
                let dist = dist_sq.sqrt().max(1.0);

                let scale = if self.nodes[i].component == self.nodes[j].component {
                    1.0
                } else {
                    INTER_COMPONENT_SCALE
                };
                let force = self.config.charge * scale / dist_sq;

                let fx = force * dx / dist;
                let fy = force * dy / dist;
                let fz = force * dz / dist;

                self.nodes[i].vx -= fx;
                self.nodes[i].vy -= fy;
                self.nodes[i].vz -= fz;
                self.nodes[j].vx += fx;
                self.nodes[j].vy += fy;
                self.nodes[j].vz += fz;
            }
        }
    }

    /// Apply spring force between connected nodes
    fn apply_link_force(&mut self) {
        for edge in &self.edges {
            let (source, target) = (edge.source, edge.target);

            let dx = self.nodes[target].x - self.nodes[source].x;
            let dy = self.nodes[target].y - self.nodes[source].y;
            let dz = self.nodes[target].z - self.nodes[source].z;

            let dist = (dx * dx + dy * dy + dz * dz).sqrt().max(1.0);

            // Hooke's law: F = k * (x - x0)
            let stretch = dist - self.config.link_distance;
            let force = self.config.link_strength * stretch / dist;

            let fx = force * dx;
            let fy = force * dy;
            let fz = force * dz;

            self.nodes[source].vx += fx;
            self.nodes[source].vy += fy;
            self.nodes[source].vz += fz;
            self.nodes[target].vx -= fx;
            self.nodes[target].vy -= fy;
            self.nodes[target].vz -= fz;
        }
    }

    /// Translate the whole layout so its centroid sits at origin (3D version
    /// of d3-force's `forceCenter`).
    fn recenter_centroid(&mut self) {
        let n = self.nodes.len();
        if n == 0 {
            return;
        }
        let mut sx = 0.0;
        let mut sy = 0.0;
        let mut sz = 0.0;
        for node in &self.nodes {
            sx += node.x;
            sy += node.y;
            sz += node.z;
        }
        let inv_n = 1.0 / n as f32;
        let cx = sx * inv_n;
        let cy = sy * inv_n;
        let cz = sz * inv_n;
        for node in &mut self.nodes {
            node.x -= cx;
            node.y -= cy;
            node.z -= cz;
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

    /// Run one simulation tick with fixed nodes that won't move
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

        // Velocity integration with a hard sphere clamp (3D version of the
        // 2D bound; see `simulation::tick_with_fixed`).
        const MAX_RADIUS: f32 = 250.0;
        const MAX_RADIUS_SQ: f32 = MAX_RADIUS * MAX_RADIUS;
        for (i, node) in self.nodes.iter_mut().enumerate() {
            if fixed_nodes.contains(&i) {
                node.vx = 0.0;
                node.vy = 0.0;
                node.vz = 0.0;
            } else {
                node.vx *= self.config.velocity_decay;
                node.vy *= self.config.velocity_decay;
                node.vz *= self.config.velocity_decay;
                node.x += node.vx * self.config.alpha;
                node.y += node.vy * self.config.alpha;
                node.z += node.vz * self.config.alpha;
                let dist_sq = node.x * node.x + node.y * node.y + node.z * node.z;
                if dist_sq > MAX_RADIUS_SQ {
                    let scale = MAX_RADIUS / dist_sq.sqrt();
                    node.x *= scale;
                    node.y *= scale;
                    node.z *= scale;
                    node.vx = 0.0;
                    node.vy = 0.0;
                    node.vz = 0.0;
                }
            }
        }

        // Resolve overlap via position correction (post-integration)
        self.apply_collide_force(fixed_nodes);

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
                let dz = self.nodes[j].z - self.nodes[i].z;
                let r_sum = self.nodes[i].radius + self.nodes[j].radius + padding;
                let dist_sq = dx * dx + dy * dy + dz * dz;

                if dist_sq >= r_sum * r_sum || dist_sq <= 0.0 {
                    continue;
                }

                let dist = dist_sq.sqrt();
                let overlap = (r_sum - dist) * strength;
                let nx = dx / dist;
                let ny = dy / dist;
                let nz = dz / dist;

                let (i_share, j_share) = match (i_fixed, j_fixed) {
                    (true, false) => (0.0, 1.0),
                    (false, true) => (1.0, 0.0),
                    _ => (0.5, 0.5),
                };

                self.nodes[i].x -= nx * overlap * i_share;
                self.nodes[i].y -= ny * overlap * i_share;
                self.nodes[i].z -= nz * overlap * i_share;
                self.nodes[j].x += nx * overlap * j_share;
                self.nodes[j].y += ny * overlap * j_share;
                self.nodes[j].z += nz * overlap * j_share;
            }
        }
    }

    /// Set a node's position directly (for dragging)
    pub fn set_node_position(&mut self, index: usize, x: f32, y: f32, z: f32) {
        if let Some(node) = self.nodes.get_mut(index) {
            node.x = x;
            node.y = y;
            node.z = z;
            // Reset velocity when manually positioning
            node.vx = 0.0;
            node.vy = 0.0;
            node.vz = 0.0;
        }
    }
}

/// Place each connected component on its own Fibonacci-sphere anchor so
/// disconnected pieces start in separate regions of 3D space. Single-component
/// graphs keep the default Fibonacci-sphere layout.
fn layout_by_component_3d(nodes: &mut [SimNode3D], components: &[usize]) {
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

    let big_radius = 150.0;
    let small_radius = 50.0;
    let golden_ratio = (1.0 + 5.0_f32.sqrt()) / 2.0;

    for (component_id, members) in by_component.iter().enumerate() {
        // Component anchor: Fibonacci point on the big sphere
        let i_f = component_id as f32;
        let n_f = num_components as f32;
        let theta = 2.0 * PI * i_f / golden_ratio;
        let phi = (1.0 - 2.0 * (i_f + 0.5) / n_f).acos();
        let cx = big_radius * phi.sin() * theta.cos();
        let cy = big_radius * phi.sin() * theta.sin();
        let cz = big_radius * phi.cos();

        // Spread members on a sub-sphere around the anchor
        let m = members.len();
        let m_f = m as f32;
        for (intra_idx, &node_idx) in members.iter().enumerate() {
            let ii = intra_idx as f32;
            let inner_theta = 2.0 * PI * ii / golden_ratio;
            let inner_phi = if m <= 1 {
                0.0
            } else {
                (1.0 - 2.0 * (ii + 0.5) / m_f).acos()
            };
            nodes[node_idx].x = cx + small_radius * inner_phi.sin() * inner_theta.cos();
            nodes[node_idx].y = cy + small_radius * inner_phi.sin() * inner_theta.sin();
            nodes[node_idx].z = cz + small_radius * inner_phi.cos();
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
                    uri_unresolved: false,
                    is_abstract: false,
                    kind_metadata: None,
                },
                GraphNode {
                    id: "b".to_string(),
                    label: "B".to_string(),
                    node_type: NodeType::Class,
                    color: [0.0, 1.0, 0.0, 1.0],
                    description: None,
                    uri: None,
                    uri_unresolved: false,
                    is_abstract: false,
                    kind_metadata: None,
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
        let sim = Simulation3D::from_graph_data(&graph);

        assert_eq!(sim.nodes.len(), 2);
        assert_eq!(sim.edges.len(), 1);
    }

    #[test]
    fn nodes_have_3d_positions() {
        let graph = make_test_graph();
        let sim = Simulation3D::from_graph_data(&graph);

        // All nodes should have z coordinates
        for node in &sim.nodes {
            // z should be non-zero for most nodes on Fibonacci sphere
            assert!(node.x.is_finite());
            assert!(node.y.is_finite());
            assert!(node.z.is_finite());
        }
    }

    #[test]
    fn simulation_runs() {
        let graph = make_test_graph();
        let mut sim = Simulation3D::from_graph_data(&graph);

        let initial_alpha = sim.config.alpha;
        sim.tick();

        assert!(sim.config.alpha < initial_alpha);
    }

    #[test]
    fn simulation_converges() {
        let graph = make_test_graph();
        let mut sim = Simulation3D::from_graph_data(&graph);

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
        let mut sim = Simulation3D::from_graph_data(&graph);

        assert_eq!(sim.nodes.len(), 0);
        assert_eq!(sim.edges.len(), 0);

        // Should not panic
        sim.tick();
        sim.run_to_convergence(100);
    }

    #[test]
    fn single_node_converges_to_origin() {
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
                uri_unresolved: false,
                is_abstract: false,
                kind_metadata: None,
            }],
            edges: vec![],
        };
        let mut sim = Simulation3D::from_graph_data(&graph);

        let initial_dist =
            (sim.nodes[0].x.powi(2) + sim.nodes[0].y.powi(2) + sim.nodes[0].z.powi(2)).sqrt();

        sim.run_to_convergence(500);

        let final_dist =
            (sim.nodes[0].x.powi(2) + sim.nodes[0].y.powi(2) + sim.nodes[0].z.powi(2)).sqrt();

        assert!(
            final_dist < initial_dist,
            "Single node should move toward origin"
        );
    }

    #[test]
    fn fibonacci_sphere_distributes_nodes() {
        // Single connected component — multi-component graphs use a different
        // per-component layout that doesn't keep all nodes equidistant.
        let graph = GraphData {
            schema_name: "multi".to_string(),
            schema_title: None,
            format_version: "1.0".to_string(),
            nodes: (0..10)
                .map(|i| GraphNode {
                    id: format!("node{}", i),
                    label: format!("Node {}", i),
                    node_type: NodeType::Class,
                    color: [1.0, 0.0, 0.0, 1.0],
                    description: None,
                    uri: None,
                    uri_unresolved: false,
                    is_abstract: false,
                    kind_metadata: None,
                })
                .collect(),
            edges: (1..10)
                .map(|i| GraphEdge {
                    source: "node0".to_string(),
                    target: format!("node{}", i),
                    edge_type: EdgeType::SubclassOf,
                    label: None,
                })
                .collect(),
        };
        let sim = Simulation3D::from_graph_data(&graph);

        // All nodes should be roughly equidistant from origin
        let distances: Vec<f32> = sim
            .nodes
            .iter()
            .map(|n| (n.x.powi(2) + n.y.powi(2) + n.z.powi(2)).sqrt())
            .collect();

        let avg_dist: f32 = distances.iter().sum::<f32>() / distances.len() as f32;

        // All should be within 20% of average (Fibonacci sphere property)
        for dist in distances {
            assert!(
                (dist - avg_dist).abs() < avg_dist * 0.2,
                "Fibonacci sphere should distribute evenly"
            );
        }
    }

    #[test]
    fn collide_force_prevents_overlap_3d() {
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
                    uri_unresolved: false,
                    is_abstract: false,
                    kind_metadata: None,
                },
                GraphNode {
                    id: "b".to_string(),
                    label: "B".to_string(),
                    node_type: NodeType::Class,
                    color: [0.0, 1.0, 0.0, 1.0],
                    description: None,
                    uri: None,
                    uri_unresolved: false,
                    is_abstract: false,
                    kind_metadata: None,
                },
            ],
            edges: vec![],
        };
        let mut sim = Simulation3D::from_graph_data(&graph);

        // Force overlap along the x-axis.
        sim.nodes[0].x = 0.0;
        sim.nodes[0].y = 0.0;
        sim.nodes[0].z = 0.0;
        sim.nodes[1].x = 1.0;
        sim.nodes[1].y = 0.0;
        sim.nodes[1].z = 0.0;

        let r_sum = sim.nodes[0].radius + sim.nodes[1].radius;

        for _ in 0..50 {
            sim.tick();
        }

        let dx = sim.nodes[1].x - sim.nodes[0].x;
        let dy = sim.nodes[1].y - sim.nodes[0].y;
        let dz = sim.nodes[1].z - sim.nodes[0].z;
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();

        assert!(
            dist >= r_sum,
            "3D collide force should resolve overlap: dist={dist}, r_sum={r_sum}"
        );
    }
}
