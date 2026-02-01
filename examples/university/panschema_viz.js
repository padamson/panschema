/* @ts-self-types="./panschema_viz.d.ts" */

/**
 * 2D Visualization state (CPU simulation + Canvas2D rendering)
 */
export class Visualization {
    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(Visualization.prototype);
        obj.__wbg_ptr = ptr;
        VisualizationFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        VisualizationFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_visualization_free(ptr, 0);
    }
    /**
     * Get the current alpha (temperature) value
     * @returns {number}
     */
    alpha() {
        const ret = wasm.visualization_alpha(this.__wbg_ptr);
        return ret;
    }
    /**
     * Clear hover state
     */
    clear_hover() {
        wasm.visualization_clear_hover(this.__wbg_ptr);
    }
    /**
     * Get number of edges
     * @returns {number}
     */
    edge_count() {
        const ret = wasm.visualization_edge_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Check if edge labels toggle is on
     * @returns {boolean}
     */
    edge_labels_enabled() {
        const ret = wasm.visualization_edge_labels_enabled(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Fit the graph to fill the canvas with padding
     * @param {number} padding
     */
    fit_to_bounds(padding) {
        wasm.visualization_fit_to_bounds(this.__wbg_ptr, padding);
    }
    /**
     * Get the currently hovered edge index (-1 if none)
     * @returns {number}
     */
    hovered_edge_index() {
        const ret = wasm.visualization_hovered_edge_index(this.__wbg_ptr);
        return ret;
    }
    /**
     * Get the currently hovered node index (-1 if none)
     * @returns {number}
     */
    hovered_node_index() {
        const ret = wasm.visualization_hovered_node_index(this.__wbg_ptr);
        return ret;
    }
    /**
     * Check if this is a 3D visualization
     * @returns {boolean}
     */
    is_3d() {
        const ret = wasm.visualization_is_3d(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Check if simulation is still running
     * @returns {boolean}
     */
    is_running() {
        const ret = wasm.visualization_is_running(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Check if all labels are enabled (master toggle)
     * @returns {boolean}
     */
    labels_enabled() {
        const ret = wasm.visualization_labels_enabled(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Create a new 2D visualization from graph JSON data
     * @param {HTMLCanvasElement} canvas
     * @param {string} graph_json
     */
    constructor(canvas, graph_json) {
        const ptr0 = passStringToWasm0(graph_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.visualization_new(canvas, ptr0, len0);
        if (ret[2]) {
            throw takeFromExternrefTable0(ret[1]);
        }
        this.__wbg_ptr = ret[0] >>> 0;
        VisualizationFinalization.register(this, this.__wbg_ptr, this);
        return this;
    }
    /**
     * Get number of nodes
     * @returns {number}
     */
    node_count() {
        const ret = wasm.visualization_node_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Check if node labels toggle is on
     * @returns {boolean}
     */
    node_labels_enabled() {
        const ret = wasm.visualization_node_labels_enabled(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Pan the view by delta pixels
     * @param {number} dx
     * @param {number} dy
     */
    pan(dx, dy) {
        wasm.visualization_pan(this.__wbg_ptr, dx, dy);
    }
    /**
     * Render the current state
     */
    render() {
        wasm.visualization_render(this.__wbg_ptr);
    }
    /**
     * Reset view to default
     */
    reset_view() {
        wasm.visualization_reset_view(this.__wbg_ptr);
    }
    /**
     * Resize the canvas
     * @param {number} width
     * @param {number} height
     */
    resize(width, height) {
        wasm.visualization_resize(this.__wbg_ptr, width, height);
    }
    /**
     * Run simulation to convergence (blocking)
     * @param {number} max_iterations
     */
    run_to_convergence(max_iterations) {
        wasm.visualization_run_to_convergence(this.__wbg_ptr, max_iterations);
    }
    /**
     * Set edge labels visibility
     * @param {boolean} visible
     */
    set_edge_labels(visible) {
        wasm.visualization_set_edge_labels(this.__wbg_ptr, visible);
    }
    /**
     * Set all labels visibility
     * @param {boolean} visible
     */
    set_labels(visible) {
        wasm.visualization_set_labels(this.__wbg_ptr, visible);
    }
    /**
     * Set node labels visibility
     * @param {boolean} visible
     */
    set_node_labels(visible) {
        wasm.visualization_set_node_labels(this.__wbg_ptr, visible);
    }
    /**
     * Check if edge labels are visible
     * @returns {boolean}
     */
    show_edge_labels() {
        const ret = wasm.visualization_show_edge_labels(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Check if node labels are visible
     * @returns {boolean}
     */
    show_node_labels() {
        const ret = wasm.visualization_show_node_labels(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Run one simulation tick
     */
    tick() {
        wasm.visualization_tick(this.__wbg_ptr);
    }
    /**
     * Toggle edge labels on/off
     */
    toggle_edge_labels() {
        wasm.visualization_toggle_edge_labels(this.__wbg_ptr);
    }
    /**
     * Toggle all labels on/off
     */
    toggle_labels() {
        wasm.visualization_toggle_labels(this.__wbg_ptr);
    }
    /**
     * Toggle node labels on/off
     */
    toggle_node_labels() {
        wasm.visualization_toggle_node_labels(this.__wbg_ptr);
    }
    /**
     * Update animation state (smooth transitions)
     */
    update_animation() {
        wasm.visualization_update_animation(this.__wbg_ptr);
    }
    /**
     * Update hover state based on canvas coordinates
     * Returns true if hover state changed
     * @param {number} canvas_x
     * @param {number} canvas_y
     * @returns {boolean}
     */
    update_hover(canvas_x, canvas_y) {
        const ret = wasm.visualization_update_hover(this.__wbg_ptr, canvas_x, canvas_y);
        return ret !== 0;
    }
    /**
     * Zoom the view by factor (1.1 = zoom in 10%, 0.9 = zoom out 10%)
     * @param {number} factor
     */
    zoom(factor) {
        wasm.visualization_zoom(this.__wbg_ptr, factor);
    }
}
if (Symbol.dispose) Visualization.prototype[Symbol.dispose] = Visualization.prototype.free;

/**
 * 3D Visualization state (3D simulation + WebGPU rendering)
 */
export class Visualization3D {
    static __wrap(ptr) {
        ptr = ptr >>> 0;
        const obj = Object.create(Visualization3D.prototype);
        obj.__wbg_ptr = ptr;
        Visualization3DFinalization.register(obj, obj.__wbg_ptr, obj);
        return obj;
    }
    __destroy_into_raw() {
        const ptr = this.__wbg_ptr;
        this.__wbg_ptr = 0;
        Visualization3DFinalization.unregister(this);
        return ptr;
    }
    free() {
        const ptr = this.__destroy_into_raw();
        wasm.__wbg_visualization3d_free(ptr, 0);
    }
    /**
     * Get the current alpha (temperature) value
     * @returns {number}
     */
    alpha() {
        const ret = wasm.visualization3d_alpha(this.__wbg_ptr);
        return ret;
    }
    /**
     * Get number of edges
     * @returns {number}
     */
    edge_count() {
        const ret = wasm.visualization3d_edge_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Check if edge labels toggle is on
     * @returns {boolean}
     */
    edge_labels_enabled() {
        const ret = wasm.visualization3d_edge_labels_enabled(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Fit the graph to fill the view with padding
     * @param {number} padding
     */
    fit_to_bounds(padding) {
        wasm.visualization3d_fit_to_bounds(this.__wbg_ptr, padding);
    }
    /**
     * Get projected edge midpoints for HTML label overlay
     * Returns JSON: [{ "label": "...", "x": f32, "y": f32, "visible": bool }, ...]
     * @returns {string}
     */
    get_projected_edges() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.visualization3d_get_projected_edges(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Get projected node positions for HTML label overlay
     * Returns JSON: [{ "id": "...", "label": "...", "x": f32, "y": f32, "visible": bool }, ...]
     * @returns {string}
     */
    get_projected_nodes() {
        let deferred1_0;
        let deferred1_1;
        try {
            const ret = wasm.visualization3d_get_projected_nodes(this.__wbg_ptr);
            deferred1_0 = ret[0];
            deferred1_1 = ret[1];
            return getStringFromWasm0(ret[0], ret[1]);
        } finally {
            wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
        }
    }
    /**
     * Check if this is a 3D visualization
     * @returns {boolean}
     */
    is_3d() {
        const ret = wasm.visualization3d_is_3d(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Check if simulation is still running
     * @returns {boolean}
     */
    is_running() {
        const ret = wasm.visualization3d_is_running(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Check if all labels are enabled (master toggle)
     * @returns {boolean}
     */
    labels_enabled() {
        const ret = wasm.visualization3d_labels_enabled(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Get number of nodes
     * @returns {number}
     */
    node_count() {
        const ret = wasm.visualization3d_node_count(this.__wbg_ptr);
        return ret >>> 0;
    }
    /**
     * Check if node labels toggle is on
     * @returns {boolean}
     */
    node_labels_enabled() {
        const ret = wasm.visualization3d_node_labels_enabled(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Orbit the camera horizontally (drag left/right in 3D mode)
     * @param {number} delta
     */
    orbit_horizontal(delta) {
        wasm.visualization3d_orbit_horizontal(this.__wbg_ptr, delta);
    }
    /**
     * Orbit the camera vertically (drag up/down in 3D mode)
     * @param {number} delta
     */
    orbit_vertical(delta) {
        wasm.visualization3d_orbit_vertical(this.__wbg_ptr, delta);
    }
    /**
     * Pan the camera (shift+drag in 3D mode)
     * @param {number} dx
     * @param {number} dy
     */
    pan(dx, dy) {
        wasm.visualization3d_pan(this.__wbg_ptr, dx, dy);
    }
    /**
     * Render the current state
     */
    render() {
        wasm.visualization3d_render(this.__wbg_ptr);
    }
    /**
     * Reset view to default
     */
    reset_view() {
        wasm.visualization3d_reset_view(this.__wbg_ptr);
    }
    /**
     * Resize the canvas
     * @param {number} width
     * @param {number} height
     */
    resize(width, height) {
        wasm.visualization3d_resize(this.__wbg_ptr, width, height);
    }
    /**
     * Run simulation to convergence (blocking)
     * @param {number} max_iterations
     */
    run_to_convergence(max_iterations) {
        wasm.visualization3d_run_to_convergence(this.__wbg_ptr, max_iterations);
    }
    /**
     * Set edge labels visibility
     * @param {boolean} visible
     */
    set_edge_labels(visible) {
        wasm.visualization3d_set_edge_labels(this.__wbg_ptr, visible);
    }
    /**
     * Set all labels visibility
     * @param {boolean} visible
     */
    set_labels(visible) {
        wasm.visualization3d_set_labels(this.__wbg_ptr, visible);
    }
    /**
     * Set node labels visibility
     * @param {boolean} visible
     */
    set_node_labels(visible) {
        wasm.visualization3d_set_node_labels(this.__wbg_ptr, visible);
    }
    /**
     * Check if edge labels are visible
     * @returns {boolean}
     */
    show_edge_labels() {
        const ret = wasm.visualization3d_show_edge_labels(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Check if node labels are visible
     * @returns {boolean}
     */
    show_node_labels() {
        const ret = wasm.visualization3d_show_node_labels(this.__wbg_ptr);
        return ret !== 0;
    }
    /**
     * Run one simulation tick
     */
    tick() {
        wasm.visualization3d_tick(this.__wbg_ptr);
    }
    /**
     * Toggle edge labels on/off
     */
    toggle_edge_labels() {
        wasm.visualization3d_toggle_edge_labels(this.__wbg_ptr);
    }
    /**
     * Toggle all labels on/off
     */
    toggle_labels() {
        wasm.visualization3d_toggle_labels(this.__wbg_ptr);
    }
    /**
     * Toggle node labels on/off
     */
    toggle_node_labels() {
        wasm.visualization3d_toggle_node_labels(this.__wbg_ptr);
    }
    /**
     * Update animation state (smooth transitions)
     */
    update_animation() {
        wasm.visualization3d_update_animation(this.__wbg_ptr);
    }
    /**
     * Zoom the view by factor
     * @param {number} factor
     */
    zoom(factor) {
        wasm.visualization3d_zoom(this.__wbg_ptr, factor);
    }
}
if (Symbol.dispose) Visualization3D.prototype[Symbol.dispose] = Visualization3D.prototype.free;

/**
 * Check if WebGPU is supported in the current browser
 * @returns {Promise<boolean>}
 */
export function check_webgpu_support() {
    const ret = wasm.check_webgpu_support();
    return ret;
}

/**
 * Create a 2D visualization (convenience function)
 * @param {HTMLCanvasElement} canvas
 * @param {string} graph_json
 * @returns {Visualization}
 */
export function create_visualization(canvas, graph_json) {
    const ptr0 = passStringToWasm0(graph_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.create_visualization(canvas, ptr0, len0);
    if (ret[2]) {
        throw takeFromExternrefTable0(ret[1]);
    }
    return Visualization.__wrap(ret[0]);
}

/**
 * Create a 3D WebGPU visualization (async, only with webgpu feature)
 * @param {HTMLCanvasElement} canvas
 * @param {string} graph_json
 * @returns {Promise<Visualization3D>}
 */
export function create_visualization_3d(canvas, graph_json) {
    const ptr0 = passStringToWasm0(graph_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.create_visualization_3d(canvas, ptr0, len0);
    return ret;
}

/**
 * Try to create a 3D visualization, falling back to 2D if WebGPU is unavailable
 * Returns a JsValue that can be either Visualization or Visualization3D
 * @param {HTMLCanvasElement} canvas
 * @param {string} graph_json
 * @returns {Promise<any>}
 */
export function create_visualization_auto(canvas, graph_json) {
    const ptr0 = passStringToWasm0(graph_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.create_visualization_auto(canvas, ptr0, len0);
    return ret;
}

/**
 * Initialize WASM panic hook for better error messages
 */
export function init() {
    wasm.init();
}

function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbg_Window_2b9b35492d4b2d63: function(arg0) {
            const ret = arg0.Window;
            return ret;
        },
        __wbg_WorkerGlobalScope_b4fb13f0ba6527ab: function(arg0) {
            const ret = arg0.WorkerGlobalScope;
            return ret;
        },
        __wbg___wbindgen_debug_string_0bc8482c6e3508ae: function(arg0, arg1) {
            const ret = debugString(arg1);
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg___wbindgen_is_function_0095a73b8b156f76: function(arg0) {
            const ret = typeof(arg0) === 'function';
            return ret;
        },
        __wbg___wbindgen_is_null_ac34f5003991759a: function(arg0) {
            const ret = arg0 === null;
            return ret;
        },
        __wbg___wbindgen_is_undefined_9e4d92534c42d778: function(arg0) {
            const ret = arg0 === undefined;
            return ret;
        },
        __wbg___wbindgen_throw_be289d5034ed271b: function(arg0, arg1) {
            throw new Error(getStringFromWasm0(arg0, arg1));
        },
        __wbg__wbg_cb_unref_d9b87ff7982e3b21: function(arg0) {
            arg0._wbg_cb_unref();
        },
        __wbg_arc_60bf829e1bd2add5: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5) {
            arg0.arc(arg1, arg2, arg3, arg4, arg5);
        }, arguments); },
        __wbg_beginPath_9873f939d695759c: function(arg0) {
            arg0.beginPath();
        },
        __wbg_beginRenderPass_f36cfdd5825e0c2e: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.beginRenderPass(arg1);
            return ret;
        }, arguments); },
        __wbg_buffer_26d0910f3a5bc899: function(arg0) {
            const ret = arg0.buffer;
            return ret;
        },
        __wbg_call_389efe28435a9388: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.call(arg1);
            return ret;
        }, arguments); },
        __wbg_call_4708e0c13bdc8e95: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.call(arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_configure_ad5aa321838c8e3b: function() { return handleError(function (arg0, arg1) {
            arg0.configure(arg1);
        }, arguments); },
        __wbg_createBindGroupLayout_b87a1f26ed22bd5d: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.createBindGroupLayout(arg1);
            return ret;
        }, arguments); },
        __wbg_createBindGroup_dfdadbbcf4dcae54: function(arg0, arg1) {
            const ret = arg0.createBindGroup(arg1);
            return ret;
        },
        __wbg_createBuffer_fb1752eab5cb2a7f: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.createBuffer(arg1);
            return ret;
        }, arguments); },
        __wbg_createCommandEncoder_92b1c283a0372974: function(arg0, arg1) {
            const ret = arg0.createCommandEncoder(arg1);
            return ret;
        },
        __wbg_createPipelineLayout_c97169a1a177450e: function(arg0, arg1) {
            const ret = arg0.createPipelineLayout(arg1);
            return ret;
        },
        __wbg_createRenderPipeline_ab453ccc40539bc0: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.createRenderPipeline(arg1);
            return ret;
        }, arguments); },
        __wbg_createShaderModule_159013272c1b4c4c: function(arg0, arg1) {
            const ret = arg0.createShaderModule(arg1);
            return ret;
        },
        __wbg_createView_e743725c577bafe5: function() { return handleError(function (arg0, arg1) {
            const ret = arg0.createView(arg1);
            return ret;
        }, arguments); },
        __wbg_document_ee35a3d3ae34ef6c: function(arg0) {
            const ret = arg0.document;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_draw_e8c430e7254c6215: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.draw(arg1 >>> 0, arg2 >>> 0, arg3 >>> 0, arg4 >>> 0);
        },
        __wbg_end_7ad26f2083234d67: function(arg0) {
            arg0.end();
        },
        __wbg_error_7534b8e9a36f1ab4: function(arg0, arg1) {
            let deferred0_0;
            let deferred0_1;
            try {
                deferred0_0 = arg0;
                deferred0_1 = arg1;
                console.error(getStringFromWasm0(arg0, arg1));
            } finally {
                wasm.__wbindgen_free(deferred0_0, deferred0_1, 1);
            }
        },
        __wbg_fillRect_d44afec47e3a3fab: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.fillRect(arg1, arg2, arg3, arg4);
        },
        __wbg_fillText_4a931850b976cc62: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4) {
            arg0.fillText(getStringFromWasm0(arg1, arg2), arg3, arg4);
        }, arguments); },
        __wbg_fill_1eb35c386c8676aa: function(arg0) {
            arg0.fill();
        },
        __wbg_finish_ac8e8f8408208d93: function(arg0) {
            const ret = arg0.finish();
            return ret;
        },
        __wbg_finish_b79779da004ef346: function(arg0, arg1) {
            const ret = arg0.finish(arg1);
            return ret;
        },
        __wbg_getContext_2966500392030d63: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.getContext(getStringFromWasm0(arg1, arg2));
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        }, arguments); },
        __wbg_getContext_2a5764d48600bc43: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.getContext(getStringFromWasm0(arg1, arg2));
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        }, arguments); },
        __wbg_getCurrentTexture_3c8710ca6e0019fc: function() { return handleError(function (arg0) {
            const ret = arg0.getCurrentTexture();
            return ret;
        }, arguments); },
        __wbg_getMappedRange_86d4a434bceeb7fc: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.getMappedRange(arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_getPreferredCanvasFormat_0988752050c788b0: function(arg0) {
            const ret = arg0.getPreferredCanvasFormat();
            return (__wbindgen_enum_GpuTextureFormat.indexOf(ret) + 1 || 96) - 1;
        },
        __wbg_get_b3ed3ad4be2bc8ac: function() { return handleError(function (arg0, arg1) {
            const ret = Reflect.get(arg0, arg1);
            return ret;
        }, arguments); },
        __wbg_get_d8db2ad31d529ff8: function(arg0, arg1) {
            const ret = arg0[arg1 >>> 0];
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_gpu_051bdce6489ddf6a: function(arg0) {
            const ret = arg0.gpu;
            return ret;
        },
        __wbg_height_38750dc6de41ee75: function(arg0) {
            const ret = arg0.height;
            return ret;
        },
        __wbg_instanceof_CanvasRenderingContext2d_4bb052fd1c3d134d: function(arg0) {
            let result;
            try {
                result = arg0 instanceof CanvasRenderingContext2D;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_GpuAdapter_aff4b0f95a6c1c3e: function(arg0) {
            let result;
            try {
                result = arg0 instanceof GPUAdapter;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_GpuCanvasContext_dc8dc7061b962990: function(arg0) {
            let result;
            try {
                result = arg0 instanceof GPUCanvasContext;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_instanceof_Window_ed49b2db8df90359: function(arg0) {
            let result;
            try {
                result = arg0 instanceof Window;
            } catch (_) {
                result = false;
            }
            const ret = result;
            return ret;
        },
        __wbg_label_c3a930571192f18e: function(arg0, arg1) {
            const ret = arg1.label;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_length_32ed9a279acd054c: function(arg0) {
            const ret = arg0.length;
            return ret;
        },
        __wbg_lineTo_c584cff6c760c4a5: function(arg0, arg1, arg2) {
            arg0.lineTo(arg1, arg2);
        },
        __wbg_moveTo_e9190fc700d55b40: function(arg0, arg1, arg2) {
            arg0.moveTo(arg1, arg2);
        },
        __wbg_navigator_43be698ba96fc088: function(arg0) {
            const ret = arg0.navigator;
            return ret;
        },
        __wbg_navigator_4478931f32ebca57: function(arg0) {
            const ret = arg0.navigator;
            return ret;
        },
        __wbg_new_361308b2356cecd0: function() {
            const ret = new Object();
            return ret;
        },
        __wbg_new_3eb36ae241fe6f44: function() {
            const ret = new Array();
            return ret;
        },
        __wbg_new_8a6f238a6ece86ea: function() {
            const ret = new Error();
            return ret;
        },
        __wbg_new_b5d9e2fb389fef91: function(arg0, arg1) {
            try {
                var state0 = {a: arg0, b: arg1};
                var cb0 = (arg0, arg1) => {
                    const a = state0.a;
                    state0.a = 0;
                    try {
                        return wasm_bindgen__convert__closures_____invoke__h55e99d643b02aaa4(a, state0.b, arg0, arg1);
                    } finally {
                        state0.a = a;
                    }
                };
                const ret = new Promise(cb0);
                return ret;
            } finally {
                state0.a = state0.b = 0;
            }
        },
        __wbg_new_from_slice_a3d2629dc1826784: function(arg0, arg1) {
            const ret = new Uint8Array(getArrayU8FromWasm0(arg0, arg1));
            return ret;
        },
        __wbg_new_no_args_1c7c842f08d00ebb: function(arg0, arg1) {
            const ret = new Function(getStringFromWasm0(arg0, arg1));
            return ret;
        },
        __wbg_new_with_byte_offset_and_length_aa261d9c9da49eb1: function(arg0, arg1, arg2) {
            const ret = new Uint8Array(arg0, arg1 >>> 0, arg2 >>> 0);
            return ret;
        },
        __wbg_prototypesetcall_bdcdcc5842e4d77d: function(arg0, arg1, arg2) {
            Uint8Array.prototype.set.call(getArrayU8FromWasm0(arg0, arg1), arg2);
        },
        __wbg_push_8ffdcb2063340ba5: function(arg0, arg1) {
            const ret = arg0.push(arg1);
            return ret;
        },
        __wbg_querySelectorAll_1283aae52043a951: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = arg0.querySelectorAll(getStringFromWasm0(arg1, arg2));
            return ret;
        }, arguments); },
        __wbg_queueMicrotask_0aa0a927f78f5d98: function(arg0) {
            const ret = arg0.queueMicrotask;
            return ret;
        },
        __wbg_queueMicrotask_5bb536982f78a56f: function(arg0) {
            queueMicrotask(arg0);
        },
        __wbg_queue_1f589e8194b004a6: function(arg0) {
            const ret = arg0.queue;
            return ret;
        },
        __wbg_requestAdapter_51be7e8ee7d08b87: function(arg0, arg1) {
            const ret = arg0.requestAdapter(arg1);
            return ret;
        },
        __wbg_requestDevice_338f0085866d40a2: function(arg0, arg1) {
            const ret = arg0.requestDevice(arg1);
            return ret;
        },
        __wbg_resolve_002c4b7d9d8f6b64: function(arg0) {
            const ret = Promise.resolve(arg0);
            return ret;
        },
        __wbg_setBindGroup_306b5f43159153da: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5, arg6) {
            arg0.setBindGroup(arg1 >>> 0, arg2, getArrayU32FromWasm0(arg3, arg4), arg5, arg6 >>> 0);
        }, arguments); },
        __wbg_setBindGroup_d3cd0c65d5718e66: function(arg0, arg1, arg2) {
            arg0.setBindGroup(arg1 >>> 0, arg2);
        },
        __wbg_setPipeline_f44bbc63b7455235: function(arg0, arg1) {
            arg0.setPipeline(arg1);
        },
        __wbg_setVertexBuffer_5e5ec203042c0564: function(arg0, arg1, arg2, arg3, arg4) {
            arg0.setVertexBuffer(arg1 >>> 0, arg2, arg3, arg4);
        },
        __wbg_setVertexBuffer_950908f301fc83b4: function(arg0, arg1, arg2, arg3) {
            arg0.setVertexBuffer(arg1 >>> 0, arg2, arg3);
        },
        __wbg_set_25cf9deff6bf0ea8: function(arg0, arg1, arg2) {
            arg0.set(arg1, arg2 >>> 0);
        },
        __wbg_set_6cb8631f80447a67: function() { return handleError(function (arg0, arg1, arg2) {
            const ret = Reflect.set(arg0, arg1, arg2);
            return ret;
        }, arguments); },
        __wbg_set_a_6ca4b80abcaa9bb0: function(arg0, arg1) {
            arg0.a = arg1;
        },
        __wbg_set_access_c17e0a436ed1d78e: function(arg0, arg1) {
            arg0.access = __wbindgen_enum_GpuStorageTextureAccess[arg1];
        },
        __wbg_set_alpha_eb6e37beb08f6a6a: function(arg0, arg1) {
            arg0.alpha = arg1;
        },
        __wbg_set_alpha_mode_2a9be051489d8bbd: function(arg0, arg1) {
            arg0.alphaMode = __wbindgen_enum_GpuCanvasAlphaMode[arg1];
        },
        __wbg_set_alpha_to_coverage_enabled_1f594c6ef9ae4caa: function(arg0, arg1) {
            arg0.alphaToCoverageEnabled = arg1 !== 0;
        },
        __wbg_set_array_layer_count_93d58eca9387b84c: function(arg0, arg1) {
            arg0.arrayLayerCount = arg1 >>> 0;
        },
        __wbg_set_array_stride_5ace211a6c31af55: function(arg0, arg1) {
            arg0.arrayStride = arg1;
        },
        __wbg_set_aspect_e3aa9cad44e6338f: function(arg0, arg1) {
            arg0.aspect = __wbindgen_enum_GpuTextureAspect[arg1];
        },
        __wbg_set_attributes_8cfe8a349778ff6d: function(arg0, arg1) {
            arg0.attributes = arg1;
        },
        __wbg_set_b_52915cc78721cadb: function(arg0, arg1) {
            arg0.b = arg1;
        },
        __wbg_set_base_array_layer_798dcd012d28aafd: function(arg0, arg1) {
            arg0.baseArrayLayer = arg1 >>> 0;
        },
        __wbg_set_base_mip_level_ff05f0742029fbd7: function(arg0, arg1) {
            arg0.baseMipLevel = arg1 >>> 0;
        },
        __wbg_set_beginning_of_pass_write_index_ad07a73147217513: function(arg0, arg1) {
            arg0.beginningOfPassWriteIndex = arg1 >>> 0;
        },
        __wbg_set_bind_group_layouts_9eff5e187a1db39e: function(arg0, arg1) {
            arg0.bindGroupLayouts = arg1;
        },
        __wbg_set_binding_3ada8a83c514d419: function(arg0, arg1) {
            arg0.binding = arg1 >>> 0;
        },
        __wbg_set_binding_9a389db987313ca9: function(arg0, arg1) {
            arg0.binding = arg1 >>> 0;
        },
        __wbg_set_blend_15fcdb6fca391aa3: function(arg0, arg1) {
            arg0.blend = arg1;
        },
        __wbg_set_buffer_581ee8422928bd0d: function(arg0, arg1) {
            arg0.buffer = arg1;
        },
        __wbg_set_buffer_ac25c198252221bd: function(arg0, arg1) {
            arg0.buffer = arg1;
        },
        __wbg_set_buffers_4515e14c72e1bc45: function(arg0, arg1) {
            arg0.buffers = arg1;
        },
        __wbg_set_clear_value_9fd25161e3ff7358: function(arg0, arg1) {
            arg0.clearValue = arg1;
        },
        __wbg_set_code_1d146372551ab97f: function(arg0, arg1, arg2) {
            arg0.code = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_color_63a788c8828014d8: function(arg0, arg1) {
            arg0.color = arg1;
        },
        __wbg_set_color_attachments_b56ec268556eb0af: function(arg0, arg1) {
            arg0.colorAttachments = arg1;
        },
        __wbg_set_compare_986db63daac4c337: function(arg0, arg1) {
            arg0.compare = __wbindgen_enum_GpuCompareFunction[arg1];
        },
        __wbg_set_count_6b3574238f446a02: function(arg0, arg1) {
            arg0.count = arg1 >>> 0;
        },
        __wbg_set_cull_mode_f1cc439f208cf7d2: function(arg0, arg1) {
            arg0.cullMode = __wbindgen_enum_GpuCullMode[arg1];
        },
        __wbg_set_depth_bias_0c225de07a2372b1: function(arg0, arg1) {
            arg0.depthBias = arg1;
        },
        __wbg_set_depth_bias_clamp_bd34181bc74b8a65: function(arg0, arg1) {
            arg0.depthBiasClamp = arg1;
        },
        __wbg_set_depth_bias_slope_scale_d43ddce65f19c9be: function(arg0, arg1) {
            arg0.depthBiasSlopeScale = arg1;
        },
        __wbg_set_depth_clear_value_eb76fedd34b20053: function(arg0, arg1) {
            arg0.depthClearValue = arg1;
        },
        __wbg_set_depth_compare_491947ed2f6065b9: function(arg0, arg1) {
            arg0.depthCompare = __wbindgen_enum_GpuCompareFunction[arg1];
        },
        __wbg_set_depth_fail_op_4983b01413b9f743: function(arg0, arg1) {
            arg0.depthFailOp = __wbindgen_enum_GpuStencilOperation[arg1];
        },
        __wbg_set_depth_load_op_c7deb718c4129a2c: function(arg0, arg1) {
            arg0.depthLoadOp = __wbindgen_enum_GpuLoadOp[arg1];
        },
        __wbg_set_depth_read_only_18602250b14fa638: function(arg0, arg1) {
            arg0.depthReadOnly = arg1 !== 0;
        },
        __wbg_set_depth_stencil_attachment_90d13c414095197d: function(arg0, arg1) {
            arg0.depthStencilAttachment = arg1;
        },
        __wbg_set_depth_stencil_e6069a8b511d1004: function(arg0, arg1) {
            arg0.depthStencil = arg1;
        },
        __wbg_set_depth_store_op_55f84f2f9039c453: function(arg0, arg1) {
            arg0.depthStoreOp = __wbindgen_enum_GpuStoreOp[arg1];
        },
        __wbg_set_depth_write_enabled_e419ffe553654371: function(arg0, arg1) {
            arg0.depthWriteEnabled = arg1 !== 0;
        },
        __wbg_set_device_91facdf766d51abf: function(arg0, arg1) {
            arg0.device = arg1;
        },
        __wbg_set_dimension_47ad758bb7805028: function(arg0, arg1) {
            arg0.dimension = __wbindgen_enum_GpuTextureViewDimension[arg1];
        },
        __wbg_set_dst_factor_abdf4d85b8f742b5: function(arg0, arg1) {
            arg0.dstFactor = __wbindgen_enum_GpuBlendFactor[arg1];
        },
        __wbg_set_end_of_pass_write_index_82a42f6ec7d55754: function(arg0, arg1) {
            arg0.endOfPassWriteIndex = arg1 >>> 0;
        },
        __wbg_set_entries_136baaaafb25087f: function(arg0, arg1) {
            arg0.entries = arg1;
        },
        __wbg_set_entries_7c41d594195ebe78: function(arg0, arg1) {
            arg0.entries = arg1;
        },
        __wbg_set_entry_point_913e091cc9a07667: function(arg0, arg1, arg2) {
            arg0.entryPoint = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_entry_point_96944272d50efb55: function(arg0, arg1, arg2) {
            arg0.entryPoint = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_fail_op_fd94b46d0cd7c4f2: function(arg0, arg1) {
            arg0.failOp = __wbindgen_enum_GpuStencilOperation[arg1];
        },
        __wbg_set_fillStyle_783d3f7489475421: function(arg0, arg1, arg2) {
            arg0.fillStyle = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_font_575685c8f7e56957: function(arg0, arg1, arg2) {
            arg0.font = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_format_29126ee763612515: function(arg0, arg1) {
            arg0.format = __wbindgen_enum_GpuTextureFormat[arg1];
        },
        __wbg_set_format_450c4be578985cb4: function(arg0, arg1) {
            arg0.format = __wbindgen_enum_GpuVertexFormat[arg1];
        },
        __wbg_set_format_6ac892268eeef402: function(arg0, arg1) {
            arg0.format = __wbindgen_enum_GpuTextureFormat[arg1];
        },
        __wbg_set_format_a622a57e42ae23e4: function(arg0, arg1) {
            arg0.format = __wbindgen_enum_GpuTextureFormat[arg1];
        },
        __wbg_set_format_bdfc7be2aa989382: function(arg0, arg1) {
            arg0.format = __wbindgen_enum_GpuTextureFormat[arg1];
        },
        __wbg_set_format_c3ba1e26468014ae: function(arg0, arg1) {
            arg0.format = __wbindgen_enum_GpuTextureFormat[arg1];
        },
        __wbg_set_fragment_84f03cfa83c432b2: function(arg0, arg1) {
            arg0.fragment = arg1;
        },
        __wbg_set_front_face_1c87b2e21f85a97f: function(arg0, arg1) {
            arg0.frontFace = __wbindgen_enum_GpuFrontFace[arg1];
        },
        __wbg_set_g_b94c63958617b86c: function(arg0, arg1) {
            arg0.g = arg1;
        },
        __wbg_set_has_dynamic_offset_9dc29179158975e4: function(arg0, arg1) {
            arg0.hasDynamicOffset = arg1 !== 0;
        },
        __wbg_set_height_b386c0f603610637: function(arg0, arg1) {
            arg0.height = arg1 >>> 0;
        },
        __wbg_set_height_f21f985387070100: function(arg0, arg1) {
            arg0.height = arg1 >>> 0;
        },
        __wbg_set_label_034d85243342ac5c: function(arg0, arg1, arg2) {
            arg0.label = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_label_1e2e0069cbf2bd78: function(arg0, arg1, arg2) {
            arg0.label = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_label_21544401e31cd317: function(arg0, arg1, arg2) {
            arg0.label = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_label_2312a64e22934a2b: function(arg0, arg1, arg2) {
            arg0.label = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_label_2ed86217d97ea3d5: function(arg0, arg1, arg2) {
            arg0.label = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_label_3f988ca8291e319f: function(arg0, arg1, arg2) {
            arg0.label = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_label_81dd67dee9cd4287: function(arg0, arg1, arg2) {
            arg0.label = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_label_8f9ebe053f8da7a0: function(arg0, arg1, arg2) {
            arg0.label = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_label_bfbd23fc748f8f94: function(arg0, arg1, arg2) {
            arg0.label = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_label_d400966bd7759b26: function(arg0, arg1, arg2) {
            arg0.label = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_label_ecb2c1eab1d46433: function(arg0, arg1, arg2) {
            arg0.label = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_layout_0770a97fe3411616: function(arg0, arg1) {
            arg0.layout = arg1;
        },
        __wbg_set_layout_0e88cce0b3d76c31: function(arg0, arg1) {
            arg0.layout = arg1;
        },
        __wbg_set_lineWidth_89fa506592f5b994: function(arg0, arg1) {
            arg0.lineWidth = arg1;
        },
        __wbg_set_load_op_6725bf0c5b509ae7: function(arg0, arg1) {
            arg0.loadOp = __wbindgen_enum_GpuLoadOp[arg1];
        },
        __wbg_set_mapped_at_creation_e0c884a30f64323b: function(arg0, arg1) {
            arg0.mappedAtCreation = arg1 !== 0;
        },
        __wbg_set_mask_9094d3e3f6f3a7dc: function(arg0, arg1) {
            arg0.mask = arg1 >>> 0;
        },
        __wbg_set_min_binding_size_4a9f4d0d9ee579af: function(arg0, arg1) {
            arg0.minBindingSize = arg1;
        },
        __wbg_set_mip_level_count_1d13855f7726190c: function(arg0, arg1) {
            arg0.mipLevelCount = arg1 >>> 0;
        },
        __wbg_set_module_882651860e912779: function(arg0, arg1) {
            arg0.module = arg1;
        },
        __wbg_set_module_b46c4a937ee89c3b: function(arg0, arg1) {
            arg0.module = arg1;
        },
        __wbg_set_multisample_0a38af2e310bacc6: function(arg0, arg1) {
            arg0.multisample = arg1;
        },
        __wbg_set_multisampled_f2de771b3ad62ff3: function(arg0, arg1) {
            arg0.multisampled = arg1 !== 0;
        },
        __wbg_set_offset_31c0a660f535c545: function(arg0, arg1) {
            arg0.offset = arg1;
        },
        __wbg_set_offset_a675629849c5f3b4: function(arg0, arg1) {
            arg0.offset = arg1;
        },
        __wbg_set_operation_879618283d591339: function(arg0, arg1) {
            arg0.operation = __wbindgen_enum_GpuBlendOperation[arg1];
        },
        __wbg_set_pass_op_238c7cbc20505ae9: function(arg0, arg1) {
            arg0.passOp = __wbindgen_enum_GpuStencilOperation[arg1];
        },
        __wbg_set_power_preference_f4cead100f48bab0: function(arg0, arg1) {
            arg0.powerPreference = __wbindgen_enum_GpuPowerPreference[arg1];
        },
        __wbg_set_primitive_01150af3e98fb372: function(arg0, arg1) {
            arg0.primitive = arg1;
        },
        __wbg_set_query_set_8441106911a3af36: function(arg0, arg1) {
            arg0.querySet = arg1;
        },
        __wbg_set_r_08c1678b22216ee0: function(arg0, arg1) {
            arg0.r = arg1;
        },
        __wbg_set_required_features_e9ee2e22feba0db3: function(arg0, arg1) {
            arg0.requiredFeatures = arg1;
        },
        __wbg_set_resolve_target_d00e2ef5a7388503: function(arg0, arg1) {
            arg0.resolveTarget = arg1;
        },
        __wbg_set_resource_5a4cc69a127b394e: function(arg0, arg1) {
            arg0.resource = arg1;
        },
        __wbg_set_sample_type_89fd8e71274ee6c2: function(arg0, arg1) {
            arg0.sampleType = __wbindgen_enum_GpuTextureSampleType[arg1];
        },
        __wbg_set_sampler_ab33334fb83c5a17: function(arg0, arg1) {
            arg0.sampler = arg1;
        },
        __wbg_set_shader_location_b905e964144cc9ad: function(arg0, arg1) {
            arg0.shaderLocation = arg1 >>> 0;
        },
        __wbg_set_size_a877ed6f434871bd: function(arg0, arg1) {
            arg0.size = arg1;
        },
        __wbg_set_size_b2cab7e432ec25dc: function(arg0, arg1) {
            arg0.size = arg1;
        },
        __wbg_set_src_factor_3bf35cc93f12e8c2: function(arg0, arg1) {
            arg0.srcFactor = __wbindgen_enum_GpuBlendFactor[arg1];
        },
        __wbg_set_stencil_back_6d0e3812c09eb489: function(arg0, arg1) {
            arg0.stencilBack = arg1;
        },
        __wbg_set_stencil_clear_value_53b51b80af22b8a4: function(arg0, arg1) {
            arg0.stencilClearValue = arg1 >>> 0;
        },
        __wbg_set_stencil_front_223b59e436e04d2d: function(arg0, arg1) {
            arg0.stencilFront = arg1;
        },
        __wbg_set_stencil_load_op_d88ff17c1f14f3b3: function(arg0, arg1) {
            arg0.stencilLoadOp = __wbindgen_enum_GpuLoadOp[arg1];
        },
        __wbg_set_stencil_read_mask_f7b2d22f2682c8f6: function(arg0, arg1) {
            arg0.stencilReadMask = arg1 >>> 0;
        },
        __wbg_set_stencil_read_only_6fba8956bae14007: function(arg0, arg1) {
            arg0.stencilReadOnly = arg1 !== 0;
        },
        __wbg_set_stencil_store_op_9637a0cb039fc7bb: function(arg0, arg1) {
            arg0.stencilStoreOp = __wbindgen_enum_GpuStoreOp[arg1];
        },
        __wbg_set_stencil_write_mask_fc2b202439c71444: function(arg0, arg1) {
            arg0.stencilWriteMask = arg1 >>> 0;
        },
        __wbg_set_step_mode_953dbc499c2ea5db: function(arg0, arg1) {
            arg0.stepMode = __wbindgen_enum_GpuVertexStepMode[arg1];
        },
        __wbg_set_storage_texture_0634dd6c87ac1132: function(arg0, arg1) {
            arg0.storageTexture = arg1;
        },
        __wbg_set_store_op_d6e36afb7a3bc15a: function(arg0, arg1) {
            arg0.storeOp = __wbindgen_enum_GpuStoreOp[arg1];
        },
        __wbg_set_strip_index_format_6813dd6e867de4f2: function(arg0, arg1) {
            arg0.stripIndexFormat = __wbindgen_enum_GpuIndexFormat[arg1];
        },
        __wbg_set_strokeStyle_087121ed5350b038: function(arg0, arg1, arg2) {
            arg0.strokeStyle = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_targets_0ab03a33d2c15ccd: function(arg0, arg1) {
            arg0.targets = arg1;
        },
        __wbg_set_textAlign_cdfa5b9f1c14f5c6: function(arg0, arg1, arg2) {
            arg0.textAlign = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_textBaseline_c7ec6538cc52b073: function(arg0, arg1, arg2) {
            arg0.textBaseline = getStringFromWasm0(arg1, arg2);
        },
        __wbg_set_texture_9dc3759e93cfbb84: function(arg0, arg1) {
            arg0.texture = arg1;
        },
        __wbg_set_timestamp_writes_736aa6c2c69ccaea: function(arg0, arg1) {
            arg0.timestampWrites = arg1;
        },
        __wbg_set_topology_84962f44b37e8986: function(arg0, arg1) {
            arg0.topology = __wbindgen_enum_GpuPrimitiveTopology[arg1];
        },
        __wbg_set_type_4ff365ea9ad896aa: function(arg0, arg1) {
            arg0.type = __wbindgen_enum_GpuBufferBindingType[arg1];
        },
        __wbg_set_type_b4b2fc6fbad39aeb: function(arg0, arg1) {
            arg0.type = __wbindgen_enum_GpuSamplerBindingType[arg1];
        },
        __wbg_set_usage_3bf7bce356282919: function(arg0, arg1) {
            arg0.usage = arg1 >>> 0;
        },
        __wbg_set_usage_a102e6844c6a65de: function(arg0, arg1) {
            arg0.usage = arg1 >>> 0;
        },
        __wbg_set_usage_ea5e5efc19daea09: function(arg0, arg1) {
            arg0.usage = arg1 >>> 0;
        },
        __wbg_set_vertex_96327c405a801524: function(arg0, arg1) {
            arg0.vertex = arg1;
        },
        __wbg_set_view_2d2806aa6c5822ca: function(arg0, arg1) {
            arg0.view = arg1;
        },
        __wbg_set_view_b7216eb00b7f584a: function(arg0, arg1) {
            arg0.view = arg1;
        },
        __wbg_set_view_dimension_c6aedf84f79e2593: function(arg0, arg1) {
            arg0.viewDimension = __wbindgen_enum_GpuTextureViewDimension[arg1];
        },
        __wbg_set_view_dimension_ccb64a21a1495106: function(arg0, arg1) {
            arg0.viewDimension = __wbindgen_enum_GpuTextureViewDimension[arg1];
        },
        __wbg_set_view_formats_d7be9eae49a0933b: function(arg0, arg1) {
            arg0.viewFormats = arg1;
        },
        __wbg_set_visibility_3445d21752d17dead: function(arg0, arg1) {
            arg0.visibility = arg1 >>> 0;
        },
        __wbg_set_width_7f07715a20503914: function(arg0, arg1) {
            arg0.width = arg1 >>> 0;
        },
        __wbg_set_width_d60bc4f2f20c56a4: function(arg0, arg1) {
            arg0.width = arg1 >>> 0;
        },
        __wbg_set_write_mask_b94f0c67654d5b00: function(arg0, arg1) {
            arg0.writeMask = arg1 >>> 0;
        },
        __wbg_stack_0ed75d68575b0f3c: function(arg0, arg1) {
            const ret = arg1.stack;
            const ptr1 = passStringToWasm0(ret, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
            const len1 = WASM_VECTOR_LEN;
            getDataViewMemory0().setInt32(arg0 + 4 * 1, len1, true);
            getDataViewMemory0().setInt32(arg0 + 4 * 0, ptr1, true);
        },
        __wbg_static_accessor_GLOBAL_12837167ad935116: function() {
            const ret = typeof global === 'undefined' ? null : global;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_GLOBAL_THIS_e628e89ab3b1c95f: function() {
            const ret = typeof globalThis === 'undefined' ? null : globalThis;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_SELF_a621d3dfbb60d0ce: function() {
            const ret = typeof self === 'undefined' ? null : self;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_static_accessor_WINDOW_f8727f0cf888e0bd: function() {
            const ret = typeof window === 'undefined' ? null : window;
            return isLikeNone(ret) ? 0 : addToExternrefTable0(ret);
        },
        __wbg_stroke_240ea7f2407d73c0: function(arg0) {
            arg0.stroke();
        },
        __wbg_submit_522f9e0b9d7e22fd: function(arg0, arg1) {
            arg0.submit(arg1);
        },
        __wbg_then_0d9fe2c7b1857d32: function(arg0, arg1, arg2) {
            const ret = arg0.then(arg1, arg2);
            return ret;
        },
        __wbg_then_b9e7b3b5f1a9e1b5: function(arg0, arg1) {
            const ret = arg0.then(arg1);
            return ret;
        },
        __wbg_unmap_a7fc4fb3238304a4: function(arg0) {
            arg0.unmap();
        },
        __wbg_visualization3d_new: function(arg0) {
            const ret = Visualization3D.__wrap(arg0);
            return ret;
        },
        __wbg_visualization_new: function(arg0) {
            const ret = Visualization.__wrap(arg0);
            return ret;
        },
        __wbg_width_5f66bde2e810fbde: function(arg0) {
            const ret = arg0.width;
            return ret;
        },
        __wbg_writeBuffer_b3540dd159ff60f1: function() { return handleError(function (arg0, arg1, arg2, arg3, arg4, arg5) {
            arg0.writeBuffer(arg1, arg2, arg3, arg4, arg5);
        }, arguments); },
        __wbindgen_cast_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Closure(Closure { dtor_idx: 80, function: Function { arguments: [Externref], shim_idx: 81, ret: Unit, inner_ret: Some(Unit) }, mutable: true }) -> Externref`.
            const ret = makeMutClosure(arg0, arg1, wasm.wasm_bindgen__closure__destroy__h6acca68855fb1e64, wasm_bindgen__convert__closures_____invoke__h622c43219b092f98);
            return ret;
        },
        __wbindgen_cast_0000000000000002: function(arg0) {
            // Cast intrinsic for `F64 -> Externref`.
            const ret = arg0;
            return ret;
        },
        __wbindgen_cast_0000000000000003: function(arg0, arg1) {
            // Cast intrinsic for `Ref(Slice(U8)) -> NamedExternref("Uint8Array")`.
            const ret = getArrayU8FromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_cast_0000000000000004: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./panschema_viz_bg.js": import0,
    };
}

function wasm_bindgen__convert__closures_____invoke__h622c43219b092f98(arg0, arg1, arg2) {
    wasm.wasm_bindgen__convert__closures_____invoke__h622c43219b092f98(arg0, arg1, arg2);
}

function wasm_bindgen__convert__closures_____invoke__h55e99d643b02aaa4(arg0, arg1, arg2, arg3) {
    wasm.wasm_bindgen__convert__closures_____invoke__h55e99d643b02aaa4(arg0, arg1, arg2, arg3);
}


const __wbindgen_enum_GpuBlendFactor = ["zero", "one", "src", "one-minus-src", "src-alpha", "one-minus-src-alpha", "dst", "one-minus-dst", "dst-alpha", "one-minus-dst-alpha", "src-alpha-saturated", "constant", "one-minus-constant", "src1", "one-minus-src1", "src1-alpha", "one-minus-src1-alpha"];


const __wbindgen_enum_GpuBlendOperation = ["add", "subtract", "reverse-subtract", "min", "max"];


const __wbindgen_enum_GpuBufferBindingType = ["uniform", "storage", "read-only-storage"];


const __wbindgen_enum_GpuCanvasAlphaMode = ["opaque", "premultiplied"];


const __wbindgen_enum_GpuCompareFunction = ["never", "less", "equal", "less-equal", "greater", "not-equal", "greater-equal", "always"];


const __wbindgen_enum_GpuCullMode = ["none", "front", "back"];


const __wbindgen_enum_GpuFrontFace = ["ccw", "cw"];


const __wbindgen_enum_GpuIndexFormat = ["uint16", "uint32"];


const __wbindgen_enum_GpuLoadOp = ["load", "clear"];


const __wbindgen_enum_GpuPowerPreference = ["low-power", "high-performance"];


const __wbindgen_enum_GpuPrimitiveTopology = ["point-list", "line-list", "line-strip", "triangle-list", "triangle-strip"];


const __wbindgen_enum_GpuSamplerBindingType = ["filtering", "non-filtering", "comparison"];


const __wbindgen_enum_GpuStencilOperation = ["keep", "zero", "replace", "invert", "increment-clamp", "decrement-clamp", "increment-wrap", "decrement-wrap"];


const __wbindgen_enum_GpuStorageTextureAccess = ["write-only", "read-only", "read-write"];


const __wbindgen_enum_GpuStoreOp = ["store", "discard"];


const __wbindgen_enum_GpuTextureAspect = ["all", "stencil-only", "depth-only"];


const __wbindgen_enum_GpuTextureFormat = ["r8unorm", "r8snorm", "r8uint", "r8sint", "r16uint", "r16sint", "r16float", "rg8unorm", "rg8snorm", "rg8uint", "rg8sint", "r32uint", "r32sint", "r32float", "rg16uint", "rg16sint", "rg16float", "rgba8unorm", "rgba8unorm-srgb", "rgba8snorm", "rgba8uint", "rgba8sint", "bgra8unorm", "bgra8unorm-srgb", "rgb9e5ufloat", "rgb10a2uint", "rgb10a2unorm", "rg11b10ufloat", "rg32uint", "rg32sint", "rg32float", "rgba16uint", "rgba16sint", "rgba16float", "rgba32uint", "rgba32sint", "rgba32float", "stencil8", "depth16unorm", "depth24plus", "depth24plus-stencil8", "depth32float", "depth32float-stencil8", "bc1-rgba-unorm", "bc1-rgba-unorm-srgb", "bc2-rgba-unorm", "bc2-rgba-unorm-srgb", "bc3-rgba-unorm", "bc3-rgba-unorm-srgb", "bc4-r-unorm", "bc4-r-snorm", "bc5-rg-unorm", "bc5-rg-snorm", "bc6h-rgb-ufloat", "bc6h-rgb-float", "bc7-rgba-unorm", "bc7-rgba-unorm-srgb", "etc2-rgb8unorm", "etc2-rgb8unorm-srgb", "etc2-rgb8a1unorm", "etc2-rgb8a1unorm-srgb", "etc2-rgba8unorm", "etc2-rgba8unorm-srgb", "eac-r11unorm", "eac-r11snorm", "eac-rg11unorm", "eac-rg11snorm", "astc-4x4-unorm", "astc-4x4-unorm-srgb", "astc-5x4-unorm", "astc-5x4-unorm-srgb", "astc-5x5-unorm", "astc-5x5-unorm-srgb", "astc-6x5-unorm", "astc-6x5-unorm-srgb", "astc-6x6-unorm", "astc-6x6-unorm-srgb", "astc-8x5-unorm", "astc-8x5-unorm-srgb", "astc-8x6-unorm", "astc-8x6-unorm-srgb", "astc-8x8-unorm", "astc-8x8-unorm-srgb", "astc-10x5-unorm", "astc-10x5-unorm-srgb", "astc-10x6-unorm", "astc-10x6-unorm-srgb", "astc-10x8-unorm", "astc-10x8-unorm-srgb", "astc-10x10-unorm", "astc-10x10-unorm-srgb", "astc-12x10-unorm", "astc-12x10-unorm-srgb", "astc-12x12-unorm", "astc-12x12-unorm-srgb"];


const __wbindgen_enum_GpuTextureSampleType = ["float", "unfilterable-float", "depth", "sint", "uint"];


const __wbindgen_enum_GpuTextureViewDimension = ["1d", "2d", "2d-array", "cube", "cube-array", "3d"];


const __wbindgen_enum_GpuVertexFormat = ["uint8", "uint8x2", "uint8x4", "sint8", "sint8x2", "sint8x4", "unorm8", "unorm8x2", "unorm8x4", "snorm8", "snorm8x2", "snorm8x4", "uint16", "uint16x2", "uint16x4", "sint16", "sint16x2", "sint16x4", "unorm16", "unorm16x2", "unorm16x4", "snorm16", "snorm16x2", "snorm16x4", "float16", "float16x2", "float16x4", "float32", "float32x2", "float32x3", "float32x4", "uint32", "uint32x2", "uint32x3", "uint32x4", "sint32", "sint32x2", "sint32x3", "sint32x4", "unorm10-10-10-2", "unorm8x4-bgra"];


const __wbindgen_enum_GpuVertexStepMode = ["vertex", "instance"];
const VisualizationFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_visualization_free(ptr >>> 0, 1));
const Visualization3DFinalization = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(ptr => wasm.__wbg_visualization3d_free(ptr >>> 0, 1));

function addToExternrefTable0(obj) {
    const idx = wasm.__externref_table_alloc();
    wasm.__wbindgen_externrefs.set(idx, obj);
    return idx;
}

const CLOSURE_DTORS = (typeof FinalizationRegistry === 'undefined')
    ? { register: () => {}, unregister: () => {} }
    : new FinalizationRegistry(state => state.dtor(state.a, state.b));

function debugString(val) {
    // primitive types
    const type = typeof val;
    if (type == 'number' || type == 'boolean' || val == null) {
        return  `${val}`;
    }
    if (type == 'string') {
        return `"${val}"`;
    }
    if (type == 'symbol') {
        const description = val.description;
        if (description == null) {
            return 'Symbol';
        } else {
            return `Symbol(${description})`;
        }
    }
    if (type == 'function') {
        const name = val.name;
        if (typeof name == 'string' && name.length > 0) {
            return `Function(${name})`;
        } else {
            return 'Function';
        }
    }
    // objects
    if (Array.isArray(val)) {
        const length = val.length;
        let debug = '[';
        if (length > 0) {
            debug += debugString(val[0]);
        }
        for(let i = 1; i < length; i++) {
            debug += ', ' + debugString(val[i]);
        }
        debug += ']';
        return debug;
    }
    // Test for built-in
    const builtInMatches = /\[object ([^\]]+)\]/.exec(toString.call(val));
    let className;
    if (builtInMatches && builtInMatches.length > 1) {
        className = builtInMatches[1];
    } else {
        // Failed to match the standard '[object ClassName]'
        return toString.call(val);
    }
    if (className == 'Object') {
        // we're a user defined class or Object
        // JSON.stringify avoids problems with cycles, and is generally much
        // easier than looping through ownProperties of `val`.
        try {
            return 'Object(' + JSON.stringify(val) + ')';
        } catch (_) {
            return 'Object';
        }
    }
    // errors
    if (val instanceof Error) {
        return `${val.name}: ${val.message}\n${val.stack}`;
    }
    // TODO we could test for more things here, like `Set`s and `Map`s.
    return className;
}

function getArrayU32FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint32ArrayMemory0().subarray(ptr / 4, ptr / 4 + len);
}

function getArrayU8FromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return getUint8ArrayMemory0().subarray(ptr / 1, ptr / 1 + len);
}

let cachedDataViewMemory0 = null;
function getDataViewMemory0() {
    if (cachedDataViewMemory0 === null || cachedDataViewMemory0.buffer.detached === true || (cachedDataViewMemory0.buffer.detached === undefined && cachedDataViewMemory0.buffer !== wasm.memory.buffer)) {
        cachedDataViewMemory0 = new DataView(wasm.memory.buffer);
    }
    return cachedDataViewMemory0;
}

function getStringFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return decodeText(ptr, len);
}

let cachedUint32ArrayMemory0 = null;
function getUint32ArrayMemory0() {
    if (cachedUint32ArrayMemory0 === null || cachedUint32ArrayMemory0.byteLength === 0) {
        cachedUint32ArrayMemory0 = new Uint32Array(wasm.memory.buffer);
    }
    return cachedUint32ArrayMemory0;
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function handleError(f, args) {
    try {
        return f.apply(this, args);
    } catch (e) {
        const idx = addToExternrefTable0(e);
        wasm.__wbindgen_exn_store(idx);
    }
}

function isLikeNone(x) {
    return x === undefined || x === null;
}

function makeMutClosure(arg0, arg1, dtor, f) {
    const state = { a: arg0, b: arg1, cnt: 1, dtor };
    const real = (...args) => {

        // First up with a closure we increment the internal reference
        // count. This ensures that the Rust closure environment won't
        // be deallocated while we're invoking it.
        state.cnt++;
        const a = state.a;
        state.a = 0;
        try {
            return f(a, state.b, ...args);
        } finally {
            state.a = a;
            real._wbg_cb_unref();
        }
    };
    real._wbg_cb_unref = () => {
        if (--state.cnt === 0) {
            state.dtor(state.a, state.b);
            state.a = 0;
            CLOSURE_DTORS.unregister(state);
        }
    };
    CLOSURE_DTORS.register(real, state, state);
    return real;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasm;
function __wbg_finalize_init(instance, module) {
    wasm = instance.exports;
    wasmModule = module;
    cachedDataViewMemory0 = null;
    cachedUint32ArrayMemory0 = null;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = module.ok && expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('panschema_viz_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
