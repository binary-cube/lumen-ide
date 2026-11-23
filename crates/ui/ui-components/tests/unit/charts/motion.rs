use super::*;
use charts_model::geometry::{Mark, MarkShape};

fn bar(height: f32) -> Mark {
    Mark {
        series: 0,
        index: 0,
        shape: MarkShape::Bar(Rect::new(10.0, 100.0 - height, 20.0, height)),
    }
}

/// The first data grows its bars from their base; at the start nothing stands, once over
/// they're whole and nothing moves.
#[test]
fn bars_grow_from_their_base_and_settle() {
    let mut m =
        Motion { asked: ChartMotion { millis: 40, ..ChartMotion::default() }, ..Motion::default() };
    m.data(true, &[], None);
    assert!(m.moving());
    let (shape, _) = m.mark(&bar(50.0), 100.0);
    let MarkShape::Bar(r) = shape else { panic!() };
    assert!(r.size.height < 25.0, "just begun: {r:?}");
    std::thread::sleep(Duration::from_millis(60));
    let (shape, alpha) = m.mark(&bar(50.0), 100.0);
    assert_eq!((shape, alpha), (bar(50.0).shape, 1.0));
    m.settle();
    assert!(!m.moving() && m.transition.is_none());
}

/// New data: a bar moves from its old height to its new one.
#[test]
fn a_bar_moves_to_its_new_value() {
    let mut m = Motion {
        asked: ChartMotion { millis: 10_000, ..ChartMotion::default() },
        ..Motion::default()
    };
    m.data(false, &[bar(20.0)], None);
    let (shape, _) = m.mark(&bar(80.0), 100.0);
    let MarkShape::Bar(r) = shape else { panic!() };
    assert!(r.size.height >= 19.0 && r.size.height < 30.0, "near its old height: {r:?}");
}

/// No motion asked: everything at once, nothing moving.
#[test]
fn no_motion_is_at_once() {
    let mut m = Motion { asked: ChartMotion::none(), ..Motion::default() };
    m.data(true, &[], None);
    m.lit();
    assert!(!m.moving());
    assert_eq!(m.mark(&bar(50.0), 100.0), (bar(50.0).shape, 1.0));
    assert_eq!(m.lit_amount(), 1.0);
}

/// Each mark finds its earlier self by series and index, whatever the order the marks came
/// in, fast enough for a scatter's many points every frame.
#[test]
fn a_mark_finds_its_earlier_self_among_many() {
    let point = |series, index, x: f32| Mark {
        series,
        index,
        shape: MarkShape::Point { center: Point::new(x, 0.0), radius: 2.0 },
    };
    let mut m = Motion::default();
    // In reverse, two series mixed.
    let earlier: Vec<Mark> = (0..50_000).rev().map(|i| point(i % 2, i / 2, i as f32)).collect();
    m.data(false, &earlier, None);
    let started = Instant::now();
    for i in 0..50_000 {
        let found = m.earlier(&point(i % 2, i / 2, 0.0)).map(|b| b.shape);
        assert_eq!(found, Some(point(0, 0, i as f32).shape));
    }
    assert!(started.elapsed() < Duration::from_millis(500), "{:?}", started.elapsed());
    assert!(m.earlier(&point(3, 0, 0.0)).is_none(), "a new series: none");
}
