//! The window's surface: how it's configured, made again, given back while hidden, and
//! its next image asked for; the bytes its images hold.

use super::*;

impl WgpuRenderer {
    pub(crate) fn surface_config(
        surface: &wgpu::Surface<'_>,
        adapter: &wgpu::Adapter,
        size: PhysicalSize,
        options: &RenderOptions,
    ) -> Result<wgpu::SurfaceConfiguration, RenderError> {
        let mut config =
            surface.get_default_config(adapter, size.width.max(1), size.height.max(1)).ok_or_else(
                || RenderError::Surface("the surface isn't supported by the adapter".into()),
            )?;
        let caps = surface.get_capabilities(adapter);

        // Format without sRGB conversion: blending in sRGB space gives text closer
        // to the system's native rendering (no thinned glyphs).
        if let Some(format) = caps.formats.iter().find(|f| !f.is_srgb()) {
            config.format = *format;
        }
        // Transparent where the scene leaves it so (the window's corners, a floating layer),
        // when it can be: premultiplied (DX12, Vulkan); on Metal "post-multiplied" only makes
        // the layer not opaque — Core Animation reads its pixels premultiplied, as drawn.
        if options.transparent {
            if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied) {
                config.alpha_mode = wgpu::CompositeAlphaMode::PreMultiplied;
            } else if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::PostMultiplied) {
                config.alpha_mode = wgpu::CompositeAlphaMode::PostMultiplied;
            }
        }
        config.present_mode = if options.vsync {
            wgpu::PresentMode::Fifo
        } else {
            [wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate]
                .into_iter()
                .find(|m| caps.present_modes.contains(m))
                .unwrap_or(wgpu::PresentMode::Fifo)
        };
        // 1 = two images (one shown, one drawn): an editor's frames are few and quick. The
        // third one exists only while resizing (see `resize`): on Metal, with only two,
        // presenting in sync with the window blocks acquiring the next frame for up to ~1 s;
        // elsewhere a drag's frames come close. A window-sized image less the rest of the time.
        config.desired_maximum_frame_latency = 1;
        Ok(config)
    }

    pub(crate) fn reconfigure(&mut self) {
        if self.config.width > 0 && self.config.height > 0 {
            self.surface.configure(&self.ctx.device, &self.config);
        }
    }

    fn recreate_surface(&mut self) -> Result<(), RenderError> {
        self.replace_surface()?;
        self.reconfigure();
        Ok(())
    }

    /// A new surface for the window, not configured yet; the old one goes with all its images
    /// (on macOS its layer is taken out of the window first: left there, it would keep
    /// showing — and holding — its last image).
    fn replace_surface(&mut self) -> Result<(), RenderError> {
        let surface = self
            .ctx
            .instance
            .create_surface(Arc::clone(&self.target))
            .map_err(|e| RenderError::Surface(e.to_string()))?;
        #[cfg(target_os = "macos")]
        {
            macos::detach_layer(&self.surface);
            macos::configure_layer(&surface);
            macos::lower_layer(&surface);
            if self.transactional_until.is_some() {
                macos::set_transactional(&surface, true);
            }
            self.spare_released = false;
        }
        self.surface = surface;
        self.spare_held_since = None;
        Ok(())
    }

    /// A window hidden a while gives back its surface's images, `true` when it did (the next
    /// frame sets the surface up again). macOS: the layer goes, with every image it holds
    /// (a covered window is given no image to replace them with: wgpu skips it,
    /// gfx-rs/wgpu#8309); a new, empty one takes its place.
    #[cfg(target_os = "macos")]
    pub(crate) fn give_back_surface(&mut self) -> bool {
        match self.replace_surface() {
            Ok(()) => true,
            Err(e) => {
                tracing::debug!(error = %e, "the hidden window's surface stays");
                false
            }
        }
    }

    /// Elsewhere: the surface made one pixel large, its images given back once a 1×1 frame
    /// replaces the one on screen.
    #[cfg(not(target_os = "macos"))]
    pub(crate) fn give_back_surface(&mut self) -> bool {
        let tiny = wgpu::SurfaceConfiguration {
            width: 1,
            height: 1,
            desired_maximum_frame_latency: 1,
            ..self.config.clone()
        };
        self.surface.configure(&self.ctx.device, &tiny);
        if let wgpu::CurrentSurfaceTexture::Success(frame)
        | wgpu::CurrentSurfaceTexture::Suboptimal(frame) = self.surface.get_current_texture()
        {
            let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder = self
                .ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("hidden") });
            drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("hidden"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
            }));
            self.ctx.queue.submit([encoder.finish()]);
            self.ctx.queue.present(frame);
        }
        true
    }

    /// The surface's next image. On resize the surface may be "outdated": it's reconfigured
    /// and asked again immediately, in the same frame, so the window doesn't show a stretched
    /// image (resize jitter).
    /// The surface's image not on screen goes back too (macOS keeps a pool of two).
    pub(crate) fn release_spare_image(&mut self) {
        self.spare_held_since = None;
        #[cfg(target_os = "macos")]
        {
            let size = (self.config.width, self.config.height);
            macos::release_spare_drawables(&self.surface, size);
            self.spare_released = true;
        }
        // What was dropped goes when the GPU says it's done with it: no frame may follow.
        let _ = self.ctx.device.poll(wgpu::PollType::Poll);
    }

    pub(crate) fn acquire_frame(&mut self) -> Result<Acquired, RenderError> {
        let skipped = FrameStats { skipped: true, ..FrameStats::default() };
        let retry = FrameStats { retry: true, ..skipped };
        let mut attempts = 0;
        loop {
            attempts += 1;
            match self.surface.get_current_texture() {
                wgpu::CurrentSurfaceTexture::Success(frame) => {
                    return Ok(Acquired::Frame { frame, suboptimal: false });
                }
                // Draw anyway (otherwise the old image stays), then reconfigure.
                wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                    return Ok(Acquired::Frame { frame, suboptimal: true });
                }
                wgpu::CurrentSurfaceTexture::Outdated if attempts < 2 => {
                    tracing::debug!("surface outdated; reconfiguring");
                    self.reconfigure();
                }
                wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Timeout => {
                    tracing::debug!("frame unavailable; retrying");
                    return Ok(Acquired::Skipped(retry));
                }
                wgpu::CurrentSurfaceTexture::Occluded => {
                    tracing::debug!("window hidden; frame skipped");
                    return Ok(Acquired::Skipped(skipped));
                }
                wgpu::CurrentSurfaceTexture::Lost => {
                    tracing::debug!("surface lost; recreating it");
                    self.recreate_surface()?;
                    return Ok(Acquired::Skipped(retry));
                }
                wgpu::CurrentSurfaceTexture::Validation => {
                    return Err(RenderError::Surface("surface validation error".into()));
                }
            }
        }
    }
}

/// The result of asking the surface for its next image.
pub(crate) enum Acquired {
    /// An image to draw into; `suboptimal`: the surface is reconfigured after presenting it.
    Frame { frame: wgpu::SurfaceTexture, suboptimal: bool },
    /// No image this time: the frame is skipped with these stats.
    Skipped(FrameStats),
}

/// Whether `size` changes the surface set up by `config`.
pub(crate) fn resizes(config: &wgpu::SurfaceConfiguration, size: PhysicalSize) -> bool {
    (config.width, config.height) != (size.width, size.height)
}

/// The bytes a window's surface images hold (4 a pixel): one per frame in flight
/// (`latency`) plus the one on screen; only that one once the spare went (macOS); while
/// hidden none (macOS: the layer went with them) or one pixel's (elsewhere: made 1×1).
pub(crate) fn surface_bytes(
    size: PhysicalSize,
    latency: u32,
    spare_released: bool,
    hidden: bool,
) -> u64 {
    let images = if spare_released { 1 } else { u64::from(latency) + 1 };
    let (width, height) = match hidden {
        false => (size.width, size.height),
        true if cfg!(target_os = "macos") => return 0,
        true => (1, 1),
    };
    u64::from(width) * u64::from(height) * 4 * images
}
