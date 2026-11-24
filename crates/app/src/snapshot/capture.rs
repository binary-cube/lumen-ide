//! The screen drawn into a PNG with the software renderer, once what it shows has settled.

use super::*;

/// Renders the screen into `out`.
pub fn render(
    out: &Path,
    options: &Options,
    mut workbench: Workbench,
    themes: &mut Themes,
    fonts: JoinHandle<(TextSystem, FontFamily)>,
) -> anyhow::Result<()> {
    let (mut text, family) =
        fonts.join().map_err(|_| anyhow::anyhow!("loading the fonts failed"))?;
    workbench.set_font_family(family);
    workbench.resize(*SIZE, 2.0);
    Steps { wb: &mut workbench, text: &mut text }.run(options);
    // `EDITOR_SNAPSHOT_TOOL_BADGES=<tool>:<count>[:warning|error],…`: counts on tool windows'
    // buttons, as their modules put them.
    if let Ok(badges) = std::env::var("EDITOR_SNAPSHOT_TOOL_BADGES") {
        use workbench_model::ViewHost as _;
        for spec in badges.split(',') {
            let mut parts = spec.split(':');
            let (Some(tool), Some(count)) = (parts.next(), parts.next()) else { continue };
            let kind = match parts.next() {
                Some("warning") => workbench_model::BadgeKind::Warning,
                Some("error") => workbench_model::BadgeKind::Error,
                _ => workbench_model::BadgeKind::Count,
            };
            let count = count.parse().unwrap_or(0);
            let badge = workbench_model::ToolBadge { count, kind, label: String::new() };
            let tool: &'static str = Box::leak(tool.to_owned().into_boxed_str());
            workbench.set_tool_badge(tool, Some(badge));
        }
    }
    // The studio's work-in-progress theme on the window (as `AppRequest::PreviewTheme`).
    if options.studio_live {
        workbench.paint(&mut text, &mut Scene::default());
        let src = workbench.take_theme_preview();
        let id = src.as_deref().map(|src| themes.register_source(src));
        match id {
            Some(Ok(id)) => {
                if let Some((palette, theme)) = themes.palette_of(&id) {
                    workbench.set_palette(palette);
                    workbench.set_theme(theme);
                }
            }
            Some(Err(e)) => anyhow::bail!("the studio's theme is invalid: {e}"),
            None => anyhow::bail!("the studio asked for no preview"),
        }
    }
    // Appear animations (menus, modals) end and views finish loading (a document's images)
    // before the capture: the same picture every run.
    // What appears once loaded (a chart's slices fading in) has its second from then.
    let (mut animations, loads) =
        (Instant::now() + Duration::from_secs(1), Instant::now() + Duration::from_secs(3));
    // The first frame painted, kept for `EDITOR_SNAPSHOT_PARTIAL` (a frame before this one).
    let mut previous = Scene::default();
    workbench.paint(&mut text, &mut previous);
    while (workbench.animating() && Instant::now() < animations)
        || (workbench.loading() && Instant::now() < loads)
    {
        if workbench.loading() {
            animations = animations.max(Instant::now() + Duration::from_secs(1));
        }
        std::thread::sleep(Duration::from_millis(16));
        // What the views' work brought meanwhile taken, as the app does every frame.
        workbench.poll_viewers();
        workbench.paint(&mut text, &mut Scene::default());
    }
    // Still moving past every transition's end: given a little longer, then whatever still
    // says it moves is named (`moving: …`): the snapshots list the screens where a part
    // keeps asking for frames (on purpose: a scene that moves; else a part to mend).
    if workbench.animating() {
        let until = Instant::now() + SETTLE_MOVING;
        while workbench.animating() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(16));
            workbench.poll_viewers();
            workbench.paint(&mut text, &mut Scene::default());
        }
        let parts = workbench.moving_parts();
        if !parts.is_empty() {
            eprintln!("moving: {}", parts.join(", "));
        }
    }
    // The settings' content scrolled by the wheel (`SETTINGS_SCROLL=<px>`, in steps of
    // 120 px, a frame painted after each).
    if let Some(total) = parsed::<f32>("SETTINGS_SCROLL") {
        let mut left = total;
        while left.abs() > 0.5 {
            let step = left.clamp(-120.0, 120.0);
            workbench.settings_wheel(step);
            workbench.paint(&mut text, &mut Scene::default());
            left -= step;
        }
    }
    // The memory once everything settled (`EDITOR_SNAPSHOT_MEMORY=1`): measurements.
    if flag("MEMORY") {
        kernel::memory::release_free();
        let sample = crate::resources::measure_now();
        eprintln!("settled memory: {} MB", sample.ram_bytes / (1024 * 1024));
    }

    // What the views and modules told meanwhile, its time stopped (a ring caught part way).
    workbench.settle_notifications();
    // Carets (the editor's, the terminal's) caught lit, whatever the time it took.
    ui_kit::caret::reset();
    let mut scene = Scene::default();
    scene.reset(workbench.background());
    // The window's own buttons (as on Windows), not maximized: its corners cut.
    if options.window_controls.as_deref() == Some("1") {
        scene.window_corners = 16.0;
    }
    if options.splash {
        let frame = ui_workbench::SplashFrame {
            bounds: kernel::Rect::from_size(SIZE.to_size()),
            scale: 2.0,
            phase: 0.45,
            opacity: 1.0,
        };
        ui_workbench::Splash.paint(
            frame,
            concat!("v", env!("CARGO_PKG_VERSION")),
            &i18n::t("splash.preparing"),
            resources::splash_art().as_ref(),
            ui_workbench::Painter {
                text: &mut text,
                scene: &mut scene,
                palette: workbench.palette(),
                icons: workbench.icons(),
            },
        );
    } else {
        // `EDITOR_SNAPSHOT_ACCESS=1`: the screen's accessibility tree, made as it's painted,
        // written beside the picture (`<out>.access.txt`).
        let access = flag("ACCESS").then(ui_kit::access::begin);
        workbench.paint(&mut text, &mut scene);
        if let Some(mut frame) = access.map(ui_kit::access::Recording::take) {
            // As the app tells it: a control without a text of its own named by its tooltip.
            frame.name_from_tips(|bounds| workbench.access_tip(bounds.center()));
            let path = out.with_extension("access.txt");
            std::fs::write(&path, crate::snapshot_gpu::access_text(&frame))
                .with_context(|| format!("cannot write {}", path.display()))?;
        }
    }
    // `EDITOR_SNAPSHOT_DEBUG_OVERLAY=1`: the diagnostics overlay (F12) over it, with made-up
    // frames (one over its budget).
    if flag("DEBUG_OVERLAY") {
        let mut overlay = ui_workbench::DebugOverlay::new("Metal (Apple M2)".into());
        overlay.toggle();
        for i in 0..60u64 {
            let stats = render_api::FrameStats {
                quads: 812,
                glyphs: 2_431,
                draw_calls: 16,
                cached_glyphs: 965,
                drawn_pixels: 120_000,
                ..render_api::FrameStats::default()
            };
            overlay.record(std::time::Duration::from_micros(2_400 + (i % 7) * 900), stats);
            overlay.record_latency(std::time::Duration::from_millis(9 + i % 5));
        }
        overlay.record(std::time::Duration::from_millis(19), render_api::FrameStats::default());
        let area = workbench.overlay_area();
        let viewport = kernel::Size::new(SIZE.width as f32, SIZE.height as f32);
        let painter = ui_workbench::Painter {
            text: &mut text,
            scene: &mut scene,
            palette: workbench.palette(),
            icons: workbench.icons(),
        };
        overlay.paint(painter, area, viewport, 2.0);
    }
    let mut renderer =
        SoftwareRenderer::new(*SIZE).context("cannot create the snapshot surface")?;
    let icons = std::sync::Arc::clone(workbench.icons());
    // `EDITOR_SNAPSHOT_PARTIAL=1`: the picture drawn after an earlier frame, only where it
    // changed (the partial redraw), must be the one drawn whole; it fails otherwise.
    if flag("PARTIAL") {
        let mut glyphs = icons::WithIcons { text: &mut text, icons: &icons };
        renderer.render(&previous, &mut glyphs)?;
        let stats = renderer.render(&scene, &mut glyphs)?;
        let mut whole =
            SoftwareRenderer::new(*SIZE).context("cannot create the snapshot surface")?;
        whole.render(&scene, &mut glyphs)?;
        anyhow::ensure!(
            renderer.encode_png()? == whole.encode_png()?,
            "the partial redraw ({} pixels) differs from the whole one",
            stats.drawn_pixels
        );
        tracing::info!(drawn = stats.drawn_pixels, "partial redraw checked");
    }
    renderer.render(&scene, &mut icons::WithIcons { text: &mut text, icons: &icons })?;
    std::fs::write(out, renderer.encode_png()?)
        .with_context(|| format!("cannot write {}", out.display()))?;
    tracing::info!(path = %out.display(), "snapshot saved");
    // On the GPU too, when asked: after the picture, so measuring never changes it.
    crate::snapshot_gpu::measure(&mut workbench, &mut text, *SIZE, &scene)?;
    Ok(())
}
