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
            prim_buffer: InstanceBuffer::new(device, "prim-instances"),
            images: Vec::new(),
            image_ids: Vec::new(),
            quads: Vec::new(),
            prims: Vec::new(),
            drawn: (0, 0),
            batches: Vec::new(),
            area_batches: Vec::new(),
            layer_plans: Vec::new(),
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
        // Not compared: whole, its live images' frames noted as drawn now.
        if assumed {
            damage.set_full_of(scene);
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
            damage.set_full_of(scene);
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

    /// Turns the scene into GPU instances and their batches, area by area (`areas`:
    /// disjoint; the whole surface for a full frame), in the scene's order: the rectangles
    /// and glyphs of consecutive layers make one batch (each cut by its layer's clip), a
    /// layer's images or blurs one of their own between them, and a layer that blurs
    /// starts a new run (a render pass may end there). An area's batches hold only the
    /// instances that touch it, so a small area costs little. A glyph that finds no room in
    /// the atlas is left out and the rest drawn; the frame then says the atlas is full.
    pub(super) fn prepare(
        &mut self,
        ctx: &GpuContext,
        scene: &Scene,
        areas: &[PixelRect],
        rasterizer: &mut dyn GlyphRasterizer,
    ) -> Result<(), AtlasFull> {
        self.quads.clear();
        self.prims.clear();
        self.drawn = (0, 0);
        self.images.clear();
        self.image_ids.clear();
        self.batches.clear();
        self.area_batches.clear();
        self.blur_instances.clear();
        self.layer_plans.clear();
        self.layer_quads.clear();
        self.layer_images.clear();
        self.layer_glyphs.clear();
        let mut state = ctx.state();
        state.frame += 1;
        let frame = state.frame;
        let Some(bounds) = areas.iter().copied().reduce(|a, b| a.union(&b)) else {
            return Ok(());
        };
        let mut full = false;
        for (index, layer) in scene.layers().iter().enumerate() {
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
            let quads = self.layer_quads.len();
            for q in &layer.quads {
                if let Some(r) = touching(PixelRect::covering(&q.bounds)) {
                    self.layer_quads.push((PrimGpu::quad(q), r));
                }
            }
            let images = self.layer_images.len();
            for instance in &layer.images {
                let Some(r) = touching(PixelRect::covering(&instance.bounds)) else {
                    mark_used(&mut state, instance.image.id, frame);
                    continue;
                };
                if ctx.upload_image(&mut state, &instance.image) {
                    self.layer_images.push((ImageGpu::of(instance), instance.image.id, r));
                }
            }
            let glyphs = self.layer_glyphs.len();
            for g in &layer.glyphs {
                let entry = match state.atlas.get_or_insert(g.key, rasterizer) {
                    Ok(Some(entry)) => entry,
                    Ok(None) => continue,
                    Err(AtlasFull) => {
                        full = true;
                        continue;
                    }
                };
                let x = g.origin.x.round() + entry.left as f32;
                let y = g.origin.y.round() - entry.top as f32;
                let rect = Rect::new(x, y, entry.width as f32, entry.height as f32);
                if let Some(r) = touching(PixelRect::covering(&rect)) {
                    let gpu = PrimGpu {
                        rect: rect_array(&rect),
                        color: color_array(g.color),
                        border_color: [0.0; 4],
                        clip: [0; 4],
                        params: [0.0, 0.0, entry.x as f32, entry.y as f32],
                        flags: if entry.color { PRIM_COLOR } else { PRIM_MASK },
                    };
                    self.layer_glyphs.push((gpu, r));
                }
            }
            // Its blur's image, when it blurs (drawn back in its rectangles).
            let blur_image = self.wanted.iter().find(|w| w.layer == index).map(|w| {
                let plan = render_api::BlurPlan::for_radius(w.radius);
                let (rw, rh) = plan.reduced(w.bounds);
                let d = plan.factor as f32;
                [w.bounds.x0 as f32, w.bounds.y0 as f32, rw as f32 * d, rh as f32 * d]
            });
            self.layer_plans.push(LayerPlan {
                index,
                clip,
                quads: quads..self.layer_quads.len(),
                images: images..self.layer_images.len(),
                glyphs: glyphs..self.layer_glyphs.len(),
                blur_image,
            });
        }
        let muted_below = scene.emphasis().map_or(0, |e| e.below());
        for area in areas {
            let start = self.batches.len();
            self.batch_area(scene, area, muted_below);
            self.area_batches.push(start..self.batches.len());
        }
        // This window's images on screen: kept whatever their age (another window's too).
        let ids = state.shown.entry(self.id).or_default();
        ids.clear();
        ids.extend(scene.layers().iter().flat_map(|l| l.images.iter().map(|i| i.image.id)));
        state.release_unshown();
        if full { Err(AtlasFull) } else { Ok(()) }
    }

    /// The batches of one of the frame's areas (see [`Self::prepare`]).
    fn batch_area(&mut self, scene: &Scene, area: &PixelRect, muted_below: usize) {
        let hits = |r: &PixelRect| r.intersect(area).is_some();
        // The run of primitives being gathered: where it starts, and its first layer.
        let mut run: Option<(u32, usize)> = None;
        let close =
            |run: &mut Option<(u32, usize)>, prims: &[PrimGpu], batches: &mut Vec<Batch>| {
                if let Some((start, layer)) = run.take()
                    && (start as usize) < prims.len()
                {
                    let step = Step::Prims(start..prims.len() as u32);
                    batches.push(Batch { layer, step });
                }
            };
        for plan in &self.layer_plans {
            let Some(scissor) = plan.clip.intersect(area) else { continue };
            let muted = plan.index < muted_below;
            // A blur made again ends the render pass before its layer: a run never goes
            // past it.
            if self.stale.contains(&plan.index) {
                close(&mut run, &self.prims, &mut self.batches);
            }
            if let Some(image) = plan.blur_image {
                let start = self.blur_instances.len() as u32;
                for b in &scene.layers()[plan.index].blurs {
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
                let range = start..self.blur_instances.len() as u32;
                if !range.is_empty() {
                    close(&mut run, &self.prims, &mut self.batches);
                    let step = Step::Blurs { scissor, range };
                    self.batches.push(Batch { layer: plan.index, step });
                }
            }
            let before = self.prims.len();
            for (q, _) in self.layer_quads[plan.quads.clone()].iter().filter(|(_, r)| hits(r)) {
                self.prims.push(q.within(scissor, muted));
            }
            self.drawn.0 += self.prims.len() - before;
            if self.prims.len() > before {
                run.get_or_insert((before as u32, plan.index));
            }
            let start = self.images.len() as u32;
            for (image, id, r) in &self.layer_images[plan.images.clone()] {
                if hits(r) {
                    self.images.push(*image);
                    self.image_ids.push(*id);
                }
            }
            let range = start..self.images.len() as u32;
            if !range.is_empty() {
                close(&mut run, &self.prims, &mut self.batches);
                let step = Step::Images { scissor, range };
                self.batches.push(Batch { layer: plan.index, step });
            }
            let before = self.prims.len();
            for (g, _) in self.layer_glyphs[plan.glyphs.clone()].iter().filter(|(_, r)| hits(r)) {
                self.prims.push(g.within(scissor, muted));
            }
            self.drawn.1 += self.prims.len() - before;
            if self.prims.len() > before {
                run.get_or_insert((before as u32, plan.index));
            }
        }
        close(&mut run, &self.prims, &mut self.batches);
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
        self.prim_buffer.upload(&ctx.device, belt, encoder, &self.prims);
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
            draw_calls += self.draw_batches(&mut pass, state, (index, area), &layers);
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

    /// The batches of the `area`-th area (`rect`), in order: runs of primitives cut by
    /// their own clips, and the images and blurs between them cut by their layers'.
    /// Returns the draw calls.
    pub(super) fn draw_batches(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        state: &SharedState,
        (area, rect): (usize, PixelRect),
        layers: &Range<usize>,
    ) -> usize {
        let scissor = |pass: &mut wgpu::RenderPass<'_>, r: PixelRect| {
            pass.set_scissor_rect(r.x0 as u32, r.y0 as u32, r.width(), r.height());
        };
        let mut draw_calls = 0;
        // The primitives' state stays bound from one run to the next unless an image or a
        // blur changed it.
        let mut prims_bound = false;
        let batches = self.area_batches.get(area).map_or(&[][..], |r| &self.batches[r.clone()]);
        for batch in batches.iter().filter(|b| layers.contains(&b.layer)) {
            match &batch.step {
                Step::Prims(range) => {
                    if !prims_bound {
                        scissor(pass, rect);
                        pass.set_pipeline(&self.pipelines.prim);
                        // One bind for every layer: an instance not muted ignores the
                        // emphasis.
                        pass.set_bind_group(0, self.globals_for(0), &[]);
                        pass.set_bind_group(1, &state.atlas_bind_group, &[]);
                        pass.set_vertex_buffer(0, self.prim_buffer.buffer.slice(..));
                        prims_bound = true;
                    }
                    pass.draw(0..4, range.clone());
                    draw_calls += 1;
                }
                Step::Images { scissor: clip, range } => {
                    prims_bound = false;
                    scissor(pass, *clip);
                    pass.set_pipeline(&self.pipelines.image);
                    pass.set_bind_group(0, self.globals_for(batch.layer), &[]);
                    pass.set_vertex_buffer(0, self.image_buffer.buffer.slice(..));
                    // The same image drawn again in a row (a decoration's particles): one
                    // draw for all of them, its texture bound once.
                    let ids = &self.image_ids[range.start as usize..range.end as usize];
                    for run in same_image_runs(ids) {
                        let Some(gpu) = state.textures.get(&ids[run.start]) else { continue };
                        pass.set_bind_group(1, &gpu.bind_group, &[]);
                        let first = range.start + run.start as u32;
                        pass.draw(0..4, first..first + run.len() as u32);
                        draw_calls += 1;
                    }
                }
                Step::Blurs { scissor: clip, range } => {
                    let (Some(pipelines), Some(buffer), Some(blur)) = (
                        &self.blur_pipelines,
                        &self.blur_buffer,
                        self.blurs.iter().find(|b| b.of.layer == batch.layer),
                    ) else {
                        continue;
                    };
                    prims_bound = false;
                    scissor(pass, *clip);
                    pass.set_pipeline(&pipelines.composite);
                    pass.set_bind_group(0, self.globals_for(batch.layer), &[]);
                    pass.set_bind_group(1, &blur.composite, &[]);
                    pass.set_vertex_buffer(0, buffer.buffer.slice(..));
                    pass.draw(0..4, range.clone());
                    draw_calls += 1;
                }
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
        // Live images as compared (a frame come since is drawn by the next).
        self.tracker.commit_drawn(scene, size, &self.damage);
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
        // Live images as compared, as for a kept frame.
        self.tracker.commit_drawn(scene, size, &self.damage);
        self.kept_stale = true;
        self.direct_frames = self.direct_frames.saturating_add(1);
        if self.direct_frames >= DIRECT_RELEASE {
            self.kept = None;
        }
        drawn
    }
}

/// The runs of the same image in a row among `ids` (their places in it).
pub(super) fn same_image_runs(ids: &[u64]) -> impl Iterator<Item = Range<usize>> + '_ {
    let mut start = 0;
    std::iter::from_fn(move || {
        let first = *ids.get(start)?;
        let len = ids[start..].iter().take_while(|&&id| id == first).count();
        let run = start..start + len;
        start += len;
        Some(run)
    })
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
