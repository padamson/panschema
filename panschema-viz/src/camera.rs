//! Camera and view calculations for 2D rendering
//!
//! This module contains pure calculation logic that can be unit tested
//! without browser dependencies.

/// Camera state for 2D view transformations
#[derive(Debug, Clone)]
pub struct Camera2D {
    /// Canvas width in pixels
    pub width: f32,
    /// Canvas height in pixels
    pub height: f32,
    /// Camera offset (pan) in world coordinates
    pub offset_x: f32,
    pub offset_y: f32,
    /// Zoom level (1.0 = 100%)
    pub scale: f32,
    /// Target values for smooth animation
    pub target_offset_x: f32,
    pub target_offset_y: f32,
    pub target_scale: f32,
    /// Whether we're currently animating
    pub is_animating: bool,
}

impl Default for Camera2D {
    fn default() -> Self {
        Self {
            width: 800.0,
            height: 600.0,
            offset_x: 0.0,
            offset_y: 0.0,
            scale: 1.0,
            target_offset_x: 0.0,
            target_offset_y: 0.0,
            target_scale: 1.0,
            is_animating: false,
        }
    }
}

impl Camera2D {
    /// Create a new camera with given dimensions
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            width,
            height,
            ..Default::default()
        }
    }

    /// Convert world coordinates to canvas coordinates
    pub fn world_to_canvas(&self, x: f32, y: f32) -> (f32, f32) {
        let cx = (x + self.offset_x) * self.scale + self.width / 2.0;
        let cy = (y + self.offset_y) * self.scale + self.height / 2.0;
        (cx, cy)
    }

    /// Convert canvas coordinates to world coordinates
    pub fn canvas_to_world(&self, cx: f32, cy: f32) -> (f32, f32) {
        let x = (cx - self.width / 2.0) / self.scale - self.offset_x;
        let y = (cy - self.height / 2.0) / self.scale - self.offset_y;
        (x, y)
    }

    /// Update animation state (call each frame)
    /// Returns true if still animating
    pub fn update_animation(&mut self) -> bool {
        if !self.is_animating {
            return false;
        }

        // Smooth interpolation factor (higher = faster)
        let lerp_factor = 0.12;

        // Interpolate towards target values
        self.scale += (self.target_scale - self.scale) * lerp_factor;
        self.offset_x += (self.target_offset_x - self.offset_x) * lerp_factor;
        self.offset_y += (self.target_offset_y - self.offset_y) * lerp_factor;

        // Check if we've reached the target (within small epsilon)
        let scale_diff = (self.target_scale - self.scale).abs();
        let offset_x_diff = (self.target_offset_x - self.offset_x).abs();
        let offset_y_diff = (self.target_offset_y - self.offset_y).abs();

        if scale_diff < 0.001 && offset_x_diff < 0.1 && offset_y_diff < 0.1 {
            // Snap to final values
            self.scale = self.target_scale;
            self.offset_x = self.target_offset_x;
            self.offset_y = self.target_offset_y;
            self.is_animating = false;
        }

        self.is_animating
    }

    /// Pan the view by delta pixels
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.offset_x += dx / self.scale;
        self.offset_y += dy / self.scale;
        // Also update targets to prevent animation fighting
        self.target_offset_x = self.offset_x;
        self.target_offset_y = self.offset_y;
    }

    /// Zoom the view by factor (1.1 = zoom in 10%, 0.9 = zoom out 10%)
    pub fn zoom(&mut self, factor: f32) {
        self.scale *= factor;
        self.scale = self.scale.clamp(0.1, 10.0);
        // Also update target to prevent animation fighting
        self.target_scale = self.scale;
    }

    /// Reset view to default
    pub fn reset_view(&mut self) {
        self.offset_x = 0.0;
        self.offset_y = 0.0;
        self.scale = 1.0;
        self.target_offset_x = 0.0;
        self.target_offset_y = 0.0;
        self.target_scale = 1.0;
        self.is_animating = false;
    }

    /// Calculate bounds to fit nodes and set animation target
    pub fn fit_to_bounds(&mut self, bounds: &BoundingBox, padding: f32) {
        if bounds.is_empty() {
            return;
        }

        let graph_width = bounds.width();
        let graph_height = bounds.height();

        // Calculate the available canvas area (with padding)
        let available_width = self.width - 2.0 * padding;
        let available_height = self.height - 2.0 * padding;

        // Calculate scale to fit the graph
        let scale_x = available_width / graph_width;
        let scale_y = available_height / graph_height;
        self.target_scale = scale_x.min(scale_y).clamp(0.1, 10.0);

        // Set target offset to center the graph
        self.target_offset_x = -bounds.center_x();
        self.target_offset_y = -bounds.center_y();

        // Start animation
        self.is_animating = true;
    }

    /// Resize the canvas dimensions
    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
    }
}

/// Axis-aligned bounding box
#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
}

impl BoundingBox {
    /// Create an empty bounding box
    pub fn empty() -> Self {
        Self {
            min_x: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            min_y: f32::INFINITY,
            max_y: f32::NEG_INFINITY,
        }
    }

    /// Check if the bounding box is empty
    pub fn is_empty(&self) -> bool {
        self.min_x > self.max_x || self.min_y > self.max_y
    }

    /// Expand the bounding box to include a point
    pub fn include_point(&mut self, x: f32, y: f32) {
        self.min_x = self.min_x.min(x);
        self.max_x = self.max_x.max(x);
        self.min_y = self.min_y.min(y);
        self.max_y = self.max_y.max(y);
    }

    /// Expand the bounding box to include a circle
    pub fn include_circle(&mut self, x: f32, y: f32, radius: f32) {
        self.min_x = self.min_x.min(x - radius);
        self.max_x = self.max_x.max(x + radius);
        self.min_y = self.min_y.min(y - radius);
        self.max_y = self.max_y.max(y + radius);
    }

    /// Width of the bounding box
    pub fn width(&self) -> f32 {
        (self.max_x - self.min_x).max(1.0)
    }

    /// Height of the bounding box
    pub fn height(&self) -> f32 {
        (self.max_y - self.min_y).max(1.0)
    }

    /// Center X coordinate
    pub fn center_x(&self) -> f32 {
        (self.min_x + self.max_x) / 2.0
    }

    /// Center Y coordinate
    pub fn center_y(&self) -> f32 {
        (self.min_y + self.max_y) / 2.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== Camera2D Tests ==========

    #[test]
    fn camera_default_values() {
        let cam = Camera2D::default();
        assert_eq!(cam.offset_x, 0.0);
        assert_eq!(cam.offset_y, 0.0);
        assert_eq!(cam.scale, 1.0);
        assert!(!cam.is_animating);
    }

    #[test]
    fn world_to_canvas_at_origin() {
        let cam = Camera2D::new(800.0, 600.0);
        // Origin in world should map to center of canvas
        let (cx, cy) = cam.world_to_canvas(0.0, 0.0);
        assert_eq!(cx, 400.0);
        assert_eq!(cy, 300.0);
    }

    #[test]
    fn world_to_canvas_with_offset() {
        let mut cam = Camera2D::new(800.0, 600.0);
        cam.offset_x = 100.0;
        cam.offset_y = 50.0;
        // World origin should shift by offset
        let (cx, cy) = cam.world_to_canvas(0.0, 0.0);
        assert_eq!(cx, 500.0); // 400 + 100
        assert_eq!(cy, 350.0); // 300 + 50
    }

    #[test]
    fn world_to_canvas_with_zoom() {
        let mut cam = Camera2D::new(800.0, 600.0);
        cam.scale = 2.0;
        // Point at (50, 50) should be twice as far from center
        let (cx, cy) = cam.world_to_canvas(50.0, 50.0);
        assert_eq!(cx, 500.0); // 400 + 50*2
        assert_eq!(cy, 400.0); // 300 + 50*2
    }

    #[test]
    fn canvas_to_world_roundtrip() {
        let mut cam = Camera2D::new(800.0, 600.0);
        cam.offset_x = 25.0;
        cam.offset_y = -15.0;
        cam.scale = 1.5;

        let world_x = 100.0;
        let world_y = -50.0;

        let (cx, cy) = cam.world_to_canvas(world_x, world_y);
        let (x, y) = cam.canvas_to_world(cx, cy);

        assert!((x - world_x).abs() < 0.001);
        assert!((y - world_y).abs() < 0.001);
    }

    #[test]
    fn pan_updates_offset() {
        let mut cam = Camera2D::new(800.0, 600.0);
        cam.pan(100.0, 50.0);
        assert_eq!(cam.offset_x, 100.0);
        assert_eq!(cam.offset_y, 50.0);
    }

    #[test]
    fn pan_respects_zoom() {
        let mut cam = Camera2D::new(800.0, 600.0);
        cam.scale = 2.0;
        cam.pan(100.0, 50.0);
        // Pan is divided by scale
        assert_eq!(cam.offset_x, 50.0);
        assert_eq!(cam.offset_y, 25.0);
    }

    #[test]
    fn zoom_multiplies_scale() {
        let mut cam = Camera2D::new(800.0, 600.0);
        cam.zoom(2.0);
        assert_eq!(cam.scale, 2.0);
        cam.zoom(0.5);
        assert_eq!(cam.scale, 1.0);
    }

    #[test]
    fn zoom_clamps_to_bounds() {
        let mut cam = Camera2D::new(800.0, 600.0);
        cam.zoom(0.01); // Try to zoom out too far
        assert_eq!(cam.scale, 0.1); // Clamped to minimum
        cam.zoom(1000.0); // Try to zoom in too far: 0.1 * 1000 = 100
        assert_eq!(cam.scale, 10.0); // Clamped to maximum
    }

    #[test]
    fn reset_view_restores_defaults() {
        let mut cam = Camera2D::new(800.0, 600.0);
        cam.offset_x = 100.0;
        cam.offset_y = 50.0;
        cam.scale = 2.0;
        cam.is_animating = true;

        cam.reset_view();

        assert_eq!(cam.offset_x, 0.0);
        assert_eq!(cam.offset_y, 0.0);
        assert_eq!(cam.scale, 1.0);
        assert!(!cam.is_animating);
    }

    #[test]
    fn animation_interpolates_towards_target() {
        let mut cam = Camera2D::new(800.0, 600.0);
        cam.target_scale = 2.0;
        cam.target_offset_x = 100.0;
        cam.is_animating = true;

        // Run several animation frames
        for _ in 0..10 {
            cam.update_animation();
        }

        // Should be closer to target but not there yet
        assert!(cam.scale > 1.0);
        assert!(cam.scale < 2.0);
        assert!(cam.offset_x > 0.0);
        assert!(cam.offset_x < 100.0);
    }

    #[test]
    fn animation_completes_and_snaps() {
        let mut cam = Camera2D::new(800.0, 600.0);
        cam.target_scale = 2.0;
        cam.target_offset_x = 100.0;
        cam.target_offset_y = 50.0;
        cam.is_animating = true;

        // Run many animation frames until complete
        for _ in 0..200 {
            if !cam.update_animation() {
                break;
            }
        }

        // Should have snapped to target values
        assert_eq!(cam.scale, 2.0);
        assert_eq!(cam.offset_x, 100.0);
        assert_eq!(cam.offset_y, 50.0);
        assert!(!cam.is_animating);
    }

    #[test]
    fn fit_to_bounds_calculates_correct_scale() {
        let mut cam = Camera2D::new(800.0, 600.0);
        let bounds = BoundingBox {
            min_x: -100.0,
            max_x: 100.0,
            min_y: -50.0,
            max_y: 50.0,
        };

        cam.fit_to_bounds(&bounds, 0.0);

        // Graph is 200x100, canvas is 800x600
        // Scale should be min(800/200, 600/100) = min(4, 6) = 4
        assert_eq!(cam.target_scale, 4.0);
        assert!(cam.is_animating);
    }

    #[test]
    fn fit_to_bounds_centers_graph() {
        let mut cam = Camera2D::new(800.0, 600.0);
        let bounds = BoundingBox {
            min_x: 0.0,
            max_x: 200.0,
            min_y: 0.0,
            max_y: 100.0,
        };

        cam.fit_to_bounds(&bounds, 0.0);

        // Center of bounds is (100, 50), so offset should be (-100, -50)
        assert_eq!(cam.target_offset_x, -100.0);
        assert_eq!(cam.target_offset_y, -50.0);
    }

    #[test]
    fn fit_to_bounds_with_padding() {
        let mut cam = Camera2D::new(800.0, 600.0);
        let bounds = BoundingBox {
            min_x: -100.0,
            max_x: 100.0,
            min_y: -50.0,
            max_y: 50.0,
        };

        cam.fit_to_bounds(&bounds, 50.0);

        // Available area is (800-100)x(600-100) = 700x500
        // Scale should be min(700/200, 500/100) = min(3.5, 5) = 3.5
        assert_eq!(cam.target_scale, 3.5);
    }

    #[test]
    fn fit_to_bounds_empty_does_nothing() {
        let mut cam = Camera2D::new(800.0, 600.0);
        cam.scale = 2.0;
        let bounds = BoundingBox::empty();

        cam.fit_to_bounds(&bounds, 0.0);

        // Should not change anything
        assert_eq!(cam.scale, 2.0);
        assert!(!cam.is_animating);
    }

    // ========== BoundingBox Tests ==========

    #[test]
    fn bounding_box_empty() {
        let bb = BoundingBox::empty();
        assert!(bb.is_empty());
    }

    #[test]
    fn bounding_box_include_point() {
        let mut bb = BoundingBox::empty();
        bb.include_point(10.0, 20.0);
        bb.include_point(-5.0, 30.0);

        assert!(!bb.is_empty());
        assert_eq!(bb.min_x, -5.0);
        assert_eq!(bb.max_x, 10.0);
        assert_eq!(bb.min_y, 20.0);
        assert_eq!(bb.max_y, 30.0);
    }

    #[test]
    fn bounding_box_include_circle() {
        let mut bb = BoundingBox::empty();
        bb.include_circle(0.0, 0.0, 10.0);

        assert_eq!(bb.min_x, -10.0);
        assert_eq!(bb.max_x, 10.0);
        assert_eq!(bb.min_y, -10.0);
        assert_eq!(bb.max_y, 10.0);
    }

    #[test]
    fn bounding_box_dimensions() {
        let bb = BoundingBox {
            min_x: 0.0,
            max_x: 100.0,
            min_y: 0.0,
            max_y: 50.0,
        };

        assert_eq!(bb.width(), 100.0);
        assert_eq!(bb.height(), 50.0);
        assert_eq!(bb.center_x(), 50.0);
        assert_eq!(bb.center_y(), 25.0);
    }

    #[test]
    fn bounding_box_width_minimum() {
        let bb = BoundingBox {
            min_x: 0.0,
            max_x: 0.0,
            min_y: 0.0,
            max_y: 0.0,
        };

        // Width/height should be at least 1.0 to avoid division by zero
        assert_eq!(bb.width(), 1.0);
        assert_eq!(bb.height(), 1.0);
    }
}
