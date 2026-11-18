//! A window's drawing: its instance buffers and pipelines, a scene's layers turned into
//! batches and drawn into a target.

use super::*;

/// Whole frames drawn straight on screen in a row after which the frame kept goes (a
/// second of an animation at 60 frames: a window's worth of memory, 31 MB at 3456 × 2234).
const DIRECT_RELEASE: u32 = 60;

/// Whole frames in a row past which the next are taken whole without comparing them (an
/// animation over the window: comparing its thousands of moving primitives only to find
/// it whole again cost as much as drawing them), and how often one is compared all the
/// same (to see the animation stop: at most that many frames drawn whole for nothing).
const WHOLE_STREAK: u32 = 4;
const WHOLE_CHECK: u32 = 8;

impl Drawer {
    pub(super) fn new(ctx: &GpuContext, format: wgpu::TextureFormat, transparent: bool) -> Self {
        let device = &ctx.device;
        let (globals, globals_bind_group) = globals_buffer(ctx, "globals");
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            globals,
            viewport: None,
            globals_bind_group,
            muted: None,
            muted_below: 0,
            // A full frame's instances fit in one chunk (~50 KB in a busy window).
            belt: wgpu::util::StagingBelt::new(device.clone(), 128 * 1024),
            pipelines: ctx.pipelines(format),
            format,
            transparent,
            image_buffer: InstanceBuffer::new(device, "image-instances"),
            quad_buffer: InstanceBuffer::new(device, "quad-instances"),
            glyph_buffer: InstanceBuffer::new(device, "glyph-instances"),
            images: Vec::new(),
            image_ids: Vec::new(),
            quads: Vec::new(),
            glyphs: Vec::new(),
            batches: Vec::new(),
            layer_quads: Vec::new(),
            layer_images: Vec::new(),
            layer_glyphs: Vec::new(),
            kept: None,
            shown: None,
            tracker: DamageTracker::new(),
            kept_stale: false,
            direct_frames: 0,
            whole_streak: 0,
            damage: Damage::default(),
            blur_pipelines: None,
            blurs: Vec::new(),
            wanted: Vec::new(),
            stale: Vec::new(),
            blur_instances: Vec::new(),
            blur_buffer: None,
        }
    }

    /// The scene's emphasis in the muted globals (written only when it changed; their
    /// buffer made at the first one, kept after: 288 bytes).
    pub(super) fn sync_muted(&mut self, ctx: &GpuContext, scene: &Scene, size: PhysicalSize) {
        let Some(emphasis) = scene.emphasis() else {
            self.muted_below = 0;
            return;
        };
        self.muted_below = emphasis.below();
        let wanted = (size, *emphasis);
        if self.muted.as_ref().is_some_and(|m| m.written == wanted) {
            return;
        }
        let globals = Globals::new(size, Some(emphasis));
        let muted = self.muted.get_or_insert_with(|| {
            let (buffer, bind_group) = globals_buffer(ctx, "muted-globals");
            MutedGlobals { buffer, bind_group, written: wanted }
        });
        ctx.queue.write_buffer(&muted.buffer, 0, bytemuck::bytes_of(&globals));
        muted.written = wanted;
    }

    /// The globals layer `layer` is drawn with: muted under the emphasis.
    pub(super) fn globals_for(&self, layer: usize) -> &wgpu::BindGroup {
        match &self.muted {
            Some(m) if layer < self.muted_below => &m.bind_group,
            _ => &self.globals_bind_group,
        }
    }

    /// Lets the blurs go (their textures; the pipelines stay, small).
    pub(super) fn release_blurs(&mut self) {
        self.blurs = Vec::new();
        self.wanted.clear();
        self.stale.clear();
    }

    pub(super) fn blur_bytes(&self) -> u64 {
        self.blurs.iter().map(GpuBlur::bytes).sum::<u64>()
            + self.blur_buffer.as_ref().map_or(0, |b| b.buffer.size())
    }

    /// The layers of `scene` that blur, and which of their blurs must be made again (new,
    /// or something under them changed: from a whole frame then). Only with a kept frame
    /// (the blur reads it).
    pub(super) fn plan_blurs(&mut self, scene: &Scene, size: PhysicalSize) {
        blur_layers(scene, PixelRect::of_size(size), &mut self.wanted);
        let first = self.damage.first_layer();
        let blurs = &self.blurs;
        self.stale.clear();
        for w in &self.wanted {
            let holds = blurs.iter().any(|b| b.of == *w) && first.is_none_or(|f| f >= w.layer);
            if !holds {
                self.stale.push(w.layer);
            }
        }
        let wanted = &self.wanted;
        self.blurs.retain(|b| wanted.iter().any(|w| w.layer == b.of.layer));
        // Those made last frame and still holding: what making them needed goes.
        for b in &mut self.blurs {
            if !self.stale.contains(&b.of.layer) {
                b.release_scratch();
            }
        }
        if !self.stale.is_empty() {
            self.damage.set_full();
        }
    }

    /// The textures and parameters of the blurs made again this frame.
    pub(super) fn ready_blurs(&mut self, ctx: &GpuContext) {
        let Some(kept) = &self.kept else { return };
        if self.stale.is_empty() {
            return;
        }
        let pipelines = self.blur_pipelines.get_or_insert_with(|| {
            Arc::clone(self.pipelines.blur.get_or_init(|| {
                Arc::new(BlurPipelines::new(
                    &ctx.device,
                    self.format,
                    &ctx.globals_layout,
                    &ctx.image_layout,
                ))
            }))
        });
        for &layer in &self.stale {
            let Some(of) = self.wanted.iter().find(|w| w.layer == layer).copied() else {
                continue;
            };
            let old = self
                .blurs
                .iter()
                .position(|b| b.of.layer == layer)
                .map(|i| self.blurs.swap_remove(i));
            let mut blur =
                GpuBlur::new(&ctx.device, pipelines, &ctx.image_layout, self.format, of, old);
            blur.ready(
                &ctx.device,
                &ctx.queue,
                pipelines,
                &kept.view,
                (kept.size.width, kept.size.height),
            );
            self.blurs.push(blur);
        }
    }

    /// Gives back the kept frame's texture (the window hidden, resizing, partial redraws off):
    /// the next frame is drawn whole.
    pub(super) fn release_kept(&mut self) {
        self.kept = None;
        self.tracker.release();
        self.kept_stale = false;
        self.direct_frames = 0;
    }

    /// The kept frame's texture for `size` (made again for another size: its contents are
    /// then undefined, so the frame must be full).
    pub(super) fn ensure_kept(&mut self, device: &wgpu::Device, size: PhysicalSize) -> bool {
        if self.kept.as_ref().is_some_and(|k| k.size == size) {
            return true;
        }
        self.tracker.invalidate();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("kept-frame"),
            size: wgpu::Extent3d {
                width: size.width,
                height: size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            // Sampled by the backdrop blur.
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.kept = Some(KeptFrame { texture, view, size });
        false
    }

    /// The kept frame copied into a buffer the CPU can map (the copy submitted), with its
    /// size and the bytes a row takes there (padded to the copy's alignment).
    pub(super) fn copy_kept(&self, ctx: &GpuContext) -> Option<(wgpu::Buffer, (u32, u32, usize))> {
        let kept = self.kept.as_ref()?;
        let (w, h) = (kept.size.width, kept.size.height);
        let row = (w * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame-readback"),
            size: u64::from(row) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("readback") });
        encoder.copy_texture_to_buffer(
            kept.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        ctx.queue.submit([encoder.finish()]);
        Some((buffer, (w, h, row as usize)))
    }

    /// Whether the surface's (and the kept frame's) pixels are BGRA.
    pub(super) fn bgra(&self) -> bool {
        matches!(self.format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb)
    }

    pub(super) fn kept_bytes(&self) -> u64 {
        self.kept.as_ref().map_or(0, |k| u64::from(k.size.width) * u64::from(k.size.height) * 4)
    }

    /// What `scene` changes from the frame kept (full when there's none, or it can't tell).
    /// Glyphs that changed are looked up in the atlas (rasterized now if new, as drawing
    /// them would).
    pub(super) fn compute_damage(
        &mut self,
        ctx: &GpuContext,
        scene: &Scene,
        size: PhysicalSize,
        rasterizer: &mut dyn GlyphRasterizer,
        keep: bool,
    ) {
        let mut damage = std::mem::take(&mut self.damage);
        // Against what's on screen: the kept frame, or (nothing kept) the last one shown.
        let comparable = if self.kept_stale {
            // Drawn straight on screen last: against that frame.
            keep && self.shown == Some(size)
        } else if keep {
            self.kept.as_ref().is_some_and(|k| k.size == size)
        } else {
            self.shown == Some(size)
        };
        // An animation going on: whole, as it was the last frames (checked now and then).
        let assumed = comparable
            && self.whole_streak >= WHOLE_STREAK
            && !self.whole_streak.is_multiple_of(WHOLE_CHECK);
        if assumed {
            damage.set_full();
        } else if comparable {
            let mut state = ctx.state();
            let mut glyph_box = |key| match state.atlas.get_or_insert(key, rasterizer) {
                Ok(Some(e)) => {
                    Some(GlyphBox { left: e.left, top: e.top, width: e.width, height: e.height })
                }
                Ok(None) => Some(GlyphBox::EMPTY),
                Err(AtlasFull) => None,
            };
            self.tracker.damage(scene, size, &mut glyph_box, &mut damage);
        } else {
            damage.set_full();
        }
        // Whole frames in a row (not those whole for want of a frame to compare with).
        self.whole_streak = match (comparable, damage.is_full()) {
            (true, true) => self.whole_streak.saturating_add(1),
            _ => 0,
        };
        self.damage = damage;
        if keep {
            self.plan_blurs(scene, size);
        }
    }

    /// Turns the scene into GPU instances, one batch per layer and area drawn (`areas`:
    /// disjoint; the whole surface for a full frame): an area's batches hold only the
    /// instances that touch it, so a small area costs few draws. Fails only if the atlas
    /// fills up.
    pub(super) fn prepare(
        &mut self,
        ctx: &GpuContext,
        scene: &Scene,
        areas: &[PixelRect],
        rasterizer: &mut dyn GlyphRasterizer,
    ) -> Result<(), AtlasFull> {
        self.quads.clear();
        self.glyphs.clear();
        self.images.clear();
        self.image_ids.clear();
        self.batches.clear();
        self.blur_instances.clear();
        let mut state = ctx.state();
        state.frame += 1;
        let frame = state.frame;
        let Some(bounds) = areas.iter().copied().reduce(|a, b| a.union(&b)) else {
            return Ok(());
        };
        for (layer_index, layer) in scene.layers().iter().enumerate() {
            let clip = match layer.clip {
                Some(c) => PixelRect::covering(&c).intersect(&bounds),
                None => Some(bounds),
            };
            let Some(clip) = clip else {
                // Not drawn this frame but still on screen (kept): not freed as unused.
                for instance in &layer.images {
                    mark_used(&mut state, instance.image.id, frame);
                }
                continue;
            };
            let touching = |r: PixelRect| r.intersect(&clip);

            self.layer_quads.clear();
            for q in &layer.quads {
                if let Some(r) = touching(PixelRect::covering(&q.bounds)) {
                    let gpu = QuadGpu {
                        rect: rect_array(&q.bounds),
                        color: color_array(q.color),
                        border_color: color_array(q.border_color),
                        params: [q.corner_radius, q.border_width],
                    };
                    self.layer_quads.push((gpu, r));
                }
            }
            self.layer_images.clear();
            for instance in &layer.images {
                let Some(r) = touching(PixelRect::covering(&instance.bounds)) else {
                    mark_used(&mut state, instance.image.id, frame);
                    continue;
                };
                if ctx.upload_image(&mut state, &instance.image) {
                    let tint =
                        instance.image.tint.map_or([0.0, 0.0, 0.0, -1.0], |c| [c.r, c.g, c.b, c.a]);
                    let gpu = ImageGpu {
                        rect: rect_array(&instance.bounds),
                        tint,
                        opacity: instance.opacity,
                        _pad: [0.0; 3],
                    };
                    self.layer_images.push((gpu, instance.image.id, r));
                }
            }
            self.layer_glyphs.clear();
            for g in &layer.glyphs {
                let Some(entry) = state.atlas.get_or_insert(g.key, rasterizer)? else {
                    continue;
                };
                let x = g.origin.x.round() + entry.left as f32;
                let y = g.origin.y.round() - entry.top as f32;
                let rect = Rect::new(x, y, entry.width as f32, entry.height as f32);
                if let Some(r) = touching(PixelRect::covering(&rect)) {
                    let gpu = GlyphGpu {
                        rect: rect_array(&rect),
                        atlas_origin: [entry.x as f32, entry.y as f32],
                        color: color_array(g.color),
                        kind: u32::from(entry.color),
                    };
                    self.layer_glyphs.push((gpu, r));
                }
            }

            // Its blur's image, when it blurs (drawn back in its rectangles).
            let image = self.wanted.iter().find(|w| w.layer == layer_index).map(|w| {
                let plan = render_api::BlurPlan::for_radius(w.radius);
                let (rw, rh) = plan.reduced(w.bounds);
                let d = plan.factor as f32;
                [w.bounds.x0 as f32, w.bounds.y0 as f32, rw as f32 * d, rh as f32 * d]
            });
            for (index, area) in areas.iter().enumerate() {
                let Some(scissor) = clip.intersect(area) else { continue };
                let hits = |r: &PixelRect| r.intersect(area).is_some();
                let blur_start = self.blur_instances.len() as u32;
                if let Some(image) = image {
                    for b in &layer.blurs {
                        if hits(&PixelRect::covering(&b.rect)) {
                            self.blur_instances.push(BlurGpu {
                                rect: rect_array(&b.rect),
                                image,
                                opacity: b.opacity,
                                corner: b.corner,
                                _pad: [0.0; 2],
                            });
                        }
                    }
                }
                let quad_start = self.quads.len() as u32;
                self.quads
                    .extend(self.layer_quads.iter().filter(|(_, r)| hits(r)).map(|(q, _)| *q));
                let image_start = self.images.len() as u32;
                for (image, id, r) in &self.layer_images {
                    if hits(r) {
                        self.images.push(*image);
                        self.image_ids.push(*id);
                    }
                }
                let glyph_start = self.glyphs.len() as u32;
                self.glyphs
                    .extend(self.layer_glyphs.iter().filter(|(_, r)| hits(r)).map(|(g, _)| *g));
                let batch = Batch {
                    layer: layer_index,
                    scissor,
                    area: index,
                    blurs: blur_start..self.blur_instances.len() as u32,
                    quads: quad_start..self.quads.len() as u32,
                    images: image_start..self.images.len() as u32,
                    glyphs: glyph_start..self.glyphs.len() as u32,
                };
                if !(batch.blurs.is_empty()
                    && batch.quads.is_empty()
                    && batch.images.is_empty()
                    && batch.glyphs.is_empty())
                {
                    self.batches.push(batch);
                }
            }
        }
        // This window's images on screen: kept whatever their age (another window's too).
        let ids = state.shown.entry(self.id).or_default();
        ids.clear();
        ids.extend(scene.layers().iter().flat_map(|l| l.images.iter().map(|i| i.image.id)));
        let shown: std::collections::HashSet<u64> =
            state.shown.values().flatten().copied().collect();
        state.textures.retain(|id, t| {
            let keep = (shown.contains(id) && !t.image.released())
                || (frame.saturating_sub(t.last_frame) < IMAGE_TTL
                    && t.last_used.elapsed() < IMAGE_IDLE
                    && !t.image.released());
            if !keep {
                t.image.evicted();
            }
            keep
        });
        Ok(())
    }

    /// [`Self::prepare`], once more after making room in the atlas if it filled up.
    pub(super) fn prepare_frame(
        &mut self,
        ctx: &GpuContext,
        scene: &Scene,
        areas: &[PixelRect],
        rasterizer: &mut dyn GlyphRasterizer,
    ) {
        if self.prepare(ctx, scene, areas, rasterizer).is_ok() {
            return;
        }
        // Atlas full: clear it and rebuild the frame. If it still doesn't fit,
        // the frame is drawn without the glyphs that didn't fit.
        make_atlas_room(ctx);
        if self.prepare(ctx, scene, areas, rasterizer).is_err() {
            tracing::warn!("the scene has more glyphs than fit in the atlas");
        }
    }

    /// Uploads the globals and the instances, with the extra quads after the scene's: the
    /// background (for partial frames) and the window's corners (erased last) when cut.
    pub(super) fn upload(
        &mut self,
        ctx: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        scene: &Scene,
        size: PhysicalSize,
    ) -> Extras {
        if self.viewport != Some(size) {
            self.viewport = Some(size);
            ctx.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&Globals::new(size, None)));
        }
        self.sync_muted(ctx, scene, size);
        let (w, h) = (size.width as f32, size.height as f32);
        let fill = self.quads.len() as u32;
        self.quads.push(QuadGpu {
            rect: [0.0, 0.0, w, h],
            color: color_array(scene.clear_color),
            border_color: [0.0; 4],
            params: [0.0, 0.0],
        });
        let corners = self.quads.len() as u32;
        let radius = scene.window_corners;
        let cut = self.transparent && radius > 0.0;
        if cut {
            let r = radius.ceil() + 1.0;
            for (x, y) in [(0.0, 0.0), (w - r, 0.0), (0.0, h - r), (w - r, h - r)] {
                self.quads.push(QuadGpu {
                    rect: [x, y, r, r],
                    color: [0.0; 4],
                    border_color: [0.0, 0.0, w, h],
                    params: [radius, 0.0],
                });
            }
        }
        let belt = &mut self.belt;
        self.quad_buffer.upload(&ctx.device, belt, encoder, &self.quads);
        self.glyph_buffer.upload(&ctx.device, belt, encoder, &self.glyphs);
        self.image_buffer.upload(&ctx.device, belt, encoder, &self.images);
        if !self.blur_instances.is_empty() {
            self.blur_buffer
                .get_or_insert_with(|| InstanceBuffer::new(&ctx.device, "blur-instances"))
                .upload(&ctx.device, belt, encoder, &self.blur_instances);
        }
        // Its buffers come back for reuse once the GPU has copied from them.
        belt.finish_and_recall_on_submit(encoder);
        Extras { corners: cut.then_some(corners), fill }
    }

    /// The frame's render passes into `view`: up to each blur made again, the blur, then
    /// on (one pass when none is).
    pub(super) fn encode_frame(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &Target<'_>,
    ) -> usize {
        let scene = target.scene;
        let mut from = 0;
        let mut draw_calls = 0;
        let pass = |encoder: &mut wgpu::CommandEncoder, layers: Range<usize>| {
            self.encode_pass(encoder, target, layers)
        };
        if let Some(pipelines) = &self.blur_pipelines {
            for &k in &self.stale {
                let Some(blur) = self.blurs.iter().find(|b| b.of.layer == k) else { continue };
                draw_calls += pass(encoder, from..k);
                blur.encode(encoder, pipelines);
                draw_calls += 3;
                from = k;
            }
        }
        draw_calls + pass(encoder, from..scene.layers().len())
    }

    /// A render pass of `layers` into `view`: from the first layer whole (cleared first),
    /// or only within `areas` over the pixels already there (each area cleared to the
    /// background, then drawn); from a later layer over what's there. Returns the draw calls.
    pub(super) fn encode_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &Target<'_>,
        layers: Range<usize>,
    ) -> usize {
        let Target { view, scene, size, extras, state, areas } = *target;
        let clear = scene.clear_color;
        let first = layers.start == 0;
        let last = layers.end >= scene.layers().len();
        let load = match areas {
            _ if !first => wgpu::LoadOp::Load,
            Some(_) => wgpu::LoadOp::Load,
            None => wgpu::LoadOp::Clear(wgpu::Color {
                r: f64::from(clear.r * clear.a),
                g: f64::from(clear.g * clear.a),
                b: f64::from(clear.b * clear.a),
                a: f64::from(clear.a),
            }),
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("main"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let whole = [PixelRect::of_size(size)];
        let mut draw_calls = 0;
        // The background is drawn as a quad over each area of a partial frame — and of a whole
        // one under an emphasis (muted as the layers over it: a clear color can't be).
        let fill = first && (areas.is_some() || self.muted_below > 0);
        for (index, &area) in areas.unwrap_or(&whole).iter().enumerate() {
            if fill {
                pass.set_scissor_rect(area.x0 as u32, area.y0 as u32, area.width(), area.height());
                pass.set_pipeline(&self.pipelines.fill);
                pass.set_bind_group(0, self.globals_for(0), &[]);
                pass.set_vertex_buffer(0, self.quad_buffer.buffer.slice(..));
                pass.draw(0..4, extras.fill..extras.fill + 1);
                draw_calls += 1;
            }
            draw_calls += self.draw_batches(&mut pass, state, index, &layers);
            if let Some(corners) = extras.corners.filter(|_| last) {
                pass.set_scissor_rect(area.x0 as u32, area.y0 as u32, area.width(), area.height());
                pass.set_pipeline(&self.pipelines.corner);
                pass.set_bind_group(0, &self.globals_bind_group, &[]);
                pass.set_vertex_buffer(0, self.quad_buffer.buffer.slice(..));
                pass.draw(0..4, corners..corners + 4);
                draw_calls += 1;
            }
        }
        draw_calls
    }

    /// The batches of the `area`-th area, clipped: their quads, images, then glyphs.
    /// Returns the draw calls.
    pub(super) fn draw_batches(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        state: &SharedState,
        area: usize,
        layers: &Range<usize>,
    ) -> usize {
        let mut draw_calls = 0;
        for batch in self.batches.iter().filter(|b| b.area == area && layers.contains(&b.layer)) {
            let clip = batch.scissor;
            let globals = self.globals_for(batch.layer);
            pass.set_scissor_rect(clip.x0 as u32, clip.y0 as u32, clip.width(), clip.height());
            if !batch.blurs.is_empty()
                && let (Some(pipelines), Some(buffer), Some(blur)) = (
                    &self.blur_pipelines,
                    &self.blur_buffer,
                    self.blurs.iter().find(|b| b.of.layer == batch.layer),
                )
            {
                pass.set_pipeline(&pipelines.composite);
                pass.set_bind_group(0, globals, &[]);
                pass.set_bind_group(1, &blur.composite, &[]);
                pass.set_vertex_buffer(0, buffer.buffer.slice(..));
                pass.draw(0..4, batch.blurs.clone());
                draw_calls += 1;
            }
            if !batch.quads.is_empty() {
                pass.set_pipeline(&self.pipelines.quad);
                pass.set_bind_group(0, globals, &[]);
                pass.set_vertex_buffer(0, self.quad_buffer.buffer.slice(..));
                pass.draw(0..4, batch.quads.clone());
                draw_calls += 1;
            }
            if !batch.images.is_empty() {
                pass.set_pipeline(&self.pipelines.image);
                pass.set_bind_group(0, globals, &[]);
                pass.set_vertex_buffer(0, self.image_buffer.buffer.slice(..));
                for i in batch.images.clone() {
                    let Some(gpu) = state.textures.get(&self.image_ids[i as usize]) else {
                        continue;
                    };
                    pass.set_bind_group(1, &gpu.bind_group, &[]);
                    pass.draw(0..4, i..i + 1);
                    draw_calls += 1;
                }
            }
            if !batch.glyphs.is_empty() {
                pass.set_pipeline(&self.pipelines.glyph);
                pass.set_bind_group(0, globals, &[]);
                pass.set_bind_group(1, &state.atlas_bind_group, &[]);
                pass.set_vertex_buffer(0, self.glyph_buffer.buffer.slice(..));
                pass.draw(0..4, batch.glyphs.clone());
                draw_calls += 1;
            }
        }
        draw_calls
    }

    /// Draws `scene` into the kept frame (`kept`: only where it changed, as
    /// [`Self::compute_damage`] found) or straight into `direct`, encoding into `encoder`.
    /// Returns the draw calls and the pixels drawn.
    pub(super) fn draw(
        &mut self,
        ctx: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        scene: &Scene,
        size: PhysicalSize,
        rasterizer: &mut dyn GlyphRasterizer,
        direct: Option<&wgpu::TextureView>,
    ) -> (usize, u64) {
        let partial = direct.is_none() && !self.damage.is_full();
        if direct.is_some() {
            // Drawn straight into the surface's image (nothing kept to blur): no blurs.
            self.release_blurs();
        }
        let whole = [PixelRect::of_size(size)];
        let damage = std::mem::take(&mut self.damage);
        let areas = if partial { damage.rects() } else { &whole };
        self.prepare_frame(ctx, scene, areas, rasterizer);
        // The glyphs placed meanwhile (by any window: the atlas is shared) on the GPU first.
        ctx.state().atlas.flush(&ctx.device, &ctx.queue, encoder);
        self.damage = damage;
        self.ready_blurs(ctx);
        let extras = self.upload(ctx, encoder, scene, size);
        let state = ctx.state();
        let (view, areas) = match (direct, &self.kept) {
            (Some(view), _) => (view, None),
            (None, Some(kept)) => (&kept.view, partial.then(|| self.damage.rects())),
            (None, None) => return (0, 0),
        };
        let target = Target { view, scene, size, extras, state: &state, areas };
        let draw_calls = self.encode_frame(encoder, &target);
        let drawn = match areas {
            Some(areas) => areas.iter().map(|a| a.area() as u64).sum(),
            None => u64::from(size.width) * u64::from(size.height),
        };
        (draw_calls, drawn)
    }

    /// Draws `scene` into the kept frame — only the damage [`Self::compute_damage`] found,
    /// or whole if the texture is new — and records it for the next frame's damage.
    pub(super) fn draw_kept(
        &mut self,
        ctx: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        scene: &Scene,
        size: PhysicalSize,
        rasterizer: &mut dyn GlyphRasterizer,
    ) -> (usize, u64) {
        // Behind (whole frames went straight on screen): drawn whole again.
        let made = !self.ensure_kept(&ctx.device, size);
        let behind = std::mem::take(&mut self.kept_stale);
        if made || behind {
            self.damage.set_full();
        }
        self.direct_frames = 0;
        let drawn = self.draw(ctx, encoder, scene, size, rasterizer, None);
        self.tracker.commit(scene, size);
        drawn
    }

    /// A whole frame (its damage full) drawn straight into `view`, the screen's: the frame
    /// kept falls behind (drawn whole when a partial one comes); after [`DIRECT_RELEASE`]
    /// of them in a row, its memory goes.
    pub(super) fn draw_straight(
        &mut self,
        ctx: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        scene: &Scene,
        size: PhysicalSize,
        rasterizer: &mut dyn GlyphRasterizer,
        view: &wgpu::TextureView,
    ) -> (usize, u64) {
        let drawn = self.draw(ctx, encoder, scene, size, rasterizer, Some(view));
        self.tracker.commit(scene, size);
        self.kept_stale = true;
        self.direct_frames = self.direct_frames.saturating_add(1);
        if self.direct_frames >= DIRECT_RELEASE {
            self.kept = None;
        }
        drawn
    }
}

/// A globals uniform (`Globals`) and its bind group.
pub(super) fn globals_buffer(
    ctx: &GpuContext,
    label: &'static str,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: std::mem::size_of::<Globals>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout: &ctx.globals_layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
    });
    (buffer, bind_group)
}

/// An image on screen but not drawn this frame (outside the damage) stays on the GPU.
pub(super) fn mark_used(state: &mut SharedState, id: u64, frame: u64) {
    if let Some(gpu) = state.textures.get_mut(&id) {
        gpu.last_frame = frame;
        gpu.last_used = std::time::Instant::now();
    }
}

/// The atlas grown past its start (a zoom, a script's many glyphs) made smaller again when
/// what it holds fits (the window idle: its glyphs are rasterized again as they're drawn).
pub(super) fn shrink_atlas(ctx: &GpuContext) {
    let mut state = ctx.state();
    if let Some(size) = state.atlas.smaller_size(ATLAS_START) {
        tracing::debug!(from = state.atlas.size(), to = size, "the glyph atlas made smaller");
        state.atlas.grow(&ctx.device, size);
        state.atlas_bind_group = atlas_bind_group(&ctx.device, &ctx.atlas_layout, &state.atlas);
    }
}

/// Doubles the full atlas or, at its largest, clears it.
pub(super) fn make_atlas_room(ctx: &GpuContext) {
    let mut state = ctx.state();
    let size = state.atlas.size();
    if size < ATLAS_MAX {
        tracing::debug!(size = size * 2, "the glyph atlas is full; growing it");
        state.atlas.grow(&ctx.device, size * 2);
        state.atlas_bind_group = atlas_bind_group(&ctx.device, &ctx.atlas_layout, &state.atlas);
    } else {
        tracing::debug!("the glyph atlas is full; clearing it");
        state.atlas.clear();
    }
}
