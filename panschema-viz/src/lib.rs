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
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use canvas2d::Canvas2DRenderer;
use simulation::CpuSimulation;

/// Initialize WASM panic hook for better error messages
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
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
}

#[wasm_bindgen]
impl Visualization {
    /// Create a new 2D visualization from graph JSON data
    #[wasm_bindgen(constructor)]
    pub fn new(canvas: HtmlCanvasElement, graph_json: &str) -> Result<Visualization, JsValue> {
        // Parse graph data
        let graph: GraphData = serde_json::from_str(graph_json)
            .map_err(|e| JsValue::from_str(&format!("Failed to parse graph JSON: {}", e)))?;

        // Create simulation
        let simulation = CpuSimulation::from_graph_data(&graph);

        // Create renderer
        let renderer = Canvas2DRenderer::new(canvas)
            .map_err(|e| JsValue::from_str(&format!("Failed to create renderer: {}", e)))?;

        Ok(Visualization {
            simulation,
            renderer,
        })
    }

    /// Run one simulation tick
    pub fn tick(&mut self) {
        self.simulation.tick();
    }

    /// Update animation state (smooth transitions)
    pub fn update_animation(&mut self) {
        self.renderer.update_animation();
    }

    /// Render the current state
    pub fn render(&self) {
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
}

/// Create a 2D visualization (convenience function)
#[wasm_bindgen]
pub fn create_visualization(
    canvas: HtmlCanvasElement,
    graph_json: &str,
) -> Result<Visualization, JsValue> {
    Visualization::new(canvas, graph_json)
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
}

#[cfg(all(feature = "webgpu", target_arch = "wasm32"))]
#[wasm_bindgen]
impl Visualization3D {
    /// Run one simulation tick
    pub fn tick(&mut self) {
        self.simulation.tick();
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
    })
}

/// Try to create a 3D visualization, falling back to 2D if WebGPU is unavailable
/// Returns a JsValue that can be either Visualization or Visualization3D
#[cfg(all(feature = "webgpu", target_arch = "wasm32"))]
#[wasm_bindgen]
pub async fn create_visualization_auto(
    canvas: HtmlCanvasElement,
    graph_json: &str,
) -> Result<JsValue, JsValue> {
    // Try WebGPU first
    if check_webgpu_support().await {
        match create_visualization_3d(canvas.clone(), graph_json).await {
            Ok(viz) => return Ok(JsValue::from(viz)),
            Err(_) => {
                // Fall through to 2D
            }
        }
    }

    // Fall back to 2D
    let viz = Visualization::new(canvas, graph_json)?;
    Ok(JsValue::from(viz))
}
