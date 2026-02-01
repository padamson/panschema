//! 3D Force simulation for graph layout
//!
//! Extends the 2D simulation with z-axis for 3D visualization.

use crate::graph_types::{EdgeType, GraphData, GraphNode};

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
            radius: 12.0,
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
    /// Center force strength
    pub center_strength: f32,
    /// Velocity decay (friction)
    pub velocity_decay: f32,
    /// Current alpha (simulation temperature)
    pub alpha: f32,
    /// Minimum alpha before stopping
    pub alpha_min: f32,
    /// Alpha decay rate
    pub alpha_decay: f32,
}

impl Default for SimulationConfig3D {
    fn default() -> Self {
        Self {
            charge: -50.0, // Stronger repulsion for 3D
            link_distance: 60.0,
            link_strength: 1.0,
            center_strength: 0.08,
            velocity_decay: 0.6,
            alpha: 1.0,
            alpha_min: 0.001,
            alpha_decay: 1.0 - 0.001_f32.powf(1.0 / 300.0),
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

    /// Run one simulation tick
    pub fn tick(&mut self) {
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

        // Apply center force (gravity toward origin)
        self.apply_center_force();

        // Apply velocity and decay
        for node in &mut self.nodes {
            node.vx *= self.config.velocity_decay;
            node.vy *= self.config.velocity_decay;
            node.vz *= self.config.velocity_decay;
            node.x += node.vx * self.config.alpha;
            node.y += node.vy * self.config.alpha;
            node.z += node.vz * self.config.alpha;
        }

        // Decay alpha
        self.config.alpha += (self.config.alpha_decay - 1.0) * self.config.alpha;
    }

    /// Apply repulsion between all node pairs
    fn apply_many_body_force(&mut self) {
        let n = self.nodes.len();

        for i in 0..n {
            for j in (i + 1)..n {
                let dx = self.nodes[j].x - self.nodes[i].x;
                let dy = self.nodes[j].y - self.nodes[i].y;
                let dz = self.nodes[j].z - self.nodes[i].z;

                let dist_sq = dx * dx + dy * dy + dz * dz;
                let dist = dist_sq.sqrt().max(1.0);

                // Coulomb's law: F = k * q1 * q2 / r^2
                let force = self.config.charge / dist_sq;

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

    /// Apply centering force toward origin
    fn apply_center_force(&mut self) {
        for node in &mut self.nodes {
            node.vx -= node.x * self.config.center_strength;
            node.vy -= node.y * self.config.center_strength;
            node.vz -= node.z * self.config.center_strength;
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
                is_abstract: false,
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
                    is_abstract: false,
                })
                .collect(),
            edges: vec![],
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
}
