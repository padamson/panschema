//! 2D Canvas rendering for graph visualization
//!
//! Renders the force simulation to a 2D HTML canvas.
//! This is the fallback renderer for browsers without WebGPU.

use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};

use std::collections::HashSet;

use crate::camera::{BoundingBox, Camera2D};
use crate::graph_types::{EdgeType, KindMetadata};
use crate::labels::LabelOptions;
use crate::simulation::{CpuSimulation, SimEdge, SimNode};

/// Canvas background; a hollow arrowhead is filled with this so the
/// edge line doesn't show through its interior before the outline is
/// stroked. Keep in sync with the `fill_rect` clear in `render`.
const CANVAS_BG: &str = "#1a1a2e";
/// Amber — the "a rule touches this" accent: the persistent rule ring and
/// the pronounced rule-hover participant ring.
const AMBER: &str = "rgba(251, 191, 36, 1.0)";
/// Blue — the selection ring.
const SELECTION_BLUE: &str = "rgba(59, 130, 246, 1.0)";
/// Ring stroke widths (device px), shared by the graph and the legend so
/// their thickness can't drift. A node touched by a rule wears the thin
/// persistent ring at rest; hovering a rule thickens its participants; the
/// selection ring sits between the two.
const RING_W_RULE: f64 = 2.0;
const RING_W_HOVER: f64 = 3.5;
const RING_W_SELECTED: f64 = 3.0;
/// Ring radius offsets (device px) past the node rim. The amber rule ring
/// hugs the node; the blue selection ring hugs too when it's the only
/// ring, but is pushed outside the rule ring when the node has both, so a
/// selected rule node shows an inner amber ring and an outer blue one
/// instead of the blue burying the amber.
const RING_OFF_RULE: f64 = 3.0;
const RING_OFF_SELECTED: f64 = 4.0;
const RING_OFF_SELECTED_WITH_RULE: f64 = 7.0;

/// Radius offset for the blue selection ring past the node rim. A node that
/// also wears a rule ring gets the selection ring pushed outside it (so
/// both read); otherwise it hugs the node.
fn selection_ring_offset(in_rule: bool) -> f64 {
    if in_rule {
        RING_OFF_SELECTED_WITH_RULE
    } else {
        RING_OFF_SELECTED
    }
}

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

/// Font size (px) for a graph label at the given zoom `scale`, or
/// `None` when the bulk all-labels pass should skip drawing it.
///
/// Labels are *world-space*: the size tracks the zoom transform with no
/// upper cap, so a label grows with its node when you zoom into a dense
/// cluster and shrinks when you zoom out — instead of pinning to a fixed
/// screen size. Below `min_readable` px the bulk pass returns `None`, so
/// a zoomed-out overview drops its labels rather than becoming a wall of
/// overlapping micro-text. A `hovered` label is always drawn, floored to
/// `min_readable` so the one label the user asked for stays legible at
/// any zoom.
fn label_font_size(base: f64, scale: f64, hovered: bool, min_readable: f64) -> Option<f64> {
    let scaled = base * scale;
    if hovered {
        Some(scaled.max(min_readable))
    } else if scaled < min_readable {
        None
    } else {
        Some(scaled)
    }
}

/// Node-label font as a multiple of the node's *world* radius — the
/// `base` fed to [`label_font_size`], so the on-screen font is
/// `node.radius * mult * scale` and the label stays proportional to the
/// rendered node (small at the fit view, growing as you zoom in) rather
/// than an arbitrary multiple of the zoom alone.
const NODE_LABEL_RADIUS_MULT: f64 = 1.3;
/// Edge-label multiple of an endpoint node's world radius — a touch
/// smaller than node labels so relation names don't dominate.
const EDGE_LABEL_RADIUS_MULT: f64 = 1.1;

/// CSS `rgba(...)` string from a normalized `[r, g, b, a]` color.
fn rgba(c: [f32; 4]) -> String {
    format!(
        "rgba({}, {}, {}, {})",
        (c[0] * 255.0) as u8,
        (c[1] * 255.0) as u8,
        (c[2] * 255.0) as u8,
        c[3]
    )
}

/// Outline shape for a node, encoding its kind (ADR-005).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum NodeShape {
    Circle,
    Rectangle,
    Diamond,
    Pill,
}

/// Map a node's resolved kind to its ADR-005 shape. `Class` →
/// circle, `Slot` → pill, `Enum` → diamond; a node with no
/// `KindMetadata` is a `Type` node → rectangle (the only metadata-less
/// kind the graph emits).
fn node_shape(kind: Option<&KindMetadata>) -> NodeShape {
    match kind {
        Some(KindMetadata::Class { .. }) => NodeShape::Circle,
        Some(KindMetadata::Slot { .. }) => NodeShape::Pill,
        Some(KindMetadata::Enum { .. }) => NodeShape::Diamond,
        None => NodeShape::Rectangle,
    }
}

/// The legend's ring rows: `(ring color, device-px width, swatch shape,
/// swatch fill, label)`. Each swatch is a *real* node the ring appears on
/// — the amber rule rings sit on a green slot pill, the blue selection
/// ring on a blue class circle — so the key never shows an impossible node
/// (e.g. a blue pill). The widths are the same `RING_W_*` constants
/// `render_nodes` strokes with, so the key can't drift from the graph. One
/// source of truth for the legend and a unit test.
fn ring_legend_rows() -> [(&'static str, f64, NodeShape, [f32; 4], &'static str); 3] {
    use crate::graph_types::colors;
    [
        (
            AMBER,
            RING_W_RULE,
            NodeShape::Pill,
            colors::SLOT,
            "Slot in a rule",
        ),
        (
            AMBER,
            RING_W_HOVER,
            NodeShape::Pill,
            colors::SLOT,
            "Rule participant (on hover)",
        ),
        (
            SELECTION_BLUE,
            RING_W_SELECTED,
            NodeShape::Circle,
            colors::CLASS,
            "Selected node",
        ),
    ]
}

/// Which ER crow's-foot terminator a slot's effective cardinality
/// maps to, drawn at the target end of a `range` edge (ADR-005).
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CardinalityGlyph {
    /// `1..1` — mandatory-one (two bars).
    MandatoryOne,
    /// `0..1` — optional-one (circle + bar).
    OptionalOne,
    /// `1..*` — mandatory-many (bar + crow's foot).
    MandatoryMany,
    /// `0..*` — optional-many (circle + crow's foot).
    OptionalMany,
    /// An explicit bound the crow's-foot vocabulary can't express
    /// (e.g. `2..5`) — shown as a `min..max` text label.
    Text(String),
}

/// Map a slot's effective cardinality to its terminator glyph.
/// `required` / `multivalued` are the reconciled flags;
/// `min` / `max` are the explicit bounds when set. A bound outside
/// the `{0,1,*}` crow's-foot vocabulary (min > 1, or a finite
/// max > 1) renders as text instead of a foot.
pub(crate) fn cardinality_glyph(
    required: bool,
    multivalued: bool,
    min: Option<u32>,
    max: Option<u32>,
) -> CardinalityGlyph {
    let exotic = min.is_some_and(|m| m > 1) || max.is_some_and(|x| x > 1);
    if exotic {
        let lo = min.map_or_else(|| "0".to_string(), |m| m.to_string());
        let hi = max.map_or_else(|| "*".to_string(), |x| x.to_string());
        return CardinalityGlyph::Text(format!("{lo}..{hi}"));
    }
    match (required, multivalued) {
        (true, false) => CardinalityGlyph::MandatoryOne,
        (false, false) => CardinalityGlyph::OptionalOne,
        (true, true) => CardinalityGlyph::MandatoryMany,
        (false, true) => CardinalityGlyph::OptionalMany,
    }
}

/// Unit vector for `(dx, dy)`, or `(0, 0)` when the input is
/// degenerate (zero length). Used to inset edge endpoints to node
/// rims along the curve tangent.
pub(crate) fn unit(dx: f64, dy: f64) -> (f64, f64) {
    let len = (dx * dx + dy * dy).sqrt();
    if len < f64::EPSILON {
        (0.0, 0.0)
    } else {
        (dx / len, dy / len)
    }
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

    // Proportional to the node radius, so it scales with zoom (zoom
    // in to inspect, out without it vanishing — `max(4)` floor). Still
    // capped at a fraction of the visible (post-radius) span so it
    // can't swallow a short edge.
    let visible = (len - target_radius).max(0.0);
    let head_len = (target_radius * 0.8).max(4.0).min(visible * 0.6);
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

    /// Canvas-space position of a world point, using the same camera
    /// transform that places the node. Exposed so a test can dispatch a
    /// real pointer event at a node without guessing screen coordinates.
    pub(crate) fn world_to_canvas(&self, x: f32, y: f32) -> (f32, f32) {
        self.camera.world_to_canvas(x, y)
    }

    /// Render the simulation state
    #[allow(clippy::too_many_arguments)]
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
        highlighted_nodes: &HashSet<usize>,
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
            highlighted_nodes,
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
        // Group edges by unordered node pair so parallel edges (e.g. a
        // slot's `domain` and `range` both pointing at the same class)
        // can be fanned apart — otherwise their distinct terminators
        // (arrow vs crow's-foot) draw on top of each other.
        let mut parallel: std::collections::HashMap<(usize, usize), Vec<usize>> =
            std::collections::HashMap::new();
        for (i, e) in edges.iter().enumerate() {
            let key = (e.source.min(e.target), e.source.max(e.target));
            parallel.entry(key).or_default().push(i);
        }

        for (edge_idx, edge) in edges.iter().enumerate() {
            // Skip edges connected to hidden nodes
            if hidden_nodes.contains(&edge.source) || hidden_nodes.contains(&edge.target) {
                continue;
            }

            let source = &nodes[edge.source];
            let target = &nodes[edge.target];

            let (x1, y1) = self.camera.world_to_canvas(source.x, source.y);
            let (x2, y2) = self.camera.world_to_canvas(target.x, target.y);

            // Determine if this edge should be dimmed (focus mode active
            // but edge not connected to the focused node).
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

            let src = (x1 as f64, y1 as f64);
            let tgt = (x2 as f64, y2 as f64);
            let source_radius = (source.radius * self.camera.scale) as f64;
            let target_radius = (target.radius * self.camera.scale) as f64;

            let (dx, dy) = (tgt.0 - src.0, tgt.1 - src.1);
            let straight_len = (dx * dx + dy * dy).sqrt();
            if straight_len < f64::EPSILON {
                continue;
            }

            // Parallel edges (e.g. a slot's `domain` and `range` both
            // pointing at the same class) share endpoints but bow apart
            // as quadratic béziers so each keeps a distinct terminator.
            // A lone edge has zero bow → its control sits at the straight
            // midpoint, i.e. it renders straight. The bow scales with the
            // node radius so the fan stays proportional at any zoom.
            let key = (edge.source.min(edge.target), edge.source.max(edge.target));
            let group = &parallel[&key];
            let bow = if group.len() > 1 {
                let pos = group.iter().position(|&i| i == edge_idx).unwrap_or(0) as f64;
                let centered = pos - (group.len() as f64 - 1.0) / 2.0;
                centered * (target_radius * 2.5).max(20.0)
            } else {
                0.0
            };
            let mid = ((src.0 + tgt.0) / 2.0, (src.1 + tgt.1) / 2.0);
            let (perp_x, perp_y) = (-dy / straight_len, dx / straight_len);
            let control = (mid.0 + perp_x * bow, mid.1 + perp_y * bow);

            // Inset each end to the node rim along the curve's tangent
            // (the bézier leaves the source toward `control` and arrives
            // at the target from `control`).
            let src_tan = unit(control.0 - src.0, control.1 - src.1);
            let tgt_tan = unit(tgt.0 - control.0, tgt.1 - control.1);
            let source_rim = (
                src.0 + src_tan.0 * source_radius,
                src.1 + src_tan.1 * source_radius,
            );
            let target_rim = (
                tgt.0 - tgt_tan.0 * target_radius,
                tgt.1 - tgt_tan.1 * target_radius,
            );

            self.ctx.set_stroke_style_str(&stroke);
            self.ctx.set_line_width(1.5);
            self.set_dash(edge_dashed(edge.edge_type));

            // Skip the connecting curve when the rims meet/overlap (short
            // hub edges); the terminator alone marks the relation.
            if straight_len > source_radius + target_radius {
                self.ctx.begin_path();
                self.ctx.move_to(source_rim.0, source_rim.1);
                self.ctx
                    .quadratic_curve_to(control.0, control.1, target_rim.0, target_rim.1);
                self.ctx.stroke();
            }

            // Terminators orient along the curve tangent: passing
            // `control` as the glyph's "from" point makes its direction
            // the bézier tangent at the rim, not the straight chord. A
            // `range` edge's terminator is an ER crow's-foot showing the
            // slot's cardinality (always drawn — cardinality is data,
            // not a direction decoration, so the Arrows toggle doesn't
            // hide it); the slot is the edge source.
            let range_cardinality = (edge.edge_type == EdgeType::Range)
                .then(|| match source.kind_metadata.as_ref() {
                    Some(KindMetadata::Slot {
                        required,
                        multivalued,
                        min,
                        max,
                        ..
                    }) => Some(cardinality_glyph(*required, *multivalued, *min, *max)),
                    _ => None,
                })
                .flatten();

            if let Some(glyph) = range_cardinality {
                self.draw_cardinality(
                    control.0,
                    control.1,
                    tgt.0,
                    tgt.1,
                    target_radius,
                    &glyph,
                    &stroke,
                );
            } else if show_arrows {
                // Hollow triangle for inheritance (UML generalization),
                // filled arrow for referential kinds; `inverse` gets one
                // at each end. Always solid-outlined even on a dashed edge.
                let hollow = edge_hollow_head(edge.edge_type);
                self.draw_head(
                    control.0,
                    control.1,
                    tgt.0,
                    tgt.1,
                    target_radius,
                    &stroke,
                    hollow,
                );
                if edge_both_ends(edge.edge_type) {
                    self.draw_head(
                        control.0,
                        control.1,
                        src.0,
                        src.1,
                        source_radius,
                        &stroke,
                        hollow,
                    );
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

    /// Draw the ER crow's-foot terminator for a `range` edge's
    /// cardinality at the target end. Elements are laid out back from
    /// the target rim toward the source: the cardinality marker (a bar
    /// for "one", a splayed foot for "many") nearest the node, then
    /// the optionality marker further out (a second bar for mandatory,
    /// an open circle for optional). Exotic bounds fall back to text.
    #[allow(clippy::too_many_arguments)]
    fn draw_cardinality(
        &self,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        target_radius: f64,
        glyph: &CardinalityGlyph,
        color: &str,
    ) {
        let (dx, dy) = (x2 - x1, y2 - y1);
        let len = (dx * dx + dy * dy).sqrt();
        if len < f64::EPSILON {
            return;
        }
        let (ux, uy) = (dx / len, dy / len);
        let (px, py) = (-uy, ux);
        let tip = (x2 - ux * target_radius, y2 - uy * target_radius);
        // Proportional to the node radius so it scales with zoom; a
        // small floor keeps it visible when zoomed out.
        let size = (target_radius * 0.8).max(5.0);
        let hw = size * 0.5;
        let at = |d: f64| (tip.0 - ux * d, tip.1 - uy * d);

        if let CardinalityGlyph::Text(text) = glyph {
            self.ctx.set_fill_style_str(color);
            self.ctx.set_font("10px sans-serif");
            let (tx, ty) = at(size + 2.0);
            let _ = self.ctx.fill_text(text, tx + px * 8.0, ty + py * 8.0);
            return;
        }

        self.set_dash(false);
        self.ctx.set_stroke_style_str(color);
        self.ctx.set_line_width(1.5);
        match glyph {
            CardinalityGlyph::MandatoryOne => {
                self.card_bar(at(size * 0.7), (px, py), hw);
                self.card_bar(at(size * 1.4), (px, py), hw);
            }
            CardinalityGlyph::OptionalOne => {
                self.card_bar(at(size * 0.7), (px, py), hw);
                self.card_circle(at(size * 1.6), size * 0.32, color);
            }
            CardinalityGlyph::MandatoryMany => {
                self.card_foot(tip, at(size), (px, py), hw);
                self.card_bar(at(size * 1.5), (px, py), hw);
            }
            CardinalityGlyph::OptionalMany => {
                self.card_foot(tip, at(size), (px, py), hw);
                self.card_circle(at(size * 1.9), size * 0.32, color);
            }
            CardinalityGlyph::Text(_) => unreachable!("handled above"),
        }
    }

    /// A perpendicular tick centered at `c`, half-width `hw`.
    fn card_bar(&self, c: (f64, f64), perp: (f64, f64), hw: f64) {
        self.ctx.begin_path();
        self.ctx.move_to(c.0 + perp.0 * hw, c.1 + perp.1 * hw);
        self.ctx.line_to(c.0 - perp.0 * hw, c.1 - perp.1 * hw);
        self.ctx.stroke();
    }

    /// An open circle (bg-filled, then stroked) centered at `c`.
    fn card_circle(&self, c: (f64, f64), r: f64, color: &str) {
        self.ctx.begin_path();
        let _ = self.ctx.arc(c.0, c.1, r, 0.0, std::f64::consts::TAU);
        self.ctx.set_fill_style_str(CANVAS_BG);
        self.ctx.fill();
        self.ctx.set_stroke_style_str(color);
        self.ctx.stroke();
    }

    /// A splayed three-prong foot: prongs fan from `apex` (back toward
    /// the source) out to the target rim around `tip`.
    fn card_foot(&self, tip: (f64, f64), apex: (f64, f64), perp: (f64, f64), hw: f64) {
        for end in [
            tip,
            (tip.0 + perp.0 * hw, tip.1 + perp.1 * hw),
            (tip.0 - perp.0 * hw, tip.1 - perp.1 * hw),
        ] {
            self.ctx.begin_path();
            self.ctx.move_to(apex.0, apex.1);
            self.ctx.line_to(end.0, end.1);
            self.ctx.stroke();
        }
    }

    /// Begin a node's outline path for the given shape, centered at
    /// `(cx, cy)` and sized to sit within `r` (so the circular
    /// hit-test still holds). Per ADR-005: class = circle, type =
    /// rectangle, enum = diamond, slot = pill — shape encodes kind
    /// redundantly with color so the graph reads in grayscale.
    fn node_path(&self, cx: f64, cy: f64, r: f64, shape: NodeShape) {
        use std::f64::consts::{PI, TAU};
        self.ctx.begin_path();
        match shape {
            NodeShape::Circle => {
                let _ = self.ctx.arc(cx, cy, r, 0.0, TAU);
            }
            NodeShape::Rectangle => {
                let (hw, hh) = (r * 0.8, r * 0.6);
                self.ctx.rect(cx - hw, cy - hh, hw * 2.0, hh * 2.0);
            }
            NodeShape::Diamond => {
                self.ctx.move_to(cx, cy - r);
                self.ctx.line_to(cx + r, cy);
                self.ctx.line_to(cx, cy + r);
                self.ctx.line_to(cx - r, cy);
                self.ctx.close_path();
            }
            NodeShape::Pill => {
                let (hw, rr) = (r * 0.9, r * 0.5);
                let (lx, rx) = (cx - (hw - rr), cx + (hw - rr));
                self.ctx.move_to(lx, cy - rr);
                self.ctx.line_to(rx, cy - rr);
                let _ = self.ctx.arc(rx, cy, rr, -PI / 2.0, PI / 2.0); // right cap
                self.ctx.line_to(lx, cy + rr);
                let _ = self.ctx.arc(lx, cy, rr, PI / 2.0, PI * 1.5); // left cap
                self.ctx.close_path();
            }
        }
    }

    /// Render the notation key onto a dedicated (small) canvas, using
    /// the very same helpers the graph uses — `node_path`, `draw_head`,
    /// `draw_cardinality` — so the legend can't drift from the glyphs it
    /// documents (ADR-005). Drawn at fixed canvas coordinates with no
    /// camera transform and no simulation. The caller sizes the canvas
    /// tall enough to hold every row.
    /// `dpr` is the device-pixel ratio the caller pre-scaled the legend
    /// context by (`setTransform(dpr, …)`). The graph canvas is *not*
    /// dpr-scaled — it strokes in raw device pixels — so every legend line
    /// width is divided by `dpr` here to render at the same on-screen
    /// thickness as the graph, rather than `dpr`× thicker.
    pub fn render_legend(&self, dpr: f64) {
        use crate::graph_types::colors;
        let lw = |w: f64| w / dpr;
        const TEXT: &str = "rgba(232, 232, 244, 0.95)";
        const HEADER: &str = "rgba(150, 150, 178, 0.95)";
        const BORDER: &str = "rgba(255, 255, 255, 0.6)";
        const BODY_FONT: &str = "12px system-ui, -apple-system, sans-serif";
        const HEADER_FONT: &str = "bold 11px system-ui, -apple-system, sans-serif";
        let label_x = 64.0;
        let glyph_x = 26.0;
        let radius = 9.0;
        let row = 21.0;
        let mut y = 18.0;

        self.ctx.set_fill_style_str(CANVAS_BG);
        self.ctx.fill_rect(
            0.0,
            0.0,
            self.camera.width as f64,
            self.camera.height as f64,
        );
        self.ctx.set_text_baseline("middle");

        // --- Nodes (shape encodes kind) ---
        self.ctx.set_font(HEADER_FONT);
        self.ctx.set_fill_style_str(HEADER);
        let _ = self.ctx.fill_text("Nodes", 12.0, y);
        y += row;
        self.ctx.set_font(BODY_FONT);
        let abstract_class = [
            colors::CLASS[0],
            colors::CLASS[1],
            colors::CLASS[2],
            colors::ABSTRACT_ALPHA,
        ];
        let nodes = [
            (NodeShape::Circle, rgba(colors::CLASS), "Class", false),
            (NodeShape::Pill, rgba(colors::SLOT), "Slot", false),
            (NodeShape::Diamond, rgba(colors::ENUM), "Enum", false),
            (NodeShape::Rectangle, rgba(colors::TYPE), "Type", false),
            (
                NodeShape::Circle,
                rgba(abstract_class),
                "Abstract class",
                true,
            ),
        ];
        for (shape, fill, label, dashed) in &nodes {
            self.node_path(glyph_x, y, radius, *shape);
            self.ctx.set_fill_style_str(fill);
            self.ctx.fill();
            self.set_dash(*dashed);
            self.ctx.set_stroke_style_str(BORDER);
            self.ctx.set_line_width(lw(if *dashed { 1.5 } else { 1.0 }));
            self.ctx.stroke();
            self.set_dash(false);
            self.ctx.set_fill_style_str(TEXT);
            let _ = self.ctx.fill_text(label, label_x, y);
            y += row;
        }

        // --- Edges (line style + arrowhead encode the relation) ---
        y += 6.0;
        self.ctx.set_font(HEADER_FONT);
        self.ctx.set_fill_style_str(HEADER);
        let _ = self.ctx.fill_text("Edges", 12.0, y);
        y += row;
        self.ctx.set_font(BODY_FONT);
        let edges = [
            (EdgeType::SubclassOf, "is_a (subclass of)"),
            (EdgeType::Mixin, "mixin"),
            (EdgeType::Domain, "domain"),
            (EdgeType::Range, "range"),
            (EdgeType::Inverse, "inverse of"),
            (EdgeType::TypeOf, "type of"),
        ];
        let (x1, x2) = (12.0, 46.0);
        for (kind, label) in &edges {
            let (r, g, b) = edge_rgb(*kind);
            let color = format!("rgba({r}, {g}, {b}, 0.95)");
            self.set_dash(edge_dashed(*kind));
            self.ctx.set_stroke_style_str(&color);
            self.ctx.set_line_width(lw(1.5));
            self.ctx.begin_path();
            self.ctx.move_to(x1, y);
            self.ctx.line_to(x2, y);
            self.ctx.stroke();
            self.set_dash(false);
            let hollow = edge_hollow_head(*kind);
            self.draw_head(x1, y, x2 + 6.0, y, 6.0, &color, hollow);
            if edge_both_ends(*kind) {
                self.draw_head(x2, y, x1 - 6.0, y, 6.0, &color, hollow);
            }
            self.ctx.set_fill_style_str(TEXT);
            let _ = self.ctx.fill_text(label, label_x, y);
            y += row;
        }

        // --- Cardinality (crow's-foot terminators on range edges) ---
        y += 6.0;
        self.ctx.set_font(HEADER_FONT);
        self.ctx.set_fill_style_str(HEADER);
        let _ = self.ctx.fill_text("Cardinality (range edges)", 12.0, y);
        y += row;
        self.ctx.set_font(BODY_FONT);
        let (cr, cg, cb) = edge_rgb(EdgeType::Range);
        let ccolor = format!("rgba({cr}, {cg}, {cb}, 0.95)");
        let cards = [
            (CardinalityGlyph::MandatoryOne, "1..1  exactly one"),
            (CardinalityGlyph::OptionalOne, "0..1  at most one"),
            (CardinalityGlyph::MandatoryMany, "1..*  one or more"),
            (CardinalityGlyph::OptionalMany, "0..*  any number"),
        ];
        let (cx1, tip_x, tr) = (12.0, 40.0, 8.0);
        for (glyph, label) in &cards {
            self.set_dash(false);
            self.ctx.set_stroke_style_str(&ccolor);
            self.ctx.set_line_width(lw(1.5));
            self.ctx.begin_path();
            self.ctx.move_to(cx1, y);
            self.ctx.line_to(tip_x, y);
            self.ctx.stroke();
            self.draw_cardinality(cx1, y, tip_x + tr, y, tr, glyph, &ccolor);
            self.ctx.set_fill_style_str(TEXT);
            let _ = self.ctx.fill_text(label, label_x, y);
            y += row;
        }

        // --- Rings (overlaid on a node) ---
        y += 6.0;
        self.ctx.set_font(HEADER_FONT);
        self.ctx.set_fill_style_str(HEADER);
        let _ = self.ctx.fill_text("Rings", 12.0, y);
        y += row;
        self.ctx.set_font(BODY_FONT);
        // Each ring is drawn by the same `draw_ring` as the graph, over the
        // real node kind it appears on (amber on a green slot pill, blue on
        // a blue class circle), so the key matches what's on screen and
        // never shows an impossible node (ADR-005). Widths come from the
        // same constants `render_nodes` uses and are dpr-corrected via `lw`.
        let node_r = radius * 0.55;
        let ring_r = node_r + 3.0;
        for (color, width, shape, fill, label) in ring_legend_rows() {
            self.node_path(glyph_x, y, node_r, shape);
            self.ctx.set_fill_style_str(&rgba(fill));
            self.ctx.fill();
            self.draw_ring(glyph_x, y, ring_r, color, lw(width));
            self.ctx.set_fill_style_str(TEXT);
            let _ = self.ctx.fill_text(label, label_x, y);
            y += row;
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
        highlighted_nodes: &HashSet<usize>,
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

            // Selection ring (blue). When the node also wears a rule ring,
            // it sits outside that ring so both read; otherwise it hugs the
            // node.
            if is_selected {
                let sel_r = radius as f64 + selection_ring_offset(node.in_rule);
                self.draw_ring(cx as f64, cy as f64, sel_r, SELECTION_BLUE, RING_W_SELECTED);
            }

            // Rule ring (amber, distinct hue from the blue selection ring so
            // both coexist). Any node a rule touches wears a persistent thin
            // ring hugging the node so rule-related fields stand out at
            // rest; hovering a rule entry in a card thickens that rule's
            // participant rings in place so the focused rule reads above the
            // baseline.
            let rule_r = radius as f64 + RING_OFF_RULE;
            if highlighted_nodes.contains(&i) {
                self.draw_ring(cx as f64, cy as f64, rule_r, AMBER, RING_W_HOVER);
            } else if node.in_rule {
                self.draw_ring(cx as f64, cy as f64, rule_r, AMBER, RING_W_RULE);
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

            // Shape encodes the node kind (ADR-005); within `radius`.
            let shape = node_shape(node.kind_metadata.as_ref());
            self.node_path(cx as f64, cy as f64, radius as f64, shape);
            self.ctx.set_fill_style_str(&color);
            self.ctx.fill();

            // Draw border. Fixed → thick orange, selected → blue,
            // abstract classes → dashed (the structural "don't
            // instantiate" cue, replacing the alpha-only hint),
            // otherwise a thin solid hairline.
            if is_fixed {
                self.ctx.set_stroke_style_str("rgba(251, 146, 60, 1.0)"); // Orange for fixed
                self.ctx.set_line_width(3.0);
                self.set_dash(false);
            } else if is_selected {
                self.ctx.set_stroke_style_str("rgba(59, 130, 246, 1.0)"); // Blue for selected
                self.ctx.set_line_width(2.0);
                self.set_dash(false);
            } else if node.is_abstract {
                self.ctx.set_stroke_style_str("rgba(255, 255, 255, 0.55)");
                self.ctx.set_line_width(1.5);
                self.set_dash(true);
            } else {
                self.ctx.set_stroke_style_str("rgba(255, 255, 255, 0.3)");
                self.ctx.set_line_width(1.0);
                self.set_dash(false);
            }
            self.ctx.stroke();
            self.set_dash(false);

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

    /// Stroke a ring of `radius` centered at `(cx, cy)`. Shared by the
    /// selection ring, the persistent governed-slot ring, and the
    /// rule-hover participant ring so they can't drift from the legend key.
    fn draw_ring(&self, cx: f64, cy: f64, radius: f64, color: &str, width: f64) {
        self.ctx.begin_path();
        self.ctx
            .arc(cx, cy, radius, 0.0, std::f64::consts::TAU)
            .ok();
        self.ctx.set_stroke_style_str(color);
        self.ctx.set_line_width(width);
        self.ctx.stroke();
    }

    /// Render node labels
    /// If `only_index` is Some, only render that specific node's label (for hover)
    fn render_node_labels(&self, nodes: &[SimNode], only_index: Option<usize>) {
        // Labels are sized from each node's on-screen radius (see the
        // per-node `label_font_size` call below), so they stay
        // proportional to the nodes: modest at the fit view, growing as
        // you zoom into a cluster, shrinking and then dropping out when
        // zoomed out. A hovered label always renders.
        let hovered = only_index.is_some();
        self.ctx.set_text_align("left");
        self.ctx.set_text_baseline("middle");

        for (i, node) in nodes.iter().enumerate() {
            // Skip if filtering and this isn't the target
            if let Some(idx) = only_index
                && i != idx
            {
                continue;
            }

            // Font is a multiple of this node's on-screen radius, so the
            // label is proportional to the node at any zoom. The bulk pass
            // drops labels too small to read; a hovered label always draws.
            let Some(font_size) = label_font_size(
                node.radius as f64 * NODE_LABEL_RADIUS_MULT,
                self.camera.scale as f64,
                hovered,
                8.0,
            ) else {
                continue;
            };
            self.ctx.set_font(&format!(
                "{}px -apple-system, BlinkMacSystemFont, sans-serif",
                font_size
            ));

            let (cx, cy) = self.camera.world_to_canvas(node.x, node.y);
            let radius = node.radius * self.camera.scale;

            // Position label to the right of the node
            let label_x = cx + radius + 4.0;
            let label_y = cy;

            // Draw highlight background for hovered label
            if only_index.is_some() {
                let text_width = node.label.len() as f64 * font_size * 0.6;
                let padding = 4.0;
                self.ctx.set_fill_style_str("rgba(59, 130, 246, 0.9)");
                self.ctx.fill_rect(
                    label_x as f64 - padding / 2.0,
                    label_y as f64 - font_size / 2.0 - padding / 2.0,
                    text_width + padding,
                    font_size + padding,
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
        // Sized like node labels — a multiple of an endpoint node's
        // on-screen radius (computed per edge below) so relation names
        // stay proportional to the graph at any zoom.
        let hovered = only_index.is_some();
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

            let Some(font_size) = label_font_size(
                source.radius as f64 * EDGE_LABEL_RADIUS_MULT,
                self.camera.scale as f64,
                hovered,
                7.0,
            ) else {
                continue;
            };
            self.ctx.set_font(&format!(
                "{}px -apple-system, BlinkMacSystemFont, sans-serif",
                font_size
            ));

            let (x1, y1) = self.camera.world_to_canvas(source.x, source.y);
            let (x2, y2) = self.camera.world_to_canvas(target.x, target.y);

            // Midpoint of edge
            let mid_x = (x1 + x2) / 2.0;
            let mid_y = (y1 + y2) / 2.0;

            // Draw background for label
            let padding = 2.0;
            let text_width = edge.label.len() as f64 * font_size * 0.6;
            let bg_width = text_width + padding * 2.0;
            let bg_height = font_size + padding * 2.0;

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

    // Each legend ring swatch is a *real* node the ring appears on — amber
    // rings on a green slot pill, the blue selection ring on a blue class
    // circle — so the key never shows an impossible node like a blue pill.
    // Widths mirror what `render_nodes` strokes, so the key can't drift.
    #[test]
    fn ring_legend_rows_use_real_node_swatches_and_shared_widths() {
        use crate::graph_types::colors;
        let rows = ring_legend_rows();

        // The two amber rule rings sit on the green slot pill.
        assert_eq!(rows[0].2, NodeShape::Pill, "in-rule swatch is a slot pill");
        assert_eq!(rows[0].3, colors::SLOT, "in-rule swatch fills slot green");
        assert_eq!(rows[1].2, NodeShape::Pill, "hover swatch is a slot pill");

        // The blue selection ring sits on a blue class circle — never a
        // blue pill, which is not a real node (classes are circles).
        assert_eq!(
            rows[2].2,
            NodeShape::Circle,
            "selection swatch is a class circle, not a pill"
        );
        assert_eq!(
            rows[2].3,
            colors::CLASS,
            "selection swatch fills class blue"
        );

        // Widths mirror the constants render_nodes strokes with.
        assert_eq!(rows[0].1, RING_W_RULE, "persistent rule ring width");
        assert_eq!(rows[1].1, RING_W_HOVER, "hover participant ring width");
        assert_eq!(rows[2].1, RING_W_SELECTED, "selection ring width");
    }

    // A node that is both selected and rule-touched shows both rings: the
    // selection ring is pushed far enough out that its inner edge clears the
    // outer edge of the (thickest, hover) rule ring, so the blue never
    // buries the amber.
    #[test]
    fn selection_ring_clears_the_rule_ring_when_a_node_has_both() {
        // Hugging offsets when only one ring is present.
        assert_eq!(selection_ring_offset(false), RING_OFF_SELECTED);
        // Pushed out when the node also wears a rule ring.
        assert_eq!(selection_ring_offset(true), RING_OFF_SELECTED_WITH_RULE);

        let selection_inner_edge = selection_ring_offset(true) - RING_W_SELECTED / 2.0;
        let rule_outer_edge = RING_OFF_RULE + RING_W_HOVER / 2.0;
        assert!(
            selection_inner_edge > rule_outer_edge,
            "selection ring (inner edge {selection_inner_edge}) must clear the \
             rule ring (outer edge {rule_outer_edge}) so both are visible"
        );
    }

    // Labels track the zoom transform with no upper cap, so a label
    // grows without bound as you zoom in (it doesn't pin to a fixed
    // screen size the way the old clamp did).
    #[test]
    fn label_font_grows_with_zoom_without_a_ceiling() {
        // base 12 at 1× → 12px; at 10× → 120px (old clamp capped at 48).
        assert_eq!(label_font_size(12.0, 1.0, false, 8.0), Some(12.0));
        assert_eq!(label_font_size(12.0, 10.0, false, 8.0), Some(120.0));
        // Far zoom keeps growing — no cap.
        assert_eq!(label_font_size(12.0, 40.0, false, 8.0), Some(480.0));
    }

    // The bulk pass drops labels once they'd be too small to read, so a
    // zoomed-out overview doesn't crowd with overlapping micro-text.
    #[test]
    fn bulk_label_pass_skips_when_below_readable_size() {
        // 12 × 0.5 = 6px < 8px floor → skipped.
        assert_eq!(label_font_size(12.0, 0.5, false, 8.0), None);
        // Right at the threshold it still draws.
        assert_eq!(label_font_size(12.0, 8.0 / 12.0, false, 8.0), Some(8.0));
    }

    // A hovered label always renders, floored to the readable minimum so
    // the one label the user asked for stays legible even zoomed out.
    #[test]
    fn hovered_label_is_floored_never_skipped() {
        // Would be 3px in the bulk pass (skipped) — hovered floors to 8.
        assert_eq!(label_font_size(12.0, 0.25, true, 8.0), Some(8.0));
        // Above the floor a hovered label still scales up.
        assert_eq!(label_font_size(12.0, 5.0, true, 8.0), Some(60.0));
    }

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

    // Endpoints inset to node rims along the (unit) tangent so the
    // curve touches the discs instead of running to center.
    #[test]
    fn unit_normalizes_and_handles_degenerate() {
        let (ux, uy) = unit(3.0, 4.0);
        assert!((ux - 0.6).abs() < 1e-9 && (uy - 0.8).abs() < 1e-9);
        assert_eq!(unit(0.0, 0.0), (0.0, 0.0), "degenerate → zero vector");
    }

    #[test]
    fn node_shape_maps_kind_to_glyph() {
        use NodeShape::*;
        let class = KindMetadata::Class {
            slots: vec![],
            parents: vec![],
            mixins: vec![],
            rules: vec![],
        };
        let slot = KindMetadata::Slot {
            domains: vec![],
            range: None,
            required: false,
            multivalued: false,
            min: None,
            max: None,
            pattern: None,
            identifier: false,
            any_of: vec![],
        };
        let enum_kind = KindMetadata::Enum {
            permissible_values: vec![],
        };
        assert_eq!(node_shape(Some(&class)), Circle);
        assert_eq!(node_shape(Some(&slot)), Pill);
        assert_eq!(node_shape(Some(&enum_kind)), Diamond);
        assert_eq!(node_shape(None), Rectangle, "Type nodes have no metadata");
    }

    #[test]
    fn cardinality_glyph_maps_flag_combinations() {
        use CardinalityGlyph::*;
        assert_eq!(cardinality_glyph(true, false, None, None), MandatoryOne);
        assert_eq!(cardinality_glyph(false, false, None, None), OptionalOne);
        assert_eq!(cardinality_glyph(true, true, None, None), MandatoryMany);
        assert_eq!(cardinality_glyph(false, true, None, None), OptionalMany);
        // Unbounded many (max None) stays a foot, not text.
        assert_eq!(cardinality_glyph(true, true, Some(1), None), MandatoryMany);
    }

    #[test]
    fn cardinality_glyph_falls_back_to_text_for_exotic_bounds() {
        use CardinalityGlyph::*;
        assert_eq!(
            cardinality_glyph(true, true, Some(1), Some(3)),
            Text("1..3".to_string()),
            "a finite max > 1 can't be a foot"
        );
        assert_eq!(
            cardinality_glyph(true, true, Some(2), None),
            Text("2..*".to_string()),
            "a min > 1 can't be a foot"
        );
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
