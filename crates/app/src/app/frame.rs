//! A frame drawn: the scene laid out and handed to the renderer, what asked for it counted
//! while a profile records, the frame skipped when nothing changed, the window's
//! decorations told how it is, and the resize diagnostic.

use super::floating::FLOATING_SHADOW;
use super::*;

/// The views' soonest time, counted per view (`probe_frame_askers`), not as one more.
const VIEWS: &str = "views";

impl EditorApp {
    pub(super) fn render(&mut self) {
        let Some(window) = self.window.clone() else { return };
        // The app moved its focus since (a command, a tab opened): the pages give the
        // keyboard back.
        self.pages_follow_focus();
        // Typing paused long enough: the changed files are saved.
        self.workbench.autosave(false);
        // The view that took the whole screen closed, however it did.
        if self.window_state.full_screen_for_view && !self.workbench.modal_open() {
            self.window_state.full_screen_for_view = false;
            window.toggle_fullscreen();
        }
        if self.window_state.occluded {
            return;
        }
        // A module asked for this window's picture: taken from the frame on screen, before
        // this one (a view just opened over it) is drawn.
        if self.workbench.take_capture_wanted() {
            self.capture_window(true);
        }
        let frame_start = Instant::now();
        // A profile recording's outermost step: what each view drew nests under it.
        let _frame = kernel::probe::time("app", "frame");
        self.count_due_askers(frame_start);
        // A view's own deadline came (a save due a moment after typing): its work is done
        // now, by its poll, not only at the next wake — or the frames it asked for would
        // follow one another until something else woke the loop.
        if self.workbench.views_deadline().is_some_and(|due| due <= frame_start) {
            self.workbench.poll_viewers();
        }
        // What the app pages told their views, before those paint.
        let told = self.web_views.take_events();
        if !told.is_empty() {
            for (page, event) in told {
                self.workbench.web_event(page, event);
            }
            // What they asked for (a tab closed, a link, a Save dialog), now: not at the next
            // unrelated wake.
            self.with_text(|wb, text| wb.run_panel_actions(text));
            self.frames.dirty = true;
        }
        self.workbench.paint_phases.begin();
        // A while without input: what modules start then (Relax mode), drawn in this frame.
        let frontmost = self.window_state.focus_turn == self.shared.focus_turn.get();
        // One due: asked of the system first, the user may be busy in another program.
        if self.workbench.idle_deadline().is_some_and(|due| due <= frame_start)
            && let Some(at) = window.system_idle().and_then(|idle| frame_start.checked_sub(idle))
        {
            self.workbench.active_elsewhere(at);
        }
        if self.workbench.run_idle_actions(frame_start, frontmost) {
            self.frames.dirty = true;
        }
        let viewport = window.inner_size().to_size();
        let scale = window.scale_factor() as f32;
        self.sync_decorations_window(&window, frame_start);
        if self.release_without_frame(frame_start) {
            // A renderer not due yet (the floating layer's, the window's): looked at again then.
            if let Some(at) = self.frames.kept_frame_check {
                window.request_redraw_at(at);
            }
            return;
        }
        // Nothing happened since the last whole frame and nothing else is due: only the
        // theme's decorations are drawn again, over the scene kept.
        let quiet = !self.frames.dirty
            && self.frames.full_due.is_some_and(|due| frame_start < due)
            && self.startup.splash.is_gone()
            && self.startup.first_frame_logged;
        let only_decorations = quiet && self.workbench.decorations_drawn();
        // Only a page the app draws has a new frame: the scene kept is drawn again (the
        // renderer redraws only where its image changed), nothing of the window repainted.
        let only_pages =
            std::mem::take(&mut self.frames.only_pages) && quiet && !self.workbench.animating();

        self.ensure_text();
        self.workbench.paint_phases.mark("text_system");
        // A page on screen: what floats is drawn on the layer over it.
        if !only_decorations && !only_pages {
            self.sync_floating(&window);
        }
        let shared = Rc::clone(&self.shared);
        let mut fonts = shared.fonts.borrow_mut();
        let (Fonts::Ready(text), Some(renderer)) = (&mut *fonts, &mut self.frames.renderer) else {
            return;
        };
        let only_decorations =
            only_decorations && self.workbench.repaint_decorations(&mut self.frames.scene);
        // The scene kept (its decorations drawn again, or only the pages' frames new).
        let only_decorations = only_decorations || only_pages;
        let access =
            access::begin(self.window.as_deref(), &mut self.frames.access, only_decorations);
        // The pages' own trees made only while a screen reader runs.
        let reading = self.window.as_deref().and_then(|w| w.access_asked()).is_some();
        self.web_views.set_access(reading);
        if access.is_some() {
            self.workbench.set_page_access(self.web_views.access_trees());
        } else if !reading {
            // The last trees let go with the screen reader (once: nothing held after).
            self.workbench.clear_page_access();
        }
        // The earliest moment the next whole frame is needed, whatever happens meanwhile.
        let looks_ahead = frame_start + Duration::from_secs(1);
        let mut full_due = looks_ahead;
        if !only_decorations {
            self.frames.whole_frames += 1;
            // The glyph keys only grow (sizes, zoom, scripts seen): past a bound they're
            // reset, with the atlas that refers to them, in the same step.
            const MAX_GLYPH_KEYS: usize = 200_000;
            if text.glyph_key_count() > MAX_GLYPH_KEYS {
                use kernel::Purge;
                tracing::debug!("too many glyph keys; clearing the shaped text and the atlas");
                text.purge();
                renderer.purge_caches();
            }
            self.frames.scene.reset(self.workbench.background());
            // The window's corners (Windows' radius): rounded unless it fills the screen;
            // cut where the surface is transparent (the splash's frames too).
            if window.window_controls() && !window.is_fullscreen() && !window.is_maximized() {
                self.frames.scene.window_corners = (WINDOW_CORNER_RADIUS * scale).round();
            }
            // The splash, only while the workbench isn't ready: alone, then faded over it.
            // (The active tab is sent to parsing even while nothing of the workbench is drawn.)
            let ready = self.startup.splash.is_gone() || {
                self.workbench.sync_syntax();
                self.workbench.poll_syntax();
                self.workbench.first_paint_ready()
            };
            let show = self.startup.splash_at(frame_start, ready);
            let workbench = match show {
                splash::SplashShow::Workbench => true,
                splash::SplashShow::Splash { opacity, .. } => opacity < 1.0,
                splash::SplashShow::Background => false,
            };
            self.workbench.paint_phases.mark("before_workbench");
            if workbench {
                // The window's own buttons in the top bar (Windows), as it is now.
                self.workbench
                    .set_window_controls(window.window_controls().then(|| window.is_maximized()));
                let full_screen = window.is_fullscreen();
                self.workbench.set_full_screen(full_screen);
                self.workbench.set_maximize_hovered(window.maximize_hovered());
                self.workbench.set_page_holes(window.web_view_holes());
                let floating = self.frames.floating.as_mut().map(|f| {
                    f.scene.reset(kernel::Rgba::TRANSPARENT);
                    &mut f.scene
                });
                self.workbench.paint_with_floating(text, &mut self.frames.scene, floating);
                // The pages over the views, where the views just drew themselves; a page
                // something opened over, its picture in its place from the next frame.
                // Asked of the renderer only when a page is made (it copies the frames).
                let drawing: &dyn render_api::Renderer = &**renderer;
                self.web_views.set_scale(scale);
                // The engine just installed: the system's pages made again by it.
                if web_engine::became_usable(self.web_views.engine_seen()) {
                    self.web_views.let_go_system_pages();
                }
                self.web_views.set_waker(|| window.waker());
                let pictured = self.web_views.sync(
                    self.workbench.web_pages(),
                    self.workbench.overlay_open(),
                    frame_start,
                    |source, area, inbox| {
                        let source = match source {
                            ui_workbench::WebSource::Html(html) => {
                                platform_api::WebSource::Html(html)
                            }
                            ui_workbench::WebSource::App { root, entry } => {
                                platform_api::WebSource::App { root, entry }
                            }
                        };
                        // Drawn by the app's own engine when it's there; else the system's.
                        let importer = web_engine::usable().then(|| drawing.frame_importer());
                        if let Some(importer) = importer.flatten() {
                            let made = web_engine::Made {
                                source: &source,
                                area,
                                scale,
                                inbox,
                                wake: window.waker(),
                                importer,
                            };
                            let page = web_engine::EnginePage::new(made);
                            let drawn = Some(page.image().clone());
                            let popup = Some(page.popup_image().clone());
                            let view = Box::new(page);
                            return Some(web_views::MadePage { view, drawn, popup });
                        }
                        let view = window.web_view(source, area, &web_views::data_dir(), inbox)?;
                        Some(web_views::MadePage { view, drawn: None, popup: None })
                    },
                );
                if pictured {
                    let popups = self.web_views.popups();
                    self.workbench.set_page_pictures(&self.web_views.pictures(), &popups);
                    window.request_redraw();
                }
                // What floats over the system's pages: drawn over them on the floating
                // layer, else cut out of them (shows through). The app's engine's pages are
                // drawn under it by the app: nothing to cut.
                let holes = match self.web_views.any_shown() {
                    true => self.workbench.page_holes(),
                    false => Vec::new(),
                };
                self.web_views.set_holes(&holes, self.frames.floating.is_none());
                // Their corners as the cards they're in.
                let workbench = &self.workbench;
                self.web_views.set_corners(|area| workbench.page_corners(area));
                if let Some(floating) = &self.frames.floating {
                    // Something open over the views (a menu, a dialog; the pages hidden
                    // under it): all of the layer; else only where something floats — while
                    // the layer is there, even on the frame the last system page hides
                    // (what floats was drawn on it: shown nowhere else).
                    let shapes = match () {
                        _ if self.workbench.overlay_open() => {
                            vec![(kernel::Rect::from_size(viewport), 0.0)]
                        }
                        _ if self.web_views.any_shown() => holes,
                        _ => self.workbench.page_holes(),
                    };
                    floating.layer.set_shapes(&shapes, (FLOATING_SHADOW * scale).ceil());
                    floating.layer.raise();
                }
                // What the views have for their app pages, once those are placed.
                self.web_views.send(self.workbench.take_web_sends());
                // Its maximize button is the system's (Windows' snap layouts on hover); full
                // screen, it's ours (it leaves full screen).
                if window.window_controls() {
                    let button = self.workbench.maximize_button().filter(|_| !full_screen);
                    window.set_maximize_button(button);
                }
                // Transitions (hover on separators) request frames only while they last: the
                // next at once while they show (the display paces what's presented), else a
                // display's frame later — something saying it moves while nothing changes
                // must not spin the processor at thousands of frames a second.
                if self.workbench.animating() {
                    if self.frames.last_skipped {
                        let at = frame_start + ANIMATION_FLOOR;
                        window.request_redraw_at(at);
                        full_due = full_due.min(at);
                    } else {
                        window.request_redraw();
                        full_due = frame_start;
                    }
                } else if self.workbench.loading() {
                    // A loader turning, a progress bar sweeping: about 30 frames a second
                    // (smooth to the eye; only while work goes on).
                    const LOADER_FRAME: std::time::Duration = std::time::Duration::from_millis(33);
                    let at = std::time::Instant::now() + LOADER_FRAME;
                    window.request_redraw_at(at);
                    full_due = full_due.min(at);
                }
            }
            if let splash::SplashShow::Splash { elapsed, opacity } = show {
                let version = concat!("v", env!("CARGO_PKG_VERSION"));
                let frame = ui_workbench::SplashFrame {
                    bounds: kernel::Rect::from_size(viewport),
                    scale,
                    phase: elapsed.as_secs_f32(),
                    opacity,
                };
                let painter = ui_workbench::Painter {
                    text,
                    scene: &mut self.frames.scene,
                    palette: self.workbench.palette(),
                    icons: self.workbench.icons(),
                };
                let art = crate::resources::splash_art();
                Splash.paint(frame, version, &i18n::t("splash.preparing"), art.as_ref(), painter);
                // The splash animation requests frames until it's gone.
                window.request_redraw();
                full_due = frame_start;
            } else if show == splash::SplashShow::Background {
                // Waiting to be ready (or for the splash's turn): the next frame looks again.
                window.request_redraw();
                full_due = frame_start;
            }
            let area = self.workbench.overlay_area();
            let (palette, icons) = (self.workbench.palette(), self.workbench.icons());
            let scene = &mut self.frames.scene;
            let painter = ui_workbench::Painter { text, scene, palette, icons };
            self.frames.overlay.paint(painter, area, viewport, scale);
        } else {
            self.frames.decoration_frames += 1;
        }

        self.workbench.paint_phases.end();
        // A change held back (told at most ten times a second): a frame when it's due.
        let access_due = access::end(
            access,
            &mut self.frames.access,
            self.window.as_deref(),
            &self.window_state.title,
            |p| self.workbench.access_tip(p),
        );
        if let Some(at) = access_due {
            window.request_redraw_at(at);
        }
        let mut glyphs = WithIcons { text: &mut **text, icons: self.workbench.icons() };
        let painted = frame_start.elapsed();
        // A profile recording's step: the scene handed to the GPU and shown.
        let rendering = kernel::probe::time("app", "render");
        let result = renderer.render(&self.frames.scene, &mut glyphs);
        // What floats, on its layer over the pages (shown once it has a frame).
        if !only_decorations && let Some(floating) = &mut self.frames.floating {
            match floating.renderer.render(&floating.scene, &mut glyphs) {
                Ok(stats) if stats.retry => window.request_redraw(),
                Ok(_) => floating.layer.set_visible(true),
                Err(e) => tracing::warn!(error = %e, "the floating layer's frame"),
            }
        }
        drop(rendering);
        // A long frame says which half took the time (the scene, the GPU).
        if frame_start.elapsed() >= Duration::from_millis(500) {
            let phases = self.workbench.paint_phases.text();
            tracing::info!(?painted, total = ?frame_start.elapsed(), phases, "a slow frame");
        }
        // A window left standing gives its kept frame back: one redraw at that moment.
        // The floating layer keeps one too: the sooner of the two.
        let floating_at =
            self.frames.floating.as_ref().and_then(|f| f.renderer.kept_frame_release_at());
        let release_at = match (renderer.kept_frame_release_at(), floating_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if let Some(at) = release_at
            && self.frames.kept_frame_check.is_none_or(|check| check <= frame_start)
        {
            self.frames.kept_frame_check = Some(at);
            window.request_redraw_at(at);
        }
        if let (Some(test), Ok(stats)) = (&mut self.frames.resize_test, &result) {
            test.record(frame_start.elapsed(), stats.skipped);
        }
        match result {
            Ok(stats) => {
                self.frames.last_skipped = stats.skipped;
                // A part saying it moves while nothing changes: told (once) by its name.
                let moving = stats.skipped && self.workbench.animating();
                let workbench = &self.workbench;
                if let Some(parts) =
                    self.frames
                        .unseen_motion
                        .frame(frame_start, (moving, stats.skipped), || workbench.moving_parts())
                {
                    tracing::warn!(?parts, "parts say they move while nothing on screen changes");
                }
                self.frames.overlay.record(frame_start.elapsed(), stats);
                if !stats.skipped {
                    kernel::probe::frame(frame_start.elapsed());
                    record_frame(&stats, self.frames.scene.layers().len());
                }
                if stats.retry {
                    window.request_redraw();
                    full_due = frame_start;
                } else if !stats.skipped
                    && let Some(input) = self.frames.pending_input.take()
                {
                    self.frames.overlay.record_latency(input.elapsed());
                }
                if !self.startup.ready_logged
                    && !stats.skipped
                    && self.workbench.first_paint_ready()
                {
                    self.startup.ready_logged = true;
                    crate::startup::mark("editor ready");
                }
                if !self.startup.first_frame_logged && !stats.skipped {
                    self.startup.first_frame_logged = true;
                    tracing::info!(elapsed = ?self.shared.started.elapsed(), "first frame shown");
                    // Diagnostics: `EDITOR_DEBUG_EYEDROPPER=1` starts the screen eyedropper.
                    if std::env::var_os("EDITOR_DEBUG_EYEDROPPER").is_some() {
                        let started = window.sample_screen_color();
                        self.window_state.sampling = started;
                        tracing::debug!(started, "eyedropper (diagnostic)");
                    }
                }
            }
            Err(RenderError::DeviceLost) => {
                tracing::error!("the GPU device was lost; recreating the renderer");
                drop(fonts);
                self.recover_renderer(&window);
                return;
            }
            Err(e) => tracing::error!(error = %e, "render error"),
        }
        // Now and then, how the decorations' frames went (`RUST_LOG=debug`).
        if only_decorations && self.frames.decoration_frames.is_multiple_of(75) {
            tracing::debug!(
                whole = self.frames.whole_frames,
                decorations_only = self.frames.decoration_frames,
                took = ?frame_start.elapsed(),
                "frames"
            );
        }
        // The caret's place for the system's input methods: told when it moves, not each
        // frame (a frame per hover would bring back their popups, macOS's "US").
        // A page with the keyboard composing text: its; else the editor's caret.
        let area = self
            .web_views
            .caret(!self.workbench.overlay_open())
            .or_else(|| self.workbench.caret_rect(text));
        if let Some(area) = area
            && self.window_state.ime_area != Some(area)
        {
            window.set_ime_cursor_area(area);
        }
        self.window_state.ime_area = area;
        // A caret on screen blinks: one redraw when it next turns on / off (only in the
        // focused window; background windows don't wake up for it).
        // Each one named: while a profile records, whose time each is is kept, and counted
        // once by the frame that comes for it (`count_due_askers`).
        let recording = kernel::probe::recording();
        let mut asked: Vec<(&'static str, Instant)> = Vec::new();
        if recording {
            self.workbench.probe_frame_askers(&mut asked);
        }
        let mut due = |owner: &'static str, at: Instant| {
            if recording && owner != VIEWS {
                asked.push((owner, at));
            }
            window.request_redraw_at(at);
            full_due = full_due.min(at);
        };
        if let Some(at) = ui_kit::caret::take_next_frame()
            && self.window_state.focused
        {
            due("caret", at);
        }
        // The code hover card / a tooltip appears after the mouse rests: one redraw then.
        if let Some(at) = self.workbench.hover_deadline() {
            due("hover", at);
        }
        if let Some(at) = self.workbench.autosave_deadline() {
            due("autosave", at);
        }
        // A language server's answer waited for: the built-in engine's when its grace ends.
        if let Some(at) = self.workbench.served_deadline() {
            due("served", at);
        }
        // The status bar's meter gliding to new heights.
        if let Some(at) = self.workbench.status_deadline() {
            due("status", at);
        }
        if let Some(at) = self.workbench.idle_deadline() {
            due("idle", at);
        }
        // The views at their pace (a scrollbar fading in or out).
        // (Each view's own is counted under its name by `probe_frame_askers`: not again here.)
        if let Some(at) = self.workbench.views_deadline() {
            due(VIEWS, at);
        }
        // A hidden page let go when its time's up (its engine holds hundreds of MB).
        if let Some(at) = self.web_views.deadline() {
            due("web-views", at);
        }
        // The windows over the editor at theirs, each by its name (a notification's ring, a
        // popover's scrollbar fading, a modal view's scene, a settings card's picture).
        for (owner, at) in self.workbench.overlays_due() {
            due(owner, at);
        }
        // The overlay shows what the frames drawn anyway measured (never a frame to measure
        // itself: idle stays idle); only its ×'s tooltip asks for one.
        if let Some(at) = self.frames.overlay.frame_deadline() {
            due("diagnostic-overlay", at);
        }
        if recording {
            self.frames.asked = asked;
        }
        if only_decorations {
            // The deadlines the last whole frame set still hold.
            full_due = full_due.min(self.frames.full_due.unwrap_or(full_due));
        } else {
            self.frames.dirty = false;
        }
        self.frames.full_due = Some(full_due);
        self.frames.other_due = (full_due < looks_ahead).then_some(full_due);
        // The theme's decorations moving: their next frame (a low rate; none while the
        // window is hidden, unfocused or idle).
        if let Some(at) = self.workbench.decorations_deadline(Instant::now()) {
            window.request_redraw_at(at);
        }
    }

    /// While a profile records: the times handed for a frame that came (one a little late
    /// still counts), each owner once — who made this frame come.
    fn count_due_askers(&mut self, now: Instant) {
        if self.frames.asked.is_empty() {
            return;
        }
        let late = now + Duration::from_millis(2);
        let mut counted: Vec<&'static str> = Vec::new();
        for (owner, at) in self.frames.asked.drain(..) {
            if at <= late && !counted.contains(&owner) {
                counted.push(owner);
                kernel::probe::trigger(owner, kernel::probe::Cause::Deadline);
            }
        }
    }

    /// The frame asked for only so that the kept frame and the surface's spare image go back
    /// (nothing happened since, nothing else is due, nothing moves): they go without drawing
    /// one, and what's on screen stays. Drawing it would show a frame anew, which starts the
    /// wait for the spare image again — a whole frame every two seconds in a window standing
    /// still, for ever, and two images of it held. `true`: released, no frame.
    pub(super) fn release_without_frame(&mut self, now: Instant) -> bool {
        let only_release = !self.frames.dirty
            && self.frames.kept_frame_check.is_some_and(|at| at <= now)
            && self.frames.other_due.is_none_or(|at| now < at)
            && self.startup.splash.is_gone()
            && self.startup.first_frame_logged
            && !self.workbench.animating()
            && !self.workbench.loading()
            && self.workbench.decorations_deadline(now).is_none();
        let Some(renderer) = self.frames.renderer.as_mut().filter(|_| only_release) else {
            return false;
        };
        if renderer.capture_waits_for_frame() {
            return false;
        }
        // Each renderer at its own time (the floating layer's may come first): one not due
        // yet is looked at again then.
        let due = |r: &dyn Renderer| r.kept_frame_release_at().is_none_or(|at| at <= now);
        if due(renderer.as_ref()) {
            renderer.release_idle();
        }
        let mut later = renderer.kept_frame_release_at().filter(|at| *at > now);
        if let Some(floating) = &mut self.frames.floating {
            if due(floating.renderer.as_ref()) {
                floating.renderer.release_idle();
            }
            let its = floating.renderer.kept_frame_release_at().filter(|at| *at > now);
            later = match (later, its) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
        self.frames.kept_frame_check = later;
        true
    }

    /// The GPU device was lost (a driver reset, a resize that broke the shared texture): the
    /// renderer and the pages' importers are dead. Drop them, make a new one, let the pages
    /// sync again with fresh importers.
    fn recover_renderer(&mut self, window: &Arc<dyn Window>) {
        self.frames.renderer = None;
        self.frames.floating = None;
        self.frames.floating_failed = false;
        self.web_views.let_go_all_pages();
        let size = window.inner_size();
        let target: Arc<dyn RenderTarget> = Arc::new(window.handles());
        let factories: [&dyn RendererFactory; 1] = [&WgpuRendererFactory];
        for factory in factories {
            match factory.create(Arc::clone(&target), size, &self.shared.render_options) {
                Ok(renderer) => {
                    let description = renderer.description();
                    tracing::info!(renderer = %description, "renderer recovered");
                    self.workbench.set_renderer(description.clone());
                    self.frames.overlay = DebugOverlay::new(description);
                    self.workbench.set_backdrop_blur(renderer.backdrop_blur());
                    self.frames.renderer = Some(renderer);
                    break;
                }
                Err(e) => {
                    tracing::error!(driver = factory.name(), error = %e, "recovery failed");
                }
            }
        }
        window.request_redraw();
    }

    /// Tells the decorations how the window is: focused, shown, and whether the system asks
    /// for less motion or saves power (asked of the system every few seconds at most).
    fn sync_decorations_window(&mut self, window: &Arc<dyn Window>, now: Instant) {
        const ASK_EVERY: Duration = Duration::from_secs(5);
        let still = match self.window_state.system_still {
            Some((still, asked)) if now.saturating_duration_since(asked) < ASK_EVERY => still,
            _ => {
                let reduce = window.reduce_motion();
                // The views animating on their own read it too (Relax mode).
                ui_workbench::decorations::set_reduce_motion(reduce);
                let still = reduce || window.power_saving();
                self.window_state.system_still = Some((still, now));
                still
            }
        };
        let (focused, visible) = (self.window_state.focused, !self.window_state.occluded);
        self.workbench.set_decorations_window(focused, visible, still);
    }

    /// The next step of the resize diagnostic. `true` when the test is finished.
    pub(super) fn step_resize_test(&mut self) -> bool {
        let (Some(test), Some(window)) = (&mut self.frames.resize_test, &self.window) else {
            return false;
        };
        if test.steps == 0 {
            test.report();
            self.frames.resize_test = None;
            return true;
        }
        test.steps -= 1;
        let base = PhysicalSize::new(1600, 1000);
        let grow = (test.steps % 10) * 60;
        window.request_inner_size(PhysicalSize::new(base.width + grow, base.height + grow / 2));
        false
    }
}

/// The results of the `EDITOR_DEBUG_RESIZE` diagnostic.
#[derive(Debug)]
pub(super) struct ResizeTest {
    pub(super) steps: u32,
    pub(super) frames: Vec<Duration>,
    pub(super) skipped: u32,
}

impl ResizeTest {
    fn record(&mut self, duration: Duration, skipped: bool) {
        self.frames.push(duration);
        self.skipped += u32::from(skipped);
    }

    fn report(&self) {
        let mut sorted = self.frames.clone();
        sorted.sort();
        let pick = |q: f32| {
            sorted
                .get(((sorted.len() as f32 * q) as usize).min(sorted.len().saturating_sub(1)))
                .copied()
                .unwrap_or_default()
        };
        tracing::debug!(
            frames = sorted.len(),
            skipped = self.skipped,
            p50 = ?pick(0.5),
            p95 = ?pick(0.95),
            max = ?sorted.last().copied().unwrap_or_default(),
            "diagnostic resize"
        );
    }
}

/// What a drawn frame cost the renderer, for a profile recording (`kernel::probe`): what
/// the scene held and what the GPU was asked to do (nothing while nothing records).
pub(super) fn record_frame(stats: &render_api::FrameStats, layers: usize) {
    use kernel::probe::{count, level, recording};
    const OWNER: &str = "render";
    if !recording() {
        return;
    }
    let n = |v: usize| u64::try_from(v).unwrap_or(u64::MAX);
    count(OWNER, "draw calls", n(stats.draw_calls));
    count(OWNER, "quads", n(stats.quads));
    count(OWNER, "glyphs", n(stats.glyphs));
    count(OWNER, "layers", n(layers));
    count(OWNER, "pixels drawn", stats.drawn_pixels);
    count(OWNER, "blurs made", n(stats.blurs));
    level(OWNER, "draw calls a frame", n(stats.draw_calls));
    level(OWNER, "glyphs cached", n(stats.cached_glyphs));
}
