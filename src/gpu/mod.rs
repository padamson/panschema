//! GPU-accelerated force-directed graph layout (d3-force on GPU)
//!
//! This module provides a high-performance implementation of force-directed
//! graph layout using WebGPU compute shaders. It can handle large graphs
//! (10,000+ nodes) at interactive frame rates.
//!
//! # Features
//!
//! - **GPU Compute**: All force calculations run on the GPU via compute shaders
//! - **3D Support**: Designed for 3D from the ground up (positions are vec3)
//! - **D3 Compatible**: API and behavior modeled after d3-force
//! - **WebGPU Ready**: Uses wgpu, which targets WebGPU in browsers
//!
//! # Example
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

mod shaders;
mod simulation;
mod types;

pub use shaders::{ForceShaders, combined_force_shader};
pub use simulation::GpuSimulation;
pub use types::{
    // Default constants for customization
    DEFAULT_ALPHA_DECAY_TICKS,
    DEFAULT_ALPHA_MIN,
    DEFAULT_CHARGE,
    DEFAULT_DISTANCE_MAX,
    DEFAULT_DISTANCE_MIN,
    DEFAULT_EDGE_DISTANCE,
    DEFAULT_EDGE_STRENGTH,
    DEFAULT_MAX_VELOCITY,
    DEFAULT_THETA,
    DEFAULT_VELOCITY_DECAY,
    GpuEdge,
    GpuNode,
    GpuSimulationConfig,
    NOT_FIXED,
    SimulationUniforms,
};
