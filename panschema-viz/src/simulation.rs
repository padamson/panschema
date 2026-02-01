//! CPU-based force simulation for graph layout
//!
//! Implements a simple force-directed layout algorithm that runs in the browser.
//! Uses the same physics as the GPU version but runs on CPU for compatibility.

use crate::graph_types::{EdgeType, GraphData, GraphNode};

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
            radius: 8.0,
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

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            charge: -30.0,
            link_distance: 50.0,
            link_strength: 1.0,
            center_strength: 0.1,
            velocity_decay: 0.6,
            alpha: 1.0,
            alpha_min: 0.001,
            alpha_decay: 1.0 - 0.001_f32.powf(1.0 / 300.0),
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
            node.x += node.vx * self.config.alpha;
            node.y += node.vy * self.config.alpha;
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

                let dist_sq = dx * dx + dy * dy;
                let dist = dist_sq.sqrt().max(1.0);

                // Coulomb's law: F = k * q1 * q2 / r^2
                let force = self.config.charge / dist_sq;

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

    /// Apply centering force toward origin
    fn apply_center_force(&mut self) {
        for node in &mut self.nodes {
            node.vx -= node.x * self.config.center_strength;
            node.vy -= node.y * self.config.center_strength;
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

        // Distance should remain significant (nodes don't collapse)
        assert!(final_dist > 10.0, "Nodes should maintain separation");
        // But center force should bring them closer than infinite repulsion would
        assert!(
            final_dist < initial_dist * 2.0,
            "Center force should limit spread"
        );
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
