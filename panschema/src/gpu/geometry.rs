//! Mesh generation for 3D graph visualization
//!
//! Provides icosphere mesh generation for rendering nodes as spheres.

use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;

/// A vertex in a mesh with position and normal.
///
/// Layout matches WGSL struct for vertex buffer upload.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct MeshVertex {
    /// Vertex position (on unit sphere for icosphere)
    pub position: [f32; 3],
    /// Vertex normal (same as position for unit sphere)
    pub normal: [f32; 3],
}

impl MeshVertex {
    /// Create a new mesh vertex
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self {
            position: [x, y, z],
            normal: [x, y, z], // For unit sphere, normal = position
        }
    }
}

/// Generate an icosphere mesh with the given subdivision level.
///
/// # Arguments
///
/// * `subdivisions` - Number of subdivision iterations:
///   - 0: 12 vertices, 20 faces (basic icosahedron)
///   - 1: 42 vertices, 80 faces
///   - 2: 162 vertices, 320 faces (recommended for nodes)
///   - 3: 642 vertices, 1280 faces
///
/// # Returns
///
/// A tuple of (vertices, indices) for the mesh.
///
/// # Example
///
/// ```
/// use panschema::gpu::geometry::icosphere;
///
/// let (vertices, indices) = icosphere(2);
/// assert_eq!(vertices.len(), 162);
/// assert_eq!(indices.len(), 320 * 3);
/// ```
pub fn icosphere(subdivisions: u32) -> (Vec<MeshVertex>, Vec<u32>) {
    // Golden ratio
    let phi = (1.0 + 5.0_f32.sqrt()) / 2.0;

    // Initial icosahedron vertices (12 vertices)
    let mut vertices: Vec<[f32; 3]> = vec![
        normalize([-1.0, phi, 0.0]),
        normalize([1.0, phi, 0.0]),
        normalize([-1.0, -phi, 0.0]),
        normalize([1.0, -phi, 0.0]),
        normalize([0.0, -1.0, phi]),
        normalize([0.0, 1.0, phi]),
        normalize([0.0, -1.0, -phi]),
        normalize([0.0, 1.0, -phi]),
        normalize([phi, 0.0, -1.0]),
        normalize([phi, 0.0, 1.0]),
        normalize([-phi, 0.0, -1.0]),
        normalize([-phi, 0.0, 1.0]),
    ];

    // Initial icosahedron faces (20 triangles)
    let mut faces: Vec<[u32; 3]> = vec![
        // 5 faces around point 0
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        // 5 adjacent faces
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        // 5 faces around point 3
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        // 5 adjacent faces
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];

    // Subdivide
    for _ in 0..subdivisions {
        let mut new_faces = Vec::with_capacity(faces.len() * 4);
        let mut midpoint_cache: HashMap<(u32, u32), u32> = HashMap::new();

        for face in &faces {
            let v1 = face[0];
            let v2 = face[1];
            let v3 = face[2];

            // Get midpoints (or create them)
            let a = get_midpoint(v1, v2, &mut vertices, &mut midpoint_cache);
            let b = get_midpoint(v2, v3, &mut vertices, &mut midpoint_cache);
            let c = get_midpoint(v3, v1, &mut vertices, &mut midpoint_cache);

            // Create 4 new triangles
            new_faces.push([v1, a, c]);
            new_faces.push([v2, b, a]);
            new_faces.push([v3, c, b]);
            new_faces.push([a, b, c]);
        }

        faces = new_faces;
    }

    // Convert to MeshVertex format
    let mesh_vertices: Vec<MeshVertex> = vertices
        .iter()
        .map(|v| MeshVertex::new(v[0], v[1], v[2]))
        .collect();

    // Flatten face indices
    let indices: Vec<u32> = faces.iter().flat_map(|f| f.iter().copied()).collect();

    (mesh_vertices, indices)
}

/// Get or create a midpoint vertex between two vertices.
fn get_midpoint(
    v1: u32,
    v2: u32,
    vertices: &mut Vec<[f32; 3]>,
    cache: &mut HashMap<(u32, u32), u32>,
) -> u32 {
    // Use sorted key for cache
    let key = if v1 < v2 { (v1, v2) } else { (v2, v1) };

    if let Some(&index) = cache.get(&key) {
        return index;
    }

    // Create new midpoint vertex
    let p1 = vertices[v1 as usize];
    let p2 = vertices[v2 as usize];
    let mid = normalize([
        (p1[0] + p2[0]) / 2.0,
        (p1[1] + p2[1]) / 2.0,
        (p1[2] + p2[2]) / 2.0,
    ]);

    let index = vertices.len() as u32;
    vertices.push(mid);
    cache.insert(key, index);
    index
}

/// Normalize a 3D vector to unit length
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-10 {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        [0.0, 0.0, 1.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mesh_vertex_size() {
        // 3 floats (position) + 3 floats (normal) = 6 floats = 24 bytes
        assert_eq!(std::mem::size_of::<MeshVertex>(), 24);
    }

    #[test]
    fn test_icosphere_level_0() {
        let (vertices, indices) = icosphere(0);
        assert_eq!(vertices.len(), 12); // Basic icosahedron
        assert_eq!(indices.len(), 60); // 20 faces × 3 indices
    }

    #[test]
    fn test_icosphere_level_1() {
        let (vertices, indices) = icosphere(1);
        assert_eq!(vertices.len(), 42);
        assert_eq!(indices.len(), 80 * 3);
    }

    #[test]
    fn test_icosphere_level_2() {
        let (vertices, indices) = icosphere(2);
        assert_eq!(vertices.len(), 162);
        assert_eq!(indices.len(), 320 * 3);
    }

    #[test]
    fn test_icosphere_level_3() {
        let (vertices, indices) = icosphere(3);
        assert_eq!(vertices.len(), 642);
        assert_eq!(indices.len(), 1280 * 3);
    }

    #[test]
    fn test_vertices_on_unit_sphere() {
        let (vertices, _) = icosphere(2);
        for v in &vertices {
            let len =
                (v.position[0].powi(2) + v.position[1].powi(2) + v.position[2].powi(2)).sqrt();
            assert!(
                (len - 1.0).abs() < 0.001,
                "Vertex at {:?} has length {} (expected 1.0)",
                v.position,
                len
            );
        }
    }

    #[test]
    fn test_normals_equal_positions() {
        let (vertices, _) = icosphere(1);
        for v in &vertices {
            assert_eq!(
                v.position, v.normal,
                "Normal should equal position for unit sphere"
            );
        }
    }

    #[test]
    fn test_valid_indices() {
        let (vertices, indices) = icosphere(2);
        for &idx in &indices {
            assert!(
                (idx as usize) < vertices.len(),
                "Index {} out of bounds (vertex count: {})",
                idx,
                vertices.len()
            );
        }
    }

    #[test]
    fn test_all_triangles() {
        let (_, indices) = icosphere(1);
        // Should be a multiple of 3 (triangles)
        assert_eq!(indices.len() % 3, 0);
    }
}
