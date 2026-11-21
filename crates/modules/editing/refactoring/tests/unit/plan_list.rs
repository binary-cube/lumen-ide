use std::path::PathBuf;

use refactor::{FilePlan, Occurrence};
use render_api::Scene;
use ui_components::Palette;

use super::*;

/// A plan of one file with `count` places.
fn plan_of(count: u32) -> Plan {
    let occurrence = |line: u32| Occurrence {
        range: 0..4,
        original: "send".into(),
        replacement: "post".into(),
        certainty: Certainty::Certain,
        chosen: true,
        line,
        line_text: "send();".into(),
        in_line: 0..4,
    };
    Plan {
        files: vec![FilePlan {
            path: PathBuf::from("/p/a.php"),
            occurrences: (0..count).map(occurrence).collect(),
        }],
        renames: Vec::new(),
    }
}

/// Paints `list` with `plan` in `area` at scale 1.
fn paint(list: &mut PlanList, plan: &Plan, area: Rect) {
    let palette = Palette::dark();
    let ds = Ctx::new(&palette, 1.0);
    let (mut text, mut scene) = (TextSystem::new(), Scene::default());
    list.paint(&mut scene, &mut text, &ds, plan, None, area);
}

/// A long list shows the shared scrollbar: its thumb pressed, dragged down and let go
/// scrolls the list; the rows leave it a lane of their own.
#[test]
fn a_long_list_s_thumb_is_dragged() {
    let mut plan = plan_of(40);
    let mut list = PlanList::default();
    let area = Rect::new(0.0, 0.0, 400.0, 200.0);
    paint(&mut list, &plan, area);
    assert!(list.max_scroll > 0.0);
    // The rows (not "every place", above the list) end before the bar's lane.
    let lane = Scrollbar::track_thickness(1.0);
    let rows = list.hits.iter().filter(|(_, hit)| *hit != Hit::All);
    assert!(rows.into_iter().all(|(r, _)| r.max_x() <= area.max_x() - lane));
    let mut clicked = None;
    let mut places = Places { list: &mut list, plan: Some(&mut plan), clicked: &mut clicked };
    // The thumb, at the top of the list's rows (below "every place" and the line).
    let thumb = Point::new(area.max_x() - 4.0, 60.0);
    assert!(places.hit(thumb));
    assert_eq!(places.press(thumb, 1), Press::Held);
    assert!(places.drag(Point::new(thumb.x, thumb.y + 40.0)));
    assert!(places.release());
    assert!(list.offset() > 0.0, "the list scrolled");
    assert!(clicked.is_none(), "no row was toggled");
}

/// The wheel scrolls the same list, within its reach.
#[test]
fn the_wheel_scrolls_a_long_list() {
    let plan = plan_of(40);
    let mut list = PlanList::default();
    paint(&mut list, &plan, Rect::new(0.0, 0.0, 400.0, 200.0));
    assert!(list.scroll(30.0));
    assert_eq!(list.offset(), 30.0);
    list.scroll(1e6);
    assert_eq!(list.offset(), list.max_scroll);
}
