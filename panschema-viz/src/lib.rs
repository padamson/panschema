//! WebGPU visualization for panschema schema graphs
//!
//! This crate provides WASM bindings for embedding interactive schema
//! visualizations in HTML documentation.

use wasm_bindgen::prelude::*;

/// Initialize WASM panic hook for better error messages
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

/// Check if WebGPU is supported in the current browser
#[wasm_bindgen]
pub async fn check_webgpu_support() -> bool {
    // TODO: Implement WebGPU detection
    false
}

/// Create a visualization on the given canvas
#[wasm_bindgen]
pub async fn create_visualization(
    _canvas: web_sys::HtmlCanvasElement,
    _graph_json: &str,
    _use_webgpu: bool,
) -> Result<JsValue, JsValue> {
    // TODO: Implement visualization
    Ok(JsValue::NULL)
}
