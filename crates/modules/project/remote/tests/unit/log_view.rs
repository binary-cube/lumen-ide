use super::*;

#[test]
fn times_read_well() {
    assert_eq!(clock(3600 + 61), "01:01:01");
}

/// The log redraws when its project's transfers say something changed — not at every wake.
#[test]
fn the_log_redraws_when_a_transfer_is_heard() {
    let events = kernel::EventBus::new();
    let mut log = TransferLog::default();
    log.set_data(&LogData { project: kernel::ProjectId::new(), events: events.clone() });
    assert!(!log.poll(), "nothing heard");
    events.publish(&remote::events::TransferFinished {
        path: "a.txt".into(),
        action: Action::Upload,
        bytes: 3,
        error: None,
    });
    assert!(log.poll());
    assert!(!log.poll(), "heard once");
    events.publish(&remote::events::TransfersChanged { running: 1, to_upload: 0 });
    assert!(log.poll());
}

/// The list's scrollbar: its thumb pressed and dragged down scrolls the list.
#[test]
fn the_list_scrolls_by_its_thumb() {
    let mut log = TransferLog::default();
    let area = Rect::new(0.0, 0.0, 400.0, 200.0);
    let palette = ui_components::Palette::dark();
    log.max_scroll = 800.0;
    log.bar.paint(&mut render_api::Scene::default(), &palette, 1.0, area, 1000.0, 0.0);
    let edge = area.max_x() - 4.0;
    assert_eq!(log.click(Point::new(edge, 6.0)), None);
    assert!(log.drag(Point::new(edge, 80.0)), "the thumb followed");
    assert!(log.scroll > 0.0, "the list scrolled");
    assert!(log.release());
}
