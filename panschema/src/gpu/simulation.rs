//! GPU-accelerated force simulation using wgpu compute shaders
//!
//! This module provides a high-performance force-directed graph layout
//! implementation that runs entirely on the GPU.

use super::shaders::ForceShaders;
use super::types::{GpuEdge, GpuNode, GpuSimulationConfig, SimulationUniforms};
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// GPU-accelerated force simulation
///
/// This struct manages the GPU resources and compute pipelines for
/// running force-directed graph layout on the GPU.
pub struct GpuSimulation {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,

    // Compute pipelines
    link_force_pipeline: wgpu::ComputePipeline,
    many_body_pipeline: wgpu::ComputePipeline,
    center_pipeline: wgpu::ComputePipeline,
    integrate_pipeline: wgpu::ComputePipeline,

    // Buffers
    node_buffer: wgpu::Buffer,
    // Kept alive to maintain GPU resource (referenced by bind_group)
    _edge_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,

    // Bind group
    bind_group: wgpu::BindGroup,
    // Kept alive to maintain GPU resource
    _bind_group_layout: wgpu::BindGroupLayout,

    // Simulation state
    config: GpuSimulationConfig,
    alpha: f32,
    node_count: u32,
    edge_count: u32,

    // Staging buffer for reading back results
    staging_buffer: wgpu::Buffer,
}

impl GpuSimulation {
    /// Create a new GPU simulation with the given nodes and edges
    pub fn new(nodes: &[GpuNode], edges: &[GpuEdge]) -> Self {
        Self::with_config(nodes, edges, GpuSimulationConfig::default())
    }

    /// Create a new GPU simulation with custom configuration
    pub fn with_config(nodes: &[GpuNode], edges: &[GpuEdge], config: GpuSimulationConfig) -> Self {
        let (device, queue) = pollster::block_on(Self::create_device());
        let device = Arc::new(device);
        let queue = Arc::new(queue);

        Self::with_device(device, queue, nodes, edges, config)
    }

    /// Create a simulation using an existing device and queue
    pub fn with_device(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        nodes: &[GpuNode],
        edges: &[GpuEdge],
        config: GpuSimulationConfig,
    ) -> Self {
        let shaders = ForceShaders::new();
        let node_count = nodes.len() as u32;
        let edge_count = edges.len() as u32;

        let link_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Link Force Shader"),
            source: wgpu::ShaderSource::Wgsl(shaders.link_force.into()),
        });

        let many_body_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Many-Body Force Shader"),
            source: wgpu::ShaderSource::Wgsl(shaders.many_body_force.into()),
        });

        let center_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Center Force Shader"),
            source: wgpu::ShaderSource::Wgsl(shaders.center_force.into()),
        });

        let integrate_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Integrate Shader"),
            source: wgpu::ShaderSource::Wgsl(shaders.integrate.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Force Simulation Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Force Simulation Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let link_force_pipeline =
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("Link Force Pipeline"),
                layout: Some(&pipeline_layout),
                module: &link_module,
                entry_point: Some("link_force"),
                compilation_options: Default::default(),
                cache: None,
            });

        let many_body_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Many-Body Force Pipeline"),
            layout: Some(&pipeline_layout),
            module: &many_body_module,
            entry_point: Some("many_body_force"),
            compilation_options: Default::default(),
            cache: None,
        });

        let center_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Center Force Pipeline"),
            layout: Some(&pipeline_layout),
            module: &center_module,
            entry_point: Some("center_force"),
            compilation_options: Default::default(),
            cache: None,
        });

        let integrate_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Integrate Pipeline"),
            layout: Some(&pipeline_layout),
            module: &integrate_module,
            entry_point: Some("integrate"),
            compilation_options: Default::default(),
            cache: None,
        });

        let node_buffer_size = std::mem::size_of_val(nodes) as u64;
        let node_buffer_size = node_buffer_size.max(16);

        let node_buffer = if nodes.is_empty() {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Node Buffer (empty)"),
                size: 16,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        } else {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Node Buffer"),
                contents: bytemuck::cast_slice(nodes),
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
            })
        };

        let edge_buffer = if edges.is_empty() {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Edge Buffer (empty)"),
                size: 16,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        } else {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Edge Buffer"),
                contents: bytemuck::cast_slice(edges),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            })
        };

        let uniforms = SimulationUniforms {
            alpha: config.alpha,
            velocity_decay: config.velocity_decay,
            node_count,
            edge_count,
            center: config.center,
            center_strength: config.center_strength,
            theta: config.theta,
            distance_min: config.distance_min,
            distance_max: config.distance_max,
            max_velocity: config.max_velocity,
        };

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Uniform Buffer"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Force Simulation Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: node_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: edge_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
        });

        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Staging Buffer"),
            size: node_buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            device,
            queue,
            link_force_pipeline,
            many_body_pipeline,
            center_pipeline,
            integrate_pipeline,
            node_buffer,
            _edge_buffer: edge_buffer,
            uniform_buffer,
            bind_group,
            _bind_group_layout: bind_group_layout,
            alpha: config.alpha,
            config,
            node_count,
            edge_count,
            staging_buffer,
        }
    }

    async fn create_device() -> (wgpu::Device, wgpu::Queue) {
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
            .expect("Failed to find suitable GPU adapter");

        adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Force Simulation Device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None, // trace path
            )
            .await
            .expect("Failed to create device")
    }

    /// Run a single simulation tick
    ///
    /// This dispatches all force compute shaders and the integration step.
    pub fn tick(&mut self) {
        self.alpha += (self.config.alpha_target - self.alpha) * self.config.alpha_decay;

        let uniforms = SimulationUniforms {
            alpha: self.alpha,
            velocity_decay: self.config.velocity_decay,
            node_count: self.node_count,
            edge_count: self.edge_count,
            center: self.config.center,
            center_strength: self.config.center_strength,
            theta: self.config.theta,
            distance_min: self.config.distance_min,
            distance_max: self.config.distance_max,
            max_velocity: self.config.max_velocity,
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let node_workgroups = self.node_count.div_ceil(256);
        let edge_workgroups = self.edge_count.div_ceil(256);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Force Simulation Encoder"),
            });

        if self.edge_count > 0 {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Link Force Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.link_force_pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.dispatch_workgroups(edge_workgroups.max(1), 1, 1);
        }

        if self.node_count > 1 {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Many-Body Force Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.many_body_pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.dispatch_workgroups(node_workgroups.max(1), 1, 1);
        }

        if self.node_count > 0 {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Center Force Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.center_pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.dispatch_workgroups(node_workgroups.max(1), 1, 1);
        }

        if self.node_count > 0 {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Integration Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.integrate_pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.dispatch_workgroups(node_workgroups.max(1), 1, 1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
    }

    /// Run multiple simulation ticks
    pub fn tick_n(&mut self, n: usize) {
        for _ in 0..n {
            self.tick();
        }
    }

    /// Run simulation until convergence (alpha < alpha_min)
    pub fn run_to_convergence(&mut self) -> usize {
        let mut ticks = 0;
        while self.alpha > self.config.alpha_min {
            self.tick();
            ticks += 1;
            // Safety limit
            if ticks > 10000 {
                break;
            }
        }
        ticks
    }

    /// Read back current node positions from GPU
    pub fn read_nodes(&self) -> Vec<GpuNode> {
        if self.node_count == 0 {
            return Vec::new();
        }

        let node_buffer_size = (self.node_count as usize * std::mem::size_of::<GpuNode>()) as u64;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Read Nodes Encoder"),
            });
        encoder.copy_buffer_to_buffer(
            &self.node_buffer,
            0,
            &self.staging_buffer,
            0,
            node_buffer_size,
        );
        self.queue.submit(std::iter::once(encoder.finish()));

        let buffer_slice = self.staging_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            tx.send(result).unwrap();
        });
        let _ = self.device.poll(wgpu::Maintain::Wait);
        rx.recv().unwrap().expect("Failed to map buffer");

        let data = buffer_slice.get_mapped_range();
        let nodes: Vec<GpuNode> = bytemuck::cast_slice(&data).to_vec();
        drop(data);
        self.staging_buffer.unmap();

        nodes
    }

    /// Get current alpha value
    pub fn alpha(&self) -> f32 {
        self.alpha
    }

    /// Check if simulation has converged
    pub fn is_converged(&self) -> bool {
        self.alpha < self.config.alpha_min
    }

    /// Get number of nodes
    pub fn node_count(&self) -> u32 {
        self.node_count
    }

    /// Get number of edges
    pub fn edge_count(&self) -> u32 {
        self.edge_count
    }

    /// Update node positions (e.g., for dragging)
    pub fn update_nodes(&mut self, nodes: &[GpuNode]) {
        assert_eq!(nodes.len() as u32, self.node_count, "Node count mismatch");
        self.queue
            .write_buffer(&self.node_buffer, 0, bytemuck::cast_slice(nodes));
    }

    /// Reheat the simulation (reset alpha)
    pub fn reheat(&mut self) {
        self.alpha = self.config.alpha;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulation_creation() {
        let nodes = vec![GpuNode::new(0.0, 0.0, 0.0), GpuNode::new(100.0, 0.0, 0.0)];
        let edges = vec![GpuEdge::new(0, 1)];

        let sim = GpuSimulation::new(&nodes, &edges);
        assert_eq!(sim.node_count(), 2);
        assert_eq!(sim.edge_count(), 1);
        assert!(!sim.is_converged());
    }

    #[test]
    fn test_simulation_tick() {
        let nodes = vec![GpuNode::new(0.0, 0.0, 0.0), GpuNode::new(100.0, 0.0, 0.0)];
        let edges = vec![GpuEdge::new(0, 1).with_distance(50.0)];

        let mut sim = GpuSimulation::new(&nodes, &edges);
        let initial_alpha = sim.alpha();

        sim.tick();

        // Alpha should have decayed
        assert!(sim.alpha() < initial_alpha);

        // Read back nodes and check they moved
        let result_nodes = sim.read_nodes();
        assert_eq!(result_nodes.len(), 2);

        // Nodes should have moved due to link force pulling them together
        // (initial distance 100, target distance 50)
        let dx = result_nodes[1].position[0] - result_nodes[0].position[0];
        assert!(dx < 100.0, "Nodes should have moved closer: dx = {}", dx);
    }

    #[test]
    fn test_simulation_convergence() {
        let nodes = vec![GpuNode::new(-50.0, 0.0, 0.0), GpuNode::new(50.0, 0.0, 0.0)];
        let edges = vec![GpuEdge::new(0, 1).with_distance(30.0)];

        let mut sim = GpuSimulation::new(&nodes, &edges);
        let ticks = sim.run_to_convergence();

        assert!(sim.is_converged());
        assert!(ticks > 0);
        assert!(ticks <= 10000);
    }

    #[test]
    fn test_empty_graph() {
        let nodes: Vec<GpuNode> = vec![];
        let edges: Vec<GpuEdge> = vec![];

        let mut sim = GpuSimulation::new(&nodes, &edges);
        sim.tick(); // Should not crash

        let result = sim.read_nodes();
        assert!(result.is_empty());
    }

    #[test]
    fn test_single_node() {
        let nodes = vec![GpuNode::new(10.0, 20.0, 30.0)];
        let edges: Vec<GpuEdge> = vec![];

        let mut sim = GpuSimulation::new(&nodes, &edges);
        sim.tick_n(10);

        let result = sim.read_nodes();
        assert_eq!(result.len(), 1);
        // Single node with centering force should move toward center
    }
}
