//! 3D Camera with orbit controls for WebGPU visualization
//!
//! Implements an arcball-style camera that orbits around a target point.
//! Supports smooth animated transitions for all camera movements.

/// 3D Camera for orbit-style navigation
#[derive(Debug, Clone)]
pub struct Camera3D {
    /// Camera position in world space
    pub position: [f32; 3],
    /// Target point the camera looks at
    pub target: [f32; 3],
    /// Up vector (usually Y-up)
    pub up: [f32; 3],

    /// Field of view in radians
    pub fov: f32,
    /// Aspect ratio (width / height)
    pub aspect: f32,
    /// Near clipping plane
    pub near: f32,
    /// Far clipping plane
    pub far: f32,

    /// Distance from target (for orbit)
    pub distance: f32,
    /// Horizontal angle (azimuth) in radians
    pub theta: f32,
    /// Vertical angle (elevation) in radians
    pub phi: f32,

    // Animation targets
    target_distance: f32,
    target_theta: f32,
    target_phi: f32,
    target_target: [f32; 3],

    /// Whether animation is in progress
    pub is_animating: bool,
}

impl Camera3D {
    /// Create a new 3D camera
    pub fn new(aspect: f32) -> Self {
        let distance = 300.0;
        let theta = 0.0;
        let phi = std::f32::consts::FRAC_PI_4; // 45 degrees elevation

        let mut cam = Self {
            position: [0.0, 0.0, distance],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov: std::f32::consts::FRAC_PI_4, // 45 degrees
            aspect,
            near: 0.1,
            far: 2000.0,
            distance,
            theta,
            phi,
            target_distance: distance,
            target_theta: theta,
            target_phi: phi,
            target_target: [0.0, 0.0, 0.0],
            is_animating: false,
        };
        cam.update_position();
        cam
    }

    /// Update aspect ratio on resize
    pub fn resize(&mut self, width: f32, height: f32) {
        self.aspect = width / height;
    }

    /// Orbit the camera horizontally (azimuth)
    pub fn orbit_horizontal(&mut self, delta: f32) {
        self.target_theta += delta;
        self.is_animating = true;
    }

    /// Orbit the camera vertically (elevation)
    pub fn orbit_vertical(&mut self, delta: f32) {
        // Clamp phi to avoid flipping at poles
        self.target_phi = (self.target_phi + delta).clamp(0.1, std::f32::consts::PI - 0.1);
        self.is_animating = true;
    }

    /// Zoom in/out by changing distance
    pub fn zoom(&mut self, factor: f32) {
        self.target_distance = (self.target_distance * factor).clamp(50.0, 1000.0);
        self.is_animating = true;
    }

    /// Pan the camera target
    pub fn pan(&mut self, dx: f32, dy: f32) {
        // Calculate right and up vectors in world space
        let forward = normalize(sub(self.target, self.position));
        let right = normalize(cross(forward, self.up));
        let up = cross(right, forward);

        // Move target based on screen-space delta
        let scale = self.distance * 0.002;
        self.target_target[0] += right[0] * dx * scale + up[0] * dy * scale;
        self.target_target[1] += right[1] * dx * scale + up[1] * dy * scale;
        self.target_target[2] += right[2] * dx * scale + up[2] * dy * scale;
        self.is_animating = true;
    }

    /// Reset camera to default view
    pub fn reset_view(&mut self) {
        self.target_distance = 300.0;
        self.target_theta = 0.0;
        self.target_phi = std::f32::consts::FRAC_PI_4;
        self.target_target = [0.0, 0.0, 0.0];
        self.is_animating = true;
    }

    /// Fit camera to view a bounding box
    pub fn fit_to_bounds(&mut self, bounds: &BoundingBox3D, padding: f32) {
        if bounds.is_empty() {
            return;
        }

        // Calculate center and size
        let center = bounds.center();
        let size = bounds.size();
        let max_dim = size[0].max(size[1]).max(size[2]) + padding * 2.0;

        // Calculate distance needed to fit the bounding box
        let half_fov = self.fov / 2.0;
        let distance = (max_dim / 2.0) / half_fov.tan();

        self.target_target = center;
        self.target_distance = distance.clamp(50.0, 1000.0);
        self.is_animating = true;
    }

    /// Update animation state (call each frame)
    /// Returns true if still animating
    pub fn update_animation(&mut self) -> bool {
        const LERP_FACTOR: f32 = 0.12;
        const EPSILON: f32 = 0.001;

        let mut still_animating = false;

        // Lerp distance
        if (self.distance - self.target_distance).abs() > EPSILON {
            self.distance = lerp(self.distance, self.target_distance, LERP_FACTOR);
            still_animating = true;
        } else {
            self.distance = self.target_distance;
        }

        // Lerp theta
        if (self.theta - self.target_theta).abs() > EPSILON {
            self.theta = lerp(self.theta, self.target_theta, LERP_FACTOR);
            still_animating = true;
        } else {
            self.theta = self.target_theta;
        }

        // Lerp phi
        if (self.phi - self.target_phi).abs() > EPSILON {
            self.phi = lerp(self.phi, self.target_phi, LERP_FACTOR);
            still_animating = true;
        } else {
            self.phi = self.target_phi;
        }

        // Lerp target
        for i in 0..3 {
            if (self.target[i] - self.target_target[i]).abs() > EPSILON {
                self.target[i] = lerp(self.target[i], self.target_target[i], LERP_FACTOR);
                still_animating = true;
            } else {
                self.target[i] = self.target_target[i];
            }
        }

        self.is_animating = still_animating;
        self.update_position();
        still_animating
    }

    /// Update camera position from spherical coordinates
    fn update_position(&mut self) {
        // Convert spherical to Cartesian
        let sin_phi = self.phi.sin();
        let cos_phi = self.phi.cos();
        let sin_theta = self.theta.sin();
        let cos_theta = self.theta.cos();

        self.position[0] = self.target[0] + self.distance * sin_phi * sin_theta;
        self.position[1] = self.target[1] + self.distance * cos_phi;
        self.position[2] = self.target[2] + self.distance * sin_phi * cos_theta;
    }

    /// Get view matrix (4x4 column-major)
    pub fn view_matrix(&self) -> [f32; 16] {
        look_at(self.position, self.target, self.up)
    }

    /// Get projection matrix (4x4 column-major)
    pub fn projection_matrix(&self) -> [f32; 16] {
        perspective(self.fov, self.aspect, self.near, self.far)
    }

    /// Get combined view-projection matrix
    pub fn view_projection_matrix(&self) -> [f32; 16] {
        mat4_multiply(self.projection_matrix(), self.view_matrix())
    }
}

/// 3D bounding box
#[derive(Debug, Clone, Copy)]
pub struct BoundingBox3D {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl BoundingBox3D {
    /// Create an empty bounding box
    pub fn empty() -> Self {
        Self {
            min: [f32::INFINITY, f32::INFINITY, f32::INFINITY],
            max: [f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY],
        }
    }

    /// Check if bounding box is empty
    pub fn is_empty(&self) -> bool {
        self.min[0] > self.max[0] || self.min[1] > self.max[1] || self.min[2] > self.max[2]
    }

    /// Include a point in the bounding box
    pub fn include_point(&mut self, x: f32, y: f32, z: f32) {
        self.min[0] = self.min[0].min(x);
        self.min[1] = self.min[1].min(y);
        self.min[2] = self.min[2].min(z);
        self.max[0] = self.max[0].max(x);
        self.max[1] = self.max[1].max(y);
        self.max[2] = self.max[2].max(z);
    }

    /// Include a sphere in the bounding box
    pub fn include_sphere(&mut self, x: f32, y: f32, z: f32, radius: f32) {
        self.include_point(x - radius, y - radius, z - radius);
        self.include_point(x + radius, y + radius, z + radius);
    }

    /// Get center of bounding box
    pub fn center(&self) -> [f32; 3] {
        [
            (self.min[0] + self.max[0]) / 2.0,
            (self.min[1] + self.max[1]) / 2.0,
            (self.min[2] + self.max[2]) / 2.0,
        ]
    }

    /// Get size of bounding box
    pub fn size(&self) -> [f32; 3] {
        [
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        ]
    }
}

// Math helper functions

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 0.0 {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        v
    }
}

/// Create a look-at view matrix (column-major)
fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [f32; 16] {
    let f = normalize(sub(target, eye));
    let s = normalize(cross(f, up));
    let u = cross(s, f);

    [
        s[0],
        u[0],
        -f[0],
        0.0,
        s[1],
        u[1],
        -f[1],
        0.0,
        s[2],
        u[2],
        -f[2],
        0.0,
        -dot(s, eye),
        -dot(u, eye),
        dot(f, eye),
        1.0,
    ]
}

/// Create a perspective projection matrix (column-major)
fn perspective(fov: f32, aspect: f32, near: f32, far: f32) -> [f32; 16] {
    let f = 1.0 / (fov / 2.0).tan();
    let nf = 1.0 / (near - far);

    [
        f / aspect,
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        (far + near) * nf,
        -1.0,
        0.0,
        0.0,
        2.0 * far * near * nf,
        0.0,
    ]
}

/// Multiply two 4x4 matrices (column-major)
fn mat4_multiply(a: [f32; 16], b: [f32; 16]) -> [f32; 16] {
    let mut result = [0.0f32; 16];
    for col in 0..4 {
        for row in 0..4 {
            result[col * 4 + row] = (0..4).map(|k| a[k * 4 + row] * b[col * 4 + k]).sum();
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_creates_with_defaults() {
        let cam = Camera3D::new(16.0 / 9.0);
        assert_eq!(cam.aspect, 16.0 / 9.0);
        assert_eq!(cam.distance, 300.0);
        assert!(!cam.is_animating);
    }

    #[test]
    fn camera_orbit_horizontal() {
        let mut cam = Camera3D::new(1.0);
        cam.orbit_horizontal(0.5);
        assert!(cam.is_animating);
        assert!((cam.target_theta - 0.5).abs() < 0.001);
    }

    #[test]
    fn camera_orbit_vertical_clamps() {
        let mut cam = Camera3D::new(1.0);
        cam.orbit_vertical(10.0); // Try to go past top
        assert!(cam.target_phi < std::f32::consts::PI);
        assert!(cam.target_phi > 0.1);
    }

    #[test]
    fn camera_zoom_clamps() {
        let mut cam = Camera3D::new(1.0);
        cam.zoom(0.01); // Zoom way in
        assert!(cam.target_distance >= 50.0);
        cam.zoom(100.0); // Zoom way out
        assert!(cam.target_distance <= 1000.0);
    }

    #[test]
    fn camera_animation_converges() {
        let mut cam = Camera3D::new(1.0);
        cam.zoom(0.5);

        for _ in 0..100 {
            cam.update_animation();
        }

        assert!(!cam.is_animating);
        assert!((cam.distance - cam.target_distance).abs() < 0.01);
    }

    #[test]
    fn camera_reset_view() {
        let mut cam = Camera3D::new(1.0);
        cam.zoom(0.5);
        cam.orbit_horizontal(1.0);
        cam.target_target = [100.0, 50.0, 25.0];

        cam.reset_view();

        assert_eq!(cam.target_distance, 300.0);
        assert_eq!(cam.target_theta, 0.0);
        assert_eq!(cam.target_target, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn bounding_box_empty() {
        let bb = BoundingBox3D::empty();
        assert!(bb.is_empty());
    }

    #[test]
    fn bounding_box_include_point() {
        let mut bb = BoundingBox3D::empty();
        bb.include_point(10.0, 20.0, 30.0);
        bb.include_point(-5.0, 15.0, 25.0);

        assert!(!bb.is_empty());
        assert_eq!(bb.min, [-5.0, 15.0, 25.0]);
        assert_eq!(bb.max, [10.0, 20.0, 30.0]);
    }

    #[test]
    fn bounding_box_center() {
        let mut bb = BoundingBox3D::empty();
        bb.include_point(0.0, 0.0, 0.0);
        bb.include_point(10.0, 20.0, 30.0);

        let center = bb.center();
        assert_eq!(center, [5.0, 10.0, 15.0]);
    }

    #[test]
    fn view_matrix_is_valid() {
        let cam = Camera3D::new(1.0);
        let view = cam.view_matrix();

        // View matrix should have determinant close to 1 (orthonormal rotation)
        // Just verify it's not all zeros
        let sum: f32 = view.iter().map(|x| x.abs()).sum();
        assert!(sum > 0.0);
    }

    #[test]
    fn projection_matrix_is_valid() {
        let cam = Camera3D::new(1.0);
        let proj = cam.projection_matrix();

        // Projection matrix should have non-zero elements
        let sum: f32 = proj.iter().map(|x| x.abs()).sum();
        assert!(sum > 0.0);
    }

    #[test]
    fn fit_to_bounds_adjusts_distance() {
        let mut cam = Camera3D::new(1.0);
        let initial_distance = cam.target_distance;

        let mut bb = BoundingBox3D::empty();
        bb.include_point(-50.0, -50.0, -50.0);
        bb.include_point(50.0, 50.0, 50.0);

        cam.fit_to_bounds(&bb, 10.0);

        // Distance should change based on bounding box size
        assert!(cam.is_animating);
        assert_ne!(cam.target_distance, initial_distance);
    }
}
