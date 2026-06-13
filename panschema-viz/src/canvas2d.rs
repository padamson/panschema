//! 2D Canvas rendering for graph visualization
//!
//! Renders the force simulation to a 2D HTML canvas.
//! This is the fallback renderer for browsers without WebGPU.

use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use std::collections::HashSet;

use crate::camera::{BoundingBox, Camera2D};
use crate::graph_types::EdgeType;
use crate::labels::LabelOptions;
use crate::simulation::{CpuSimulation, SimEdge, SimNode};

/// Canvas background; a hollow arrowhead is filled with this so the
/// edge line doesn't show through its interior before the outline is
/// stroked. Keep in sync with the `fill_rect` clear in `render`.
const CANVAS_BG: &str = "#1a1a2e";

/// Base RGB for an edge kind (alpha is applied at draw time for the
/// focus-mode dim). Per ADR-005: structural kinds (`subclassOf`,
/// `mixin`) share a neutral hue; referential kinds get distinct muted
/// tints — desaturated so the colored nodes still pop. Color is
/// reinforcing only; line style + head shape carry the distinction in
/// grayscale.
fn edge_rgb(kind: EdgeType) -> (u8, u8, u8) {
    match kind {
        EdgeType::SubclassOf | EdgeType::Mixin => (160, 160, 185),
        EdgeType::Domain => (120, 165, 170),
        EdgeType::Range => (190, 165, 120),
        EdgeType::Inverse => (165, 140, 185),
        EdgeType::TypeOf => (190, 150, 160),
    }
}

/// Dashed line for `mixin` (UML realization analog) and `inverse`
/// (symmetric, not a single solid direction); solid otherwise.
fn edge_dashed(kind: EdgeType) -> bool {
    matches!(kind, EdgeType::Mixin | EdgeType::Inverse)
}

/// Hollow-triangle head (UML generalization) for the inheritance
/// kinds; a filled arrow for the referential kinds.
fn edge_hollow_head(kind: EdgeType) -> bool {
    matches!(kind, EdgeType::SubclassOf | EdgeType::Mixin)
}

/// `inverse` is symmetric — draw a head at both ends; every other
/// kind points at its target only.
fn edge_both_ends(kind: EdgeType) -> bool {
    matches!(kind, EdgeType::Inverse)
}

/// The visible span of an edge in canvas space: from the source
/// node's rim to the target node's rim, inset by each node's rendered
/// radius along the edge direction so the line touches the discs
/// rather than running center-to-center into them.
///
/// Returns `None` when the rims meet or overlap (`src_r + tgt_r >=
/// distance`) — the case for short hub edges (e.g. a class with many
/// tightly-packed spokes), where an inset segment would invert. The
/// caller skips the connecting line and lets the arrowhead alone mark
/// the relation.
pub(crate) fn edge_segment(
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    src_r: f64,
    tgt_r: f64,
) -> Option<((f64, f64), (f64, f64))> {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len = (dx * dx + dy * dy).sqrt();
    if len <= src_r + tgt_r {
        return None;
    }
    let (ux, uy) = (dx / len, dy / len);
    Some((
        (x1 + ux * src_r, y1 + uy * src_r),
        (x2 - ux * tgt_r, y2 - uy * tgt_r),
    ))
}

/// The three canvas-space points of a directed-edge arrowhead: the
/// tip (touching the target node's perimeter) followed by the two
/// base corners. All coordinates are in canvas pixels.
///
/// `target_radius` is the target node's *rendered* radius, so the tip
/// sits on the node's edge rather than at its centre. The arrowhead
/// scales with that radius (staying legible at any zoom) but is capped
/// at a fraction of the edge length so it never dominates a short
/// edge. Returns `None` when the source and target are coincident
/// (degenerate — no direction to point).
pub(crate) fn arrowhead_points(
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    target_radius: f64,
) -> Option<[(f64, f64); 3]> {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len = (dx * dx + dy * dy).sqrt();
    if len < f64::EPSILON {
        return None;
    }
    let (ux, uy) = (dx / len, dy / len); // unit vector source → target

    // Tip on the target node's perimeter.
    let tip_x = x2 - ux * target_radius;
    let tip_y = y2 - uy * target_radius;

    // Scale with the node radius; cap so the head can't swallow the
    // visible (post-radius) span of a short edge.
    let visible = (len - target_radius).max(0.0);
    let head_len = (target_radius * 1.4).clamp(5.0, 14.0).min(visible * 0.6);
    let half_w = head_len * 0.5;

    // Base centre, stepped back from the tip along the edge.
    let bx = tip_x - ux * head_len;
    let by = tip_y - uy * head_len;
    // Perpendicular offsets for the two base corners.
    let (px, py) = (-uy, ux);
    Some([
        (tip_x, tip_y),
        (bx + px * half_w, by + py * half_w),
        (bx - px * half_w, by - py * half_w),
    ])
}

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
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        sim: &CpuSimulation,
        labels: &LabelOptions,
        hovered_node: Option<usize>,
        hovered_edge: Option<usize>,
        selected_node: Option<usize>,
        fixed_nodes: &HashSet<usize>,
        focused_node: Option<usize>,
        focused_connected: &HashSet<usize>,
        hidden_nodes: &HashSet<usize>,
        show_arrows: bool,
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
        self.render_edges(
            &sim.edges,
            &sim.nodes,
            focused_node,
            focused_connected,
            hidden_nodes,
            show_arrows,
        );

        // Draw nodes
        self.render_nodes(
            &sim.nodes,
            selected_node,
            fixed_nodes,
            focused_node,
            focused_connected,
            hidden_nodes,
        );

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
    fn render_edges(
        &self,
        edges: &[SimEdge],
        nodes: &[SimNode],
        focused_node: Option<usize>,
        focused_connected: &HashSet<usize>,
        hidden_nodes: &HashSet<usize>,
        show_arrows: bool,
    ) {
        for edge in edges {
            // Skip edges connected to hidden nodes
            if hidden_nodes.contains(&edge.source) || hidden_nodes.contains(&edge.target) {
                continue;
            }

            let source = &nodes[edge.source];
            let target = &nodes[edge.target];

            let (x1, y1) = self.camera.world_to_canvas(source.x, source.y);
            let (x2, y2) = self.camera.world_to_canvas(target.x, target.y);

            // Determine if this edge should be dimmed (focus mode active but edge not connected to focused node)
            let is_connected = if let Some(focused) = focused_node {
                edge.source == focused
                    || edge.target == focused
                    || focused_connected.contains(&edge.source)
                    || focused_connected.contains(&edge.target)
            } else {
                true // No focus, all edges visible
            };

            // Per-kind color (ADR-005), with alpha for the focus dim.
            let (r, g, b) = edge_rgb(edge.edge_type);
            let alpha = if is_connected { 0.85 } else { 0.25 };
            let stroke = format!("rgba({r}, {g}, {b}, {alpha})");

            self.ctx.set_stroke_style_str(&stroke);
            self.ctx.set_line_width(1.5);
            self.set_dash(edge_dashed(edge.edge_type));

            let (sx1, sy1, sx2, sy2) = (x1 as f64, y1 as f64, x2 as f64, y2 as f64);
            let source_radius = (source.radius * self.camera.scale) as f64;
            let target_radius = (target.radius * self.camera.scale) as f64;

            // Inset the line to the node rims so it touches the discs
            // instead of running center-to-center into them. Short hub
            // edges whose rims overlap skip the line — the arrowhead
            // alone marks the relation.
            if let Some(((lx1, ly1), (lx2, ly2))) =
                edge_segment(sx1, sy1, sx2, sy2, source_radius, target_radius)
            {
                self.ctx.begin_path();
                self.ctx.move_to(lx1, ly1);
                self.ctx.line_to(lx2, ly2);
                self.ctx.stroke();
            }

            // Heads read direction at a glance: hollow triangle for
            // inheritance (UML generalization), filled arrow for
            // referential kinds; `inverse` gets one at each end.
            // Heads are always solid-outlined even on a dashed edge.
            if show_arrows {
                let hollow = edge_hollow_head(edge.edge_type);
                self.draw_head(sx1, sy1, sx2, sy2, target_radius, &stroke, hollow);
                if edge_both_ends(edge.edge_type) {
                    self.draw_head(sx2, sy2, sx1, sy1, source_radius, &stroke, hollow);
                }
            }
        }
        // Leave the context in solid-line state for node rendering.
        self.set_dash(false);
    }

    /// Set or clear the canvas line-dash pattern. Edges that should be
    /// dashed (`mixin`, `inverse`) call this with `true`; everything
    /// else — and node rendering — expects solid.
    fn set_dash(&self, dashed: bool) {
        let segments = js_sys::Array::new();
        if dashed {
            segments.push(&6.0.into());
            segments.push(&4.0.into());
        }
        let _ = self.ctx.set_line_dash(&segments);
    }

    /// Draw one arrowhead pointing from `(x1,y1)` toward `(x2,y2)`,
    /// its tip on the target node's perimeter (`target_radius`).
    /// `hollow` strokes a background-filled triangle (UML
    /// generalization); otherwise the triangle is filled with the
    /// edge color. The head is always solid-outlined, even when the
    /// edge line is dashed.
    #[allow(clippy::too_many_arguments)]
    fn draw_head(
        &self,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        target_radius: f64,
        color: &str,
        hollow: bool,
    ) {
        let Some([tip, b1, b2]) = arrowhead_points(x1, y1, x2, y2, target_radius) else {
            return;
        };
        self.set_dash(false);
        self.ctx.begin_path();
        self.ctx.move_to(tip.0, tip.1);
        self.ctx.line_to(b1.0, b1.1);
        self.ctx.line_to(b2.0, b2.1);
        self.ctx.close_path();
        if hollow {
            // Background fill so the edge line doesn't show through,
            // then a solid colored outline — the open-triangle look.
            self.ctx.set_fill_style_str(CANVAS_BG);
            self.ctx.fill();
            self.ctx.set_stroke_style_str(color);
            self.ctx.set_line_width(1.5);
            self.ctx.stroke();
        } else {
            self.ctx.set_fill_style_str(color);
            self.ctx.fill();
        }
    }

    /// Render all nodes
    #[allow(clippy::too_many_arguments)]
    fn render_nodes(
        &self,
        nodes: &[SimNode],
        selected_node: Option<usize>,
        fixed_nodes: &HashSet<usize>,
        focused_node: Option<usize>,
        focused_connected: &HashSet<usize>,
        hidden_nodes: &HashSet<usize>,
    ) {
        for (i, node) in nodes.iter().enumerate() {
            // Skip hidden nodes
            if hidden_nodes.contains(&i) {
                continue;
            }

            let (cx, cy) = self.camera.world_to_canvas(node.x, node.y);
            let radius = node.radius * self.camera.scale;

            let is_selected = selected_node == Some(i);
            let is_fixed = fixed_nodes.contains(&i);

            // Check if this node should be dimmed (focus mode active but node not connected)
            let is_focused_or_connected = if let Some(focused) = focused_node {
                i == focused || focused_connected.contains(&i)
            } else {
                true // No focus, all nodes visible
            };

            // Draw selection highlight ring behind the node
            if is_selected {
                self.ctx.begin_path();
                self.ctx
                    .arc(
                        cx as f64,
                        cy as f64,
                        (radius + 4.0) as f64,
                        0.0,
                        std::f64::consts::TAU,
                    )
                    .ok();
                self.ctx.set_stroke_style_str("rgba(59, 130, 246, 1.0)"); // Blue selection ring
                self.ctx.set_line_width(3.0);
                self.ctx.stroke();
            }

            // Convert color to CSS, with reduced alpha if dimmed
            let alpha = if is_focused_or_connected {
                node.color[3]
            } else {
                node.color[3] * 0.25 // Dim unconnected nodes
            };
            let color = format!(
                "rgba({}, {}, {}, {})",
                (node.color[0] * 255.0) as u8,
                (node.color[1] * 255.0) as u8,
                (node.color[2] * 255.0) as u8,
                alpha
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

            // Draw border - fixed nodes get a thicker orange border
            if is_fixed {
                self.ctx.set_stroke_style_str("rgba(251, 146, 60, 1.0)"); // Orange for fixed
                self.ctx.set_line_width(3.0);
            } else if is_selected {
                self.ctx.set_stroke_style_str("rgba(59, 130, 246, 1.0)"); // Blue for selected
                self.ctx.set_line_width(2.0);
            } else {
                self.ctx.set_stroke_style_str("rgba(255, 255, 255, 0.3)");
                self.ctx.set_line_width(1.0);
            }
            self.ctx.stroke();

            // Draw pin indicator for fixed nodes
            if is_fixed {
                self.ctx.set_fill_style_str("rgba(251, 146, 60, 1.0)");
                // Draw small dot at top of node
                self.ctx.begin_path();
                self.ctx
                    .arc(
                        cx as f64,
                        (cy - radius - 3.0) as f64,
                        3.0,
                        0.0,
                        std::f64::consts::TAU,
                    )
                    .ok();
                self.ctx.fill();
            }
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
            if let Some(idx) = only_index
                && i != idx
            {
                continue;
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
            if let Some(idx) = only_index
                && i != idx
            {
                continue;
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

    /// Convert canvas coordinates to world coordinates (for dragging)
    pub fn canvas_to_world(&self, canvas_x: f32, canvas_y: f32) -> (f32, f32) {
        self.camera.canvas_to_world(canvas_x, canvas_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tip sits on the target node's perimeter, on the line from
    // source to target, pointing at the target.
    #[test]
    fn arrowhead_tip_lands_on_target_perimeter() {
        // Horizontal edge (0,0) → (100,0), target radius 10.
        let [tip, b1, b2] = arrowhead_points(0.0, 0.0, 100.0, 0.0, 10.0).unwrap();
        assert!(
            (tip.0 - 90.0).abs() < 1e-9,
            "tip x on perimeter; got {}",
            tip.0
        );
        assert!(tip.1.abs() < 1e-9, "tip y on the edge line; got {}", tip.1);
        // Base corners sit behind the tip (toward the source) and
        // straddle the line symmetrically.
        assert!(b1.0 < tip.0 && b2.0 < tip.0, "base is behind the tip");
        assert!(
            (b1.1 + b2.1).abs() < 1e-9,
            "base corners straddle the line symmetrically"
        );
    }

    // Direction follows the edge: a vertical edge points straight down.
    #[test]
    fn arrowhead_points_along_edge_direction() {
        let [tip, _, _] = arrowhead_points(0.0, 0.0, 0.0, 50.0, 5.0).unwrap();
        assert!(tip.0.abs() < 1e-9, "tip stays on the vertical line");
        assert!((tip.1 - 45.0).abs() < 1e-9, "tip on perimeter below center");
    }

    // Coincident endpoints have no direction → no arrowhead.
    #[test]
    fn arrowhead_none_for_coincident_endpoints() {
        assert!(arrowhead_points(10.0, 10.0, 10.0, 10.0, 5.0).is_none());
    }

    // The connecting line spans rim to rim, inset by each node's
    // radius, so it touches the discs instead of running to center.
    #[test]
    fn edge_segment_insets_to_both_rims() {
        let ((a, b), (c, d)) = edge_segment(0.0, 0.0, 100.0, 0.0, 10.0, 10.0).unwrap();
        assert!(
            (a - 10.0).abs() < 1e-9 && b.abs() < 1e-9,
            "source rim at +r"
        );
        assert!(
            (c - 90.0).abs() < 1e-9 && d.abs() < 1e-9,
            "target rim at -r"
        );
    }

    // Short hub edges whose rims meet or overlap get no line (the
    // arrowhead alone marks the relation), so the inset can't invert.
    #[test]
    fn edge_segment_none_when_rims_overlap() {
        // len 15 < src_r + tgt_r = 20.
        assert!(edge_segment(0.0, 0.0, 15.0, 0.0, 10.0, 10.0).is_none());
        // Exactly touching (len == sum) is also skipped.
        assert!(edge_segment(0.0, 0.0, 20.0, 0.0, 10.0, 10.0).is_none());
    }

    // The head can't swallow a short edge: it's capped at a fraction
    // of the visible (post-radius) span.
    #[test]
    fn arrowhead_capped_on_short_edge() {
        // Edge length 14, radius 10 → visible span 4; head_len capped
        // at 0.6 * 4 = 2.4, so the base is ~2.4px behind the tip.
        let [tip, b1, _] = arrowhead_points(0.0, 0.0, 14.0, 0.0, 10.0).unwrap();
        let back = tip.0 - b1.0;
        assert!(
            back <= 2.5,
            "head length capped on a short edge; got {back}"
        );
        assert!(back > 0.0, "head still has some length");
    }
}
