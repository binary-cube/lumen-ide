use super::*;

const VIEW: Rect = Rect {
    origin: Point { x: 100.0, y: 50.0 },
    size: kernel::Size { width: 800.0, height: 600.0 },
};

/// Zoomed at a point: the diagram's point under it stays under it; the limits hold.
#[test]
fn zooming_keeps_the_point_under_the_mouse() {
    let mut cam = Camera::default();
    let at = Point::new(400.0, 300.0);
    let before = cam.to_diagram(at, VIEW, 2.0);
    assert!(cam.zoom_at(1.5, at, VIEW, 2.0));
    let after = cam.to_diagram(at, VIEW, 2.0);
    assert!((before.x - after.x).abs() < 1e-3 && (before.y - after.y).abs() < 1e-3);
    assert!(!Camera { zoom: MAX_ZOOM, ..cam }.zoom_at(2.0, at, VIEW, 2.0), "at its limit");
    let back = cam.to_screen(after, VIEW, 2.0);
    assert!((back.x - at.x).abs() < 1e-3);
}

/// Fitted: the content in the middle, as large as fits, never past 100%.
#[test]
fn fitting_centers_the_content() {
    let mut cam = Camera::default();
    let content = Rect::new(-100.0, 0.0, 2000.0, 300.0);
    cam.fit(content, VIEW, 1.0);
    assert!((cam.zoom - 0.4).abs() < 1e-4, "800 / 2000");
    let left = cam.to_screen(Point::new(-100.0, 0.0), VIEW, 1.0);
    assert!((left.x - VIEW.min_x()).abs() < 1e-3);
    let mut small = Camera::default();
    small.fit(Rect::new(0.0, 0.0, 100.0, 50.0), VIEW, 2.0);
    assert_eq!(small.zoom, 1.0, "never past 100%");
    let c = small.to_screen(Point::new(50.0, 25.0), VIEW, 2.0);
    assert!((c.x - VIEW.center().x).abs() < 1e-3 && (c.y - VIEW.center().y).abs() < 1e-3);
}
