//! GPU-accelerated force-directed graph layout and 3D visualization
//!
//! This module provides a high-performance implementation of force-directed
//! graph layout using WebGPU compute shaders, plus 3D rendering of graphs.
//! It can handle large graphs (5,000+ nodes) at interactive frame rates.
//!
//! # Features
//!
//! - **GPU Compute**: All force calculations run on the GPU via compute shaders
//! - **3D Support**: Designed for 3D from the ground up (positions are vec3)
//! - **3D Rendering**: Render nodes as spheres and edges as lines
//! - **D3 Compatible**: API and behavior modeled after d3-force
//! - **WebGPU Ready**: Uses wgpu, which targets WebGPU in browsers
//!
//! # Example: Force Simulation
//!
//! ```rust,ignore
//! use panschema::gpu::{GpuSimulation, GpuNode, GpuEdge};
//!
//! // Create nodes
//! let nodes = vec![
//!     GpuNode::new(0.0, 0.0, 0.0),
//!     GpuNode::new(100.0, 0.0, 0.0),
//!     GpuNode::new(50.0, 86.6, 0.0),
//! ];
//!
//! // Create edges (triangle)
//! let edges = vec![
//!     GpuEdge::new(0, 1).with_distance(100.0),
//!     GpuEdge::new(1, 2).with_distance(100.0),
//!     GpuEdge::new(2, 0).with_distance(100.0),
//! ];
//!
//! // Create simulation
//! let mut sim = GpuSimulation::new(&nodes, &edges);
//!
//! // Run to convergence
//! sim.run_to_convergence();
//!
//! // Read back results
//! let result = sim.read_nodes();
//! for node in &result {
//!     println!("Position: {:?}", node.position);
//! }
//! ```
//!
//! # Example: 3D Rendering
//!
//! ```rust,ignore
//! use panschema::gpu::{GpuRenderer, Camera3D, NodeInstance, EdgeInstance, RenderConfig};
//! use std::sync::Arc;
//!
//! // Create renderer (device/queue from wgpu)
//! let renderer = GpuRenderer::new(Arc::new(device), Arc::new(queue), RenderConfig::default());
//!
//! // Set up camera
//! let camera = Camera3D::new(800.0 / 600.0);
//!
//! // Create render instances from simulation results
//! let node_instances: Vec<NodeInstance> = sim_nodes.iter()
//!     .map(|n| NodeInstance::new(n.position[0], n.position[1], n.position[2]))
//!     .collect();
//!
//! let edge_instances: Vec<EdgeInstance> = edges.iter()
//!     .map(|e| EdgeInstance::new(
//!         sim_nodes[e.source as usize].position,
//!         sim_nodes[e.target_node as usize].position,
//!     ))
//!     .collect();
//!
//! // Render frame
//! renderer.render(&camera, &node_instances, &edge_instances);
//! ```
//!
//! # Forces
//!
//! The simulation includes these forces:
//!
//! - **Link Force**: Spring forces between connected nodes (edges)
//! - **Many-Body Force**: Repulsion between all nodes (Coulomb's law)
//! - **Center Force**: Gravity toward the center point
//!
//! # Performance
//!
//! The GPU implementation provides significant speedups for large graphs,
//! especially as node count increases (GPU parallelism scales well).
//!
//! Note: Current implementation uses O(n²) brute force for many-body.
//! Barnes-Hut optimization (O(n log n)) is planned for even better scaling.

// Simulation modules
mod shaders;
mod simulation;
mod types;

// Rendering modules
pub mod camera;
pub mod geometry;
mod render_shaders;
mod renderer;

// Re-export simulation types
pub use shaders::{ForceShaders, combined_force_shader};
pub use simulation::GpuSimulation;
pub use types::{
    // Render types
    CameraUniforms,
    // Simulation default constants
    DEFAULT_ALPHA_DECAY_TICKS,
    DEFAULT_ALPHA_MIN,
    DEFAULT_CHARGE,
    DEFAULT_DISTANCE_MAX,
    DEFAULT_DISTANCE_MIN,
    // Render default constants
    DEFAULT_EDGE_ALPHA,
    DEFAULT_EDGE_DISTANCE,
    DEFAULT_EDGE_STRENGTH,
    DEFAULT_MAX_VELOCITY,
    DEFAULT_NODE_RADIUS,
    DEFAULT_THETA,
    DEFAULT_VELOCITY_DECAY,
    EdgeInstance,
    // Simulation types
    GpuEdge,
    GpuNode,
    GpuSimulationConfig,
    NOT_FIXED,
    NodeInstance,
    RenderConfig,
    SimulationUniforms,
};

// Re-export rendering types
pub use camera::Camera3D;
pub use geometry::{MeshVertex, icosphere};
pub use render_shaders::{edge_shader, node_shader};
pub use renderer::{GpuRenderer, create_render_device};
