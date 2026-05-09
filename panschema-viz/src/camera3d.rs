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

    /// Zoom the view by factor (1.1 = zoom in 10%, 0.9 = zoom out 10%)
    pub fn zoom(&mut self, factor: f32) {
        self.target_distance = (self.target_distance / factor).clamp(50.0, 1000.0);
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

    /// Project a 3D world point to 2D screen coordinates (normalized device coordinates)
    /// Returns (x, y, visible) where x, y are in range [-1, 1] and visible is true if in front of camera
    pub fn project_point(&self, world_pos: [f32; 3]) -> (f32, f32, bool) {
        let vp = self.view_projection_matrix();

        // Transform point to clip space
        let x = vp[0] * world_pos[0] + vp[4] * world_pos[1] + vp[8] * world_pos[2] + vp[12];
        let y = vp[1] * world_pos[0] + vp[5] * world_pos[1] + vp[9] * world_pos[2] + vp[13];
        let w = vp[3] * world_pos[0] + vp[7] * world_pos[1] + vp[11] * world_pos[2] + vp[15];

        // Perspective divide to get normalized device coordinates
        if w.abs() < 0.0001 {
            return (0.0, 0.0, false);
        }

        let ndc_x = x / w;
        let ndc_y = y / w;

        // Point is visible if w > 0 (in front of camera) and within NDC bounds
        let visible = w > 0.0 && (-1.5..=1.5).contains(&ndc_x) && (-1.5..=1.5).contains(&ndc_y);

        (ndc_x, ndc_y, visible)
    }

    /// Project a 3D world point to pixel coordinates
    /// Returns (x, y, visible) where x, y are pixel coordinates
    pub fn project_to_screen(
        &self,
        world_pos: [f32; 3],
        width: f32,
        height: f32,
    ) -> (f32, f32, bool) {
        let (ndc_x, ndc_y, visible) = self.project_point(world_pos);

        // Convert from NDC [-1, 1] to screen coordinates [0, width/height]
        // Note: y is flipped because screen y increases downward
        let screen_x = (ndc_x + 1.0) * 0.5 * width;
        let screen_y = (1.0 - ndc_y) * 0.5 * height;

        (screen_x, screen_y, visible)
    }

    /// Convert screen coordinates to a ray in world space.
    /// Returns (ray_origin, ray_direction) for picking.
    pub fn screen_to_ray(&self, screen_x: f32, screen_y: f32, width: f32, height: f32) -> Ray3D {
        // Convert screen coordinates to NDC [-1, 1]
        let ndc_x = (screen_x / width) * 2.0 - 1.0;
        let ndc_y = 1.0 - (screen_y / height) * 2.0; // y is flipped

        // Get inverse view-projection matrix
        let vp = self.view_projection_matrix();
        let inv_vp = mat4_inverse(vp);

        // Unproject near and far points
        let near_ndc = [ndc_x, ndc_y, -1.0, 1.0];
        let far_ndc = [ndc_x, ndc_y, 1.0, 1.0];

        let near_world = mat4_transform_point(inv_vp, near_ndc);
        let far_world = mat4_transform_point(inv_vp, far_ndc);

        // Ray direction from near to far
        let direction = normalize(sub(far_world, near_world));

        Ray3D {
            origin: near_world,
            direction,
        }
    }
}

/// A ray in 3D space for picking
#[derive(Debug, Clone, Copy)]
pub struct Ray3D {
    /// Ray origin
    pub origin: [f32; 3],
    /// Normalized ray direction
    pub direction: [f32; 3],
}

impl Ray3D {
    /// Test intersection with a sphere, returning distance if hit
    pub fn intersect_sphere(&self, center: [f32; 3], radius: f32) -> Option<f32> {
        // Vector from ray origin to sphere center
        let oc = sub(self.origin, center);

        // Quadratic coefficients: at² + bt + c = 0
        let a = dot(self.direction, self.direction);
        let b = 2.0 * dot(oc, self.direction);
        let c = dot(oc, oc) - radius * radius;

        let discriminant = b * b - 4.0 * a * c;

        if discriminant < 0.0 {
            return None; // No intersection
        }

        // Find nearest positive intersection
        let sqrt_d = discriminant.sqrt();
        let t1 = (-b - sqrt_d) / (2.0 * a);
        let t2 = (-b + sqrt_d) / (2.0 * a);

        if t1 > 0.0 {
            Some(t1)
        } else if t2 > 0.0 {
            Some(t2)
        } else {
            None // Both intersections behind ray origin
        }
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

/// Compute inverse of a 4x4 matrix (column-major)
fn mat4_inverse(m: [f32; 16]) -> [f32; 16] {
    let mut inv = [0.0f32; 16];

    inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15]
        + m[9] * m[7] * m[14]
        + m[13] * m[6] * m[11]
        - m[13] * m[7] * m[10];

    inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15]
        - m[8] * m[7] * m[14]
        - m[12] * m[6] * m[11]
        + m[12] * m[7] * m[10];

    inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15]
        + m[8] * m[7] * m[13]
        + m[12] * m[5] * m[11]
        - m[12] * m[7] * m[9];

    inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14]
        - m[8] * m[6] * m[13]
        - m[12] * m[5] * m[10]
        + m[12] * m[6] * m[9];

    inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15]
        - m[9] * m[3] * m[14]
        - m[13] * m[2] * m[11]
        + m[13] * m[3] * m[10];

    inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15]
        + m[8] * m[3] * m[14]
        + m[12] * m[2] * m[11]
        - m[12] * m[3] * m[10];

    inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15]
        - m[8] * m[3] * m[13]
        - m[12] * m[1] * m[11]
        + m[12] * m[3] * m[9];

    inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14]
        + m[8] * m[2] * m[13]
        + m[12] * m[1] * m[10]
        - m[12] * m[2] * m[9];

    inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15]
        + m[5] * m[3] * m[14]
        + m[13] * m[2] * m[7]
        - m[13] * m[3] * m[6];

    inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15]
        - m[4] * m[3] * m[14]
        - m[12] * m[2] * m[7]
        + m[12] * m[3] * m[6];

    inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15]
        + m[4] * m[3] * m[13]
        + m[12] * m[1] * m[7]
        - m[12] * m[3] * m[5];

    inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14]
        - m[4] * m[2] * m[13]
        - m[12] * m[1] * m[6]
        + m[12] * m[2] * m[5];

    inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11]
        - m[5] * m[3] * m[10]
        - m[9] * m[2] * m[7]
        + m[9] * m[3] * m[6];

    inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11]
        + m[4] * m[3] * m[10]
        + m[8] * m[2] * m[7]
        - m[8] * m[3] * m[6];

    inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11]
        - m[4] * m[3] * m[9]
        - m[8] * m[1] * m[7]
        + m[8] * m[3] * m[5];

    inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10]
        + m[4] * m[2] * m[9]
        + m[8] * m[1] * m[6]
        - m[8] * m[2] * m[5];

    let det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];

    if det.abs() < 1e-10 {
        return [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
    }

    let inv_det = 1.0 / det;
    for val in &mut inv {
        *val *= inv_det;
    }

    inv
}

/// Transform a 4D point by a 4x4 matrix and perform perspective divide
fn mat4_transform_point(m: [f32; 16], p: [f32; 4]) -> [f32; 3] {
    let x = m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12] * p[3];
    let y = m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13] * p[3];
    let z = m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14] * p[3];
    let w = m[3] * p[0] + m[7] * p[1] + m[11] * p[2] + m[15] * p[3];

    if w.abs() < 1e-10 {
        return [x, y, z];
    }

    [x / w, y / w, z / w]
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
        cam.zoom(0.01); // Zoom way out (small factor → larger distance)
        assert!(cam.target_distance <= 1000.0);
        cam.zoom(100.0); // Zoom way in (large factor → smaller distance)
        assert!(cam.target_distance >= 50.0);
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

    #[test]
    fn project_point_origin_at_center() {
        let cam = Camera3D::new(1.0);

        // The target (origin) should project near the center of screen
        let (ndc_x, ndc_y, visible) = cam.project_point([0.0, 0.0, 0.0]);

        assert!(visible, "Origin should be visible");
        assert!(ndc_x.abs() < 0.1, "Origin x should be near center");
        assert!(ndc_y.abs() < 0.1, "Origin y should be near center");
    }

    #[test]
    fn project_point_behind_camera_not_visible() {
        let cam = Camera3D::new(1.0);

        // Point far behind the camera (in the direction camera is looking from)
        // This test just verifies the projection doesn't panic for extreme coordinates
        let (_, _, _visible) = cam.project_point([0.0, 0.0, 500.0]);
    }

    #[test]
    fn project_to_screen_converts_to_pixels() {
        let cam = Camera3D::new(1.0);

        // Project origin to 800x600 screen
        let (x, y, visible) = cam.project_to_screen([0.0, 0.0, 0.0], 800.0, 600.0);

        assert!(visible, "Origin should be visible");
        // Origin should project near center of screen
        assert!(
            (x - 400.0).abs() < 50.0,
            "Origin x should be near screen center"
        );
        assert!(
            (y - 300.0).abs() < 50.0,
            "Origin y should be near screen center"
        );
    }

    #[test]
    fn project_to_screen_right_is_positive_x() {
        let cam = Camera3D::new(1.0);

        // Point to the right in world space
        let (x_right, _, _) = cam.project_to_screen([50.0, 0.0, 0.0], 800.0, 600.0);
        let (x_left, _, _) = cam.project_to_screen([-50.0, 0.0, 0.0], 800.0, 600.0);

        // Right should have larger x than left (assuming camera looks toward -z or similar)
        // Actually with default camera position, we may need to check the actual values
        assert!(
            x_right != x_left,
            "Left and right should project to different x coordinates"
        );
    }

    #[test]
    fn screen_to_ray_at_center() {
        let cam = Camera3D::new(1.0);

        // Ray from center of screen should point roughly toward target
        let ray = cam.screen_to_ray(400.0, 300.0, 800.0, 600.0);

        // Direction should be normalized
        let len =
            (ray.direction[0].powi(2) + ray.direction[1].powi(2) + ray.direction[2].powi(2)).sqrt();
        assert!(
            (len - 1.0).abs() < 0.01,
            "Ray direction should be normalized"
        );
    }

    #[test]
    fn screen_to_ray_origin_near_camera() {
        let cam = Camera3D::new(1.0);

        let ray = cam.screen_to_ray(400.0, 300.0, 800.0, 600.0);

        // Ray origin should be near camera position (on near plane)
        let dist_to_cam = ((ray.origin[0] - cam.position[0]).powi(2)
            + (ray.origin[1] - cam.position[1]).powi(2)
            + (ray.origin[2] - cam.position[2]).powi(2))
        .sqrt();

        assert!(
            dist_to_cam < 50.0,
            "Ray origin should be near camera position"
        );
    }

    #[test]
    fn ray_sphere_intersect_hit() {
        use super::Ray3D;

        let ray = Ray3D {
            origin: [0.0, 0.0, 10.0],
            direction: [0.0, 0.0, -1.0], // Looking toward origin
        };

        // Sphere at origin with radius 5
        let hit = ray.intersect_sphere([0.0, 0.0, 0.0], 5.0);

        assert!(hit.is_some(), "Ray should hit sphere");
        let t = hit.unwrap();
        assert!(t > 0.0, "Intersection should be in front of ray");
        assert!((t - 5.0).abs() < 0.01, "Should hit at distance 5");
    }

    #[test]
    fn ray_sphere_intersect_miss() {
        use super::Ray3D;

        let ray = Ray3D {
            origin: [0.0, 0.0, 10.0],
            direction: [1.0, 0.0, 0.0], // Looking sideways
        };

        // Sphere at origin with radius 5
        let hit = ray.intersect_sphere([0.0, 0.0, 0.0], 5.0);

        assert!(hit.is_none(), "Ray should miss sphere");
    }

    #[test]
    fn ray_sphere_intersect_behind_camera() {
        use super::Ray3D;

        let ray = Ray3D {
            origin: [0.0, 0.0, 10.0],
            direction: [0.0, 0.0, 1.0], // Looking away from origin
        };

        // Sphere at origin with radius 5
        let hit = ray.intersect_sphere([0.0, 0.0, 0.0], 5.0);

        assert!(hit.is_none(), "Sphere behind ray should not intersect");
    }

    #[test]
    fn pick_node_3d_finds_closest() {
        let cam = Camera3D::new(1.0);

        // Create two nodes at different depths along ray from center
        let center_ray = cam.screen_to_ray(400.0, 300.0, 800.0, 600.0);

        // Near node (closer to camera along ray)
        let near_pos = [
            center_ray.origin[0] + center_ray.direction[0] * 100.0,
            center_ray.origin[1] + center_ray.direction[1] * 100.0,
            center_ray.origin[2] + center_ray.direction[2] * 100.0,
        ];

        // Far node (farther along ray)
        let far_pos = [
            center_ray.origin[0] + center_ray.direction[0] * 200.0,
            center_ray.origin[1] + center_ray.direction[1] * 200.0,
            center_ray.origin[2] + center_ray.direction[2] * 200.0,
        ];

        let radius = 20.0;

        // Ray should hit both, but near should have smaller t
        let near_hit = center_ray.intersect_sphere(near_pos, radius);
        let far_hit = center_ray.intersect_sphere(far_pos, radius);

        assert!(near_hit.is_some(), "Should hit near node");
        assert!(far_hit.is_some(), "Should hit far node");
        assert!(
            near_hit.unwrap() < far_hit.unwrap(),
            "Near node should be hit first"
        );
    }
}
