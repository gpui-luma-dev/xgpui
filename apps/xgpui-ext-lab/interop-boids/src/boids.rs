// Ported from wgpu v30.0.1, examples/features/src/boids/mod.rs
// (https://github.com/gfx-rs/wgpu/tree/v30.0.1/examples/features/src/boids).
// Copyright (c) 2025 The gfx-rs developers. Licensed under either of the
// Apache License, Version 2.0 or the MIT license, at your option.
//
// Changes from the original:
// - The particle count is a parameter instead of a constant.
// - `render` records into a caller's encoder and target view instead of
//   submitting to a swapchain frame, and can draw without stepping (used to
//   show a new flock while paused).
// - `set_delta_t` changes the simulation's time step (deltaT, 0.04 in the
//   original), so speed can change while every step is still shown.
// - The random generator is a local WyRand step instead of the `nanorand`
//   crate, and buffer contents are converted without `bytemuck`.
// - The winit framework hooks (`update`, `resize`, `main`, the test) are gone.

//! Flocking boids example with gpu compute update pass
//! adapted from <https://github.com/austinEng/webgpu-samples/blob/master/src/examples/computeBoids.ts>

use xgpui_gpu_interop::wgpu::{self, util::DeviceExt};

/// wgpu's default number of boid particles to simulate.
pub const DEFAULT_PARTICLES: u32 = 1500;

/// Number of single-particle calculations (invocations) in each gpu work group.
const PARTICLES_PER_GROUP: u32 = 64;

/// Holds references to wgpu resources and frame persistent data.
pub struct Boids {
    particle_bind_groups: Vec<wgpu::BindGroup>,
    particle_buffers: Vec<wgpu::Buffer>,
    vertices_buffer: wgpu::Buffer,
    compute_pipeline: wgpu::ComputePipeline,
    render_pipeline: wgpu::RenderPipeline,
    work_group_count: u32,
    sim_param_buffer: wgpu::Buffer,
    particles: u32,
    frame_num: usize,
}

impl Boids {
    /// The limits the example asks for.
    pub fn required_limits() -> wgpu::Limits {
        wgpu::Limits::downlevel_defaults()
    }

    /// Constructs the simulation for `particles` boids, drawing into `format`.
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, particles: u32) -> Self {
        let compute_shader = device.create_shader_module(wgpu::include_wgsl!("compute.wgsl"));
        let draw_shader = device.create_shader_module(wgpu::include_wgsl!("draw.wgsl"));

        // buffer for simulation parameters uniform

        let sim_param_data = [
            0.04f32, // deltaT
            0.1,     // rule1Distance
            0.025,   // rule2Distance
            0.025,   // rule3Distance
            0.02,    // rule1Scale
            0.05,    // rule2Scale
            0.005,   // rule3Scale
        ];
        let sim_param_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Simulation Parameter Buffer"),
            contents: &f32_bytes(&sim_param_data),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        // create compute bind layout group and compute pipeline layout

        let compute_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(
                                (sim_param_data.len() * size_of::<f32>()) as _,
                            ),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new((particles * 16) as _),
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new((particles * 16) as _),
                        },
                        count: None,
                    },
                ],
                label: None,
            });
        let compute_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("compute"),
                bind_group_layouts: &[Some(&compute_bind_group_layout)],
                immediate_size: 0,
            });

        // create render pipeline with empty bind group layout

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("render"),
                bind_group_layouts: &[],
                immediate_size: 0,
            });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &draw_shader,
                entry_point: Some("main_vs"),
                compilation_options: Default::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: 4 * 4,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: 2 * 4,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![2 => Float32x2],
                    }),
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &draw_shader,
                entry_point: Some("main_fs"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // create compute pipeline

        let compute_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Compute pipeline"),
            layout: Some(&compute_pipeline_layout),
            module: &compute_shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        // buffer for the three 2d triangle vertices of each instance

        let vertex_buffer_data = [-0.01f32, -0.02, 0.01, -0.02, 0.00, 0.02];
        let vertices_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: &f32_bytes(&vertex_buffer_data),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });

        // buffer for all particles data of type [(posx,posy,velx,vely),...]

        let mut initial_particle_data = vec![0.0f32; (4 * particles) as usize];
        let mut rng = WyRand(42);
        let mut unif = || rng.next_f32() * 2f32 - 1f32; // Generate a num (-1, 1)
        for particle_instance_chunk in initial_particle_data.chunks_mut(4) {
            particle_instance_chunk[0] = unif(); // posx
            particle_instance_chunk[1] = unif(); // posy
            particle_instance_chunk[2] = unif() * 0.1; // velx
            particle_instance_chunk[3] = unif() * 0.1; // vely
        }

        // creates two buffers of particle data each of size `particles`
        // the two buffers alternate as dst and src for each frame

        let initial_particle_bytes = f32_bytes(&initial_particle_data);
        let mut particle_buffers = Vec::<wgpu::Buffer>::new();
        let mut particle_bind_groups = Vec::<wgpu::BindGroup>::new();
        for i in 0..2 {
            particle_buffers.push(
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(&format!("Particle Buffer {i}")),
                    contents: &initial_particle_bytes,
                    usage: wgpu::BufferUsages::VERTEX
                        | wgpu::BufferUsages::STORAGE
                        | wgpu::BufferUsages::COPY_DST,
                }),
            );
        }

        // create two bind groups, one for each buffer as the src
        // where the alternate buffer is used as the dst

        for i in 0..2 {
            particle_bind_groups.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &compute_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: sim_param_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: particle_buffers[i].as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: particle_buffers[(i + 1) % 2].as_entire_binding(), // bind to opposite buffer
                    },
                ],
                label: None,
            }));
        }

        // calculates number of work groups from PARTICLES_PER_GROUP constant
        let work_group_count = particles.div_ceil(PARTICLES_PER_GROUP);

        Boids {
            particle_bind_groups,
            particle_buffers,
            vertices_buffer,
            compute_pipeline,
            render_pipeline,
            work_group_count,
            sim_param_buffer,
            particles,
            frame_num: 0,
        }
    }

    /// The original's time step per simulation step.
    pub const DELTA_T: f32 = 0.04;

    /// Set deltaT, the first simulation parameter, for the next steps.
    pub fn set_delta_t(&self, queue: &wgpu::Queue, delta_t: f32) {
        queue.write_buffer(&self.sim_param_buffer, 0, &delta_t.to_ne_bytes());
    }

    pub fn particles(&self) -> u32 {
        self.particles
    }

    /// Records one frame into `encoder`: when `step`, a compute pass that
    /// moves every boid, then a TriangleList draw call for all particles at 3
    /// vertices each into `view`. Without `step` it redraws the current flock.
    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        step: bool,
    ) {
        // create render pass descriptor and its color attachments
        let color_attachments = [Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })];
        let render_pass_descriptor = wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &color_attachments,
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        };

        if step {
            encoder.push_debug_group("compute boid movement");
            {
                // compute pass
                let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: None,
                    timestamp_writes: None,
                });
                cpass.set_pipeline(&self.compute_pipeline);
                cpass.set_bind_group(0, &self.particle_bind_groups[self.frame_num % 2], &[]);
                cpass.dispatch_workgroups(self.work_group_count, 1, 1);
            }
            encoder.pop_debug_group();
            // update frame count
            self.frame_num += 1;
        }

        encoder.push_debug_group("render boids");
        {
            // render pass
            let mut rpass = encoder.begin_render_pass(&render_pass_descriptor);
            rpass.set_pipeline(&self.render_pipeline);
            // render the most recently written particles
            rpass.set_vertex_buffer(0, self.particle_buffers[self.frame_num % 2].slice(..));
            // the three instance-local vertices
            rpass.set_vertex_buffer(1, self.vertices_buffer.slice(..));
            rpass.draw(0..3, 0..self.particles);
        }
        encoder.pop_debug_group();
    }
}

fn f32_bytes(values: &[f32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_ne_bytes())
        .collect()
}

/// The WyRand generator `nanorand` uses, seeded like the original.
struct WyRand(u64);
impl WyRand {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0xa076_1d64_78bd_642f);
        let t = u128::from(self.0).wrapping_mul(u128::from(self.0 ^ 0xe703_7ed1_a0b4_28db));
        ((t >> 64) ^ t) as u64
    }
    /// Uniform in [0, 1).
    fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
}
