//! WGPU-based 3D preview renderer

use super::camera::OrbitCamera;
use super::mesh::PreviewVertex;
use wgpu::*;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct PreviewUniforms {
    view_proj: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    time: f32,
    _padding: [f32; 3],
}

const VERTEX_SHADER_SRC: &str = r#"
struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) world_pos: vec3<f32>,
};

struct Uniforms {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    time: f32,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

@vertex
fn vertex_main(input: VertexInput) -> VertexOutput {
    let world_pos = (uniforms.model * vec4(input.position, 1.0)).xyz;
    let normal_mat = transpose(inverse(mat3x3(uniforms.model)));

    var output: VertexOutput;
    output.position = uniforms.view_proj * vec4(world_pos, 1.0);
    output.uv = input.uv;
    output.normal = normalize(normal_mat * input.normal);
    output.world_pos = world_pos;
    return output;
}
"#;

const FRAGMENT_SHADER_WRAPPER_PREFIX: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) world_pos: vec3<f32>,
};

struct Uniforms {
    view_proj: mat4x4<f32>,
    model: mat4x4<f32>,
    time: f32,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

@fragment
fn fragment_main(input: VertexOutput) -> @location(0) vec4<f32> {
    // User shader code follows:
"#;

const FRAGMENT_SHADER_WRAPPER_SUFFIX: &str = r#"
}
"#;

pub struct PreviewRenderer {
    pub device: Option<Device>,
    pub queue: Option<Queue>,
    pipeline: Option<RenderPipeline>,
    uniform_buffer: Option<Buffer>,
    bind_group: Option<BindGroup>,
    pub mesh_vertex_buffer: Option<Buffer>,
    pub mesh_index_buffer: Option<Buffer>,
    pub mesh_index_count: u32,
    pub camera: OrbitCamera,
    shader_module: Option<ShaderModule>,
    surface_config: Option<SurfaceConfiguration>,
    needs_recompile: bool,
    needs_upload: bool,
    start_time: std::time::Instant,
}

impl PreviewRenderer {
    pub fn new() -> Self {
        Self {
            device: None,
            queue: None,
            pipeline: None,
            uniform_buffer: None,
            bind_group: None,
            mesh_vertex_buffer: None,
            mesh_index_buffer: None,
            mesh_index_count: 0,
            camera: OrbitCamera {
                yaw: 0.0,
                pitch: 0.4,
                distance: 3.0,
                target: [0.0, 0.0, 0.0],
                fov_y: 45.0_f32.to_radians(),
                aspect: 1.0,
                near: 0.1,
                far: 100.0,
            },
            shader_module: None,
            surface_config: None,
            needs_recompile: true,
            needs_upload: true,
            start_time: std::time::Instant::now(),
        }
    }

    pub fn initialize(
        &mut self,
        device: &Device,
        queue: &Queue,
        config: &SurfaceConfiguration,
    ) {
        self.device = Some(device.clone());
        self.queue = Some(queue.clone());
        self.surface_config = Some(config.clone());
        self.camera.aspect = config.width as f32 / config.height as f32;

        let uniform_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("preview uniforms"),
            size: std::mem::size_of::<PreviewUniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.uniform_buffer = Some(uniform_buffer);

        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("preview bind group layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("preview pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("preview bind group"),
            layout: &bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: self.uniform_buffer.as_ref().unwrap().as_entire_binding(),
            }],
        });
        self.bind_group = Some(bind_group);
    }

    pub fn update_shader(&mut self, wgsl_source: &str) {
        let wrapper_source = format!(
            "{}\n{}\n{}",
            FRAGMENT_SHADER_WRAPPER_PREFIX, wgsl_source, FRAGMENT_SHADER_WRAPPER_SUFFIX
        );

        if let Some(device) = &self.device {
            let vs_module = device.create_shader_module(ShaderModuleDescriptor {
                label: Some("preview vertex shader"),
                source: ShaderSource::Wgsl(VERTEX_SHADER_SRC.into()),
            });

            let fs_module = match device.create_shader_module(ShaderModuleDescriptor {
                label: Some("preview fragment shader"),
                source: ShaderSource::Wgsl(wrapper_source.into()),
            }) {
                module => module,
            };

            let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
                label: Some("preview pipeline"),
                layout: None,
                vertex: VertexState {
                    module: &vs_module,
                    entry_point: Some("vertex_main"),
                    compilation_options: PipelineCompilationOptions::default(),
                    buffers: &[VertexBufferLayout {
                        array_stride: std::mem::size_of::<PreviewVertex>() as u64,
                        step_mode: VertexStepMode::Vertex,
                        attributes: &[
                            VertexAttribute {
                                format: VertexFormat::Float32x3,
                                offset: 0,
                                shader_location: 0,
                            },
                            VertexAttribute {
                                format: VertexFormat::Float32x3,
                                offset: 12,
                                shader_location: 1,
                            },
                            VertexAttribute {
                                format: VertexFormat::Float32x2,
                                offset: 24,
                                shader_location: 2,
                            },
                        ],
                    }],
                },
                fragment: Some(FragmentState {
                    module: &fs_module,
                    entry_point: Some("fragment_main"),
                    compilation_options: PipelineCompilationOptions::default(),
                    targets: &[Some(ColorTargetState {
                        format: self
                            .surface_config
                            .as_ref()
                            .map(|c| c.format)
                            .unwrap_or(TextureFormat::Bgra8Unorm),
                        blend: Some(BlendState::ALPHA_BLENDING),
                        write_mask: ColorWrites::ALL,
                    })],
                }),
                primitive: PrimitiveState {
                    topology: PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: FrontFace::Ccw,
                    cull_mode: Some(Face::Back),
                    unclipped_depth: false,
                    polygon_mode: PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil: None,
                multisample: MultisampleState::default(),
                multiview: None,
                cache: None,
            });
            self.pipeline = Some(pipeline);
        }
        self.needs_recompile = false;
    }

    pub fn update_mesh(
        &mut self,
        vertices: &[PreviewVertex],
        indices: &[u32],
        index_count: u32,
    ) {
        if let Some(device) = &self.device {
            let vertex_buffer = device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("preview vertex buffer"),
                usage: BufferUsages::VERTEX,
                contents: bytemuck::cast_slice(vertices),
            });
            let index_buffer = device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("preview index buffer"),
                usage: BufferUsages::INDEX,
                contents: bytemuck::cast_slice(indices),
            });
            self.mesh_vertex_buffer = Some(vertex_buffer);
            self.mesh_index_buffer = Some(index_buffer);
            self.mesh_index_count = index_count;
        }
        self.needs_upload = false;
    }

    pub fn update_camera(&mut self, aspect: f32) {
        self.camera.aspect = aspect;
    }

    pub fn render(&self, output: &TextureView) {
        let Some(device) = &self.device else { return };
        let Some(queue) = &self.queue else { return };
        let Some(pipeline) = &self.pipeline else { return };
        let Some(uniform_buffer) = &self.uniform_buffer else { return };
        let Some(bind_group) = &self.bind_group else { return };
        let Some(vertex_buffer) = &self.mesh_vertex_buffer else { return };
        let Some(index_buffer) = &self.mesh_index_buffer else { return };
        if self.mesh_index_count == 0 {
            return;
        }

        let elapsed = self.start_time.elapsed().as_secs_f32();
        let model: [[f32; 4]; 4] = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];

        let view = self.camera.view_matrix();
        let proj = self.camera.projection_matrix();

        let mut view_proj = [[0.0_f32; 4]; 4];
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    view_proj[i][j] += proj[i][k] * view[k][j];
                }
            }
        }

        let uniforms = PreviewUniforms {
            view_proj,
            model,
            time: elapsed,
            _padding: [0.0; 3],
        };

        queue.write_buffer(uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("preview encoder"),
        });

        {
            let mut rpass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("preview render pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: output,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color {
                            r: 0.1,
                            g: 0.1,
                            b: 0.1,
                            a: 1.0,
                        }),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            rpass.set_pipeline(pipeline);
            rpass.set_bind_group(0, bind_group, &[]);
            rpass.set_vertex_buffer(0, vertex_buffer.slice(..));
            rpass.set_index_buffer(index_buffer.slice(..), IndexFormat::Uint32);
            rpass.draw_indexed(0..self.mesh_index_count, 0, 0..1);
        }

        queue.submit(Some(encoder.finish()));
    }

    pub fn resize(&mut self, width: u32, height: u32, config: &SurfaceConfiguration) {
        self.surface_config = Some(config.clone());
        self.camera.aspect = width as f32 / height as f32;
    }
}
