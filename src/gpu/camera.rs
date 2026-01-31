//! 3D camera with orbit controls for graph visualization
//!
//! Provides a camera with spherical coordinate controls (orbit, zoom, pan)
//! and view/projection matrix computation.

use crate::gpu::types::CameraUniforms;
use std::f32::consts::{FRAC_PI_2, PI};

/// Default field of view in radians (45 degrees)
pub const DEFAULT_FOV: f32 = PI / 4.0;

/// Default near clip plane
pub const DEFAULT_NEAR: f32 = 0.1;

/// Default far clip plane
pub const DEFAULT_FAR: f32 = 10000.0;

/// Default camera distance from target
pub const DEFAULT_DISTANCE: f32 = 500.0;

/// Minimum camera distance (zoom limit)
pub const MIN_DISTANCE: f32 = 1.0;

/// Maximum camera distance (zoom limit)
pub const MAX_DISTANCE: f32 = 10000.0;

/// Elevation angle limit (prevent gimbal lock)
const ELEVATION_LIMIT: f32 = FRAC_PI_2 - 0.01;

/// 3D camera using spherical coordinates for orbit controls.
///
/// The camera looks at a target point from a position defined by:
/// - `distance`: how far from the target
/// - `azimuth`: horizontal angle around the target (radians)
/// - `elevation`: vertical angle above/below the target (radians)
///
/// # Example
///
/// ```
/// use panschema::gpu::camera::Camera3D;
///
/// let mut camera = Camera3D::new(800.0 / 600.0);
///
/// // Orbit around the target
/// camera.orbit(0.1, 0.05);
///
/// // Zoom in
/// camera.zoom(0.5);
///
/// // Get uniforms for GPU
/// let uniforms = camera.uniforms();
/// ```
#[derive(Debug, Clone)]
pub struct Camera3D {
    /// Distance from target (spherical radius)
    pub distance: f32,
    /// Horizontal angle in radians (0 = looking along +Z)
    pub azimuth: f32,
    /// Vertical angle in radians (0 = level, positive = looking down)
    pub elevation: f32,
    /// Target point the camera looks at
    pub target: [f32; 3],
    /// Field of view in radians
    pub fov: f32,
    /// Aspect ratio (width / height)
    pub aspect: f32,
    /// Near clip plane distance
    pub near: f32,
    /// Far clip plane distance
    pub far: f32,
}

impl Camera3D {
    /// Create a new camera with the given aspect ratio
    pub fn new(aspect: f32) -> Self {
        Self {
            distance: DEFAULT_DISTANCE,
            azimuth: 0.0,
            elevation: 0.3, // Slightly above level for better initial view
            target: [0.0, 0.0, 0.0],
            fov: DEFAULT_FOV,
            aspect,
            near: DEFAULT_NEAR,
            far: DEFAULT_FAR,
        }
    }

    /// Compute camera position in world space from spherical coordinates
    pub fn position(&self) -> [f32; 3] {
        // Spherical to Cartesian conversion
        // azimuth: rotation around Y axis
        // elevation: angle from XZ plane
        let cos_elev = self.elevation.cos();
        let sin_elev = self.elevation.sin();
        let cos_azim = self.azimuth.cos();
        let sin_azim = self.azimuth.sin();

        let x = self.distance * cos_elev * sin_azim + self.target[0];
        let y = self.distance * sin_elev + self.target[1];
        let z = self.distance * cos_elev * cos_azim + self.target[2];

        [x, y, z]
    }

    /// Compute view matrix (world -> camera space)
    pub fn view_matrix(&self) -> [[f32; 4]; 4] {
        let eye = self.position();
        look_at(eye, self.target, [0.0, 1.0, 0.0])
    }

    /// Compute perspective projection matrix (camera -> clip space)
    pub fn projection_matrix(&self) -> [[f32; 4]; 4] {
        perspective(self.fov, self.aspect, self.near, self.far)
    }

    /// Get combined camera uniforms for GPU upload
    pub fn uniforms(&self) -> CameraUniforms {
        CameraUniforms {
            view: self.view_matrix(),
            projection: self.projection_matrix(),
            camera_pos: self.position(),
            _padding: 0.0,
        }
    }

    /// Orbit the camera around the target.
    ///
    /// - `delta_azimuth`: horizontal rotation in radians (positive = rotate right)
    /// - `delta_elevation`: vertical rotation in radians (positive = rotate up)
    pub fn orbit(&mut self, delta_azimuth: f32, delta_elevation: f32) {
        self.azimuth += delta_azimuth;
        // Wrap azimuth to [-PI, PI]
        while self.azimuth > PI {
            self.azimuth -= 2.0 * PI;
        }
        while self.azimuth < -PI {
            self.azimuth += 2.0 * PI;
        }

        self.elevation =
            (self.elevation + delta_elevation).clamp(-ELEVATION_LIMIT, ELEVATION_LIMIT);
    }

    /// Zoom the camera (change distance from target).
    ///
    /// - `delta`: zoom amount (positive = zoom in, negative = zoom out)
    ///
    /// Uses exponential zoom for consistent feel at all distances.
    pub fn zoom(&mut self, delta: f32) {
        // Exponential zoom: multiply distance by a factor
        let factor = 1.0 - delta * 0.1;
        self.distance = (self.distance * factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }

    /// Pan the camera target in screen space.
    ///
    /// - `delta_x`: horizontal pan (positive = move right)
    /// - `delta_y`: vertical pan (positive = move up)
    ///
    /// Pan amount is scaled by distance for consistent feel.
    pub fn pan(&mut self, delta_x: f32, delta_y: f32) {
        // Compute camera right and up vectors
        let (right, up) = self.screen_vectors();

        // Scale by distance for consistent pan speed
        let scale = self.distance * 0.001;

        self.target[0] += (right[0] * delta_x + up[0] * delta_y) * scale;
        self.target[1] += (right[1] * delta_x + up[1] * delta_y) * scale;
        self.target[2] += (right[2] * delta_x + up[2] * delta_y) * scale;
    }

    /// Reset camera to default view
    pub fn reset(&mut self) {
        self.distance = DEFAULT_DISTANCE;
        self.azimuth = 0.0;
        self.elevation = 0.3;
        self.target = [0.0, 0.0, 0.0];
    }

    /// Focus camera on a specific point
    pub fn focus(&mut self, point: [f32; 3]) {
        self.target = point;
    }

    /// Set aspect ratio (call when window resizes)
    pub fn set_aspect(&mut self, aspect: f32) {
        self.aspect = aspect;
    }

    /// Compute camera right and up vectors in world space
    fn screen_vectors(&self) -> ([f32; 3], [f32; 3]) {
        // Forward vector (from camera to target)
        let pos = self.position();
        let forward = normalize([
            self.target[0] - pos[0],
            self.target[1] - pos[1],
            self.target[2] - pos[2],
        ]);

        // World up
        let world_up = [0.0, 1.0, 0.0];

        // Right = forward × up
        let right = normalize(cross(forward, world_up));

        // True up = right × forward
        let up = cross(right, forward);

        (right, up)
    }
}

impl Default for Camera3D {
    fn default() -> Self {
        Self::new(4.0 / 3.0)
    }
}

// =============================================================================
// Matrix Math Helpers
// =============================================================================

/// Compute a look-at view matrix
fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [[f32; 4]; 4] {
    // Forward vector (from eye to target)
    let f = normalize([target[0] - eye[0], target[1] - eye[1], target[2] - eye[2]]);

    // Right vector
    let r = normalize(cross(f, up));

    // True up vector
    let u = cross(r, f);

    // View matrix: rotation followed by translation
    // Note: OpenGL/WebGPU convention - negate forward
    [
        [r[0], u[0], -f[0], 0.0],
        [r[1], u[1], -f[1], 0.0],
        [r[2], u[2], -f[2], 0.0],
        [-dot(r, eye), -dot(u, eye), dot(f, eye), 1.0],
    ]
}

/// Compute a perspective projection matrix
fn perspective(fov: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let f = 1.0 / (fov / 2.0).tan();
    let nf = 1.0 / (near - far);

    // Column-major order for WGSL
    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, (far + near) * nf, -1.0],
        [0.0, 0.0, 2.0 * far * near * nf, 0.0],
    ]
}

/// Normalize a 3D vector
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-10 {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        [0.0, 0.0, 1.0] // Fallback for zero-length vectors
    }
}

/// Cross product of two 3D vectors
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Dot product of two 3D vectors
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camera_creation() {
        let camera = Camera3D::new(16.0 / 9.0);
        assert_eq!(camera.aspect, 16.0 / 9.0);
        assert_eq!(camera.target, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_camera_position_at_origin() {
        let mut camera = Camera3D::new(1.0);
        camera.azimuth = 0.0;
        camera.elevation = 0.0;
        camera.distance = 100.0;
        camera.target = [0.0, 0.0, 0.0];

        let pos = camera.position();
        // At azimuth=0, elevation=0, camera should be on +Z axis
        assert!((pos[0]).abs() < 0.001);
        assert!((pos[1]).abs() < 0.001);
        assert!((pos[2] - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_camera_orbit() {
        let mut camera = Camera3D::new(1.0);
        let initial_azimuth = camera.azimuth;

        camera.orbit(0.5, 0.0);
        assert!((camera.azimuth - initial_azimuth - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_camera_orbit_elevation_clamping() {
        let mut camera = Camera3D::new(1.0);

        // Try to orbit way up - should clamp
        camera.orbit(0.0, 10.0);
        assert!(camera.elevation < FRAC_PI_2);
        assert!(camera.elevation > 0.0);

        // Try to orbit way down - should clamp
        camera.orbit(0.0, -20.0);
        assert!(camera.elevation > -FRAC_PI_2);
        assert!(camera.elevation < 0.0);
    }

    #[test]
    fn test_camera_zoom() {
        let mut camera = Camera3D::new(1.0);
        let initial_distance = camera.distance;

        camera.zoom(1.0); // Zoom in
        assert!(camera.distance < initial_distance);

        camera.zoom(-1.0); // Zoom out
        // Should be back close to original (exponential, so not exact)
    }

    #[test]
    fn test_camera_zoom_limits() {
        let mut camera = Camera3D::new(1.0);

        // Try to zoom in too far
        for _ in 0..100 {
            camera.zoom(1.0);
        }
        assert!(camera.distance >= MIN_DISTANCE);

        // Try to zoom out too far
        for _ in 0..100 {
            camera.zoom(-1.0);
        }
        assert!(camera.distance <= MAX_DISTANCE);
    }

    #[test]
    fn test_camera_pan() {
        let mut camera = Camera3D::new(1.0);

        camera.pan(100.0, 0.0);
        // Target should have moved
        assert!(camera.target[0] != 0.0 || camera.target[2] != 0.0);
    }

    #[test]
    fn test_camera_reset() {
        let mut camera = Camera3D::new(1.0);
        camera.orbit(1.0, 0.5);
        camera.zoom(2.0);
        camera.pan(100.0, 50.0);

        camera.reset();

        assert_eq!(camera.distance, DEFAULT_DISTANCE);
        assert_eq!(camera.target, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_camera_focus() {
        let mut camera = Camera3D::new(1.0);
        camera.focus([10.0, 20.0, 30.0]);
        assert_eq!(camera.target, [10.0, 20.0, 30.0]);
    }

    #[test]
    fn test_view_matrix_not_identity() {
        let camera = Camera3D::new(1.0);
        let view = camera.view_matrix();

        // View matrix should not be identity
        let is_identity = view[0][0] == 1.0
            && view[1][1] == 1.0
            && view[2][2] == 1.0
            && view[3][3] == 1.0
            && view[0][1] == 0.0;
        assert!(!is_identity);
    }

    #[test]
    fn test_projection_matrix_valid() {
        let camera = Camera3D::new(1.0);
        let proj = camera.projection_matrix();

        // Projection matrix should have non-zero diagonal elements
        assert!(proj[0][0] != 0.0);
        assert!(proj[1][1] != 0.0);
        assert!(proj[2][2] != 0.0);
    }

    #[test]
    fn test_camera_uniforms() {
        let camera = Camera3D::new(1.0);
        let uniforms = camera.uniforms();

        // Camera position should match
        let pos = camera.position();
        assert_eq!(uniforms.camera_pos, pos);
    }

    #[test]
    fn test_normalize() {
        let v = normalize([3.0, 4.0, 0.0]);
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        assert!((len - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cross_product() {
        // X cross Y = Z
        let result = cross([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        assert!((result[0]).abs() < 0.001);
        assert!((result[1]).abs() < 0.001);
        assert!((result[2] - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_dot_product() {
        let result = dot([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]);
        assert_eq!(result, 32.0); // 1*4 + 2*5 + 3*6 = 32
    }

    #[test]
    fn test_camera_set_aspect() {
        let mut camera = Camera3D::new(4.0 / 3.0);
        assert!((camera.aspect - 4.0 / 3.0).abs() < 0.001);

        camera.set_aspect(16.0 / 9.0);
        assert!((camera.aspect - 16.0 / 9.0).abs() < 0.001);
    }

    #[test]
    fn test_camera_default() {
        let camera = Camera3D::default();
        assert!((camera.aspect - 4.0 / 3.0).abs() < 0.001);
        assert_eq!(camera.distance, DEFAULT_DISTANCE);
        assert_eq!(camera.target, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn test_camera_azimuth_wrapping() {
        let mut camera = Camera3D::new(1.0);

        // Orbit more than 2*PI to test wrapping
        camera.orbit(7.0, 0.0);
        assert!(camera.azimuth >= -PI && camera.azimuth <= PI);

        camera.orbit(-14.0, 0.0);
        assert!(camera.azimuth >= -PI && camera.azimuth <= PI);
    }
}
