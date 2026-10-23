//! Drawing the terrain: the block texture array on the GPU, the two
//! pipelines, and one draw per visible sub-chunk mesh.
//!
//! **Two passes** (worth teaching). Opaque, cutout and overlay faces go
//! first, writing depth, so each pixel keeps only the nearest face. Blended
//! faces (water, ice, stained glass) must show what's behind them, so they
//! come second, without writing depth, and sorted back to front by
//! sub-chunk: blending a far face over a near one would look wrong. Sorting
//! whole sub-chunks rather than faces is M4's simplification.
//!
//! **Camera-relative positions** (worth teaching). Each draw gets its
//! sub-chunk's origin minus the camera, worked out in `f64` and only then
//! made `f32` (see `camera.rs`), through a small per-draw instance buffer.

use bytemuck::{Pod, Zeroable};
use glam::{DVec3, Vec3};

use super::{Gpu, SKY, camera::Camera, meshes::Meshes, textures::TextureArray};

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Six indices per quad: two triangles, 0-1-2 and 0-2-3, counter-clockwise.
const INDICES_PER_QUAD: u32 = 6;

/// The per-frame uniform: rotation and projection.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniform {
    view_projection: [[f32; 4]; 4],
}

/// The terrain's GPU state.
pub struct Terrain {
    opaque: wgpu::RenderPipeline,
    blend: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    frame: wgpu::Buffer,
    depth: wgpu::TextureView,
    depth_size: (u32, u32),
    /// The quad index pattern, shared by every mesh.
    indices: wgpu::Buffer,
    index_quads: u32,
    /// One camera-relative offset per draw.
    instances: wgpu::Buffer,
    instance_capacity: usize,
    /// The last frame's numbers, for the title and the benchmark.
    pub draws: u32,
    pub visible: u32,
}

impl Terrain {
    /// Uploads the texture array and builds the pipelines.
    #[must_use]
    pub fn new(gpu: &Gpu, textures: &TextureArray) -> Self {
        let device = &gpu.device;
        let texture_view = upload_textures(gpu, textures);
        // Nearest texels up close keep the pixel art sharp; between mip
        // levels the blend is linear, so the distance doesn't shimmer as
        // much. Clamping keeps a face's edge from sampling the other edge.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("block textures"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let frame = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame"),
            size: size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<FrameUniform>() as u64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: frame.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("terrain"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("terrain.wgsl"));
        let format = gpu.config.format;
        let opaque = pipeline(device, &pipeline_layout, &shader, format, false);
        let blend = pipeline(device, &pipeline_layout, &shader, format, true);
        let (depth, depth_size) = depth_view(gpu);
        let (indices, index_quads) = index_buffer(device, 4096);
        let instance_capacity = 1024;
        let instances = instance_buffer(device, instance_capacity);
        Self {
            opaque,
            blend,
            bind_group,
            frame,
            depth,
            depth_size,
            indices,
            index_quads,
            instances,
            instance_capacity,
            draws: 0,
            visible: 0,
        }
    }

    /// Clears to the sky and draws every mesh the camera can see.
    pub fn draw(
        &mut self,
        gpu: &Gpu,
        target: &wgpu::TextureView,
        encoder: &mut wgpu::CommandEncoder,
        camera: Option<&Camera>,
        meshes: &Meshes,
    ) {
        if self.depth_size != (gpu.config.width, gpu.config.height) {
            (self.depth, self.depth_size) = depth_view(gpu);
        }

        // Which meshes are in view, and where they are relative to the eye.
        let mut visible = Vec::new();
        if let Some(camera) = camera {
            let frustum = camera.frustum();
            for mesh in meshes.iter() {
                let origin = DVec3::new(
                    f64::from(mesh.origin[0]),
                    f64::from(mesh.origin[1]),
                    f64::from(mesh.origin[2]),
                );
                let offset = (origin - camera.position).as_vec3();
                if frustum.intersects_box(offset, offset + Vec3::splat(16.0)) {
                    visible.push((mesh, offset));
                }
            }
            let uniform = FrameUniform {
                view_projection: camera.view_projection().to_cols_array_2d(),
            };
            gpu.queue.write_buffer(&self.frame, 0, bytemuck::bytes_of(&uniform));
        }
        if visible.len() > self.instance_capacity {
            self.instance_capacity = visible.len().next_power_of_two();
            self.instances = instance_buffer(&gpu.device, self.instance_capacity);
        }
        if meshes.max_quads() > self.index_quads {
            (self.indices, self.index_quads) =
                index_buffer(&gpu.device, meshes.max_quads().next_power_of_two());
        }
        let offsets: Vec<[f32; 4]> = visible
            .iter()
            .map(|(_, offset)| [offset.x, offset.y, offset.z, 0.0])
            .collect();
        if !offsets.is_empty() {
            gpu.queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&offsets));
        }

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("terrain"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(SKY),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        self.draws = 0;
        self.visible = visible.len() as u32;
        if visible.is_empty() {
            return;
        }
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.set_vertex_buffer(1, self.instances.slice(..));

        pass.set_pipeline(&self.opaque);
        for (instance, (mesh, _)) in (0u32..).zip(&visible) {
            if mesh.opaque_quads > 0 {
                pass.set_vertex_buffer(0, mesh.buffer.slice(..));
                pass.draw_indexed(0..mesh.opaque_quads * INDICES_PER_QUAD, 0, instance..instance + 1);
                self.draws += 1;
            }
        }

        // Back to front by the distance to each sub-chunk's centre.
        let mut blended: Vec<(u32, f32)> = (0u32..)
            .zip(&visible)
            .filter(|(_, (mesh, _))| mesh.blend_quads > 0)
            .map(|(instance, (_, offset))| (instance, (*offset + Vec3::splat(8.0)).length_squared()))
            .collect();
        blended.sort_unstable_by(|a, b| b.1.total_cmp(&a.1));
        pass.set_pipeline(&self.blend);
        for (instance, _) in blended {
            let (mesh, _) = visible[instance as usize];
            pass.set_vertex_buffer(0, mesh.buffer.slice(..));
            // The blended vertices follow the opaque ones in the buffer.
            let base = (mesh.opaque_quads * 4) as i32;
            pass.draw_indexed(0..mesh.blend_quads * INDICES_PER_QUAD, base, instance..instance + 1);
            self.draws += 1;
        }
    }
}

/// Uploads every layer and mip level, and returns a 2D-array view.
fn upload_textures(gpu: &Gpu, textures: &TextureArray) -> wgpu::TextureView {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("block textures"),
        size: wgpu::Extent3d {
            width: textures.size,
            height: textures.size,
            depth_or_array_layers: textures.layers,
        },
        mip_level_count: textures.mips.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // The PNGs and TGAs hold sRGB colours: an sRGB format makes the GPU
        // turn them into linear light as it samples.
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, texels) in (0u32..).zip(&textures.mips) {
        let size = (textures.size >> level).max(1);
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * size),
                rows_per_image: Some(size),
            },
            wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: textures.layers,
            },
        );
    }
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

/// The opaque pipeline writes depth; the blend pipeline blends and doesn't.
/// Both cull the back of every quad: counter-clockwise is the front.
fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    blended: bool,
) -> wgpu::RenderPipeline {
    let vertex_attributes = wgpu::vertex_attr_array![0 => Uint16x4, 1 => Uint8x4, 2 => Unorm8x4];
    let instance_attributes = wgpu::vertex_attr_array![3 => Float32x4];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(if blended { "terrain, blended" } else { "terrain" }),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vertex_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[
                Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<super::mesher::Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &vertex_attributes,
                }),
                Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<[f32; 4]>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &instance_attributes,
                }),
            ],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(!blended),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fragment_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: blended.then_some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn depth_view(gpu: &Gpu) -> (wgpu::TextureView, (u32, u32)) {
    let size = (gpu.config.width.max(1), gpu.config.height.max(1));
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    (texture.create_view(&wgpu::TextureViewDescriptor::default()), size)
}

/// The index pattern for `quads` quads: 0 1 2, 0 2 3, then the same plus 4,
/// and so on. One buffer serves every mesh, since each mesh starts its
/// vertices at 0 (or at `base_vertex` for its blended part).
fn index_buffer(device: &wgpu::Device, quads: u32) -> (wgpu::Buffer, u32) {
    use wgpu::util::DeviceExt;
    let indices: Vec<u32> = (0..quads)
        .flat_map(|quad| {
            let v = quad * 4;
            [v, v + 1, v + 2, v, v + 2, v + 3]
        })
        .collect();
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("quad indices"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    (buffer, quads)
}

fn instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sub-chunk offsets"),
        size: (capacity * size_of::<[f32; 4]>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    /// The shader is compiled by the GPU driver's wgpu at startup; this
    /// catches what naga, wgpu's own compiler, would reject, without a GPU.
    #[test]
    fn the_shader_parses_and_validates() {
        let source = include_str!("terrain.wgsl");
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|err| panic!("{}", err.emit_to_string(source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|err| panic!("{err:?}"));
    }
}
