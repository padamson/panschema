//! WebGPU visualization for panschema schema graphs
//!
//! This crate provides WASM bindings for embedding interactive schema
//! visualizations in HTML documentation.
//!
//! ## Features
//!
//! - **CPU Fallback**: 2D Canvas rendering with CPU force simulation (default)
//! - **WebGPU** (optional): GPU-accelerated 3D rendering (with `webgpu` feature)

pub mod camera;
mod canvas2d;
mod graph_types;
mod interaction;
mod labels;
pub mod layout;
mod sim_common;
mod simulation;

#[cfg(all(feature = "webgpu", target_arch = "wasm32"))]
pub mod camera3d;
#[cfg(all(feature = "webgpu", target_arch = "wasm32"))]
mod simulation3d;
#[cfg(all(feature = "webgpu", target_arch = "wasm32"))]
mod webgpu;

// For native builds, expose camera3d and simulation3d for testing
// Allow dead_code since these are only used in tests on native
#[cfg(all(feature = "webgpu", not(target_arch = "wasm32")))]
pub mod camera3d;
#[cfg(all(feature = "webgpu", not(target_arch = "wasm32")))]
#[allow(dead_code)]
mod simulation3d;

use graph_types::GraphData;
use interaction::InteractionState;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use canvas2d::Canvas2DRenderer;
use labels::LabelOptions;
use simulation::CpuSimulation;

/// Initialize WASM panic hook for better error messages
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

/// Recommend the 2D default layout for a graph (canonical identifier
/// string) from its inheritance density — `is_a`-heavy schemas get
/// `hierarchical`, mixed-edge schemas `sgd`. The JS layer calls this
/// when neither a persisted user choice nor a manifest
/// `html_default_layout` pins the layout. Malformed JSON falls back to
/// `sgd`.
#[wasm_bindgen]
pub fn recommend_default_layout(graph_json: &str) -> String {
    serde_json::from_str::<GraphData>(graph_json)
        .map(|graph| {
            layout::recommend_default_layout(&graph)
                .as_str()
                .to_string()
        })
        .unwrap_or_else(|_| layout::LayoutAlgorithm::Sgd.as_str().to_string())
}

/// Render the notation legend onto a standalone canvas, reusing the
/// graph's own drawing helpers so the key stays faithful to the
/// glyphs it documents (ADR-005). The caller sizes the canvas tall
/// enough for every row.
#[wasm_bindgen]
pub fn render_legend(canvas: HtmlCanvasElement) -> Result<(), JsValue> {
    let renderer = Canvas2DRenderer::new(canvas).map_err(|e| JsValue::from_str(&e))?;
    renderer.render_legend();
    Ok(())
}

/// Check if WebGPU is supported in the current browser
#[wasm_bindgen]
pub async fn check_webgpu_support() -> bool {
    // Check navigator.gpu availability
    let window = match web_sys::window() {
        Some(w) => w,
        None => return false,
    };

    let navigator = window.navigator();

    // navigator.gpu is not yet in web-sys stable, so we check via js_sys
    let gpu = js_sys::Reflect::get(&navigator, &JsValue::from_str("gpu"));
    match gpu {
        Ok(val) => !val.is_undefined() && !val.is_null(),
        Err(_) => false,
    }
}

/// 2D Visualization state (CPU simulation + Canvas2D rendering)
#[wasm_bindgen]
pub struct Visualization {
    simulation: CpuSimulation,
    renderer: Canvas2DRenderer,
    labels: LabelOptions,
    interaction: InteractionState,
    hovered_node: Option<usize>,
    hovered_edge: Option<usize>,
    /// `true` for static layouts (KK / Sugiyama) whose `freeze_at` set
    /// the simulation to `alpha_min`. The drag handler reads this to
    /// decide whether a click-or-drag should reheat physics:
    /// force-directed → yes (so other nodes adjust); static → no
    /// (only the dragged node moves; rest of layout is preserved).
    is_static_layout: bool,
    /// Draw an arrowhead at each edge's target end. Every schema-graph
    /// edge is directional; the arrowheads make direction readable
    /// without parsing the label. Toggleable (edge-dense graphs read
    /// cleaner without them); defaults on.
    show_arrows: bool,
}

#[wasm_bindgen]
impl Visualization {
    /// Create a new 2D visualization from graph JSON data.
    ///
    /// `aspect_w` and `aspect_h` bias the simulation's settled layout
    /// toward a bounding box of that aspect ratio (e.g. `16, 8` for a
    /// landscape container). Use `1, 1` for the historical circular
    /// equilibrium.
    ///
    /// `layout` selects which algorithm produces node positions. See
    /// [`layout::LayoutAlgorithm`] for the canonical identifiers and
    /// [`layout::LayoutAlgorithm::is_implemented`] for which variants
    /// resolve to a real implementation. Unimplemented variants return
    /// a clear error so the picker UI can validate availability
    /// without losing typing information.
    #[wasm_bindgen(constructor)]
    pub fn new(
        canvas: HtmlCanvasElement,
        graph_json: &str,
        aspect_w: u32,
        aspect_h: u32,
        layout: &str,
    ) -> Result<Visualization, JsValue> {
        use std::str::FromStr;
        let algorithm = layout::LayoutAlgorithm::from_str(layout)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        if !algorithm.is_implemented() {
            return Err(JsValue::from_str(&format!(
                "layout algorithm `{}` is not yet implemented",
                algorithm.as_str()
            )));
        }

        // Parse graph data
        let graph: GraphData = serde_json::from_str(graph_json)
            .map_err(|e| JsValue::from_str(&format!("Failed to parse graph JSON: {}", e)))?;

        // Create simulation. (1, 1) → no-op centering; the historical
        // default. Non-square aspects activate anisotropic forceX/forceY.
        let mut simulation = if aspect_w != aspect_h && aspect_w > 0 && aspect_h > 0 {
            CpuSimulation::from_graph_data(&graph).with_aspect_ratio(aspect_w, aspect_h)
        } else {
            CpuSimulation::from_graph_data(&graph)
        };

        // Static layouts compute final positions up-front and then halt
        // the per-tick physics; the simulation acts as a position
        // container plus drag/hover state. Aspect bias is baked into
        // the algorithm output (not the per-tick forces).
        let mut static_positions: Option<Vec<(f32, f32)>> = match algorithm {
            layout::LayoutAlgorithm::KamadaKawai => Some(layout::kamada_kawai(
                &graph,
                aspect_w as f32,
                aspect_h as f32,
            )),
            layout::LayoutAlgorithm::Hierarchical => Some(layout::hierarchical(
                &graph,
                aspect_w as f32,
                aspect_h as f32,
            )),
            layout::LayoutAlgorithm::Stress => Some(layout::stress_majorization(
                &graph,
                aspect_w as f32,
                aspect_h as f32,
            )),
            layout::LayoutAlgorithm::Sgd => {
                Some(layout::sgd(&graph, aspect_w as f32, aspect_h as f32))
            }
            _ => None,
        };
        let is_static_layout = static_positions.is_some();
        if let Some(positions) = static_positions.as_mut() {
            layout::scale_to_world(positions, layout::WORLD_TARGET_DIMENSION);
            simulation.freeze_at(positions);
        }

        // Create renderer
        let renderer = Canvas2DRenderer::new(canvas)
            .map_err(|e| JsValue::from_str(&format!("Failed to create renderer: {}", e)))?;

        Ok(Visualization {
            simulation,
            renderer,
            labels: LabelOptions::new(),
            interaction: InteractionState::new(),
            hovered_node: None,
            hovered_edge: None,
            is_static_layout,
            show_arrows: true,
        })
    }

    /// Run one simulation tick
    pub fn tick(&mut self) {
        // Use fixed nodes from interaction state, plus dragging node
        let mut fixed = self.interaction.fixed_nodes.clone();
        if let Some(drag_node) = self.interaction.dragging_node() {
            fixed.insert(drag_node);
        }
        self.simulation.tick_with_fixed(&fixed);
    }

    /// Update animation state (smooth transitions)
    pub fn update_animation(&mut self) {
        self.renderer.update_animation();
    }

    /// Render the current state
    pub fn render(&self) {
        // Compute connected nodes for focus mode
        let focused_connected = self.get_focused_connected_set();

        // Compute hidden node indices based on type filter
        let hidden_nodes = self.get_hidden_node_set();

        self.renderer.render(
            &self.simulation,
            &self.labels,
            self.hovered_node,
            self.hovered_edge,
            self.interaction.selected_node,
            &self.interaction.fixed_nodes,
            self.interaction.focused_node,
            &focused_connected,
            &hidden_nodes,
            self.show_arrows,
        );
    }

    /// Get set of node indices that should be hidden based on type filter
    fn get_hidden_node_set(&self) -> std::collections::HashSet<usize> {
        let mut hidden = std::collections::HashSet::new();
        if self.interaction.hidden_types.is_empty() {
            return hidden;
        }
        for (i, node) in self.simulation.nodes.iter().enumerate() {
            let node_type = node_type_string(&node.color);
            if self.interaction.hidden_types.contains(node_type) {
                hidden.insert(i);
            }
        }
        hidden
    }

    /// Get the cached set of nodes within the focused node's
    /// neighborhood. The set is computed once at `focus_node()` time
    /// (the BFS frontier expansion in `InteractionState::focus_node`)
    /// and cleared on `clear_focus()`, so this is an O(1) clone per
    /// render frame instead of an O(E × hop_depth) re-walk.
    fn get_focused_connected_set(&self) -> std::collections::HashSet<usize> {
        self.interaction.focused_neighbors.clone()
    }

    /// Check if simulation is still running
    pub fn is_running(&self) -> bool {
        self.simulation.is_running()
    }

    /// Get the current alpha (temperature) value
    pub fn alpha(&self) -> f32 {
        self.simulation.config.alpha
    }

    /// Pan the view by delta pixels
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.renderer.pan(dx, dy);
    }

    /// Zoom the view by factor (1.1 = zoom in 10%, 0.9 = zoom out 10%)
    pub fn zoom(&mut self, factor: f32) {
        self.renderer.zoom(factor);
    }

    /// Reset view to default
    pub fn reset_view(&mut self) {
        self.renderer.reset_view();
    }

    /// Get number of nodes
    pub fn node_count(&self) -> usize {
        self.simulation.nodes.len()
    }

    /// Get number of edges
    pub fn edge_count(&self) -> usize {
        self.simulation.edges.len()
    }

    /// Count the number of edge-segment pairs that cross in the
    /// current 2D layout. Shared-endpoint pairs are excluded; only
    /// proper crossings count. Used by the screenshot harness to
    /// compare layout quality between single-seed and multi-seed
    /// constructors.
    pub fn edge_crossings(&self) -> usize {
        let positions: Vec<(f32, f32)> = self.simulation.nodes.iter().map(|n| (n.x, n.y)).collect();
        let edges: Vec<(usize, usize)> = self
            .simulation
            .edges
            .iter()
            .map(|e| (e.source, e.target))
            .collect();
        crate::sim_common::count_edge_crossings_2d(&positions, &edges)
    }

    /// Resize the canvas
    pub fn resize(&mut self, width: u32, height: u32) {
        self.renderer.resize(width, height);
    }

    /// Run simulation to convergence (blocking)
    pub fn run_to_convergence(&mut self, max_iterations: usize) {
        self.simulation.run_to_convergence(max_iterations);
    }

    /// Fit the graph to fill the canvas with padding
    pub fn fit_to_bounds(&mut self, padding: f32) {
        self.renderer.fit_to_bounds(&self.simulation.nodes, padding);
    }

    /// Check if this is a 3D visualization
    pub fn is_3d(&self) -> bool {
        false
    }

    // ========================================================================
    // Label visibility controls
    // ========================================================================

    /// Toggle all labels on/off
    pub fn toggle_labels(&mut self) {
        self.labels.toggle_all();
    }

    /// Toggle node labels on/off
    pub fn toggle_node_labels(&mut self) {
        self.labels.toggle_node_labels();
    }

    /// Toggle edge labels on/off
    pub fn toggle_edge_labels(&mut self) {
        self.labels.toggle_edge_labels();
    }

    /// Set all labels visibility
    pub fn set_labels(&mut self, visible: bool) {
        self.labels.set_all(visible);
    }

    /// Set node labels visibility
    pub fn set_node_labels(&mut self, visible: bool) {
        self.labels.set_node_labels(visible);
    }

    /// Set edge labels visibility
    pub fn set_edge_labels(&mut self, visible: bool) {
        self.labels.set_edge_labels(visible);
    }

    /// Check if node labels are visible
    pub fn show_node_labels(&self) -> bool {
        self.labels.show_node_labels()
    }

    /// Check if edge labels are visible
    pub fn show_edge_labels(&self) -> bool {
        self.labels.show_edge_labels()
    }

    /// Toggle directed-edge arrowheads on/off.
    pub fn set_arrows(&mut self, visible: bool) {
        self.show_arrows = visible;
    }

    /// Whether directed-edge arrowheads are drawn.
    pub fn show_arrows(&self) -> bool {
        self.show_arrows
    }

    /// Check if all labels are enabled (master toggle)
    pub fn labels_enabled(&self) -> bool {
        self.labels.all_labels
    }

    /// Check if node labels toggle is on
    pub fn node_labels_enabled(&self) -> bool {
        self.labels.node_labels
    }

    /// Check if edge labels toggle is on
    pub fn edge_labels_enabled(&self) -> bool {
        self.labels.edge_labels
    }

    // ========================================================================
    // Hover detection
    // ========================================================================

    /// Update hover state based on canvas coordinates
    /// Returns true if hover state changed
    pub fn update_hover(&mut self, canvas_x: f32, canvas_y: f32) -> bool {
        let old_node = self.hovered_node;
        let old_edge = self.hovered_edge;

        // Check for hovered node first
        self.hovered_node = self
            .renderer
            .node_at(canvas_x, canvas_y, &self.simulation.nodes);

        // Only check edges if no node is hovered
        if self.hovered_node.is_none() {
            self.hovered_edge = self.renderer.edge_at(
                canvas_x,
                canvas_y,
                &self.simulation.edges,
                &self.simulation.nodes,
                30.0, // threshold in pixels
            );
        } else {
            self.hovered_edge = None;
        }

        old_node != self.hovered_node || old_edge != self.hovered_edge
    }

    /// Clear hover state
    pub fn clear_hover(&mut self) {
        self.hovered_node = None;
        self.hovered_edge = None;
    }

    /// Get the currently hovered node index (-1 if none)
    pub fn hovered_node_index(&self) -> i32 {
        self.hovered_node.map(|i| i as i32).unwrap_or(-1)
    }

    /// Get the currently hovered edge index (-1 if none)
    pub fn hovered_edge_index(&self) -> i32 {
        self.hovered_edge.map(|i| i as i32).unwrap_or(-1)
    }

    // ========================================================================
    // Selection and interaction
    // ========================================================================

    /// Select node at canvas coordinates, or deselect if clicking empty space.
    /// Returns the selected node index (-1 if none).
    pub fn select_at(&mut self, canvas_x: f32, canvas_y: f32) -> i32 {
        // Check for node at position
        let node_index = self
            .renderer
            .node_at(canvas_x, canvas_y, &self.simulation.nodes);

        self.interaction.select_node(node_index);
        self.selected_node_index()
    }

    /// Get the currently selected node index (-1 if none)
    pub fn selected_node_index(&self) -> i32 {
        self.interaction
            .selected_node
            .map(|i| i as i32)
            .unwrap_or(-1)
    }

    /// Deselect the current node
    pub fn deselect(&mut self) {
        self.interaction.deselect();
    }

    /// Check if a node is currently fixed
    pub fn is_node_fixed(&self, index: usize) -> bool {
        self.interaction.is_fixed(index)
    }

    // ========================================================================
    // Drag operations
    // ========================================================================

    /// Start dragging a node at canvas coordinates.
    /// Returns the node index if a node was found (-1 if none).
    pub fn start_drag_at(&mut self, canvas_x: f32, canvas_y: f32) -> i32 {
        if let Some(index) = self
            .renderer
            .node_at(canvas_x, canvas_y, &self.simulation.nodes)
        {
            self.interaction.start_drag(index, canvas_x, canvas_y);
            // Force-directed layouts reheat so the rest of the graph
            // adjusts around the drag. Static layouts (KK / Sugiyama)
            // skip reheat — `drag_to` moves the dragged node directly
            // via `set_node_position`, and the rest of the layout
            // stays where the static algorithm placed it.
            if !self.is_static_layout {
                self.simulation.reheat(0.3);
            }
            index as i32
        } else {
            -1
        }
    }

    /// Move the currently dragged node to new canvas coordinates.
    pub fn drag_to(&mut self, canvas_x: f32, canvas_y: f32) {
        if let Some(index) = self.interaction.dragging_node() {
            let (world_x, world_y) = self.renderer.canvas_to_world(canvas_x, canvas_y);
            self.simulation.set_node_position(index, world_x, world_y);
        }
    }

    /// End the drag operation.
    /// If `keep_fixed` is true, the node will remain fixed after release.
    pub fn end_drag(&mut self, keep_fixed: bool) {
        self.interaction.end_drag(keep_fixed);
    }

    /// Check if we're currently dragging a node
    pub fn is_dragging(&self) -> bool {
        self.interaction.dragging_node().is_some()
    }

    /// Get the index of the node being dragged (-1 if none)
    pub fn dragging_node_index(&self) -> i32 {
        self.interaction
            .dragging_node()
            .map(|i| i as i32)
            .unwrap_or(-1)
    }

    /// Toggle the fixed state of a node at canvas coordinates.
    /// Returns the node index if found (-1 if no node at position).
    pub fn toggle_fixed_at(&mut self, canvas_x: f32, canvas_y: f32) -> i32 {
        if let Some(index) = self
            .renderer
            .node_at(canvas_x, canvas_y, &self.simulation.nodes)
        {
            self.interaction.toggle_fixed(index);
            index as i32
        } else {
            -1
        }
    }

    /// Toggle the fixed state of a node by index (used by shift+click).
    pub fn toggle_fixed(&mut self, index: usize) {
        if index < self.simulation.nodes.len() {
            self.interaction.toggle_fixed(index);
        }
    }

    /// Unfix a node by index (let it move freely in simulation)
    pub fn unfix_node(&mut self, index: usize) {
        self.interaction.unfix_node(index);
    }

    /// Get details for a node as JSON.
    /// Returns empty object if index is out of bounds.
    pub fn get_node_details(&self, index: usize) -> String {
        if index >= self.simulation.nodes.len() {
            return "{}".to_string();
        }
        let node = &self.simulation.nodes[index];
        let is_fixed = self.interaction.is_fixed(index);
        let connections = self.get_connected_node_ids(index);
        build_node_details_json(node, is_fixed, connections)
    }

    /// Get details for an edge as JSON. Returns an empty object if
    /// `index` is out of bounds. Used by the hover-driven ephemeral
    /// details card to surface edge metadata without committing to a
    /// click. The card consumer reads `type`, `source`, and `target`
    /// to render a "<source-label> --(<edge-type>)--> <target-label>"
    /// triple.
    pub fn get_edge_details(&self, index: usize) -> String {
        if index >= self.simulation.edges.len() {
            return "{}".to_string();
        }
        let edge = &self.simulation.edges[index];
        let source_node = &self.simulation.nodes[edge.source];
        let target_node = &self.simulation.nodes[edge.target];
        build_edge_details_json(edge, source_node, target_node)
    }

    /// Get IDs of nodes directly connected to the given node
    fn get_connected_node_ids(&self, index: usize) -> Vec<String> {
        let mut connected = Vec::new();
        for edge in &self.simulation.edges {
            if edge.source == index {
                connected.push(self.simulation.nodes[edge.target].id.clone());
            } else if edge.target == index {
                connected.push(self.simulation.nodes[edge.source].id.clone());
            }
        }
        connected.sort();
        connected.dedup();
        connected
    }

    // ========================================================================
    // Focus mode
    // ========================================================================

    /// Set focus on a node (dims everything outside the node's
    /// neighborhood). The neighborhood is expanded `max_hops` levels
    /// from the focal node — the typical schema-author setting is 2,
    /// which reveals the local cluster without dragging in the whole
    /// graph.
    pub fn focus_node(&mut self, index: usize, max_hops: usize) {
        if index < self.simulation.nodes.len() {
            // Flatten the simulation's edge list into the
            // (source_idx, target_idx) shape `InteractionState::focus_node`
            // walks. Cheap (O(E), small constants) and avoids
            // leaking the SimEdge type across the interaction layer.
            let edges: Vec<(usize, usize)> = self
                .simulation
                .edges
                .iter()
                .map(|e| (e.source, e.target))
                .collect();
            self.interaction.focus_node(index, &edges, max_hops);
        }
    }

    /// Clear focus mode.
    pub fn clear_focus(&mut self) {
        self.interaction.clear_focus();
    }

    /// Get the focused node index (-1 if none).
    pub fn focused_node_index(&self) -> i32 {
        self.interaction
            .focused_node()
            .map(|i| i as i32)
            .unwrap_or(-1)
    }

    /// Get indices of nodes connected to the given node as JSON.
    pub fn get_connected_indices_json(&self, index: usize) -> String {
        let mut connected = Vec::new();
        for edge in &self.simulation.edges {
            if edge.source == index {
                connected.push(edge.target);
            } else if edge.target == index {
                connected.push(edge.source);
            }
        }
        connected.sort();
        connected.dedup();
        serde_json::to_string(&connected).unwrap_or_else(|_| "[]".to_string())
    }

    // ========================================================================
    // Type filtering
    // ========================================================================

    /// Toggle visibility of a node type (Class, Slot, Enum, Type).
    pub fn toggle_type_filter(&mut self, node_type: &str) {
        self.interaction.toggle_type(node_type);
    }

    /// Check if a node type is visible.
    pub fn is_type_visible(&self, node_type: &str) -> bool {
        self.interaction.is_type_visible(node_type)
    }

    /// Get the type of a node by index.
    pub fn get_node_type(&self, index: usize) -> String {
        if index >= self.simulation.nodes.len() {
            return "Unknown".to_string();
        }
        node_type_string(&self.simulation.nodes[index].color).to_string()
    }
}

/// Build the JSON payload returned by `Visualization::get_node_details`
/// — extracted as a free function so it's unit-testable without a
/// `Visualization` (which needs an `HtmlCanvasElement` and so can only
/// be constructed in a browser). Both the 2D and 3D wasm-bindgen
/// methods delegate here.
pub(crate) fn build_node_details_json(
    node: &crate::simulation::SimNode,
    is_fixed: bool,
    connections: Vec<String>,
) -> String {
    let node_type = node_type_string(&node.color);
    serde_json::json!({
        "id": node.id,
        "label": node.label,
        "type": node_type,
        "isAbstract": node.is_abstract,
        "uri": node.uri,
        "uriUnresolved": node.uri_unresolved,
        "description": node.description,
        "isFixed": is_fixed,
        "connections": connections,
        "kindMetadata": node.kind_metadata,
        "x": node.x,
        "y": node.y,
    })
    .to_string()
}

/// Build the JSON payload returned by `Visualization::get_edge_details`
/// — extracted alongside [`build_node_details_json`] for the same
/// testability reason. The hover-card JS consumer reads `type`,
/// `source.{id,label}`, and `target.{id,label}` to render an
/// edge-flavored triple.
pub(crate) fn build_edge_details_json(
    edge: &crate::simulation::SimEdge,
    source_node: &crate::simulation::SimNode,
    target_node: &crate::simulation::SimNode,
) -> String {
    serde_json::json!({
        "type": edge.label,
        "kind": edge_type_tag(edge.edge_type),
        "source": {
            "id": source_node.id,
            "label": source_node.label,
        },
        "target": {
            "id": target_node.id,
            "label": target_node.label,
        },
    })
    .to_string()
}

/// Stable string tag for an [`EdgeType`]. Matches the lower-camel
/// labels the hover card already shows, but is produced from the
/// type tag rather than the edge label so author-overridden labels
/// don't break the JS lookup of the matching blurb.
fn edge_type_tag(edge_type: crate::graph_types::EdgeType) -> &'static str {
    use crate::graph_types::EdgeType;
    match edge_type {
        EdgeType::SubclassOf => "subclassOf",
        EdgeType::Mixin => "mixin",
        EdgeType::Domain => "domain",
        EdgeType::Range => "range",
        EdgeType::Inverse => "inverseOf",
        EdgeType::TypeOf => "typeOf",
    }
}

/// Helper to determine node type from color
fn node_type_string(color: &[f32; 4]) -> &'static str {
    // Match colors defined in graph_types::colors
    const BLUE_R: f32 = 0.290;
    const GREEN_R: f32 = 0.314;
    const PURPLE_R: f32 = 0.608;
    const ORANGE_R: f32 = 0.902;

    let r = color[0];
    if (r - BLUE_R).abs() < 0.1 {
        "Class"
    } else if (r - GREEN_R).abs() < 0.1 {
        "Slot"
    } else if (r - PURPLE_R).abs() < 0.1 {
        "Enum"
    } else if (r - ORANGE_R).abs() < 0.1 {
        "Type"
    } else {
        "Unknown"
    }
}

#[cfg(test)]
mod details_json_tests {
    use super::*;
    use crate::graph_types::{
        EdgeType, GraphEdge, GraphNode, KindMetadata, NodeType, PermissibleValueSummary,
        SlotSummary,
    };
    use crate::simulation::{SimEdge, SimNode};

    fn make_class_node(id: &str, label: &str) -> SimNode {
        SimNode::from_graph_node(
            &GraphNode {
                id: id.to_string(),
                label: label.to_string(),
                node_type: NodeType::Class,
                color: NodeType::Class.color(),
                description: None,
                uri: None,
                uri_unresolved: false,
                is_abstract: false,
                kind_metadata: None,
            },
            0,
            1,
        )
    }

    fn make_node_with_metadata(
        id: &str,
        label: &str,
        description: Option<&str>,
        uri: Option<&str>,
        is_abstract: bool,
    ) -> SimNode {
        SimNode::from_graph_node(
            &GraphNode {
                id: id.to_string(),
                label: label.to_string(),
                node_type: NodeType::Class,
                color: NodeType::Class.color(),
                description: description.map(|s| s.to_string()),
                uri: uri.map(|s| s.to_string()),
                uri_unresolved: false,
                is_abstract,
                kind_metadata: None,
            },
            0,
            1,
        )
    }

    #[test]
    fn build_node_details_json_emits_required_fields_for_minimal_node() {
        // A node with no description, no IRI, no abstract flag, and no
        // connections still produces a JSON object with every field
        // the hover-card consumer reads. Missing values come through
        // as `null` (description, uri) or empty arrays (connections),
        // not absent keys.
        let node = make_class_node("class:Foo", "Foo");
        let json: serde_json::Value =
            serde_json::from_str(&build_node_details_json(&node, false, Vec::new())).unwrap();
        assert_eq!(json["id"], "class:Foo");
        assert_eq!(json["label"], "Foo");
        assert_eq!(json["type"], "Class");
        assert_eq!(json["isFixed"], false);
        assert_eq!(json["isAbstract"], false);
        assert!(json["description"].is_null(), "description should be null");
        assert!(json["uri"].is_null(), "uri should be null");
        assert_eq!(json["connections"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn build_node_details_json_surfaces_description_uri_and_abstract() {
        // When the LinkML source provides description / uri /
        // abstract=true, the wire format carries them verbatim so the
        // hover card can render them. Pinned because these three are
        // the schema-author-facing fields the card was designed
        // around — silently dropping any of them turns the card back
        // into label-echo.
        let node = make_node_with_metadata(
            "class:BFOEntity",
            "BFOEntity",
            Some("The root of the BFO upper ontology."),
            Some("http://purl.obolibrary.org/obo/BFO_0000001"),
            true,
        );
        let json: serde_json::Value =
            serde_json::from_str(&build_node_details_json(&node, false, Vec::new())).unwrap();
        assert_eq!(json["isAbstract"], true);
        assert_eq!(json["description"], "The root of the BFO upper ontology.");
        assert_eq!(json["uri"], "http://purl.obolibrary.org/obo/BFO_0000001");
    }

    #[test]
    fn build_node_details_json_returns_connection_ids_in_order() {
        // The connection list is the caller's responsibility (the
        // wasm-bindgen wrapper computes it from the simulation
        // edges). The JSON builder must pass it through unchanged so
        // the consumer can show the count and, in a future slice, the
        // list itself.
        let node = make_class_node("class:Foo", "Foo");
        let json: serde_json::Value = serde_json::from_str(&build_node_details_json(
            &node,
            true,
            vec!["class:Bar".to_string(), "class:Baz".to_string()],
        ))
        .unwrap();
        assert_eq!(json["isFixed"], true);
        let connections = json["connections"].as_array().unwrap();
        assert_eq!(connections.len(), 2);
        assert_eq!(connections[0], "class:Bar");
        assert_eq!(connections[1], "class:Baz");
    }

    #[test]
    fn build_edge_details_json_emits_source_target_triple() {
        // The hover card renders an edge as
        // "<source-label> ↓ <edge-type> ↓ <target-label>". The
        // builder's contract is to surface labels (for display) and
        // ids (for any future hop-out affordance) on both endpoints
        // plus the edge's type string.
        let source = make_class_node("class:Hypothesis", "Hypothesis");
        let target = make_class_node("class:Premise", "Premise");
        let edge = SimEdge {
            source: 0,
            target: 1,
            label: SimEdge::format_edge_type_external(EdgeType::SubclassOf),
            edge_type: EdgeType::SubclassOf,
        };
        let json: serde_json::Value =
            serde_json::from_str(&build_edge_details_json(&edge, &source, &target)).unwrap();
        assert_eq!(json["type"], "subclassOf");
        assert_eq!(json["source"]["id"], "class:Hypothesis");
        assert_eq!(json["source"]["label"], "Hypothesis");
        assert_eq!(json["target"]["id"], "class:Premise");
        assert_eq!(json["target"]["label"], "Premise");
    }

    // Used here to exercise the same edge-type → string mapping the
    // production path uses, without exposing the inner `format_edge_type`
    // helper publicly. Avoids the test reinventing the label table.
    impl SimEdge {
        fn format_edge_type_external(edge_type: EdgeType) -> String {
            // Mirror the production helper. If the production helper's
            // mapping ever changes, this test will catch the divergence
            // when the asserted label no longer matches.
            let edge = GraphEdge {
                source: "a".into(),
                target: "b".into(),
                edge_type,
                label: None,
            };
            // Construct via the same factory the simulation uses so we
            // catch the exact label format. Two nodes — the simulation
            // drops edges whose endpoints aren't declared.
            let nodes = vec![
                GraphNode {
                    id: "a".into(),
                    label: "a".into(),
                    node_type: NodeType::Class,
                    color: NodeType::Class.color(),
                    description: None,
                    uri: None,
                    uri_unresolved: false,
                    is_abstract: false,
                    kind_metadata: None,
                },
                GraphNode {
                    id: "b".into(),
                    label: "b".into(),
                    node_type: NodeType::Class,
                    color: NodeType::Class.color(),
                    description: None,
                    uri: None,
                    uri_unresolved: false,
                    is_abstract: false,
                    kind_metadata: None,
                },
            ];
            let graph = crate::graph_types::GraphData {
                schema_name: "x".into(),
                schema_title: None,
                format_version: "1.0".into(),
                nodes,
                edges: vec![edge],
            };
            let sim = crate::simulation::CpuSimulation::from_graph_data(&graph);
            sim.edges
                .first()
                .map(|e| e.label.clone())
                .unwrap_or_default()
        }
    }

    fn make_node_with_kind(id: &str, label: &str, kind: KindMetadata) -> SimNode {
        SimNode::from_graph_node(
            &GraphNode {
                id: id.to_string(),
                label: label.to_string(),
                node_type: NodeType::Class,
                color: NodeType::Class.color(),
                description: None,
                uri: None,
                uri_unresolved: false,
                is_abstract: false,
                kind_metadata: Some(kind),
            },
            0,
            1,
        )
    }

    fn pv(text: &str, description: Option<&str>, meaning: Option<&str>) -> PermissibleValueSummary {
        PermissibleValueSummary {
            text: text.to_string(),
            description: description.map(str::to_string),
            meaning: meaning.map(str::to_string),
        }
    }

    #[test]
    fn build_node_details_json_omits_kind_metadata_when_none() {
        // A node with no resolved metadata (the common case for `Type`
        // nodes today) must still produce a JSON object whose
        // `kindMetadata` key is `null`, not absent. The hover-card JS
        // distinguishes "no extra context yet" (`null`) from "consumer
        // hasn't shipped" (key missing) — keeping the key present
        // means the JS branch reads `details.kindMetadata == null`
        // rather than `'kindMetadata' in details`.
        let node = make_class_node("class:Foo", "Foo");
        let json: serde_json::Value =
            serde_json::from_str(&build_node_details_json(&node, false, Vec::new())).unwrap();
        assert!(
            json.get("kindMetadata").is_some(),
            "kindMetadata key must be present"
        );
        assert!(json["kindMetadata"].is_null());
    }

    #[test]
    fn build_node_details_json_surfaces_class_kind_metadata() {
        // A LinkML class with resolved slots/parents/mixins must
        // round-trip the lists verbatim and tag the payload as
        // `kind: "class"`. The JS hover card dispatches on the tag to
        // pick which section to render, so the tag is part of the
        // contract — not implementation detail.
        let node = make_node_with_kind(
            "class:Activity",
            "Activity",
            KindMetadata::Class {
                slots: vec![
                    SlotSummary {
                        name: "startedAt".into(),
                        range: Some("datetime".into()),
                        required: true,
                        multivalued: false,
                        min: None,
                        max: None,
                        origin: None,
                    },
                    SlotSummary {
                        name: "endedAt".into(),
                        range: None,
                        required: false,
                        multivalued: false,
                        min: None,
                        max: None,
                        origin: Some("mixin Auditable".into()),
                    },
                ],
                parents: vec!["Entity".into()],
                mixins: vec!["Auditable".into()],
            },
        );
        let json: serde_json::Value =
            serde_json::from_str(&build_node_details_json(&node, false, Vec::new())).unwrap();
        let km = &json["kindMetadata"];
        assert_eq!(km["kind"], "class");
        assert_eq!(km["slots"][0]["name"], "startedAt");
        assert_eq!(km["slots"][0]["range"], "datetime");
        assert_eq!(km["slots"][0]["required"], true);
        assert_eq!(km["slots"][1]["name"], "endedAt");
        assert_eq!(km["slots"][1]["origin"], "mixin Auditable");
        assert_eq!(km["parents"][0], "Entity");
        assert_eq!(km["mixins"][0], "Auditable");
    }

    #[test]
    fn build_node_details_json_surfaces_slot_kind_metadata() {
        // Slots carry domain/range plus boolean flags. The booleans
        // ride along even when `false` so the JS card can render an
        // explicit "single-valued" badge instead of guessing from key
        // absence.
        let node = make_node_with_kind(
            "slot:hasOwner",
            "hasOwner",
            KindMetadata::Slot {
                domain: Some("Animal".into()),
                range: Some("Person".into()),
                required: true,
                multivalued: false,
                min: None,
                max: None,
                pattern: Some("^[A-Z]".into()),
                identifier: true,
                any_of: vec!["Person".into(), "Organization".into()],
            },
        );
        let json: serde_json::Value =
            serde_json::from_str(&build_node_details_json(&node, false, Vec::new())).unwrap();
        let km = &json["kindMetadata"];
        assert_eq!(km["kind"], "slot");
        assert_eq!(km["domain"], "Animal");
        assert_eq!(km["range"], "Person");
        assert_eq!(km["required"], true);
        // `multivalued: false` is skipped by serde to keep the wire
        // small; the JS card treats absent as `false`.
        assert!(km.get("multivalued").is_none());
        assert_eq!(km["pattern"], "^[A-Z]");
        assert_eq!(km["identifier"], true);
        assert_eq!(km["anyOf"], serde_json::json!(["Person", "Organization"]));
    }

    #[test]
    fn build_node_details_json_surfaces_enum_permissible_values_in_order() {
        // Enum permissible-value order is meaningful (it matches the
        // LinkML schema's declaration order, which authors often use
        // for severity scales / ordinal categories). The wire format
        // must preserve that order without resorting alphabetically.
        let node = make_node_with_kind(
            "enum:Severity",
            "Severity",
            KindMetadata::Enum {
                permissible_values: vec![
                    pv("low", None, None),
                    pv("medium", Some("Moderate severity"), None),
                    pv("high", None, Some("ex:High")),
                ],
            },
        );
        let json: serde_json::Value =
            serde_json::from_str(&build_node_details_json(&node, false, Vec::new())).unwrap();
        let km = &json["kindMetadata"];
        assert_eq!(km["kind"], "enum");
        let pvs = km["permissibleValues"].as_array().unwrap();
        assert_eq!(pvs.len(), 3);
        assert_eq!(pvs[0]["text"], "low");
        // `description` / `meaning` are omitted when absent.
        assert!(pvs[0].get("description").is_none());
        assert_eq!(pvs[1]["text"], "medium");
        assert_eq!(pvs[1]["description"], "Moderate severity");
        assert_eq!(pvs[2]["text"], "high");
        assert_eq!(pvs[2]["meaning"], "ex:High");
    }

    #[test]
    fn build_edge_details_json_emits_kind_tag_independent_of_label() {
        // The `kind` tag comes from the edge_type, not the label.
        // Author-supplied labels can override the displayed text
        // (e.g. a domain-specific "produces" instead of "range"); the
        // hover card needs the stable tag to look up the matching
        // semantic blurb.
        let source = make_class_node("class:A", "A");
        let target = make_class_node("class:B", "B");
        let edge = SimEdge {
            source: 0,
            target: 1,
            label: "produces".to_string(),
            edge_type: EdgeType::Range,
        };
        let json: serde_json::Value =
            serde_json::from_str(&build_edge_details_json(&edge, &source, &target)).unwrap();
        assert_eq!(json["type"], "produces");
        assert_eq!(json["kind"], "range");
    }
}

/// Create a 2D visualization (convenience function). Pass `1, 1` for
/// the original circular equilibrium and `"force-directed"` for the
/// only currently-implemented layout.
#[wasm_bindgen]
pub fn create_visualization(
    canvas: HtmlCanvasElement,
    graph_json: &str,
    aspect_w: u32,
    aspect_h: u32,
    layout: &str,
) -> Result<Visualization, JsValue> {
    Visualization::new(canvas, graph_json, aspect_w, aspect_h, layout)
}

// ============================================================================
// WebGPU 3D Visualization (only when webgpu feature is enabled)
// ============================================================================

#[cfg(all(feature = "webgpu", target_arch = "wasm32"))]
use simulation3d::Simulation3D;
#[cfg(all(feature = "webgpu", target_arch = "wasm32"))]
use webgpu::WebGpuRenderer;

/// 3D Visualization state (3D simulation + WebGPU rendering)
#[cfg(all(feature = "webgpu", target_arch = "wasm32"))]
#[wasm_bindgen]
pub struct Visualization3D {
    simulation: Simulation3D,
    renderer: WebGpuRenderer,
    labels: LabelOptions,
    interaction: InteractionState,
    hovered_node: Option<usize>,
}

#[cfg(all(feature = "webgpu", target_arch = "wasm32"))]
#[wasm_bindgen]
impl Visualization3D {
    /// Run one simulation tick
    pub fn tick(&mut self) {
        // Use fixed nodes from interaction state, plus dragging node
        let mut fixed = self.interaction.fixed_nodes.clone();
        if let Some(drag_node) = self.interaction.dragging_node() {
            fixed.insert(drag_node);
        }
        self.simulation.tick_with_fixed(&fixed);
    }

    /// Update animation state (smooth transitions)
    pub fn update_animation(&mut self) {
        self.renderer.update_animation();
    }

    /// Render the current state
    pub fn render(&mut self) {
        self.renderer.render(&self.simulation);
    }

    /// Check if simulation is still running
    pub fn is_running(&self) -> bool {
        self.simulation.is_running()
    }

    /// Get the current alpha (temperature) value
    pub fn alpha(&self) -> f32 {
        self.simulation.config.alpha
    }

    /// Orbit the camera horizontally (drag left/right in 3D mode)
    pub fn orbit_horizontal(&mut self, delta: f32) {
        self.renderer.orbit_horizontal(delta);
    }

    /// Orbit the camera vertically (drag up/down in 3D mode)
    pub fn orbit_vertical(&mut self, delta: f32) {
        self.renderer.orbit_vertical(delta);
    }

    /// Pan the camera (shift+drag in 3D mode)
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.renderer.pan(dx, dy);
    }

    /// Zoom the view by factor
    pub fn zoom(&mut self, factor: f32) {
        self.renderer.zoom(factor);
    }

    /// Reset view to default
    pub fn reset_view(&mut self) {
        self.renderer.reset_view();
    }

    /// Get number of nodes
    pub fn node_count(&self) -> usize {
        self.simulation.nodes.len()
    }

    /// Get number of edges
    pub fn edge_count(&self) -> usize {
        self.simulation.edges.len()
    }

    /// Resize the canvas
    pub fn resize(&mut self, width: u32, height: u32) {
        self.renderer.resize(width, height);
    }

    /// Run simulation to convergence (blocking)
    pub fn run_to_convergence(&mut self, max_iterations: usize) {
        self.simulation.run_to_convergence(max_iterations);
    }

    /// Fit the graph to fill the view with padding
    pub fn fit_to_bounds(&mut self, padding: f32) {
        self.renderer.fit_to_bounds(&self.simulation.nodes, padding);
    }

    /// Check if this is a 3D visualization
    pub fn is_3d(&self) -> bool {
        true
    }

    // ========================================================================
    // Label support for 3D mode (HTML overlay labels)
    // ========================================================================

    /// Get projected node positions for HTML label overlay
    /// Returns JSON: [{ "id": "...", "label": "...", "x": f32, "y": f32, "visible": bool }, ...]
    pub fn get_projected_nodes(&self) -> String {
        let projected: Vec<serde_json::Value> = self
            .simulation
            .nodes
            .iter()
            .map(|node| {
                let (x, y, visible) = self.renderer.project_to_screen([node.x, node.y, node.z]);
                serde_json::json!({
                    "id": node.id,
                    "label": node.label,
                    "x": x,
                    "y": y,
                    "visible": visible
                })
            })
            .collect();

        serde_json::to_string(&projected).unwrap_or_else(|_| "[]".to_string())
    }

    /// Get projected edge midpoints for HTML label overlay
    /// Returns JSON: [{ "label": "...", "x": f32, "y": f32, "visible": bool }, ...]
    pub fn get_projected_edges(&self) -> String {
        let projected: Vec<serde_json::Value> = self
            .simulation
            .edges
            .iter()
            .map(|edge| {
                let source = &self.simulation.nodes[edge.source];
                let target = &self.simulation.nodes[edge.target];

                // Calculate midpoint in 3D space
                let mid_x = (source.x + target.x) / 2.0;
                let mid_y = (source.y + target.y) / 2.0;
                let mid_z = (source.z + target.z) / 2.0;

                let (x, y, visible) = self.renderer.project_to_screen([mid_x, mid_y, mid_z]);

                serde_json::json!({
                    "label": edge.label,
                    "x": x,
                    "y": y,
                    "visible": visible
                })
            })
            .collect();

        serde_json::to_string(&projected).unwrap_or_else(|_| "[]".to_string())
    }

    // Label visibility state (mirroring 2D API for consistency)
    // These control what JavaScript should display

    /// Toggle all labels on/off
    pub fn toggle_labels(&mut self) {
        self.labels.toggle_all();
    }

    /// Toggle node labels on/off
    pub fn toggle_node_labels(&mut self) {
        self.labels.toggle_node_labels();
    }

    /// Toggle edge labels on/off
    pub fn toggle_edge_labels(&mut self) {
        self.labels.toggle_edge_labels();
    }

    /// Set all labels visibility
    pub fn set_labels(&mut self, visible: bool) {
        self.labels.set_all(visible);
    }

    /// Set node labels visibility
    pub fn set_node_labels(&mut self, visible: bool) {
        self.labels.set_node_labels(visible);
    }

    /// Set edge labels visibility
    pub fn set_edge_labels(&mut self, visible: bool) {
        self.labels.set_edge_labels(visible);
    }

    /// Check if node labels are visible
    pub fn show_node_labels(&self) -> bool {
        self.labels.show_node_labels()
    }

    /// Check if edge labels are visible
    pub fn show_edge_labels(&self) -> bool {
        self.labels.show_edge_labels()
    }

    /// Check if all labels are enabled (master toggle)
    pub fn labels_enabled(&self) -> bool {
        self.labels.all_labels
    }

    /// Check if node labels toggle is on
    pub fn node_labels_enabled(&self) -> bool {
        self.labels.node_labels
    }

    /// Check if edge labels toggle is on
    pub fn edge_labels_enabled(&self) -> bool {
        self.labels.edge_labels
    }

    // ========================================================================
    // Hover detection (3D)
    // ========================================================================

    /// Update hover state based on screen coordinates
    /// Returns true if hover state changed
    pub fn update_hover(&mut self, screen_x: f32, screen_y: f32, width: f32, height: f32) -> bool {
        let old_hovered = self.hovered_node;

        // Cast ray from screen coordinates
        let ray = self
            .renderer
            .screen_to_ray(screen_x, screen_y, width, height);

        // Find closest intersected node
        self.hovered_node = self.pick_node_3d(&ray);

        old_hovered != self.hovered_node
    }

    /// Clear hover state
    pub fn clear_hover(&mut self) {
        self.hovered_node = None;
    }

    /// Get the currently hovered node index (-1 if none)
    pub fn hovered_node_index(&self) -> i32 {
        self.hovered_node.map(|i| i as i32).unwrap_or(-1)
    }

    // ========================================================================
    // Selection and interaction (3D)
    // ========================================================================

    /// Select node at screen coordinates, or deselect if clicking empty space.
    /// Returns the selected node index (-1 if none).
    pub fn select_at(&mut self, screen_x: f32, screen_y: f32, width: f32, height: f32) -> i32 {
        let ray = self
            .renderer
            .screen_to_ray(screen_x, screen_y, width, height);
        let node_index = self.pick_node_3d(&ray);
        self.interaction.select_node(node_index);
        self.selected_node_index()
    }

    /// Get the currently selected node index (-1 if none)
    pub fn selected_node_index(&self) -> i32 {
        self.interaction
            .selected_node
            .map(|i| i as i32)
            .unwrap_or(-1)
    }

    /// Deselect the current node
    pub fn deselect(&mut self) {
        self.interaction.deselect();
    }

    /// Check if a node is currently fixed
    pub fn is_node_fixed(&self, index: usize) -> bool {
        self.interaction.is_fixed(index)
    }

    // ========================================================================
    // Drag operations (3D)
    // ========================================================================

    /// Start dragging a node at screen coordinates.
    /// Returns the node index if a node was found (-1 if none).
    pub fn start_drag_at(&mut self, screen_x: f32, screen_y: f32, width: f32, height: f32) -> i32 {
        let ray = self
            .renderer
            .screen_to_ray(screen_x, screen_y, width, height);
        if let Some(index) = self.pick_node_3d(&ray) {
            self.interaction.start_drag(index, screen_x, screen_y);
            // Reheat simulation so physics runs while dragging
            self.simulation.reheat(0.3);
            index as i32
        } else {
            -1
        }
    }

    /// Move the currently dragged node based on screen coordinates.
    /// Projects the movement onto a plane perpendicular to the camera.
    pub fn drag_to(&mut self, screen_x: f32, screen_y: f32, width: f32, height: f32) {
        if let Some(index) = self.interaction.dragging_node() {
            // Get the current node position
            let node = &self.simulation.nodes[index];
            let node_pos = [node.x, node.y, node.z];

            // Project new screen position to the plane containing the node
            let new_pos = self
                .renderer
                .unproject_to_plane(screen_x, screen_y, width, height, node_pos);

            self.simulation
                .set_node_position(index, new_pos[0], new_pos[1], new_pos[2]);
        }
    }

    /// End the drag operation.
    /// If `keep_fixed` is true, the node will remain fixed after release.
    pub fn end_drag(&mut self, keep_fixed: bool) {
        self.interaction.end_drag(keep_fixed);
    }

    /// Check if we're currently dragging a node
    pub fn is_dragging(&self) -> bool {
        self.interaction.dragging_node().is_some()
    }

    /// Get the index of the node being dragged (-1 if none)
    pub fn dragging_node_index(&self) -> i32 {
        self.interaction
            .dragging_node()
            .map(|i| i as i32)
            .unwrap_or(-1)
    }

    /// Toggle the fixed state of a node at screen coordinates.
    /// Returns the node index if found (-1 if no node at position).
    pub fn toggle_fixed_at(
        &mut self,
        screen_x: f32,
        screen_y: f32,
        width: f32,
        height: f32,
    ) -> i32 {
        let ray = self
            .renderer
            .screen_to_ray(screen_x, screen_y, width, height);
        if let Some(index) = self.pick_node_3d(&ray) {
            self.interaction.toggle_fixed(index);
            index as i32
        } else {
            -1
        }
    }

    /// Toggle the fixed state of a node by index (used by shift+click).
    pub fn toggle_fixed(&mut self, index: usize) {
        if index < self.simulation.nodes.len() {
            self.interaction.toggle_fixed(index);
        }
    }

    /// Unfix a node by index (let it move freely in simulation)
    pub fn unfix_node(&mut self, index: usize) {
        self.interaction.unfix_node(index);
    }

    // ========================================================================
    // Node details
    // ========================================================================

    /// Get details for a node as JSON.
    /// Returns empty object if index is out of bounds.
    pub fn get_node_details(&self, index: usize) -> String {
        if index >= self.simulation.nodes.len() {
            return "{}".to_string();
        }

        let node = &self.simulation.nodes[index];
        let node_type = node_type_string_3d(&node.color);
        let is_fixed = self.interaction.is_fixed(index);

        // Get connected nodes
        let connected = self.get_connected_node_ids(index);

        serde_json::json!({
            "id": node.id,
            "label": node.label,
            "type": node_type,
            "isFixed": is_fixed,
            "connections": connected,
            "x": node.x,
            "y": node.y,
            "z": node.z
        })
        .to_string()
    }

    // ========================================================================
    // Focus mode
    // ========================================================================

    /// Set focus on a node (dims everything outside the node's
    /// neighborhood). The neighborhood is expanded `max_hops` levels
    /// from the focal node — the typical schema-author setting is 2,
    /// which reveals the local cluster without dragging in the whole
    /// graph.
    pub fn focus_node(&mut self, index: usize, max_hops: usize) {
        if index < self.simulation.nodes.len() {
            // Flatten the simulation's edge list into the
            // (source_idx, target_idx) shape `InteractionState::focus_node`
            // walks. Cheap (O(E), small constants) and avoids
            // leaking the SimEdge type across the interaction layer.
            let edges: Vec<(usize, usize)> = self
                .simulation
                .edges
                .iter()
                .map(|e| (e.source, e.target))
                .collect();
            self.interaction.focus_node(index, &edges, max_hops);
        }
    }

    /// Clear focus mode.
    pub fn clear_focus(&mut self) {
        self.interaction.clear_focus();
    }

    /// Get the focused node index (-1 if none).
    pub fn focused_node_index(&self) -> i32 {
        self.interaction
            .focused_node()
            .map(|i| i as i32)
            .unwrap_or(-1)
    }

    // ========================================================================
    // Internal helpers
    // ========================================================================

    /// Pick the closest node intersected by a ray
    fn pick_node_3d(&self, ray: &camera3d::Ray3D) -> Option<usize> {
        let mut closest: Option<(usize, f32)> = None;

        for (i, node) in self.simulation.nodes.iter().enumerate() {
            let center = [node.x, node.y, node.z];
            if let Some(t) = ray.intersect_sphere(center, node.radius) {
                match closest {
                    None => closest = Some((i, t)),
                    Some((_, closest_t)) if t < closest_t => closest = Some((i, t)),
                    _ => {}
                }
            }
        }

        closest.map(|(i, _)| i)
    }

    /// Get IDs of nodes directly connected to the given node
    fn get_connected_node_ids(&self, index: usize) -> Vec<String> {
        let mut connected = Vec::new();
        for edge in &self.simulation.edges {
            if edge.source == index {
                connected.push(self.simulation.nodes[edge.target].id.clone());
            } else if edge.target == index {
                connected.push(self.simulation.nodes[edge.source].id.clone());
            }
        }
        connected.sort();
        connected.dedup();
        connected
    }
}

/// Helper to determine node type from color (3D version)
#[cfg(all(feature = "webgpu", target_arch = "wasm32"))]
fn node_type_string_3d(color: &[f32; 4]) -> &'static str {
    // Match colors defined in graph_types::colors
    const BLUE_R: f32 = 0.290;
    const GREEN_R: f32 = 0.314;
    const PURPLE_R: f32 = 0.608;
    const ORANGE_R: f32 = 0.902;

    let r = color[0];
    if (r - BLUE_R).abs() < 0.1 {
        "Class"
    } else if (r - GREEN_R).abs() < 0.1 {
        "Slot"
    } else if (r - PURPLE_R).abs() < 0.1 {
        "Enum"
    } else if (r - ORANGE_R).abs() < 0.1 {
        "Type"
    } else {
        "Unknown"
    }
}

/// Create a 3D WebGPU visualization (async, only with webgpu feature)
#[cfg(all(feature = "webgpu", target_arch = "wasm32"))]
#[wasm_bindgen]
pub async fn create_visualization_3d(
    canvas: HtmlCanvasElement,
    graph_json: &str,
) -> Result<Visualization3D, JsValue> {
    // Parse graph data
    let graph: GraphData = serde_json::from_str(graph_json)
        .map_err(|e| JsValue::from_str(&format!("Failed to parse graph JSON: {}", e)))?;

    // Create 3D simulation
    let simulation = Simulation3D::from_graph_data(&graph);

    // Create WebGPU renderer (async)
    let renderer = WebGpuRenderer::new(canvas)
        .await
        .map_err(|e| JsValue::from_str(&format!("Failed to create WebGPU renderer: {}", e)))?;

    Ok(Visualization3D {
        simulation,
        renderer,
        labels: LabelOptions::new(),
        interaction: InteractionState::new(),
        hovered_node: None,
    })
}

/// Try to create a 3D visualization, falling back to 2D if WebGPU is unavailable
/// Returns a JsValue that can be either Visualization or Visualization3D.
/// `aspect_w` / `aspect_h` configure the 2D fallback's aspect bias;
/// `layout` selects the 2D fallback's algorithm. The 3D path currently
/// ignores both — ellipsoid biasing and layout-picker support on
/// the WebGPU renderer are still to be built.
#[cfg(all(feature = "webgpu", target_arch = "wasm32"))]
#[wasm_bindgen]
pub async fn create_visualization_auto(
    canvas: HtmlCanvasElement,
    graph_json: &str,
    aspect_w: u32,
    aspect_h: u32,
    layout: &str,
) -> Result<JsValue, JsValue> {
    if !check_webgpu_support().await {
        web_sys::console::info_1(
            &"panschema-viz: navigator.gpu unavailable; rendering 2D Canvas.".into(),
        );
        let viz = Visualization::new(canvas, graph_json, aspect_w, aspect_h, layout)?;
        return Ok(JsValue::from(viz));
    }

    match create_visualization_3d(canvas.clone(), graph_json).await {
        Ok(viz) => Ok(JsValue::from(viz)),
        Err(err) => {
            let cause = err.as_string().unwrap_or_else(|| format!("{:?}", err));
            web_sys::console::warn_1(
                &format!(
                    "panschema-viz: navigator.gpu present but 3D init failed; \
                     rendering 2D Canvas. Cause: {cause}"
                )
                .into(),
            );
            let viz = Visualization::new(canvas, graph_json, aspect_w, aspect_h, layout)?;
            Ok(JsValue::from(viz))
        }
    }
}
