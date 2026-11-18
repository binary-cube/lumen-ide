//! The backdrop blur on the GPU (`blur.wgsl`, `blur_composite.wgsl`): the kept frame, drawn
//! up to the layer that blurs, reduced into a small texture, blurred both ways in it (two
//! more passes, ping-ponging with a second one), then drawn back in the layer's rectangles.
//! The result stays while the layer keeps blurring the same thing (the second texture goes
//! once it's made); it goes with the blur. The pipelines are made at the first blur (a
//! window that never blurs has none).

use bytemuck::{Pod, Zeroable};
use render_api::{BlurLayer, BlurPlan, PixelRect};

/// What the reduce and blur passes read: see `BlurParams` in `blur.wgsl`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct BlurParams {
    p0: [f32; 4],
    p1: [f32; 4],
    p2: [f32; 4],
    p3: [f32; 4],
}

/// A blur drawn back: its rectangle, the blurred image's place, its opacity, its corners'
/// radius.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub(crate) struct BlurGpu {
    pub rect: [f32; 4],
    pub image: [f32; 4],
    pub opacity: f32,
    pub corner: f32,
    pub _pad: [f32; 2],
}

/// The blur's pipelines and what they bind (for the window's format).
#[derive(Debug)]
pub(crate) struct BlurPipelines {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    reduce: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    pub composite: wgpu::RenderPipeline,
}

impl BlurPipelines {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        globals: &wgpu::BindGroupLayout,
        image: &wgpu::BindGroupLayout,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blur"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blur.wgsl").into()),
        });
        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blur-composite"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(include_str!("common.wgsl"), include_str!("blur_composite.wgsl")).into(),
            ),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blur"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let pass_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blur"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let full = |entry: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pass_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_full"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let reduce = full("fs_reduce");
        let blur = full("fs_blur");
        let composite = crate::create_pipeline(
            device,
            &composite_shader,
            format,
            &[Some(globals), Some(image)],
            ("vs_composite", "fs_composite"),
            (
                &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32, 3 => Float32],
                std::mem::size_of::<BlurGpu>() as u64,
            ),
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        );
        Self { layout, sampler, reduce, blur, composite }
    }
}

/// A layer's blur on the GPU: its small texture (the result) and what binds it; while it's
/// being made, a second one to blur through.
#[derive(Debug)]
pub(crate) struct GpuBlur {
    pub of: BlurLayer,
    pub plan: BlurPlan,
    size: (u32, u32),
    format: wgpu::TextureFormat,
    a: wgpu::TextureView,
    params: [wgpu::Buffer; 3],
    /// The second texture and the passes' bindings: made with each blur, let go once it's
    /// made (half the memory while it stays).
    scratch: Option<Scratch>,
    /// The kept frame read by the reduce pass (made again with each blur: the kept frame's
    /// texture may be another).
    reduce: Option<wgpu::BindGroup>,
    /// The result, for drawing it back.
    pub composite: wgpu::BindGroup,
}

/// What making a blur needs beside its result.
#[derive(Debug)]
struct Scratch {
    b: wgpu::TextureView,
    blur_h: wgpu::BindGroup,
    blur_v: wgpu::BindGroup,
}

fn small_texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    label: &str,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

impl GpuBlur {
    /// The textures for `of` (reused from `old` when the same size).
    pub fn new(
        device: &wgpu::Device,
        pipelines: &BlurPipelines,
        image_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        of: BlurLayer,
        old: Option<Self>,
    ) -> Self {
        let plan = BlurPlan::for_radius(of.radius);
        let size = plan.reduced(of.bounds);
        if let Some(old) = old.filter(|o| o.size == size && o.plan == plan) {
            return Self { of, ..old };
        }
        let a = small_texture(device, format, size, "blur-a");
        let params = std::array::from_fn(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("blur-params"),
                size: std::mem::size_of::<BlurParams>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let composite = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blur-composite"),
            layout: image_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&a),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&pipelines.sampler),
                },
            ],
        });
        Self { of, plan, size, format, a, params, scratch: None, reduce: None, composite }
    }

    /// The bytes its textures take.
    pub fn bytes(&self) -> u64 {
        let one = u64::from(self.size.0) * u64::from(self.size.1) * 4;
        one * if self.scratch.is_some() { 2 } else { 1 }
    }

    /// Lets go of what only making it needed (it's made, and stays).
    pub fn release_scratch(&mut self) {
        self.scratch = None;
        self.reduce = None;
    }

    /// Readies a blur of `kept` (`kept_size`: its texture's): the passes' parameters
    /// written, the reduce pass bound to it.
    pub fn ready(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pipelines: &BlurPipelines,
        kept: &wgpu::TextureView,
        kept_size: (u32, u32),
    ) {
        let b: PixelRect = self.of.bounds;
        let plan = self.plan;
        let reduce = BlurParams {
            p0: [b.x0 as f32, b.y0 as f32, 1.0 / kept_size.0 as f32, 1.0 / kept_size.1 as f32],
            p1: [b.x0 as f32, b.y0 as f32, b.x1 as f32, b.y1 as f32],
            p2: [0.0, 0.0, plan.factor as f32, plan.sigma],
            p3: [plan.taps as f32, 0.0, 0.0, 0.0],
        };
        let texel = [1.0 / self.size.0 as f32, 1.0 / self.size.1 as f32];
        let along = |dir: [f32; 2]| BlurParams {
            p0: [0.0, 0.0, texel[0], texel[1]],
            p1: [0.0; 4],
            p2: [dir[0], dir[1], plan.factor as f32, plan.sigma],
            p3: [plan.taps as f32, 0.0, 0.0, 0.0],
        };
        for (buffer, params) in
            self.params.iter().zip([reduce, along([1.0, 0.0]), along([0.0, 1.0])])
        {
            queue.write_buffer(buffer, 0, bytemuck::bytes_of(&params));
        }
        self.reduce = Some(pass_group(device, pipelines, kept, &self.params[0]));
        if self.scratch.is_none() {
            let b = small_texture(device, self.format, self.size, "blur-b");
            let blur_h = pass_group(device, pipelines, &self.a, &self.params[1]);
            let blur_v = pass_group(device, pipelines, &b, &self.params[2]);
            self.scratch = Some(Scratch { b, blur_h, blur_v });
        }
    }

    /// The three passes: reduce into `a`, blur across into `b`, down into `a`.
    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, pipelines: &BlurPipelines) {
        let (Some(reduce), Some(scratch)) = (&self.reduce, &self.scratch) else { return };
        let steps = [
            (&self.a, &pipelines.reduce, reduce),
            (&scratch.b, &pipelines.blur, &scratch.blur_h),
            (&self.a, &pipelines.blur, &scratch.blur_v),
        ];
        for (target, pipeline, group) in steps {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("blur"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

fn pass_group(
    device: &wgpu::Device,
    pipelines: &BlurPipelines,
    view: &wgpu::TextureView,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("blur-pass"),
        layout: &pipelines.layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&pipelines.sampler),
            },
            wgpu::BindGroupEntry { binding: 2, resource: params.as_entire_binding() },
        ],
    })
}
