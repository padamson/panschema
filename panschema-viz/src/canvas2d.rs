//! 2D Canvas rendering for graph visualization
//!
//! Renders the force simulation to a 2D HTML canvas.
//! This is the fallback renderer for browsers without WebGPU.

use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use crate::camera::{BoundingBox, Camera2D};
use crate::labels::LabelOptions;
use crate::simulation::{CpuSimulation, SimEdge, SimNode};

/// 2D Canvas renderer
pub struct Canvas2DRenderer {
    canvas: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    /// Camera for view transformations
    camera: Camera2D,
}

impl Canvas2DRenderer {
    /// Create renderer from canvas element
    pub fn new(canvas: HtmlCanvasElement) -> Result<Self, String> {
        let ctx = canvas
            .get_context("2d")
            .map_err(|e| format!("Failed to get 2d context: {:?}", e))?
            .ok_or("2d context not available")?
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|_| "Failed to cast to CanvasRenderingContext2d")?;

        let width = canvas.width() as f32;
        let height = canvas.height() as f32;

        Ok(Self {
            canvas,
            ctx,
            camera: Camera2D::new(width, height),
        })
    }

    /// Update canvas dimensions
    pub fn resize(&mut self, width: u32, height: u32) {
        self.canvas.set_width(width);
        self.canvas.set_height(height);
        self.camera.resize(width as f32, height as f32);
    }

    /// Update animation state (call before render)
    pub fn update_animation(&mut self) {
        self.camera.update_animation();
    }

    /// Render the simulation state
    pub fn render(
        &self,
        sim: &CpuSimulation,
        labels: &LabelOptions,
        hovered_node: Option<usize>,
        hovered_edge: Option<usize>,
    ) {
        // Clear canvas
        self.ctx.set_fill_style_str("#1a1a2e");
        self.ctx.fill_rect(
            0.0,
            0.0,
            self.camera.width as f64,
            self.camera.height as f64,
        );

        // Draw edges first (behind nodes)
        self.render_edges(&sim.edges, &sim.nodes);

        // Draw nodes
        self.render_nodes(&sim.nodes);

        // Draw labels on top (if enabled or hovered)
        if labels.show_edge_labels() {
            self.render_edge_labels(&sim.edges, &sim.nodes, None);
        } else if let Some(idx) = hovered_edge {
            // Only render the hovered edge label
            self.render_edge_labels(&sim.edges, &sim.nodes, Some(idx));
        }

        if labels.show_node_labels() {
            self.render_node_labels(&sim.nodes, None);
        } else if let Some(idx) = hovered_node {
            // Only render the hovered node label
            self.render_node_labels(&sim.nodes, Some(idx));
        }
    }

    /// Render all edges
    fn render_edges(&self, edges: &[SimEdge], nodes: &[SimNode]) {
        self.ctx.set_stroke_style_str("rgba(100, 100, 120, 0.5)");
        self.ctx.set_line_width(1.0);

        for edge in edges {
            let source = &nodes[edge.source];
            let target = &nodes[edge.target];

            let (x1, y1) = self.camera.world_to_canvas(source.x, source.y);
            let (x2, y2) = self.camera.world_to_canvas(target.x, target.y);

            self.ctx.begin_path();
            self.ctx.move_to(x1 as f64, y1 as f64);
            self.ctx.line_to(x2 as f64, y2 as f64);
            self.ctx.stroke();
        }
    }

    /// Render all nodes
    fn render_nodes(&self, nodes: &[SimNode]) {
        for node in nodes {
            let (cx, cy) = self.camera.world_to_canvas(node.x, node.y);
            let radius = node.radius * self.camera.scale;

            // Convert color to CSS
            let color = format!(
                "rgba({}, {}, {}, {})",
                (node.color[0] * 255.0) as u8,
                (node.color[1] * 255.0) as u8,
                (node.color[2] * 255.0) as u8,
                node.color[3]
            );

            self.ctx.begin_path();
            self.ctx
                .arc(
                    cx as f64,
                    cy as f64,
                    radius as f64,
                    0.0,
                    std::f64::consts::TAU,
                )
                .ok();
            self.ctx.set_fill_style_str(&color);
            self.ctx.fill();

            // Draw border
            self.ctx.set_stroke_style_str("rgba(255, 255, 255, 0.3)");
            self.ctx.set_line_width(1.0);
            self.ctx.stroke();
        }
    }

    /// Render node labels
    /// If `only_index` is Some, only render that specific node's label (for hover)
    fn render_node_labels(&self, nodes: &[SimNode], only_index: Option<usize>) {
        let font_size = (12.0 * self.camera.scale).clamp(8.0, 16.0);
        let font = format!(
            "{}px -apple-system, BlinkMacSystemFont, sans-serif",
            font_size
        );
        self.ctx.set_font(&font);
        self.ctx.set_text_align("left");
        self.ctx.set_text_baseline("middle");

        for (i, node) in nodes.iter().enumerate() {
            // Skip if filtering and this isn't the target
            if let Some(idx) = only_index {
                if i != idx {
                    continue;
                }
            }

            let (cx, cy) = self.camera.world_to_canvas(node.x, node.y);
            let radius = node.radius * self.camera.scale;

            // Position label to the right of the node
            let label_x = cx + radius + 4.0;
            let label_y = cy;

            // Draw highlight background for hovered label
            if only_index.is_some() {
                let text_width = node.label.len() as f64 * font_size as f64 * 0.6;
                let padding = 4.0;
                self.ctx.set_fill_style_str("rgba(59, 130, 246, 0.9)");
                self.ctx.fill_rect(
                    label_x as f64 - padding / 2.0,
                    label_y as f64 - font_size as f64 / 2.0 - padding / 2.0,
                    text_width + padding,
                    font_size as f64 + padding,
                );
                self.ctx.set_fill_style_str("white");
            } else {
                self.ctx.set_fill_style_str("rgba(255, 255, 255, 0.9)");
            }

            let _ = self
                .ctx
                .fill_text(&node.label, label_x as f64, label_y as f64);
        }
    }

    /// Render edge labels at midpoints
    /// If `only_index` is Some, only render that specific edge's label (for hover)
    fn render_edge_labels(&self, edges: &[SimEdge], nodes: &[SimNode], only_index: Option<usize>) {
        let font_size = (10.0 * self.camera.scale).clamp(6.0, 12.0);
        let font = format!(
            "{}px -apple-system, BlinkMacSystemFont, sans-serif",
            font_size
        );
        self.ctx.set_font(&font);
        self.ctx.set_text_align("center");
        self.ctx.set_text_baseline("middle");

        for (i, edge) in edges.iter().enumerate() {
            // Skip if filtering and this isn't the target
            if let Some(idx) = only_index {
                if i != idx {
                    continue;
                }
            }

            let source = &nodes[edge.source];
            let target = &nodes[edge.target];

            let (x1, y1) = self.camera.world_to_canvas(source.x, source.y);
            let (x2, y2) = self.camera.world_to_canvas(target.x, target.y);

            // Midpoint of edge
            let mid_x = (x1 + x2) / 2.0;
            let mid_y = (y1 + y2) / 2.0;

            // Draw background for label
            let padding = 2.0;
            let text_width = edge.label.len() as f64 * font_size as f64 * 0.6;
            let bg_width = text_width + padding * 2.0;
            let bg_height = font_size as f64 + padding * 2.0;

            // Use highlight color for hovered label
            if only_index.is_some() {
                self.ctx.set_fill_style_str("rgba(59, 130, 246, 0.9)");
            } else {
                self.ctx.set_fill_style_str("rgba(26, 26, 46, 0.85)");
            }
            self.ctx.fill_rect(
                mid_x as f64 - bg_width / 2.0,
                mid_y as f64 - bg_height / 2.0,
                bg_width,
                bg_height,
            );

            // Draw label text
            if only_index.is_some() {
                self.ctx.set_fill_style_str("white");
            } else {
                self.ctx.set_fill_style_str("rgba(180, 180, 200, 0.9)");
            }
            let _ = self.ctx.fill_text(&edge.label, mid_x as f64, mid_y as f64);
        }
    }

    /// Pan the view
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.camera.pan(dx, dy);
    }

    /// Zoom the view
    pub fn zoom(&mut self, factor: f32) {
        self.camera.zoom(factor);
    }

    /// Reset view to default
    pub fn reset_view(&mut self) {
        self.camera.reset_view();
    }

    /// Fit the graph to the canvas bounds with padding (animated)
    pub fn fit_to_bounds(&mut self, nodes: &[SimNode], padding: f32) {
        if nodes.is_empty() {
            return;
        }

        // Calculate bounding box of all nodes (accounting for labels)
        let mut bounds = BoundingBox::empty();
        for node in nodes {
            // Include node circle
            bounds.include_circle(node.x, node.y, node.radius);
            // Include some extra space for the label (approximate)
            let label_width = node.label.len() as f32 * 8.0;
            bounds.include_point(node.x + node.radius + label_width, node.y);
        }

        self.camera.fit_to_bounds(&bounds, padding);
    }

    /// Find node at canvas coordinates (for click/hover detection)
    pub fn node_at(&self, canvas_x: f32, canvas_y: f32, nodes: &[SimNode]) -> Option<usize> {
        for (i, node) in nodes.iter().enumerate() {
            let (cx, cy) = self.camera.world_to_canvas(node.x, node.y);
            let radius = node.radius * self.camera.scale;

            let dx = canvas_x - cx;
            let dy = canvas_y - cy;

            if dx * dx + dy * dy <= radius * radius {
                return Some(i);
            }
        }
        None
    }

    /// Find edge near canvas coordinates (for hover detection)
    pub fn edge_at(
        &self,
        canvas_x: f32,
        canvas_y: f32,
        edges: &[SimEdge],
        nodes: &[SimNode],
        threshold: f32,
    ) -> Option<usize> {
        for (i, edge) in edges.iter().enumerate() {
            let source = &nodes[edge.source];
            let target = &nodes[edge.target];

            let (x1, y1) = self.camera.world_to_canvas(source.x, source.y);
            let (x2, y2) = self.camera.world_to_canvas(target.x, target.y);

            // Check distance to edge midpoint (simplified check)
            let mid_x = (x1 + x2) / 2.0;
            let mid_y = (y1 + y2) / 2.0;

            let dx = canvas_x - mid_x;
            let dy = canvas_y - mid_y;
            let dist = (dx * dx + dy * dy).sqrt();

            if dist < threshold {
                return Some(i);
            }
        }
        None
    }
}
