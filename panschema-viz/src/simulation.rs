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
    /// Create from GraphNode with a deterministic initial position on a
    /// circle of radius 100.
    pub fn from_graph_node(node: &GraphNode, index: usize, total: usize) -> Self {
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
    /// Harmonic pull toward `x = 0`, applied per tick as `vx -= gx · x`.
    /// Zero (the default) preserves the historical circular equilibrium.
    /// Use `with_aspect_ratio` to derive non-zero values from a target
    /// bbox aspect.
    pub gravity_x_strength: f32,
    /// Harmonic pull toward `y = 0`, applied per tick as `vy -= gy · y`.
    /// Setting `gravity_y / gravity_x = (w/h)³` biases the post-settle
    /// bbox aspect toward `w/h` (derivation: equilibrium `d_a³ = R / g_a`
    /// for cluster repulsion `R/d²` balanced against centering `g·d`).
    pub gravity_y_strength: f32,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            // Tuned for legible labels on schema sizes we care about
            // (~30-100 connected nodes): repulsion strong enough to
            // separate siblings of a tree node, link rest length long
            // enough that adjacent labels rarely touch. `from_graph_data`
            // applies further √N scaling on top so very-small and
            // very-large graphs both fit on screen.
            charge: -120.0,
            link_distance: 100.0,
            link_strength: 1.0,
            velocity_decay: 0.6,
            alpha: 1.0,
            alpha_min: 0.001,
            alpha_decay: 1.0 - 0.001_f32.powf(1.0 / 300.0),
            collide_padding: 2.0,
            collide_strength: 0.7,
            gravity_x_strength: 0.0,
            gravity_y_strength: 0.0,
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
    /// Create simulation from graph data.
    ///
    /// Force parameters scale with `√N` so the equilibrium cluster
    /// size grows with the node count. Without this, a 100-node
    /// cluster packs into the same world-space radius as a 10-node
    /// cluster, with predictable label overlap and unreadable
    /// rendering on large schemas. The `√N` ratio is the same one
    /// d3-force uses (`linkDistance().strength()` and `charge()` both
    /// scale this way under the hood).
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

        // Scale collide padding with √N. The collide force enforces a
        // minimum geometric distance between every node pair, which is
        // the only force in the system that breaks up "siblings stacked
        // on top of each other" for high-branching tree clusters
        // (springs and repulsion alone can't, because siblings at the
        // same link_distance from a parent are forced close together
        // at angle 2π/B). Scaling with √N keeps small graphs from
        // looking sparse while still spreading dense ones.
        //
        // Modest √N scaling on link_distance + charge so very-small
        // graphs (≤10 nodes) still get visible spacing even before the
        // collide force kicks in. Aggressive scaling here backfires:
        // it blows the world bbox out past the viewport and
        // fit_to_bounds zooms back in to fit, making the rendered
        // cluster tinier than before.
        let mut config = SimulationConfig::default();
        let n_scale = (total.max(1) as f32).sqrt();
        config.link_distance *= 1.0 + n_scale * 0.10;
        config.charge *= 1.0 + n_scale * 0.10;
        config.collide_padding = 4.0 + n_scale * 4.0;

        Self {
            nodes,
            edges,
            config,
            node_id_to_index,
        }
    }

    /// Configure anisotropic axial centering so the post-settle bounding
    /// box approximates the given aspect ratio. The y-pull is scaled by
    /// `(w/h)³` relative to the x-pull, which (given `1/d²` repulsion
    /// from the connected cluster on each isolated node) produces an
    /// equilibrium where `d_x / d_y ≈ w/h`.
    ///
    /// The absolute strength `GRAVITY_X_BASE` is calibrated against
    /// scimantic-v0.2.0-scale schemas (~75 connected + ~10 isolated).
    /// Smaller graphs settle at proportionally smaller radii (still
    /// aspect-correct), which is generally fine — small graphs have less
    /// reason to spread out.
    pub fn with_aspect_ratio(mut self, w: u32, h: u32) -> Self {
        // Split a base strength asymmetrically so the *ratio* matches
        // (gy / gx) = (w/h)³ — the value needed to balance 1/d² cluster
        // repulsion against linear centering at a target d_x/d_y = w/h
        // — while the geometric mean √(gx · gy) stays equal to the
        // base. This keeps both axes near the same convergence speed
        // regardless of which aspect is configured: landscape and
        // portrait reach equilibrium in symmetric numbers of ticks.
        const GRAVITY_BASE: f32 = 0.003;
        let wf = w as f32;
        let hf = h as f32;
        // gx · gy = base² ; gy / gx = (w/h)³  →  solve.
        let aspect_sqrt_cubed = (wf / hf).powf(1.5);
        self.config.gravity_x_strength = GRAVITY_BASE / aspect_sqrt_cubed;
        self.config.gravity_y_strength = GRAVITY_BASE * aspect_sqrt_cubed;
        self
    }

    /// Check if simulation is still running
    pub fn is_running(&self) -> bool {
        self.config.alpha > self.config.alpha_min
    }

    /// Overwrite node positions with the supplied seeds and stop
    /// physics. `positions[i]` becomes node `i`'s `(x, y)` for every
    /// `i < min(self.nodes.len(), positions.len())`; extra positions
    /// or extra nodes are silently ignored.
    ///
    /// Setting `alpha = alpha_min` makes `is_running()` return false so
    /// the per-frame `tick_with_fixed` becomes a no-op. Used by static
    /// (non-force-directed) layouts that produce final positions up
    /// front and don't want force interaction afterwards. Drag handlers
    /// can still reposition individual nodes via `set_node_position`.
    pub fn freeze_at(&mut self, positions: &[(f32, f32)]) {
        for (node, &(x, y)) in self.nodes.iter_mut().zip(positions.iter()) {
            node.x = x;
            node.y = y;
            node.vx = 0.0;
            node.vy = 0.0;
        }
        self.config.alpha = self.config.alpha_min;
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

        // Apply anisotropic axial centering. Default strengths are 0.0
        // (no-op); `with_aspect_ratio` derives non-zero values from the
        // configured aspect so the post-settle bounding box approximates
        // that aspect.
        self.apply_gravity_force(fixed_nodes);

        // Velocity integration with a hard sphere clamp — bounds drift so
        // fit_to_bounds always frames a sensible region. Set well past
        // the equilibrium radius the centering force produces for the
        // graph sizes we care about (~75-node clusters with ~10
        // isolated nodes), so the clamp is a safety net for runaway
        // dynamics rather than the dominant geometry-setting force.
        const MAX_RADIUS: f32 = 800.0;
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

    /// Apply harmonic axial centering: `vx -= gx · x`, `vy -= gy · y`.
    /// Strengths default to 0.0 (no-op); `with_aspect_ratio` sets them
    /// so the equilibrium bbox approximates a target aspect ratio.
    /// Fixed (pinned) nodes are skipped.
    fn apply_gravity_force(&mut self, fixed_nodes: &std::collections::HashSet<usize>) {
        let gx = self.config.gravity_x_strength;
        let gy = self.config.gravity_y_strength;
        if gx == 0.0 && gy == 0.0 {
            return;
        }
        let any_fixed = !fixed_nodes.is_empty();
        for (i, node) in self.nodes.iter_mut().enumerate() {
            if any_fixed && fixed_nodes.contains(&i) {
                continue;
            }
            node.vx -= gx * node.x;
            node.vy -= gy * node.y;
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

    // The largest component is placed at origin; smaller components ring
    // around it. Placing the largest off-origin would interact poorly
    // with the anisotropic gravity in `with_aspect_ratio`: gravity can
    // only partially correct the offset within the alpha schedule, so
    // any off-origin start of the dominant component bakes a layout
    // asymmetry into the rest of the simulation.
    let largest_component_id = by_component
        .iter()
        .enumerate()
        .max_by_key(|(_, members)| members.len())
        .map(|(id, _)| id);

    let big_radius = 150.0;
    let small_radius = 50.0;
    // Outer components share the ring; the largest occupies the center,
    // so divide the full circle by the count of non-largest components.
    let outer_count = if largest_component_id.is_some() {
        num_components - 1
    } else {
        num_components
    };
    let arc_per_outer = if outer_count > 0 {
        2.0 * PI / outer_count as f32
    } else {
        0.0
    };

    let mut outer_index: usize = 0;
    for (component_id, members) in by_component.iter().enumerate() {
        let (cx, cy) = if Some(component_id) == largest_component_id {
            (0.0, 0.0)
        } else {
            let component_angle = outer_index as f32 * arc_per_outer;
            outer_index += 1;
            (
                big_radius * component_angle.cos(),
                big_radius * component_angle.sin(),
            )
        };
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

    #[test]
    fn from_graph_node_places_nodes_on_circle_at_correct_angles() {
        // Build a 4-node graph and check that each node lands at the
        // expected angle on a circle of radius 100. Catches mutants
        // that alter the `2π · i / total` formula (e.g., `*` → `+` or
        // `/` → `%`), which would put nodes at wildly wrong angles.
        let g = GraphData {
            schema_name: "ring".to_string(),
            schema_title: None,
            format_version: "1.0".to_string(),
            nodes: (0..4)
                .map(|i| GraphNode {
                    id: format!("n{i}"),
                    label: format!("N{i}"),
                    node_type: NodeType::Class,
                    color: [1.0, 0.0, 0.0, 1.0],
                    description: None,
                    uri: None,
                    is_abstract: false,
                })
                .collect(),
            edges: vec![],
        };
        let nodes: Vec<SimNode> = g
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| SimNode::from_graph_node(n, i, 4))
            .collect();
        // All nodes on the radius-100 circle.
        for n in &nodes {
            let r = (n.x * n.x + n.y * n.y).sqrt();
            assert!((r - 100.0).abs() < 0.01, "expected radius 100, got {r}");
        }
        // Specific angle assertions catch `*` → `+`, `/` → `%`, etc.
        // Index 0 → angle 0 → (100, 0).
        assert!((nodes[0].x - 100.0).abs() < 0.01);
        assert!(nodes[0].y.abs() < 0.01);
        // Index 1 → angle π/2 → (0, 100).
        assert!(nodes[1].x.abs() < 0.01);
        assert!((nodes[1].y - 100.0).abs() < 0.01);
        // Index 2 → angle π → (-100, 0).
        assert!((nodes[2].x + 100.0).abs() < 0.01);
        assert!(nodes[2].y.abs() < 0.01);
        // Index 3 → angle 3π/2 → (0, -100).
        assert!(nodes[3].x.abs() < 0.01);
        assert!((nodes[3].y + 100.0).abs() < 0.01);
    }

    fn make_ring_graph(n: usize) -> GraphData {
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
            schema_name: "ring".to_string(),
            schema_title: None,
            format_version: "1.0".to_string(),
            nodes,
            edges,
        }
    }

    /// Connected ring of `connected_n` + `isolated_n` disconnected
    /// singletons. Edge-per-node ratio = 1, matching scimantic v0.2.0
    /// density. Used to exercise centering's effect on isolated nodes.
    fn make_lopsided_graph(connected_n: usize, isolated_n: usize) -> GraphData {
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
            schema_name: "lopsided".to_string(),
            schema_title: None,
            format_version: "1.0".to_string(),
            nodes,
            edges,
        }
    }

    fn bbox(sim: &CpuSimulation) -> (f32, f32) {
        let xs: Vec<f32> = sim.nodes.iter().map(|n| n.x).collect();
        let ys: Vec<f32> = sim.nodes.iter().map(|n| n.y).collect();
        let w = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
            - xs.iter().cloned().fold(f32::INFINITY, f32::min);
        let h = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
            - ys.iter().cloned().fold(f32::INFINITY, f32::min);
        (w, h)
    }

    fn dist_from_centroid(sim: &CpuSimulation, i: usize) -> f32 {
        let n = sim.nodes.len() as f32;
        let cx: f32 = sim.nodes.iter().map(|n| n.x).sum::<f32>() / n;
        let cy: f32 = sim.nodes.iter().map(|n| n.y).sum::<f32>() / n;
        let dx = sim.nodes[i].x - cx;
        let dy = sim.nodes[i].y - cy;
        (dx * dx + dy * dy).sqrt()
    }

    #[test]
    fn gravity_strengths_default_to_zero() {
        let config = SimulationConfig::default();
        assert_eq!(config.gravity_x_strength, 0.0);
        assert_eq!(config.gravity_y_strength, 0.0);
    }

    #[test]
    fn with_aspect_ratio_sets_gravity_strengths_in_cube_ratio() {
        // gy / gx = (w/h)³, derived from equilibrium d_a³ = R/g_a for
        // 1/d² cluster repulsion. (16,8) is 2:1, so the cube ratio is 8.
        let sim = CpuSimulation::from_graph_data(&make_ring_graph(5)).with_aspect_ratio(16, 8);
        let ratio = sim.config.gravity_y_strength / sim.config.gravity_x_strength;
        assert!(
            (ratio - 8.0).abs() < 0.01,
            "expected gy/gx ≈ 8 for (16,8) aspect, got {ratio}"
        );
    }

    #[test]
    fn default_aspect_settles_to_roughly_square_bbox() {
        // No `with_aspect_ratio` → gravity strengths are 0.0 → historical
        // circular equilibrium preserved.
        let graph = make_ring_graph(15);
        let mut sim = CpuSimulation::from_graph_data(&graph);
        for _ in 0..400 {
            sim.tick();
        }
        let (w, h) = bbox(&sim);
        let aspect = w / h;
        assert!(
            (0.6..=1.66).contains(&aspect),
            "default aspect should settle near 1:1, got w/h={aspect}"
        );
    }

    #[test]
    fn aspect_16_8_biases_layout_wider_than_tall() {
        // Anisotropic axial centering biases the equilibrium layout
        // toward the configured aspect, but the strength of the bias
        // depends on graph topology — tightly-connected graphs are
        // dominated by spring forces and bias less than loosely
        // connected ones. This test asserts directional bias (wider
        // than tall) without pinning a specific ratio, which is what
        // forceX/forceY can reliably guarantee across graph shapes.
        let graph = make_lopsided_graph(20, 8);
        let mut sim = CpuSimulation::from_graph_data(&graph).with_aspect_ratio(16, 8);
        sim.run_to_convergence(500);
        let (w, h) = bbox(&sim);
        assert!(
            w > h * 1.3,
            "aspect (16,8) should bias bbox toward wider-than-tall (\
             w > h * 1.3); got w={w:.1} h={h:.1} (ratio {:.2})",
            w / h
        );
    }

    #[test]
    fn aspect_8_16_biases_layout_taller_than_wide() {
        let graph = make_lopsided_graph(20, 8);
        let mut sim = CpuSimulation::from_graph_data(&graph).with_aspect_ratio(8, 16);
        sim.run_to_convergence(500);
        let (w, h) = bbox(&sim);
        assert!(
            h > w * 1.3,
            "aspect (8,16) should bias bbox toward taller-than-wide (\
             h > w * 1.3); got w={w:.1} h={h:.1} (ratio {:.2})",
            w / h
        );
    }

    #[test]
    fn lopsided_graph_has_no_extreme_outlier_nodes() {
        // No single node should drift far enough from the cluster to
        // dominate fit_to_bounds, which would zoom the whole layout
        // out and shrink the cluster into an unreadable patch. The
        // metric — max distance from centroid ≤ 3× median — catches
        // the "one node escaped" failure mode without pinning specific
        // positions.
        let graph = make_lopsided_graph(30, 5);
        let mut sim = CpuSimulation::from_graph_data(&graph).with_aspect_ratio(16, 8);
        sim.run_to_convergence(500);
        let mut dists: Vec<f32> = (0..sim.nodes.len())
            .map(|i| dist_from_centroid(&sim, i))
            .collect();
        dists.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = dists[dists.len() / 2];
        let max = *dists.last().unwrap();
        assert!(
            max <= median * 3.0,
            "no node should be more than 3× median distance from \
             centroid; got max={max:.1}, median={median:.1} (ratio {:.2})",
            max / median
        );
    }

    #[test]
    fn lopsided_graph_isolated_nodes_distribute_around_perimeter() {
        // Without anisotropic centering, isolated nodes that happen to
        // be numbered consecutively (singleton property nodes after the
        // connected classes) pile up on one side of the layout. Asserts
        // the largest open arc among isolated-node angles is < π.
        let graph = make_lopsided_graph(30, 6);
        let mut sim = CpuSimulation::from_graph_data(&graph).with_aspect_ratio(16, 8);
        sim.run_to_convergence(500);
        let n = sim.nodes.len() as f32;
        let cx: f32 = sim.nodes.iter().map(|n| n.x).sum::<f32>() / n;
        let cy: f32 = sim.nodes.iter().map(|n| n.y).sum::<f32>() / n;
        let mut isolated_angles: Vec<f32> = (30..36)
            .map(|i| (sim.nodes[i].y - cy).atan2(sim.nodes[i].x - cx))
            .collect();
        isolated_angles.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut max_gap: f32 = 0.0;
        for w in isolated_angles.windows(2) {
            max_gap = max_gap.max(w[1] - w[0]);
        }
        let wrap_gap = std::f32::consts::TAU
            - (isolated_angles.last().unwrap() - isolated_angles.first().unwrap());
        max_gap = max_gap.max(wrap_gap);
        assert!(
            max_gap < std::f32::consts::PI,
            "largest open arc among isolated-node angles should be < π; \
             got {max_gap:.2} rad. Angles = {isolated_angles:?}"
        );
    }

    #[test]
    fn lopsided_graph_bbox_fills_a_substantial_extent() {
        // Anisotropic centering must not over-tighten the cluster. The
        // 200-world-unit floor is a hard "didn't collapse to origin"
        // check — large enough to catch the failure mode where every
        // node sits within ~50 units of the centroid, small enough that
        // the test isn't a self-fulfilling proxy for our own scaling.
        let graph = make_lopsided_graph(30, 5);
        let mut sim = CpuSimulation::from_graph_data(&graph).with_aspect_ratio(16, 8);
        sim.run_to_convergence(500);
        let (w, _h) = bbox(&sim);
        let floor = 200.0;
        assert!(
            w >= floor,
            "post-settle bbox width should be >= {floor:.0} (catches \
             'collapsed to origin'); got {w:.1}"
        );
    }

    #[test]
    fn freeze_at_overwrites_positions_and_halts_simulation() {
        // freeze_at is the static-layout integration point: it accepts
        // pre-computed final positions and stops the per-tick physics
        // so the simulation acts purely as a position + interaction
        // container. The test asserts both contracts in one pass.
        let graph = make_ring_graph(5);
        let mut sim = CpuSimulation::from_graph_data(&graph);
        assert!(sim.is_running(), "simulation must start hot");

        let seed = vec![
            (10.0, 20.0),
            (30.0, 40.0),
            (50.0, 60.0),
            (70.0, 80.0),
            (90.0, 100.0),
        ];
        sim.freeze_at(&seed);

        for (i, (sx, sy)) in seed.iter().enumerate() {
            assert_eq!(sim.nodes[i].x, *sx);
            assert_eq!(sim.nodes[i].y, *sy);
            assert_eq!(sim.nodes[i].vx, 0.0);
            assert_eq!(sim.nodes[i].vy, 0.0);
        }
        assert!(!sim.is_running(), "freeze_at must halt physics");

        // tick_with_fixed early-exits when not running, so the frozen
        // positions survive an arbitrary number of frames.
        for _ in 0..10 {
            sim.tick();
        }
        for (i, (sx, sy)) in seed.iter().enumerate() {
            assert_eq!(sim.nodes[i].x, *sx);
            assert_eq!(sim.nodes[i].y, *sy);
        }
    }

    #[test]
    fn dragging_one_node_in_a_frozen_simulation_leaves_other_nodes_untouched() {
        // Contract that the Visualization wrapper depends on for static
        // layouts (KK / Sugiyama): after `freeze_at`, the drag handler
        // moves only the dragged node via `set_node_position` and
        // per-tick physics stays a no-op. Other nodes must NOT drift —
        // otherwise the static layout decays into force-directed the
        // moment the user touches any node. (The bug this test pins:
        // the drag handler used to reheat unconditionally, restoring
        // alpha above alpha_min and re-enabling physics for every node.)
        let graph = make_ring_graph(5);
        let mut sim = CpuSimulation::from_graph_data(&graph);

        let seed = vec![
            (10.0, 20.0),
            (30.0, 40.0),
            (50.0, 60.0),
            (70.0, 80.0),
            (90.0, 100.0),
        ];
        sim.freeze_at(&seed);

        // Drag node 2 to a new position. Static-layout drag path:
        // direct positional write, NO reheat.
        sim.set_node_position(2, 500.0, 600.0);

        // Tick a few frames — `is_running()` is false, so each tick is
        // a no-op. None of the non-dragged nodes should drift.
        for _ in 0..10 {
            sim.tick();
        }

        assert!(
            !sim.is_running(),
            "frozen simulation must stay halted after a single-node drag"
        );
        // The dragged node landed where we put it.
        assert_eq!(sim.nodes[2].x, 500.0);
        assert_eq!(sim.nodes[2].y, 600.0);
        // Every other node is still where the static layout placed it.
        for i in [0_usize, 1, 3, 4] {
            assert_eq!(sim.nodes[i].x, seed[i].0, "node {i} drifted on x");
            assert_eq!(sim.nodes[i].y, seed[i].1, "node {i} drifted on y");
        }
    }
}
