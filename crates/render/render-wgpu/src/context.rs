//! The shared GPU context made: the instance, the adapter and device chosen, the atlas and
//! the layouts; what it keeps of the images and the atlas over frames.

use super::*;

/// How the GPU's memory is allocated. `MemoryUsage`: the DX12 / Vulkan allocator grows its
/// blocks from 8 MB (device) and 4 MB (upload) as needed; `Performance` reserves blocks of
/// 128 MB and 64 MB at once — memory an editor's few textures (the glyph atlas, images, the
/// window's surfaces) never fill, and which, on an integrated GPU, is the computer's RAM.
/// (Metal ignores it.) `EDITOR_GPU_MEMORY=performance` goes back to it, to compare.
pub(super) fn memory_hints() -> wgpu::MemoryHints {
    match std::env::var("EDITOR_GPU_MEMORY").as_deref() {
        Ok("performance") => wgpu::MemoryHints::Performance,
        _ => wgpu::MemoryHints::MemoryUsage,
    }
}

impl GpuContext {
    pub(super) fn state(&self) -> MutexGuard<'_, SharedState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The existing context (if the surface works on its adapter), the one made ahead (the
    /// first window; with the pipelines made for it, if they fit) or a new one.
    pub(super) async fn for_target(
        target: &Arc<dyn RenderTarget>,
        options: &RenderOptions,
    ) -> Result<(Arc<Self>, wgpu::Surface<'static>, Option<Drawer>), RenderError> {
        let existing = CONTEXT.lock().ok().and_then(|c| c.as_ref().and_then(Weak::upgrade));
        if let Some(ctx) = existing
            && let Ok(surface) = ctx.instance.create_surface(Arc::clone(target))
            && ctx.adapter.is_surface_supported(&surface)
        {
            tracing::info!("new window on the existing GPU");
            return Ok((ctx, surface, None));
        }
        if let Some(prepared) = prepared::take()
            && let Ok(surface) = prepared.ctx.instance.create_surface(Arc::clone(target))
            && prepared.ctx.adapter.is_surface_supported(&surface)
        {
            tracing::info!("the window on the GPU made ahead");
            if let Ok(mut slot) = CONTEXT.lock() {
                *slot = Some(Arc::downgrade(&prepared.ctx));
            }
            return Ok((prepared.ctx, surface, prepared.drawer));
        }
        let (instance, surface, adapter) = WgpuRenderer::select_adapter(target, options).await?;
        let ctx = Arc::new(Self::new(instance, adapter).await?);
        if let Ok(mut slot) = CONTEXT.lock() {
            *slot = Some(Arc::downgrade(&ctx));
        }
        Ok((ctx, surface, None))
    }

    pub(super) async fn new(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
    ) -> Result<Self, RenderError> {
        let adapter_info = adapter.get_info();
        tracing::info!(backend = ?adapter_info.backend, adapter = %adapter_info.name, "GPU selected");
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("quads"),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: memory_hints(),
                ..Default::default()
            })
            .await
            .map_err(|e| RenderError::Device(e.to_string()))?;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shaders"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(include_str!("common.wgsl"), include_str!("shaders.wgsl")).into(),
            ),
        });
        let image_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image-shader"),
            source: wgpu::ShaderSource::Wgsl(
                concat!(include_str!("common.wgsl"), include_str!("image.wgsl")).into(),
            ),
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                // The viewport for the vertices, the emphasis for the fragments.
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let atlas = GlyphAtlas::new(&device, ATLAS_START);
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let atlas_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glyph-atlas"),
            entries: &[texture_entry(0), texture_entry(1)],
        });
        let atlas_bind_group = atlas_bind_group(&device, &atlas_layout, &atlas);
        let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image"),
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
            ],
        });
        let image_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("image"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Ok(Self {
            instance,
            adapter,
            adapter_info,
            device,
            queue,
            globals_layout,
            atlas_layout,
            image_layout,
            image_sampler,
            shader,
            image_shader,
            pipelines: Mutex::default(),
            state: Mutex::new(SharedState {
                atlas,
                atlas_bind_group,
                textures: HashMap::new(),
                frame: 0,
                shown: HashMap::new(),
                sources: crate::import::Sources::default(),
            }),
        })
    }

    /// Uploads the image to the GPU (only once per `id`, for all windows).
    /// `false` if that's not possible.
    pub(super) fn upload_image(
        &self,
        state: &mut SharedState,
        image: &render_api::ImageData,
    ) -> bool {
        if let Some(gpu) = state.textures.get_mut(&image.id) {
            gpu.last_frame = state.frame;
            gpu.last_used = std::time::Instant::now();
            // A live image's texture made but not filled yet: nothing to draw.
            return gpu.filled != crate::instances::Filled::Empty;
        }
        // A live image has no pixels: it's drawn once its first frame is in.
        if image.is_live() {
            return false;
        }
        let max = self.device.limits().max_texture_dimension_2d;
        // A mask: one byte of coverage per pixel.
        let bpp = image.bytes_per_pixel() as u32;
        let expected = image.width as usize * image.height as usize * bpp as usize;
        if image.width == 0 || image.height == 0 || image.width > max || image.height > max {
            return false;
        }
        let size =
            wgpu::Extent3d { width: image.width, height: image.height, depth_or_array_layers: 1 };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: if bpp == 1 {
                wgpu::TextureFormat::R8Unorm
            } else {
                wgpu::TextureFormat::Rgba8Unorm
            },
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        // The pixels may be gone (a `gpu_only` image whose texture was freed): its owner
        // decodes it again.
        let written = image.with_pixels(|pixels| {
            if pixels.len() < expected {
                return false;
            }
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &pixels[..expected],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(image.width * bpp),
                    rows_per_image: None,
                },
                size,
            );
            true
        });
        if written != Some(true) {
            return false;
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image"),
            layout: &self.image_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.image_sampler),
                },
            ],
        });
        state.textures.insert(
            image.id,
            GpuImage {
                bind_group,
                bytes: expected as u64,
                last_frame: state.frame,
                last_used: std::time::Instant::now(),
                image: image.clone(),
                live: None,
                filled: crate::instances::Filled::Whole,
            },
        );
        image.uploaded();
        true
    }
}
