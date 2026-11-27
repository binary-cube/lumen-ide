//! The window's renderer as the app sees it (`Renderer`): resizing, drawing a scene,
//! captures, what it holds and lets go when hidden or idle.

use super::*;

impl Renderer for WgpuRenderer {
    fn description(&self) -> String {
        let info = &self.ctx.adapter_info;
        format!("wgpu / {:?} / {}", info.backend, info.name)
    }

    fn purge_caches(&mut self) {
        // The GPU context is shared by windows: clearing affects all of them.
        let mut state = self.ctx.state();
        // Back to the starting size (it grows again only if needed).
        state.atlas.grow(&self.ctx.device, ATLAS_START);
        state.atlas_bind_group =
            atlas_bind_group(&self.ctx.device, &self.ctx.atlas_layout, &state.atlas);
        // A live image (a page's) keeps its texture as it would unpurged: it has no pixels
        // to upload again, only the frames its engine sends when something changes.
        let frame = state.frame;
        let shown = state.shown_ids();
        state.retain_textures(|id, t| t.image.is_live() && t.kept(shown.contains(&id), frame));
        // The text system reissues glyph keys with it: every window's next frame is whole.
        render_api::glyph_keys_reset();
    }

    fn frame_importer(&self) -> Option<std::sync::Arc<dyn render_api::FrameImporter>> {
        // Every GPU: frames as textures where they can be opened, as pixels elsewhere.
        Some(std::sync::Arc::new(crate::import::Importer::new(std::sync::Arc::clone(&self.ctx))))
    }

    fn gpu_memory(&self) -> Option<render_api::GpuMemory> {
        // Shared by the windows (one GPU context): the atlas (R8 mask + RGBA8 color) and the
        // images' textures. This window's: its instance buffers, surface images
        // ([`surface_bytes`]), the kept frame and the blurs.
        let state = self.ctx.state();
        let side = u64::from(state.atlas.size());
        let color_side = (side / 4).max(256);
        let shared = side * side
            + color_side * color_side * 4
            + state.textures.values().map(|t| t.bytes).sum::<u64>();
        drop(state);
        let drawer = &self.drawer;
        let buffers = drawer.quad_buffer.buffer.size()
            + drawer.prim_buffer.buffer.size()
            + drawer.image_buffer.buffer.size();
        #[cfg(target_os = "macos")]
        let spare_released = self.spare_released;
        #[cfg(not(target_os = "macos"))]
        let spare_released = false;
        let surface = surface_bytes(
            PhysicalSize::new(self.config.width, self.config.height),
            self.config.desired_maximum_frame_latency,
            spare_released,
            self.hidden_released,
        );
        let window = buffers + surface + drawer.kept_bytes() + drawer.blur_bytes();
        Some(render_api::GpuMemory { shared, window })
    }

    fn set_visible(&mut self, visible: bool) {
        if !visible {
            self.release_idle();
        } else if std::mem::take(&mut self.hidden_released) {
            self.reconfigure();
        }
    }

    fn release_hidden(&mut self) {
        if self.hidden_released || self.config.width == 0 || self.config.height == 0 {
            return;
        }
        self.release_idle();
        // The frame on screen goes with the surface: the next one is drawn whole.
        self.drawer.shown = None;
        if self.give_back_surface() {
            self.hidden_released = true;
        }
        // What was given back goes when the GPU is done with it.
        let _ = self.ctx.device.poll(wgpu::PollType::Poll);
    }

    fn release_idle(&mut self) {
        // A capture's copy of the kept frame was submitted already: it keeps what it reads.
        self.drawer.release_kept();
        self.drawer.release_blurs();
        self.release_spare_image();
        drawer::shrink_atlas(&self.ctx);
        // No frame may come for long (the app in the background): the images on screen
        // nowhere go now if they're old enough, not at the next frame.
        self.ctx.state().release_unshown();
    }

    fn show_with_window(&mut self) {
        #[cfg(target_os = "macos")]
        {
            self.with_window = WithWindow::Asked;
        }
    }

    fn images_released(&self) -> bool {
        // The spare images' release leaves the layer as it was (its size set back at once):
        // only the hidden window's empty surface waits for a frame.
        self.hidden_released
    }

    fn kept_frame_release_at(&self) -> Option<std::time::Instant> {
        let quiet = self.activity.quiet_at()?;
        let kept = self.drawer.kept.as_ref().map(|_| quiet);
        let spare = self.spare_held_since.map(|shown| quiet.max(shown + SPARE_AFTER));
        // An image off every screen (a tab left) is freed when it's old enough even if no
        // frame comes then (frames free it themselves while drawing).
        let images = self.ctx.state().unshown_release_at().map(|at| quiet.max(at));
        // Only ahead: one that can't go yet (a blur reads it) waits for the next frame.
        let now = std::time::Instant::now();
        kept.into_iter().chain(spare).chain(images).filter(|at| *at > now).min()
    }

    fn capture_waits_for_frame(&self) -> bool {
        self.pending_capture.is_some()
    }

    fn backdrop_blur(&self) -> bool {
        // The blur reads the kept frame.
        self.partial
    }

    fn transparent(&self) -> bool {
        self.drawer.transparent
    }

    fn capture(
        &mut self,
        max_width: u32,
        done: Box<dyn FnOnce(render_api::Captured) + Send>,
    ) -> bool {
        // A quiet window keeps no frame: the next one drawn is kept for this.
        if self.drawer.kept.is_none() {
            self.pending_capture = Some(PendingCapture { max_width, done });
            return true;
        }
        // One at a time: a frame's read-back buffer is as large as the window.
        if self.capturing.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return false;
        }
        // Copied now (the next frame may change the kept one); waited for and made small in
        // the background, from where it's mapped.
        let Some((buffer, layout)) = self.drawer.copy_kept(&self.ctx) else {
            self.capturing.store(false, std::sync::atomic::Ordering::Release);
            return false;
        };
        let (ctx, bgra, capturing) =
            (Arc::clone(&self.ctx), self.drawer.bgra(), Arc::clone(&self.capturing));
        kernel::work::run("frame-capture", move || {
            let picture = with_mapped(&ctx, &buffer, |data| {
                render_api::shrink(data, layout, bgra, max_width)
            });
            // The buffer goes before the next capture may make another.
            drop(buffer);
            capturing.store(false, std::sync::atomic::Ordering::Release);
            if let Some(picture) = picture {
                done(picture);
            }
        });
        true
    }

    fn resize(&mut self, size: PhysicalSize) {
        if self.ctx.lost.load(std::sync::atomic::Ordering::Acquire) {
            return;
        }
        // The size it has already (the system tells it again as the window opens): nothing to
        // set up, and no third image for a resize that isn't one.
        if !resizes(&self.config, size) {
            return;
        }
        self.config.width = size.width;
        self.config.height = size.height;
        if size.width == 0 || size.height == 0 {
            // Minimized.
            self.drawer.release_kept();
        }
        self.config.desired_maximum_frame_latency = 2;
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.reconfigure())).is_err() {
            self.ctx.lost.store(true, std::sync::atomic::Ordering::Release);
            tracing::error!("the renderer panicked during resize");
            return;
        }
        #[cfg(target_os = "macos")]
        {
            if self.transactional_until.is_none() {
                macos::set_transactional(&self.surface, true);
            }
            self.transactional_until =
                Some(std::time::Instant::now() + std::time::Duration::from_millis(300));
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.resized_until = Some(std::time::Instant::now() + crate::instances::RESIZE_SETTLE);
        }
    }

    fn render(
        &mut self,
        scene: &Scene,
        rasterizer: &mut dyn GlyphRasterizer,
    ) -> Result<FrameStats, RenderError> {
        if self.ctx.lost.load(std::sync::atomic::Ordering::Acquire) {
            return Err(RenderError::DeviceLost);
        }
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.do_render(scene, rasterizer)
        })) {
            Ok(result) => result,
            Err(panic) => {
                self.ctx.lost.store(true, std::sync::atomic::Ordering::Release);
                let message = crate::panic_message(&panic);
                tracing::error!(message, "the renderer panicked");
                Err(RenderError::DeviceLost)
            }
        }
    }
}

impl WgpuRenderer {
    fn do_render(
        &mut self,
        scene: &Scene,
        rasterizer: &mut dyn GlyphRasterizer,
    ) -> Result<FrameStats, RenderError> {
        // The resize is over: frames no longer wait for the window's transaction.
        #[cfg(target_os = "macos")]
        if self.transactional_until.is_some_and(|t| std::time::Instant::now() >= t) {
            self.transactional_until = None;
            macos::set_transactional(&self.surface, false);
            // Back to two drawables (the third is needed only while resizing).
            self.config.desired_maximum_frame_latency = 1;
            self.reconfigure();
        }
        if self.config.width == 0 || self.config.height == 0 {
            self.done_with_window();
            return Ok(FrameStats { skipped: true, ..FrameStats::default() });
        }
        // Drawn while hidden (or shown without being told): the window's size again.
        if std::mem::take(&mut self.hidden_released) {
            self.reconfigure();
        }
        let size = PhysicalSize::new(self.config.width, self.config.height);
        // While resizing, every frame is whole and of another size: no kept frame (a texture
        // made per step), straight into the surface's image.
        #[cfg(target_os = "macos")]
        let resizing = self.transactional_until.is_some();
        #[cfg(not(target_os = "macos"))]
        let resizing = {
            if self.resized_until.is_some_and(|t| std::time::Instant::now() >= t) {
                self.resized_until = None;
                // Back to two images (the third is needed only while resizing).
                self.config.desired_maximum_frame_latency = 1;
                self.reconfigure();
            }
            self.resized_until.is_some()
        };
        let now = std::time::Instant::now();
        self.activity.frame(now);
        // A window standing still gives its kept frame back (as large as the window): frames
        // are few, drawing one whole costs little. The blurs and a capture read it.
        let blurs = scene.layers().iter().any(|l| !l.blurs.is_empty());
        let keep = self.partial
            && !resizing
            && (!self.activity.quiet(now) || blurs || self.pending_capture.is_some());
        if !keep {
            let had_one = self.drawer.kept.is_some();
            self.drawer.release_kept();
            if had_one && !resizing {
                self.release_spare_image();
            }
        }
        if self.partial && !resizing {
            self.drawer.compute_damage(&self.ctx, scene, size, rasterizer, keep);
            // Stood still long enough (a caret's blink never does): the spare image goes too.
            if !keep
                && self.drawer.damage.is_empty()
                && self.spare_held_since.is_some_and(|shown| now >= shown + SPARE_AFTER)
            {
                self.release_spare_image();
            }
            // Nothing changed: what's on screen is already this frame.
            if self.drawer.damage.is_empty() {
                self.done_with_window();
                let state = self.ctx.state();
                return Ok(FrameStats {
                    quads: scene.quad_count(),
                    glyphs: scene.glyph_count(),
                    cached_glyphs: state.atlas.len(),
                    ..FrameStats::default()
                });
            }
        }
        self.drawer.shown = None;
        // Its spare images given back: its surface set up again before drawing.
        #[cfg(target_os = "macos")]
        if std::mem::take(&mut self.spare_released) {
            self.reconfigure();
        }
        // Shown with the window's transaction when asked (it comes back), for this frame
        // only: one left on (the frame skipped) goes off here.
        #[cfg(target_os = "macos")]
        {
            let resizing = self.transactional_until.is_some();
            match self.with_window {
                WithWindow::Asked => {
                    self.with_window = WithWindow::On;
                    if !resizing {
                        macos::set_transactional(&self.surface, true);
                    }
                }
                WithWindow::On => {
                    self.with_window = WithWindow::Off;
                    if !resizing {
                        macos::set_transactional(&self.surface, false);
                    }
                }
                WithWindow::Off => {}
            }
        }
        let (frame, suboptimal) = match self.acquire_frame()? {
            Acquired::Frame { frame, suboptimal } => (frame, suboptimal),
            Acquired::Skipped(stats) => {
                if !stats.retry {
                    // Hidden: the kept frame's memory goes back meanwhile.
                    self.drawer.release_kept();
                }
                // A frame to be shown with the window: retried, it still is (the one that
                // reaches the screen); hidden, nothing waits for the window any more.
                #[cfg(target_os = "macos")]
                if self.with_window == WithWindow::On {
                    self.with_window =
                        if stats.retry { WithWindow::Asked } else { WithWindow::Off };
                    if !stats.retry && self.transactional_until.is_none() {
                        macos::set_transactional(&self.surface, false);
                    }
                }
                return Ok(stats);
            }
        };

        let surface_view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        // Glyph rasterization and atlas uploads only once a frame can actually be drawn
        // (a hidden window or a timeout would waste that work).
        let ctx = Arc::clone(&self.ctx);
        // The whole frame changes (an animation over the window): straight on screen, no
        // copy (measured: the copy and its pass were a fifth of the frame's CPU); the frame
        // kept catches up when a partial one comes. Not with blurs or a capture: they read it.
        let whole =
            keep && self.drawer.damage.is_full() && !blurs && self.pending_capture.is_none();
        let (draw_calls, drawn_pixels) = if whole {
            self.drawer.draw_straight(&ctx, &mut encoder, scene, size, rasterizer, &surface_view)
        } else if keep {
            let drawn = self.drawer.draw_kept(&ctx, &mut encoder, scene, size, rasterizer);
            if let Some(kept) = &self.drawer.kept {
                encoder.copy_texture_to_texture(
                    kept.texture.as_image_copy(),
                    frame.texture.as_image_copy(),
                    wgpu::Extent3d {
                        width: size.width,
                        height: size.height,
                        depth_or_array_layers: 1,
                    },
                );
            }
            drawn
        } else {
            self.drawer.draw(&ctx, &mut encoder, scene, size, rasterizer, Some(&surface_view))
        };
        self.ctx.queue.submit([encoder.finish()]);
        self.ctx.queue.present(frame);
        // Shown: frames go back to not waiting for the window (unless it's being resized).
        #[cfg(target_os = "macos")]
        if self.with_window == WithWindow::On {
            self.with_window = WithWindow::Off;
            if self.transactional_until.is_none() {
                macos::set_transactional(&self.surface, false);
            }
        }
        self.drawer.shown = Some(size);
        self.spare_held_since = cfg!(target_os = "macos").then(std::time::Instant::now);
        if suboptimal {
            self.reconfigure();
        }
        if let Some(PendingCapture { max_width, done }) = self.pending_capture.take() {
            self.capture(max_width, done);
        }

        Ok(FrameStats {
            quads: self.drawer.drawn.0,
            glyphs: self.drawer.drawn.1,
            draw_calls,
            drawn_pixels,
            cached_glyphs: ctx.state().atlas.len(),
            blurs: if keep { self.drawer.stale.len() } else { 0 },
            skipped: false,
            retry: false,
        })
    }
}

impl WgpuRenderer {
    /// No frame drawn after all (minimized, nothing changed): a frame asked to be shown with
    /// the window isn't left for a later, unrelated one, and frames don't wait for it.
    fn done_with_window(&mut self) {
        #[cfg(target_os = "macos")]
        {
            if self.with_window == WithWindow::On && self.transactional_until.is_none() {
                macos::set_transactional(&self.surface, false);
            }
            self.with_window = WithWindow::Off;
        }
    }
}
