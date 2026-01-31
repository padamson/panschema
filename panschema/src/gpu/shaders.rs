//! WGSL compute shaders for GPU force simulation
//!
//! These shaders implement force-directed graph layout on the GPU.
//! The simulation uses velocity Verlet integration with configurable forces.

/// Common type definitions shared by all shaders
pub const TYPES: &str = r#"
struct Node {
    position: vec3<f32>,
    charge: f32,
    velocity: vec3<f32>,
    mass: f32,
    fixed: vec3<f32>,
    _padding: f32,
}

struct Edge {
    source: u32,
    target_node: u32,
    strength: f32,
    distance: f32,
}

struct Uniforms {
    alpha: f32,
    velocity_decay: f32,
    node_count: u32,
    edge_count: u32,
    center: vec3<f32>,
    center_strength: f32,
    theta: f32,
    distance_min: f32,
    distance_max: f32,
    max_velocity: f32,
}

@group(0) @binding(0) var<storage, read_write> nodes: array<Node>;
@group(0) @binding(1) var<storage, read> edges: array<Edge>;
@group(0) @binding(2) var<uniform> uniforms: Uniforms;
"#;

/// Link force compute shader - applies spring forces between connected nodes
pub const LINK_FORCE: &str = r#"
@compute @workgroup_size(256)
fn link_force(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let edge_idx = global_id.x;
    if (edge_idx >= uniforms.edge_count) {
        return;
    }

    let edge = edges[edge_idx];
    let source_idx = edge.source;
    let target_idx = edge.target_node;

    let source_pos = nodes[source_idx].position;
    let target_pos = nodes[target_idx].position;

    var delta = target_pos - source_pos;
    var dist = length(delta);

    // Avoid division by zero - add jiggle if too close
    if (dist < 1.0) {
        // Deterministic jiggle based on indices
        let seed = f32(source_idx * 12345u + target_idx * 67890u);
        let jx = fract(sin(seed) * 43758.5453) - 0.5;
        let jy = fract(sin(seed * 1.1) * 43758.5453) - 0.5;
        let jz = fract(sin(seed * 1.2) * 43758.5453) - 0.5;
        delta += vec3<f32>(jx, jy, jz) * 1.0;
        dist = length(delta);
    }

    // Spring force: pull toward rest length
    let diff = dist - edge.distance;
    let force_mag = diff * edge.strength * uniforms.alpha / dist;
    let force = delta * force_mag;

    nodes[source_idx].velocity += force;
    nodes[target_idx].velocity -= force;
}
"#;

/// Many-body force compute shader (brute force O(n²) version)
pub const MANY_BODY_FORCE_BRUTE: &str = r#"
@compute @workgroup_size(256)
fn many_body_force(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let node_idx = global_id.x;
    if (node_idx >= uniforms.node_count) {
        return;
    }

    let node = nodes[node_idx];
    var force = vec3<f32>(0.0, 0.0, 0.0);

    // Calculate repulsion from all other nodes
    for (var j = 0u; j < uniforms.node_count; j++) {
        if (j == node_idx) {
            continue;
        }

        let other = nodes[j];
        var delta = node.position - other.position;
        var dist_sq = dot(delta, delta);

        // Clamp to minimum distance to avoid singularity
        let min_dist_sq = uniforms.distance_min * uniforms.distance_min;
        dist_sq = max(dist_sq, min_dist_sq);

        // Skip if beyond maximum distance
        let max_dist_sq = uniforms.distance_max * uniforms.distance_max;
        if (dist_sq > max_dist_sq) {
            continue;
        }

        // Repulsion: F = charge * alpha / r²
        // Both charges are negative, so charge * charge is positive
        // We want repulsion, so force points away from other node
        let dist = sqrt(dist_sq);
        let strength = node.charge * uniforms.alpha / dist_sq;
        force += delta / dist * strength;
    }

    nodes[node_idx].velocity += force;
}
"#;

/// Centering force compute shader - keeps nodes centered around a point
pub const CENTER_FORCE: &str = r#"
@compute @workgroup_size(256)
fn center_force(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let node_idx = global_id.x;
    if (node_idx >= uniforms.node_count) {
        return;
    }

    // Simple centering: move all nodes toward center
    let node = nodes[node_idx];
    let to_center = uniforms.center - node.position;
    let force = to_center * uniforms.center_strength * uniforms.alpha / f32(uniforms.node_count);

    nodes[node_idx].velocity += force;
}
"#;

/// Integration compute shader - updates positions from velocities
pub const INTEGRATE: &str = r#"
@compute @workgroup_size(256)
fn integrate(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let node_idx = global_id.x;
    if (node_idx >= uniforms.node_count) {
        return;
    }

    var node = nodes[node_idx];

    node.velocity *= uniforms.velocity_decay;

    // Clamp velocity to prevent explosion
    let max_vel = uniforms.max_velocity;
    node.velocity = clamp(node.velocity, vec3<f32>(-max_vel), vec3<f32>(max_vel));

    // Update position, respecting fixed coordinates
    // We use -1e9 as sentinel for "not fixed"
    // If fixed < -1e8, it's the sentinel (not fixed), so we update position
    let sentinel_threshold = -1e8;

    if (node.fixed.x < sentinel_threshold) {
        // Not fixed - update from velocity
        node.position.x += node.velocity.x;
    } else {
        // Fixed - snap to fixed position
        node.position.x = node.fixed.x;
        node.velocity.x = 0.0;
    }

    if (node.fixed.y < sentinel_threshold) {
        node.position.y += node.velocity.y;
    } else {
        node.position.y = node.fixed.y;
        node.velocity.y = 0.0;
    }

    if (node.fixed.z < sentinel_threshold) {
        node.position.z += node.velocity.z;
    } else {
        node.position.z = node.fixed.z;
        node.velocity.z = 0.0;
    }

    nodes[node_idx] = node;
}
"#;

/// Combined shader source for all force computations (useful for debugging)
pub fn combined_force_shader() -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}",
        TYPES, LINK_FORCE, MANY_BODY_FORCE_BRUTE, CENTER_FORCE, INTEGRATE
    )
}

/// Get individual shader modules for pipeline creation
pub struct ForceShaders {
    pub link_force: String,
    pub many_body_force: String,
    pub center_force: String,
    pub integrate: String,
}

impl ForceShaders {
    pub fn new() -> Self {
        Self {
            link_force: format!("{}\n{}", TYPES, LINK_FORCE),
            many_body_force: format!("{}\n{}", TYPES, MANY_BODY_FORCE_BRUTE),
            center_force: format!("{}\n{}", TYPES, CENTER_FORCE),
            integrate: format!("{}\n{}", TYPES, INTEGRATE),
        }
    }
}

impl Default for ForceShaders {
    fn default() -> Self {
        Self::new()
    }
}
