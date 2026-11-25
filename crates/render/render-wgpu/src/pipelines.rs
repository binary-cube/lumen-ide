//! What draws into a surface: the pipelines of one format, made once for every window
//! with it, and the atlas's bind group.

use super::*;

/// What draws into a surface of one format: the same for every window with it.
#[derive(Debug)]
pub(crate) struct Pipelines {
    /// Every layer's rectangles and glyphs, in one draw ([`PrimGpu`]).
    pub(crate) prim: wgpu::RenderPipeline,
    /// Writes the background over a partial frame's areas.
    pub(crate) fill: wgpu::RenderPipeline,
    pub(crate) corner: wgpu::RenderPipeline,
    pub(crate) image: wgpu::RenderPipeline,
    /// Made when the first blur is drawn.
    pub(crate) blur: std::sync::OnceLock<Arc<BlurPipelines>>,
}

impl Pipelines {
    pub(crate) fn new(ctx: &GpuContext, format: wgpu::TextureFormat) -> Self {
        let device = &ctx.device;
        let quad_attributes = &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32x2];
        let quad_layout = (&quad_attributes[..], std::mem::size_of::<QuadGpu>() as u64);
        // Pipelines depend on the surface format (of each window).
        let fill_pipeline = create_pipeline(
            device,
            &ctx.shader,
            format,
            &[Some(&ctx.globals_layout)],
            ("vs_quad", "fs_quad"),
            quad_layout,
            wgpu::BlendState::REPLACE,
        );
        let prim_pipeline = create_pipeline(
            device,
            &ctx.shader,
            format,
            &[Some(&ctx.globals_layout), Some(&ctx.atlas_layout)],
            ("vs_prim", "fs_prim"),
            (
                &wgpu::vertex_attr_array![
                    0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Uint16x4,
                    4 => Float32x4, 5 => Uint32
                ],
                std::mem::size_of::<PrimGpu>() as u64,
            ),
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        );
        // The window's corners: what's drawn there is erased (`dst * (1 - alpha)`).
        let erase = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        };
        let corner_pipeline = create_pipeline(
            device,
            &ctx.shader,
            format,
            &[Some(&ctx.globals_layout)],
            ("vs_quad", "fs_window_corner"),
            quad_layout,
            wgpu::BlendState { color: erase, alpha: erase },
        );
        let image_pipeline = create_pipeline(
            device,
            &ctx.image_shader,
            format,
            &[Some(&ctx.globals_layout), Some(&ctx.image_layout)],
            ("vs_image", "fs_image"),
            (
                &wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x2, 3 => Uint32x2],
                std::mem::size_of::<ImageGpu>() as u64,
            ),
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        );
        Self {
            prim: prim_pipeline,
            fill: fill_pipeline,
            corner: corner_pipeline,
            image: image_pipeline,
            blur: std::sync::OnceLock::new(),
        }
    }
}

/// The bind group exposing the atlas textures to the glyph shader.
pub(crate) fn atlas_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    atlas: &GlyphAtlas,
) -> wgpu::BindGroup {
    let (mask_view, color_view) = (atlas.mask_view(), atlas.color_view());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("glyph-atlas"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&mask_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&color_view),
            },
        ],
    })
}

pub(crate) fn create_pipeline(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    bind_group_layouts: &[Option<&wgpu::BindGroupLayout>],
    (vertex_entry, fragment_entry): (&str, &str),
    (attributes, stride): (&[wgpu::VertexAttribute], u64),
    blend: wgpu::BlendState,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(vertex_entry),
        bind_group_layouts,
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(vertex_entry),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vertex_entry),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: stride,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes,
            })],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment_entry),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}
