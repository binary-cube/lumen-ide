use super::*;

#[test]
fn header_runs_cover_the_text() {
    let red = Rgba::rgb(0xff0000);
    let r = runs("function stop()", &[(0..8, red), (9..13, red)]);
    let joined: String = r.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(joined, "function stop()");
    assert_eq!(r[0], ("function".to_owned(), Some(red)));
    assert_eq!(r[1].1, None);
}

#[test]
fn header_wraps_after_a_space_keeping_colors() {
    let red = Rgba::rgb(0xff0000);
    let runs = vec![("function".to_owned(), Some(red)), (" stop(int $speed)".to_owned(), None)];
    let lines = wrap(&runs, 16);
    let text: Vec<String> =
        lines.iter().map(|line| line.iter().map(|(s, _)| s.as_str()).collect()).collect();
    assert_eq!(text, ["function ", "stop(int $speed)"]);
    assert_eq!(lines[0][0], ("function".to_owned(), Some(red)));
}

/// A panel of `rows` outline rows painted `w` × `h` at scale 1.
fn painted(panel: &mut StructurePanel, rows: usize, w: f32, h: f32) {
    panel.nodes = (0..rows)
        .map(|i| Node {
            label: format!("item{i}"),
            glyph: "m",
            depth: 0,
            offset: i,
            symbol: i,
            active: false,
            private: false,
            note: None,
        })
        .collect();
    let (palette, icons) = (ui_components::Palette::dark(), icons::Icons::builtin());
    let (mut text, mut scene) = (TextSystem::new(), Scene::default());
    panel.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, w, h),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
}

/// How far in from the panel's right edge its scrollbar's lane is pressed.
const LANE_INSET: f32 = 4.0;

/// Its scrollbar, shown once scrolled: its thumb pressed, dragged and let go scrolls the
/// outline; painted again, its fade wants a frame.
#[test]
fn the_outline_scrolls_by_its_thumb() {
    let mut panel = StructurePanel::default();
    let (w, h) = (300.0, 400.0);
    painted(&mut panel, 300, w, h);
    // Hidden at rest: a scroll shows it.
    assert!(panel.scroll(1.0));
    painted(&mut panel, 300, w, h);
    let thumb = Point::new(w - LANE_INSET, 20.0);
    panel.hover(thumb);
    assert_eq!(panel.click(thumb), None, "the thumb takes the press");
    assert!(panel.drag(Point::new(thumb.x, thumb.y + 60.0)));
    assert!(panel.scroll > 0.0, "the outline scrolled");
    assert!(panel.release(), "the thumb let go");
    painted(&mut panel, 300, w, h);
    assert!(panel.frame_deadline().is_some(), "it fades out after a while");
}
