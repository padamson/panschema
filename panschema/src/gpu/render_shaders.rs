//! WGSL render shaders for 3D graph visualization
//!
//! Contains vertex and fragment shaders for rendering nodes (instanced spheres)
//! and edges (lines).

/// Camera uniform struct used by all render shaders
pub const CAMERA_UNIFORMS: &str = r#"
struct CameraUniforms {
    view: mat4x4<f32>,
    projection: mat4x4<f32>,
    camera_pos: vec3<f32>,
    _padding: f32,
}

@group(0) @binding(0) var<uniform> camera: CameraUniforms;
"#;

/// Node vertex shader for instanced sphere rendering
///
/// Uses icosphere mesh vertices with per-instance transformation.
pub const NODE_VERTEX_SHADER: &str = r#"
struct MeshVertex {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
}

struct NodeInstance {
    @location(2) world_pos: vec3<f32>,
    @location(3) radius: f32,
    @location(4) color: vec4<f32>,
    @location(5) selected: f32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) world_position: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) selected: f32,
}

@vertex
fn vs_node(
    mesh: MeshVertex,
    instance: NodeInstance,
) -> VertexOutput {
    var out: VertexOutput;

    // Scale mesh vertex by radius and translate to instance position
    let world_pos = mesh.position * instance.radius + instance.world_pos;

    // Transform to clip space
    let view_pos = camera.view * vec4<f32>(world_pos, 1.0);
    out.clip_position = camera.projection * view_pos;

    // Pass through for lighting (normal unchanged since uniform scale)
    out.world_normal = mesh.normal;
    out.world_position = world_pos;
    out.color = instance.color;
    out.selected = instance.selected;

    return out;
}
"#;

/// Node fragment shader with Blinn-Phong lighting
pub const NODE_FRAGMENT_SHADER: &str = r#"
struct FragmentInput {
    @location(0) world_normal: vec3<f32>,
    @location(1) world_position: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) selected: f32,
}

@fragment
fn fs_node(in: FragmentInput) -> @location(0) vec4<f32> {
    // Normalize interpolated normal
    let normal = normalize(in.world_normal);

    // Light direction (fixed directional light from upper-right-front)
    let light_dir = normalize(vec3<f32>(0.5, 1.0, 0.3));

    // View direction
    let view_dir = normalize(camera.camera_pos - in.world_position);

    // Half vector for Blinn-Phong
    let half_dir = normalize(light_dir + view_dir);

    // Lighting components
    let ambient = 0.3;
    let diffuse = max(dot(normal, light_dir), 0.0) * 0.5;
    let specular = pow(max(dot(normal, half_dir), 0.0), 32.0) * 0.3;

    // Combine lighting with base color
    var color = in.color.rgb * (ambient + diffuse) + vec3<f32>(specular);

    // Selection glow: brighten if selected
    if (in.selected > 0.5) {
        color = color + in.color.rgb * 0.4;
    }

    return vec4<f32>(color, in.color.a);
}
"#;

/// Edge vertex shader for line rendering
///
/// Each edge instance provides start and end positions.
/// Vertex index 0 = start, vertex index 1 = end.
pub const EDGE_VERTEX_SHADER: &str = r#"
struct EdgeInstance {
    @location(0) start: vec3<f32>,
    @location(1) alpha: f32,
    @location(2) end: vec3<f32>,
    @location(3) _padding: f32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) alpha: f32,
}

@vertex
fn vs_edge(
    @builtin(vertex_index) vertex_idx: u32,
    instance: EdgeInstance,
) -> VertexOutput {
    var out: VertexOutput;

    // Select start or end based on vertex index
    var pos: vec3<f32>;
    if (vertex_idx == 0u) {
        pos = instance.start;
    } else {
        pos = instance.end;
    }

    // Transform to clip space
    let view_pos = camera.view * vec4<f32>(pos, 1.0);
    out.clip_position = camera.projection * view_pos;
    out.alpha = instance.alpha;

    return out;
}
"#;

/// Edge fragment shader
pub const EDGE_FRAGMENT_SHADER: &str = r#"
struct FragmentInput {
    @location(0) alpha: f32,
}

@fragment
fn fs_edge(in: FragmentInput) -> @location(0) vec4<f32> {
    // Gray edges with instance alpha
    return vec4<f32>(0.5, 0.5, 0.5, in.alpha);
}
"#;

/// Get the complete node shader source
pub fn node_shader() -> String {
    format!(
        "{}\n{}\n{}",
        CAMERA_UNIFORMS, NODE_VERTEX_SHADER, NODE_FRAGMENT_SHADER
    )
}

/// Get the complete edge shader source
pub fn edge_shader() -> String {
    format!(
        "{}\n{}\n{}",
        CAMERA_UNIFORMS, EDGE_VERTEX_SHADER, EDGE_FRAGMENT_SHADER
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_shader_compiles() {
        // Just verify the shader source is valid (actual compilation requires GPU)
        let shader = node_shader();
        assert!(shader.contains("vs_node"));
        assert!(shader.contains("fs_node"));
        assert!(shader.contains("CameraUniforms"));
    }

    #[test]
    fn test_edge_shader_compiles() {
        let shader = edge_shader();
        assert!(shader.contains("vs_edge"));
        assert!(shader.contains("fs_edge"));
        assert!(shader.contains("CameraUniforms"));
    }

    #[test]
    fn test_shaders_have_camera_binding() {
        let node = node_shader();
        let edge = edge_shader();

        // Both shaders should bind camera at group 0, binding 0
        assert!(node.contains("@group(0) @binding(0)"));
        assert!(edge.contains("@group(0) @binding(0)"));
    }
}
