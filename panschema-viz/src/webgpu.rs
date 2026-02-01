//! WebGPU 3D renderer for graph visualization
//!
//! Renders nodes as circles and edges as lines in 3D space using WebGPU.

use wasm_bindgen::JsCast;
use web_sys::HtmlCanvasElement;
use wgpu::util::DeviceExt;

use crate::camera3d::{BoundingBox3D, Camera3D};
use crate::simulation3d::{SimEdge3D, SimNode3D, Simulation3D};

/// Vertex for node rendering (instanced circles)
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct NodeVertex {
    position: [f32; 2], // Local vertex position
}

/// Instance data for each node
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct NodeInstance {
    world_pos: [f32; 3],
    radius: f32,
    color: [f32; 4],
}

/// Vertex for edge rendering
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct EdgeVertex {
    position: [f32; 3],
    color: [f32; 4],
}

/// Uniform buffer for camera matrices
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniforms {
    view_proj: [f32; 16],
    canvas_size: [f32; 2],
    _padding: [f32; 2],
}

/// WebGPU 3D renderer
pub struct WebGpuRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,

    // Node rendering
    node_pipeline: wgpu::RenderPipeline,
    node_vertex_buffer: wgpu::Buffer,
    node_instance_buffer: wgpu::Buffer,
    node_instance_count: u32,

    // Edge rendering
    edge_pipeline: wgpu::RenderPipeline,
    edge_vertex_buffer: wgpu::Buffer,
    edge_vertex_count: u32,

    // Uniforms
    camera_uniform_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,

    // Camera
    camera: Camera3D,

    // Canvas reference
    width: u32,
    height: u32,
}

impl WebGpuRenderer {
    /// Create a new WebGPU renderer (async)
    pub async fn new(canvas: HtmlCanvasElement) -> Result<Self, String> {
        let width = canvas.width();
        let height = canvas.height();

        // Create wgpu instance
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..Default::default()
        });

        // Create surface from canvas
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(|e| format!("Failed to create surface: {}", e))?;

        // Request adapter
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or("Failed to find adapter")?;

        // Request device
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("panschema-viz"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                    memory_hints: Default::default(),
                },
                None,
            )
            .await
            .map_err(|e| format!("Failed to create device: {}", e))?;

        // Configure surface
        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // Create camera uniform buffer
        let camera = Camera3D::new(width as f32 / height as f32);
        let camera_uniforms = CameraUniforms {
            view_proj: camera.view_projection_matrix(),
            canvas_size: [width as f32, height as f32],
            _padding: [0.0, 0.0],
        };

        let camera_uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Uniform Buffer"),
            contents: bytemuck::cast_slice(&[camera_uniforms]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Camera Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Camera Bind Group"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_uniform_buffer.as_entire_binding(),
            }],
        });

        // Create node pipeline
        let node_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Node Shader"),
            source: wgpu::ShaderSource::Wgsl(NODE_SHADER.into()),
        });

        let node_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Node Pipeline Layout"),
            bind_group_layouts: &[&camera_bind_group_layout],
            push_constant_ranges: &[],
        });

        let node_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Node Pipeline"),
            layout: Some(&node_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &node_shader,
                entry_point: Some("vs_main"),
                buffers: &[
                    // Vertex buffer (quad)
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<NodeVertex>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                    },
                    // Instance buffer
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<NodeInstance>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![1 => Float32x3, 2 => Float32, 3 => Float32x4],
                    },
                ],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &node_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Create node vertex buffer (quad for each node)
        let node_vertices = [
            NodeVertex {
                position: [-1.0, -1.0],
            },
            NodeVertex {
                position: [1.0, -1.0],
            },
            NodeVertex {
                position: [1.0, 1.0],
            },
            NodeVertex {
                position: [-1.0, -1.0],
            },
            NodeVertex {
                position: [1.0, 1.0],
            },
            NodeVertex {
                position: [-1.0, 1.0],
            },
        ];

        let node_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Node Vertex Buffer"),
            contents: bytemuck::cast_slice(&node_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        // Create node instance buffer (will be updated each frame)
        let node_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Node Instance Buffer"),
            size: 1024 * std::mem::size_of::<NodeInstance>() as u64, // Max 1024 nodes
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create edge pipeline
        let edge_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Edge Shader"),
            source: wgpu::ShaderSource::Wgsl(EDGE_SHADER.into()),
        });

        let edge_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Edge Pipeline Layout"),
            bind_group_layouts: &[&camera_bind_group_layout],
            push_constant_ranges: &[],
        });

        let edge_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Edge Pipeline"),
            layout: Some(&edge_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &edge_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<EdgeVertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4],
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &edge_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Create edge vertex buffer
        let edge_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Edge Vertex Buffer"),
            size: 4096 * std::mem::size_of::<EdgeVertex>() as u64, // Max 2048 edges
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            node_pipeline,
            node_vertex_buffer,
            node_instance_buffer,
            node_instance_count: 0,
            edge_pipeline,
            edge_vertex_buffer,
            edge_vertex_count: 0,
            camera_uniform_buffer,
            camera_bind_group,
            camera,
            width,
            height,
        })
    }

    /// Resize the renderer
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.width = width;
            self.height = height;
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            self.camera.resize(width as f32, height as f32);
        }
    }

    /// Update animation state
    pub fn update_animation(&mut self) {
        self.camera.update_animation();
    }

    /// Render the simulation state
    pub fn render(&mut self, sim: &Simulation3D) {
        // Update node instances
        self.update_node_instances(&sim.nodes);

        // Update edge vertices
        self.update_edge_vertices(&sim.edges, &sim.nodes);

        // Update camera uniforms
        let camera_uniforms = CameraUniforms {
            view_proj: self.camera.view_projection_matrix(),
            canvas_size: [self.width as f32, self.height as f32],
            _padding: [0.0, 0.0],
        };
        self.queue.write_buffer(
            &self.camera_uniform_buffer,
            0,
            bytemuck::cast_slice(&[camera_uniforms]),
        );

        // Get surface texture
        let output = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(_) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.102,
                            g: 0.102,
                            b: 0.18,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Draw edges first (behind nodes)
            if self.edge_vertex_count > 0 {
                render_pass.set_pipeline(&self.edge_pipeline);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.edge_vertex_buffer.slice(..));
                render_pass.draw(0..self.edge_vertex_count, 0..1);
            }

            // Draw nodes
            if self.node_instance_count > 0 {
                render_pass.set_pipeline(&self.node_pipeline);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.node_vertex_buffer.slice(..));
                render_pass.set_vertex_buffer(1, self.node_instance_buffer.slice(..));
                render_pass.draw(0..6, 0..self.node_instance_count);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }

    /// Update node instance buffer
    fn update_node_instances(&mut self, nodes: &[SimNode3D]) {
        let instances: Vec<NodeInstance> = nodes
            .iter()
            .map(|n| NodeInstance {
                world_pos: [n.x, n.y, n.z],
                radius: n.radius,
                color: n.color,
            })
            .collect();

        if !instances.is_empty() {
            self.queue.write_buffer(
                &self.node_instance_buffer,
                0,
                bytemuck::cast_slice(&instances),
            );
        }
        self.node_instance_count = instances.len() as u32;
    }

    /// Update edge vertex buffer
    fn update_edge_vertices(&mut self, edges: &[SimEdge3D], nodes: &[SimNode3D]) {
        let edge_color = [0.4, 0.4, 0.47, 0.5];

        let vertices: Vec<EdgeVertex> = edges
            .iter()
            .flat_map(|e| {
                let source = &nodes[e.source];
                let target = &nodes[e.target];
                [
                    EdgeVertex {
                        position: [source.x, source.y, source.z],
                        color: edge_color,
                    },
                    EdgeVertex {
                        position: [target.x, target.y, target.z],
                        color: edge_color,
                    },
                ]
            })
            .collect();

        if !vertices.is_empty() {
            self.queue
                .write_buffer(&self.edge_vertex_buffer, 0, bytemuck::cast_slice(&vertices));
        }
        self.edge_vertex_count = vertices.len() as u32;
    }

    /// Orbit camera horizontally
    pub fn orbit_horizontal(&mut self, delta: f32) {
        self.camera.orbit_horizontal(delta);
    }

    /// Orbit camera vertically
    pub fn orbit_vertical(&mut self, delta: f32) {
        self.camera.orbit_vertical(delta);
    }

    /// Zoom camera
    pub fn zoom(&mut self, factor: f32) {
        self.camera.zoom(factor);
    }

    /// Pan camera
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.camera.pan(dx, dy);
    }

    /// Reset camera view
    pub fn reset_view(&mut self) {
        self.camera.reset_view();
    }

    /// Fit camera to graph bounds
    pub fn fit_to_bounds(&mut self, nodes: &[SimNode3D], padding: f32) {
        if nodes.is_empty() {
            return;
        }

        let mut bounds = BoundingBox3D::empty();
        for node in nodes {
            bounds.include_sphere(node.x, node.y, node.z, node.radius);
        }

        self.camera.fit_to_bounds(&bounds, padding);
    }
}

// WGSL Shaders

const NODE_SHADER: &str = r#"
struct CameraUniforms {
    view_proj: mat4x4<f32>,
    canvas_size: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

struct VertexInput {
    @location(0) position: vec2<f32>,
}

struct InstanceInput {
    @location(1) world_pos: vec3<f32>,
    @location(2) radius: f32,
    @location(3) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) local_pos: vec2<f32>,
}

@vertex
fn vs_main(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    var out: VertexOutput;

    // Project center to clip space
    let center_clip = camera.view_proj * vec4<f32>(instance.world_pos, 1.0);

    // Billboard: offset in clip space (GPU will do perspective divide)
    // The projection matrix already handles perspective, so we just need to
    // scale the world-space radius by the projection's y-scale factor.
    // After GPU perspective divide, this gives correct screen-space size.
    let aspect = camera.canvas_size.x / camera.canvas_size.y;
    let billboard_scale = instance.radius * 2.0;

    out.clip_position = center_clip;
    out.clip_position.x += vertex.position.x * billboard_scale / aspect;
    out.clip_position.y += vertex.position.y * billboard_scale;

    out.color = instance.color;
    out.local_pos = vertex.position;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Draw circle
    let dist = length(in.local_pos);
    if (dist > 1.0) {
        discard;
    }

    // Soft edge
    let alpha = smoothstep(1.0, 0.9, dist);

    // Simple lighting (hemisphere)
    let brightness = 0.6 + 0.4 * (1.0 - dist);

    return vec4<f32>(in.color.rgb * brightness, in.color.a * alpha);
}
"#;

const EDGE_SHADER: &str = r#"
struct CameraUniforms {
    view_proj: mat4x4<f32>,
    canvas_size: vec2<f32>,
    _padding: vec2<f32>,
}

@group(0) @binding(0)
var<uniform> camera: CameraUniforms;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_main(vertex: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = camera.view_proj * vec4<f32>(vertex.position, 1.0);
    out.color = vertex.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
"#;
