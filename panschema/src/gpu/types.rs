//! GPU buffer types for force simulation and rendering
//!
//! These types are designed to be uploaded directly to GPU buffers.
//! All use f32 for GPU compatibility and are repr(C) for predictable layout.

use bytemuck::{Pod, Zeroable};

// =============================================================================
// Default Constants
// =============================================================================

/// Default charge for many-body repulsion (negative = repulsion, matches D3.js)
pub const DEFAULT_CHARGE: f32 = -30.0;

/// Default edge/link distance (rest length of spring)
pub const DEFAULT_EDGE_DISTANCE: f32 = 30.0;

/// Default edge/link strength (spring constant)
pub const DEFAULT_EDGE_STRENGTH: f32 = 1.0;

/// Default velocity decay factor (0-1, applied each tick)
pub const DEFAULT_VELOCITY_DECAY: f32 = 0.6;

/// Default Barnes-Hut theta approximation threshold (0 = exact, 1 = fast)
pub const DEFAULT_THETA: f32 = 0.9;

/// Default minimum distance for force calculations (avoids singularity)
pub const DEFAULT_DISTANCE_MIN: f32 = 1.0;

/// Default maximum distance for many-body force calculations (cutoff optimization)
pub const DEFAULT_DISTANCE_MAX: f32 = 1000.0;

/// Default maximum velocity (prevents numerical explosion)
pub const DEFAULT_MAX_VELOCITY: f32 = 100.0;

/// Default minimum alpha before simulation stops
pub const DEFAULT_ALPHA_MIN: f32 = 0.001;

/// Default number of ticks for alpha decay (D3.js uses 300)
pub const DEFAULT_ALPHA_DECAY_TICKS: f32 = 300.0;

/// A node in the GPU force simulation.
///
/// Layout matches WGSL struct for direct buffer upload.
/// Uses f32 for GPU compatibility (GPUs prefer f32 over f64).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct GpuNode {
    /// Position in 3D space
    pub position: [f32; 3],
    /// Charge/strength for many-body force (negative = repulsion)
    pub charge: f32,
    /// Velocity in 3D space
    pub velocity: [f32; 3],
    /// Mass (used for force accumulation)
    pub mass: f32,
    /// Fixed position (NOT_FIXED sentinel = not fixed, otherwise fixed to this value)
    pub fixed: [f32; 3],
    /// Padding for 16-byte alignment
    pub _padding: f32,
}

/// Sentinel value indicating "not fixed" (negative value for easy GPU comparison)
pub const NOT_FIXED: f32 = -1e9;

impl GpuNode {
    /// Create a new node at the given position
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self {
            position: [x, y, z],
            charge: DEFAULT_CHARGE,
            velocity: [0.0, 0.0, 0.0],
            mass: 1.0,
            fixed: [NOT_FIXED, NOT_FIXED, NOT_FIXED],
            _padding: 0.0,
        }
    }

    /// Set the charge (negative = repulsion, positive = attraction)
    pub fn with_charge(mut self, charge: f32) -> Self {
        self.charge = charge;
        self
    }

    /// Fix the node at its current position
    pub fn with_fixed(mut self, fx: f32, fy: f32, fz: f32) -> Self {
        self.fixed = [fx, fy, fz];
        self
    }

    /// Check if this node has a fixed position
    pub fn is_fixed(&self) -> bool {
        // Node is fixed if any coordinate is NOT the sentinel value
        self.fixed[0] > NOT_FIXED + 1.0
            || self.fixed[1] > NOT_FIXED + 1.0
            || self.fixed[2] > NOT_FIXED + 1.0
    }
}

impl Default for GpuNode {
    fn default() -> Self {
        Self::new(0.0, 0.0, 0.0)
    }
}

/// An edge (link) between two nodes in the GPU force simulation.
///
/// Layout matches WGSL struct for direct buffer upload.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct GpuEdge {
    /// Index of source node
    pub source: u32,
    /// Index of target node
    pub target_node: u32,
    /// Spring strength (higher = stronger pull)
    pub strength: f32,
    /// Rest length (target distance between nodes)
    pub distance: f32,
}

impl GpuEdge {
    /// Create a new edge between two nodes
    pub fn new(source: u32, target_node: u32) -> Self {
        Self {
            source,
            target_node,
            strength: DEFAULT_EDGE_STRENGTH,
            distance: DEFAULT_EDGE_DISTANCE,
        }
    }

    /// Set the spring strength
    pub fn with_strength(mut self, strength: f32) -> Self {
        self.strength = strength;
        self
    }

    /// Set the rest length (target distance)
    pub fn with_distance(mut self, distance: f32) -> Self {
        self.distance = distance;
        self
    }
}

/// Simulation parameters passed to GPU as uniforms
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SimulationUniforms {
    /// Current alpha (simulation "temperature")
    pub alpha: f32,
    /// Velocity decay factor (0-1, applied each tick)
    pub velocity_decay: f32,
    /// Number of nodes
    pub node_count: u32,
    /// Number of edges
    pub edge_count: u32,
    /// Center of the simulation (for centering force)
    pub center: [f32; 3],
    /// Centering force strength
    pub center_strength: f32,
    /// Barnes-Hut theta (approximation threshold)
    pub theta: f32,
    /// Minimum distance for force calculation (avoids singularity)
    pub distance_min: f32,
    /// Maximum distance for force calculation (cutoff)
    pub distance_max: f32,
    /// Maximum velocity (prevents numerical explosion)
    pub max_velocity: f32,
}

impl Default for SimulationUniforms {
    fn default() -> Self {
        Self {
            alpha: 1.0,
            velocity_decay: DEFAULT_VELOCITY_DECAY,
            node_count: 0,
            edge_count: 0,
            center: [0.0, 0.0, 0.0],
            center_strength: 1.0,
            theta: DEFAULT_THETA,
            distance_min: DEFAULT_DISTANCE_MIN,
            distance_max: DEFAULT_DISTANCE_MAX,
            max_velocity: DEFAULT_MAX_VELOCITY,
        }
    }
}

/// Configuration for the GPU force simulation
#[derive(Debug, Clone)]
pub struct GpuSimulationConfig {
    /// Initial alpha value
    pub alpha: f32,
    /// Minimum alpha before simulation stops
    pub alpha_min: f32,
    /// Alpha decay rate per tick
    pub alpha_decay: f32,
    /// Target alpha value
    pub alpha_target: f32,
    /// Velocity decay factor
    pub velocity_decay: f32,
    /// Center position for centering force
    pub center: [f32; 3],
    /// Centering force strength
    pub center_strength: f32,
    /// Barnes-Hut theta (approximation threshold, 0 = exact, 1 = fast)
    pub theta: f32,
    /// Minimum distance for force calculations (avoids singularity)
    pub distance_min: f32,
    /// Maximum distance for many-body force (cutoff for optimization)
    pub distance_max: f32,
    /// Maximum velocity (prevents numerical explosion)
    pub max_velocity: f32,
}

impl Default for GpuSimulationConfig {
    fn default() -> Self {
        Self {
            alpha: 1.0,
            alpha_min: DEFAULT_ALPHA_MIN,
            // Decay formula from D3: 1 - alpha_min^(1/300)
            alpha_decay: 1.0 - DEFAULT_ALPHA_MIN.powf(1.0 / DEFAULT_ALPHA_DECAY_TICKS),
            alpha_target: 0.0,
            velocity_decay: DEFAULT_VELOCITY_DECAY,
            center: [0.0, 0.0, 0.0],
            center_strength: 1.0,
            theta: DEFAULT_THETA,
            distance_min: DEFAULT_DISTANCE_MIN,
            distance_max: DEFAULT_DISTANCE_MAX,
            max_velocity: DEFAULT_MAX_VELOCITY,
        }
    }
}

// =============================================================================
// Render Types
// =============================================================================

/// Default node radius for rendering
pub const DEFAULT_NODE_RADIUS: f32 = 5.0;

/// Default edge alpha for rendering
pub const DEFAULT_EDGE_ALPHA: f32 = 0.6;

/// Instance data for rendering a node as a sphere.
///
/// Layout matches WGSL struct for instanced rendering.
/// Used as vertex buffer with per-instance step mode.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct NodeInstance {
    /// World position (from simulation)
    pub position: [f32; 3],
    /// Sphere radius
    pub radius: f32,
    /// RGBA color
    pub color: [f32; 4],
    /// Selection state (0.0 = not selected, 1.0 = selected)
    pub selected: f32,
    /// Padding for 16-byte alignment
    pub _padding: [f32; 3],
}

impl NodeInstance {
    /// Create a new node instance at the given position
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self {
            position: [x, y, z],
            radius: DEFAULT_NODE_RADIUS,
            color: [0.5, 0.5, 0.5, 1.0],
            selected: 0.0,
            _padding: [0.0; 3],
        }
    }

    /// Set the color
    pub fn with_color(mut self, r: f32, g: f32, b: f32, a: f32) -> Self {
        self.color = [r, g, b, a];
        self
    }

    /// Set the radius
    pub fn with_radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// Set the selection state
    pub fn with_selected(mut self, selected: bool) -> Self {
        self.selected = if selected { 1.0 } else { 0.0 };
        self
    }
}

impl Default for NodeInstance {
    fn default() -> Self {
        Self::new(0.0, 0.0, 0.0)
    }
}

/// Instance data for rendering an edge as a line.
///
/// Layout matches WGSL struct for line rendering.
/// Each edge has start and end positions directly (denormalized from node indices).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct EdgeInstance {
    /// Start position (source node)
    pub start: [f32; 3],
    /// Edge alpha (for fading distant/unimportant edges)
    pub alpha: f32,
    /// End position (target node)
    pub end: [f32; 3],
    /// Padding for 16-byte alignment
    pub _padding: f32,
}

impl EdgeInstance {
    /// Create a new edge instance between two positions
    pub fn new(start: [f32; 3], end: [f32; 3]) -> Self {
        Self {
            start,
            alpha: DEFAULT_EDGE_ALPHA,
            end,
            _padding: 0.0,
        }
    }

    /// Set the alpha value
    pub fn with_alpha(mut self, alpha: f32) -> Self {
        self.alpha = alpha;
        self
    }
}

impl Default for EdgeInstance {
    fn default() -> Self {
        Self::new([0.0, 0.0, 0.0], [0.0, 0.0, 0.0])
    }
}

/// Camera matrices passed to GPU for 3D rendering.
///
/// Layout matches WGSL struct for uniform binding.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct CameraUniforms {
    /// View matrix (world -> camera space)
    pub view: [[f32; 4]; 4],
    /// Projection matrix (camera -> clip space)
    pub projection: [[f32; 4]; 4],
    /// Camera position in world space (for lighting)
    pub camera_pos: [f32; 3],
    /// Padding for 16-byte alignment
    pub _padding: f32,
}

impl Default for CameraUniforms {
    fn default() -> Self {
        Self {
            view: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            projection: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
            camera_pos: [0.0, 0.0, 0.0],
            _padding: 0.0,
        }
    }
}

/// Configuration for the GPU renderer
#[derive(Debug, Clone)]
pub struct RenderConfig {
    /// Render target width in pixels
    pub width: u32,
    /// Render target height in pixels
    pub height: u32,
    /// Background clear color (RGBA)
    pub clear_color: [f32; 4],
    /// Default node radius
    pub node_radius: f32,
    /// Selection glow intensity (0.0 to 1.0)
    pub selection_glow: f32,
    /// Default edge alpha
    pub edge_alpha: f32,
    /// MSAA sample count (1 = no MSAA, 4 = 4x MSAA)
    pub msaa_samples: u32,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            width: 800,
            height: 600,
            clear_color: [0.1, 0.1, 0.15, 1.0], // Dark background
            node_radius: DEFAULT_NODE_RADIUS,
            selection_glow: 0.3,
            edge_alpha: DEFAULT_EDGE_ALPHA,
            msaa_samples: 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_node_size() {
        // Ensure struct is correctly sized for GPU upload
        // 3 floats (position) + 1 float (charge) + 3 floats (velocity) + 1 float (mass)
        // + 3 floats (fixed) + 1 float (padding) = 12 floats = 48 bytes
        assert_eq!(std::mem::size_of::<GpuNode>(), 48);
    }

    #[test]
    fn test_gpu_edge_size() {
        // 2 u32 + 2 f32 = 16 bytes
        assert_eq!(std::mem::size_of::<GpuEdge>(), 16);
    }

    #[test]
    fn test_uniforms_size() {
        // Should be a multiple of 16 for GPU alignment
        let size = std::mem::size_of::<SimulationUniforms>();
        assert_eq!(
            size % 16,
            0,
            "Uniforms size {} is not 16-byte aligned",
            size
        );
    }

    #[test]
    fn test_node_fixed_detection() {
        let free_node = GpuNode::new(1.0, 2.0, 3.0);
        assert!(!free_node.is_fixed());

        let fixed_node = GpuNode::new(1.0, 2.0, 3.0).with_fixed(1.0, 2.0, 3.0);
        assert!(fixed_node.is_fixed());
    }

    #[test]
    fn test_node_instance_size() {
        // 3 floats (position) + 1 float (radius) + 4 floats (color)
        // + 1 float (selected) + 3 floats (padding) = 12 floats = 48 bytes
        assert_eq!(std::mem::size_of::<NodeInstance>(), 48);
    }

    #[test]
    fn test_edge_instance_size() {
        // 3 floats (start) + 1 float (alpha) + 3 floats (end) + 1 float (padding) = 8 floats = 32 bytes
        assert_eq!(std::mem::size_of::<EdgeInstance>(), 32);
    }

    #[test]
    fn test_camera_uniforms_size() {
        // 16 floats (view) + 16 floats (projection) + 3 floats (camera_pos) + 1 float (padding)
        // = 36 floats = 144 bytes
        let size = std::mem::size_of::<CameraUniforms>();
        assert_eq!(size, 144);
        // Should be a multiple of 16 for GPU alignment
        assert_eq!(
            size % 16,
            0,
            "CameraUniforms size {} is not 16-byte aligned",
            size
        );
    }

    #[test]
    fn test_node_instance_builder_methods() {
        let node = NodeInstance::new(1.0, 2.0, 3.0)
            .with_color(0.5, 0.6, 0.7, 0.8)
            .with_radius(10.0)
            .with_selected(true);

        assert_eq!(node.position, [1.0, 2.0, 3.0]);
        assert_eq!(node.color, [0.5, 0.6, 0.7, 0.8]);
        assert_eq!(node.radius, 10.0);
        assert_eq!(node.selected, 1.0);
    }

    #[test]
    fn test_node_instance_default() {
        let node = NodeInstance::default();

        assert_eq!(node.position, [0.0, 0.0, 0.0]);
        assert_eq!(node.radius, DEFAULT_NODE_RADIUS);
        assert_eq!(node.color, [0.5, 0.5, 0.5, 1.0]); // Gray default
        assert_eq!(node.selected, 0.0);
    }

    #[test]
    fn test_edge_instance_builder_methods() {
        let edge = EdgeInstance::new([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]).with_alpha(0.3);

        assert_eq!(edge.start, [1.0, 2.0, 3.0]);
        assert_eq!(edge.end, [4.0, 5.0, 6.0]);
        assert_eq!(edge.alpha, 0.3);
    }

    #[test]
    fn test_edge_instance_default() {
        let edge = EdgeInstance::default();

        assert_eq!(edge.start, [0.0, 0.0, 0.0]);
        assert_eq!(edge.end, [0.0, 0.0, 0.0]);
        assert_eq!(edge.alpha, DEFAULT_EDGE_ALPHA);
    }

    #[test]
    fn test_render_config_default() {
        let config = RenderConfig::default();

        assert_eq!(config.width, 800);
        assert_eq!(config.height, 600);
        assert_eq!(config.clear_color, [0.1, 0.1, 0.15, 1.0]);
        assert_eq!(config.node_radius, DEFAULT_NODE_RADIUS);
        assert_eq!(config.selection_glow, 0.3);
        assert_eq!(config.edge_alpha, DEFAULT_EDGE_ALPHA);
        assert_eq!(config.msaa_samples, 4);
    }

    #[test]
    fn test_camera_uniforms_default() {
        let uniforms = CameraUniforms::default();

        // View and projection should be identity matrices
        let identity = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        assert_eq!(uniforms.view, identity);
        assert_eq!(uniforms.projection, identity);
        assert_eq!(uniforms.camera_pos, [0.0, 0.0, 0.0]);
    }
}
