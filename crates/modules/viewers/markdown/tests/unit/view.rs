use text_layout::TextSystem;
use ui_components::Palette;

use super::*;

fn request() -> DiagramRequest {
    let r = mm::render("graph LR\nA[One] --> B[Two]", &Look::of(&Palette::default()))
        .unwrap_or_else(|_| panic!("drawn"));
    DiagramRequest {
        scene: r.scene,
        source: "graph LR\nA-->B".into(),
        title: Some("Flow: A/B".into()),
        dir: PathBuf::from("/tmp"),
    }
}

#[test]
fn copies_as_svg_source_and_picture() {
    let mut view = for_preview(request(), Look::of(&Palette::default()));
    assert!(view.run("diagram.copy-svg"));
    match view.take_action() {
        Some(ViewerAction::Request(AppRequest::CopyText(svg))) => assert!(svg.contains("One")),
        other => panic!("{other:?}"),
    }
    assert!(view.run("diagram.copy-source"));
    assert_eq!(
        view.take_action(),
        Some(ViewerAction::Request(AppRequest::CopyText("graph LR\nA-->B".into())))
    );
    // The picture is made on the pool.
    assert!(view.run("diagram.copy-png"));
    let started = std::time::Instant::now();
    let action = loop {
        let (action, _) = view.settle();
        if let Some(a) = action {
            break a;
        }
        assert!(started.elapsed().as_secs() < 20, "the picture arrives");
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    match action {
        ViewerAction::Request(AppRequest::CopyImage { width, height, rgba }) => {
            assert_eq!(rgba.len(), width * height * 4);
            assert!(width > 100);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn saving_asks_for_a_path_named_after_the_title() {
    let mut view = for_preview(request(), Look::of(&Palette::default()));
    assert!(view.run("diagram.save-svg"));
    match view.take_action() {
        Some(ViewerAction::Request(AppRequest::PickSavePath { name, .. })) => {
            assert_eq!(name, "Flow--A-B.svg")
        }
        other => panic!("{other:?}"),
    }
    let dir = std::env::temp_dir().join(format!("viewer-markdown-view-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("d.svg");
    assert!(view.command(&format!("picked-save:{}", path.display())));
    assert!(std::fs::read_to_string(&path).expect("written").starts_with("<svg"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_modal_draws_the_diagram_and_zooms() {
    let mut view = DiagramView::default();
    view.set_data(&request());
    view.tiles = Jobs::inline();
    let mut text = TextSystem::new();
    let mut scene = render_api::Scene::default();
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    let bounds = Rect::new(0.0, 0.0, 900.0, 600.0);
    let mut paint = |view: &mut DiagramView, scene: &mut render_api::Scene| {
        view.paint(ViewerPaint {
            bounds,
            scale: 2.0,
            text: &mut text,
            scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
    };
    paint(&mut view, &mut scene);
    assert!(scene.glyph_count() > 0, "its texts");
    assert!(view.shown_k.is_some(), "its shapes");
    let before = view.viewport.zoom;
    assert!(view.zoom(1.5, Point::new(450.0, 300.0)));
    assert!(view.viewport.zoom > before);
    assert!(view.key('0'));
    let mut scene = render_api::Scene::default();
    paint(&mut view, &mut scene);
    assert!(scene.glyph_count() > 0);
}
