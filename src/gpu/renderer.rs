//! GPU renderer for 3D graph visualization
//!
//! Provides `GpuRenderer` for rendering force-directed graphs using WebGPU.
//! Renders nodes as instanced spheres and edges as lines.

use std::sync::Arc;

use bytemuck;
use wgpu;
use wgpu::util::DeviceExt;

use crate::gpu::camera::Camera3D;
use crate::gpu::geometry::{MeshVertex, icosphere};
use crate::gpu::render_shaders::{edge_shader, node_shader};
use crate::gpu::types::{CameraUniforms, EdgeInstance, NodeInstance, RenderConfig};

/// Default icosphere subdivision level (162 vertices, smooth appearance)
const ICOSPHERE_SUBDIVISIONS: u32 = 2;

/// GPU renderer for 3D graph visualization.
///
/// Renders nodes as instanced spheres and edges as lines using WebGPU.
///
/// # Example
///
/// ```ignore
/// use panschema::gpu::{GpuRenderer, Camera3D, NodeInstance, EdgeInstance, RenderConfig};
///
/// // Create renderer (requires GPU device)
/// let renderer = GpuRenderer::new(device, queue, RenderConfig::default());
///
/// // Set up camera
/// let camera = Camera3D::new(800.0 / 600.0);
///
/// // Create nodes and edges
/// let nodes = vec![
///     NodeInstance::new(0.0, 0.0, 0.0).with_color(1.0, 0.0, 0.0, 1.0),
///     NodeInstance::new(100.0, 0.0, 0.0).with_color(0.0, 1.0, 0.0, 1.0),
/// ];
/// let edges = vec![
///     EdgeInstance::new([0.0, 0.0, 0.0], [100.0, 0.0, 0.0]),
/// ];
///
/// // Render frame
/// renderer.render(&camera, &nodes, &edges);
/// ```
pub struct GpuRenderer {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,

    // Render pipelines
    node_pipeline: wgpu::RenderPipeline,
    edge_pipeline: wgpu::RenderPipeline,

    // Bind groups
    camera_bind_group: wgpu::BindGroup,
    #[allow(dead_code)]
    camera_bind_group_layout: wgpu::BindGroupLayout,

    // Buffers
    camera_buffer: wgpu::Buffer,
    node_instance_buffer: wgpu::Buffer,
    edge_instance_buffer: wgpu::Buffer,
    icosphere_vertex_buffer: wgpu::Buffer,
    icosphere_index_buffer: wgpu::Buffer,

    // Render targets
    color_texture: wgpu::Texture,
    depth_texture: wgpu::Texture,
    staging_buffer: wgpu::Buffer,

    // State
    config: RenderConfig,
    icosphere_index_count: u32,
    max_nodes: u32,
    max_edges: u32,
}

impl GpuRenderer {
    /// Create a new renderer with the given device, queue, and configuration.
    ///
    /// # Arguments
    ///
    /// * `device` - The wgpu device
    /// * `queue` - The wgpu queue
    /// * `config` - Render configuration (size, clear color, etc.)
    pub fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>, config: RenderConfig) -> Self {
        Self::with_capacity(device, queue, config, 10000, 20000)
    }

    /// Create a renderer with specific buffer capacities.
    ///
    /// # Arguments
    ///
    /// * `max_nodes` - Maximum number of nodes to render
    /// * `max_edges` - Maximum number of edges to render
    pub fn with_capacity(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        config: RenderConfig,
        max_nodes: u32,
        max_edges: u32,
    ) -> Self {
        // Generate icosphere mesh
        let (icosphere_vertices, icosphere_indices) = icosphere(ICOSPHERE_SUBDIVISIONS);
        let icosphere_index_count = icosphere_indices.len() as u32;

        // Create shader modules
        let node_shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Node Shader"),
            source: wgpu::ShaderSource::Wgsl(node_shader().into()),
        });

        let edge_shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Edge Shader"),
            source: wgpu::ShaderSource::Wgsl(edge_shader().into()),
        });

        // Create bind group layout for camera
        let camera_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Camera Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        // Create pipeline layouts
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Render Pipeline Layout"),
            bind_group_layouts: &[&camera_bind_group_layout],
            push_constant_ranges: &[],
        });

        // Create node render pipeline
        let node_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Node Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &node_shader_module,
                entry_point: Some("vs_node"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    // Icosphere vertices (per-vertex)
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<MeshVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[
                            // position
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x3,
                                offset: 0,
                                shader_location: 0,
                            },
                            // normal
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x3,
                                offset: 12,
                                shader_location: 1,
                            },
                        ],
                    },
                    // Node instances (per-instance)
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<NodeInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[
                            // world_pos
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x3,
                                offset: 0,
                                shader_location: 2,
                            },
                            // radius
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32,
                                offset: 12,
                                shader_location: 3,
                            },
                            // color
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x4,
                                offset: 16,
                                shader_location: 4,
                            },
                            // selected
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32,
                                offset: 32,
                                shader_location: 5,
                            },
                        ],
                    },
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &node_shader_module,
                entry_point: Some("fs_node"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1, // No MSAA for simplicity in off-screen rendering
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        // Create edge render pipeline
        let edge_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Edge Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &edge_shader_module,
                entry_point: Some("vs_edge"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    // Edge instances (per-instance)
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<EdgeInstance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[
                            // start
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x3,
                                offset: 0,
                                shader_location: 0,
                            },
                            // alpha
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32,
                                offset: 12,
                                shader_location: 1,
                            },
                            // end
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x3,
                                offset: 16,
                                shader_location: 2,
                            },
                            // _padding
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32,
                                offset: 28,
                                shader_location: 3,
                            },
                        ],
                    },
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &edge_shader_module,
                entry_point: Some("fs_edge"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        // Create camera uniform buffer
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Camera Uniform Buffer"),
            size: std::mem::size_of::<CameraUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create camera bind group
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Camera Bind Group"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        // Create instance buffers
        let node_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Node Instance Buffer"),
            size: (max_nodes as usize * std::mem::size_of::<NodeInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let edge_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Edge Instance Buffer"),
            size: (max_edges as usize * std::mem::size_of::<EdgeInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create icosphere buffers
        let icosphere_vertex_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Icosphere Vertex Buffer"),
                contents: bytemuck::cast_slice(&icosphere_vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });

        let icosphere_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Icosphere Index Buffer"),
            contents: bytemuck::cast_slice(&icosphere_indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // Create render targets
        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Color Texture"),
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Depth Texture"),
            size: wgpu::Extent3d {
                width: config.width,
                height: config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });

        // Create staging buffer for pixel readback (with row alignment padding)
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = (config.width * 4).div_ceil(align) * align;
        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Staging Buffer"),
            size: (padded_bytes_per_row * config.height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Self {
            device,
            queue,
            node_pipeline,
            edge_pipeline,
            camera_bind_group,
            camera_bind_group_layout,
            camera_buffer,
            node_instance_buffer,
            edge_instance_buffer,
            icosphere_vertex_buffer,
            icosphere_index_buffer,
            color_texture,
            depth_texture,
            staging_buffer,
            config,
            icosphere_index_count,
            max_nodes,
            max_edges,
        }
    }

    /// Render a frame with the given camera, nodes, and edges.
    ///
    /// # Arguments
    ///
    /// * `camera` - The camera for view/projection
    /// * `nodes` - Node instances to render
    /// * `edges` - Edge instances to render
    pub fn render(&self, camera: &Camera3D, nodes: &[NodeInstance], edges: &[EdgeInstance]) {
        // Validate counts
        assert!(
            nodes.len() <= self.max_nodes as usize,
            "Too many nodes: {} > {}",
            nodes.len(),
            self.max_nodes
        );
        assert!(
            edges.len() <= self.max_edges as usize,
            "Too many edges: {} > {}",
            edges.len(),
            self.max_edges
        );

        // Update camera uniform buffer
        let camera_uniforms = camera.uniforms();
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&camera_uniforms));

        // Update instance buffers
        if !nodes.is_empty() {
            self.queue
                .write_buffer(&self.node_instance_buffer, 0, bytemuck::cast_slice(nodes));
        }
        if !edges.is_empty() {
            self.queue
                .write_buffer(&self.edge_instance_buffer, 0, bytemuck::cast_slice(edges));
        }

        // Create command encoder
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        // Create texture views
        let color_view = self
            .color_texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let depth_view = self
            .depth_texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Begin render pass
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Main Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: self.config.clear_color[0] as f64,
                            g: self.config.clear_color[1] as f64,
                            b: self.config.clear_color[2] as f64,
                            a: self.config.clear_color[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Draw edges first (behind nodes)
            if !edges.is_empty() {
                render_pass.set_pipeline(&self.edge_pipeline);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.edge_instance_buffer.slice(..));
                render_pass.draw(0..2, 0..edges.len() as u32);
            }

            // Draw nodes
            if !nodes.is_empty() {
                render_pass.set_pipeline(&self.node_pipeline);
                render_pass.set_bind_group(0, &self.camera_bind_group, &[]);
                render_pass.set_vertex_buffer(0, self.icosphere_vertex_buffer.slice(..));
                render_pass.set_vertex_buffer(1, self.node_instance_buffer.slice(..));
                render_pass.set_index_buffer(
                    self.icosphere_index_buffer.slice(..),
                    wgpu::IndexFormat::Uint32,
                );
                render_pass.draw_indexed(0..self.icosphere_index_count, 0, 0..nodes.len() as u32);
            }
        }

        // Submit commands
        self.queue.submit(std::iter::once(encoder.finish()));
    }

    /// Read back the rendered image as RGBA pixels.
    ///
    /// This is primarily for testing. It blocks until the GPU has finished
    /// rendering and the pixels have been copied to CPU memory.
    ///
    /// # Returns
    ///
    /// A vector of RGBA pixel data (width × height × 4 bytes).
    pub fn read_pixels(&self) -> Vec<u8> {
        // Calculate aligned bytes per row (must be multiple of COPY_BYTES_PER_ROW_ALIGNMENT = 256)
        let unpadded_bytes_per_row = self.config.width * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Readback Encoder"),
            });

        // Copy texture to staging buffer with aligned bytes per row
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.color_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.staging_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(self.config.height),
                },
            },
            wgpu::Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit(std::iter::once(encoder.finish()));

        // Map staging buffer and read
        let buffer_slice = self.staging_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            tx.send(result).unwrap();
        });
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv().unwrap().unwrap();

        let data = buffer_slice.get_mapped_range();

        // Remove padding from each row
        let mut pixels = Vec::with_capacity((self.config.width * self.config.height * 4) as usize);
        for y in 0..self.config.height {
            let start = (y * padded_bytes_per_row) as usize;
            let end = start + unpadded_bytes_per_row as usize;
            pixels.extend_from_slice(&data[start..end]);
        }

        drop(data);
        self.staging_buffer.unmap();

        pixels
    }

    /// Get the render configuration
    pub fn config(&self) -> &RenderConfig {
        &self.config
    }

    /// Get the device
    pub fn device(&self) -> &Arc<wgpu::Device> {
        &self.device
    }

    /// Get the queue
    pub fn queue(&self) -> &Arc<wgpu::Queue> {
        &self.queue
    }
}

/// Create a GPU device and queue for rendering.
///
/// This is a convenience function for creating a renderer without
/// an existing simulation.
pub async fn create_render_device() -> (wgpu::Device, wgpu::Queue) {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        })
        .await
        .expect("Failed to find GPU adapter");

    adapter
        .request_device(&wgpu::DeviceDescriptor::default(), None)
        .await
        .expect("Failed to create GPU device")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_device() -> (Arc<wgpu::Device>, Arc<wgpu::Queue>) {
        let (device, queue) = pollster::block_on(create_render_device());
        (Arc::new(device), Arc::new(queue))
    }

    #[test]
    fn test_renderer_creation() {
        let (device, queue) = create_test_device();
        let _renderer = GpuRenderer::new(device, queue, RenderConfig::default());
        // Should not panic
    }

    #[test]
    fn test_render_empty_scene() {
        let (device, queue) = create_test_device();
        let renderer = GpuRenderer::new(device, queue, RenderConfig::default());
        let camera = Camera3D::new(800.0 / 600.0);

        renderer.render(&camera, &[], &[]);

        let pixels = renderer.read_pixels();
        assert!(!pixels.is_empty());
        // Check that we got the expected size
        assert_eq!(pixels.len(), 800 * 600 * 4);
    }

    #[test]
    fn test_render_single_node() {
        let (device, queue) = create_test_device();
        let renderer = GpuRenderer::new(device, queue, RenderConfig::default());
        let camera = Camera3D::new(800.0 / 600.0);

        let nodes = vec![NodeInstance::new(0.0, 0.0, 0.0).with_color(1.0, 0.0, 0.0, 1.0)];

        renderer.render(&camera, &nodes, &[]);

        let pixels = renderer.read_pixels();
        assert!(!pixels.is_empty());

        // Check that there are some non-background pixels (the node)
        let background = [
            (0.1 * 255.0) as u8,
            (0.1 * 255.0) as u8,
            (0.15 * 255.0) as u8,
        ];
        let has_non_background = pixels.chunks(4).any(|pixel| {
            (pixel[0] as i32 - background[0] as i32).abs() > 10
                || (pixel[1] as i32 - background[1] as i32).abs() > 10
                || (pixel[2] as i32 - background[2] as i32).abs() > 10
        });
        assert!(has_non_background, "Expected to see the rendered node");
    }

    #[test]
    fn test_render_with_edges() {
        let (device, queue) = create_test_device();
        let renderer = GpuRenderer::new(device, queue, RenderConfig::default());
        let camera = Camera3D::new(800.0 / 600.0);

        let nodes = vec![
            NodeInstance::new(-50.0, 0.0, 0.0).with_color(1.0, 0.0, 0.0, 1.0),
            NodeInstance::new(50.0, 0.0, 0.0).with_color(0.0, 1.0, 0.0, 1.0),
        ];
        let edges = vec![EdgeInstance::new([-50.0, 0.0, 0.0], [50.0, 0.0, 0.0])];

        renderer.render(&camera, &nodes, &edges);

        let pixels = renderer.read_pixels();
        assert!(!pixels.is_empty());
    }

    #[test]
    fn test_render_multiple_nodes() {
        let (device, queue) = create_test_device();
        let renderer = GpuRenderer::new(device, queue, RenderConfig::default());
        let camera = Camera3D::new(800.0 / 600.0);

        let nodes: Vec<NodeInstance> = (0..100)
            .map(|i| {
                let angle = (i as f32 / 100.0) * std::f32::consts::PI * 2.0;
                let x = angle.cos() * 100.0;
                let y = angle.sin() * 100.0;
                NodeInstance::new(x, y, 0.0).with_color(
                    i as f32 / 100.0,
                    1.0 - i as f32 / 100.0,
                    0.5,
                    1.0,
                )
            })
            .collect();

        renderer.render(&camera, &nodes, &[]);

        let pixels = renderer.read_pixels();
        assert!(!pixels.is_empty());
    }
}
