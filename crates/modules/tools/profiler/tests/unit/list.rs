use super::*;

/// A folder of recordings: only the files named as one are listed, the newest first, with
/// their date read from the name; the filter keeps those of a date; a click opens one.
#[test]
fn the_recordings_are_listed_newest_first_and_filtered() {
    let dir = std::env::temp_dir().join(format!("profiles-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("profile-20261120-090000.json")).unwrap();
    std::fs::write(dir.join("profile-20261119-101500.json"), "{}").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(dir.join("profile-20261120-140000.json"), "{}").unwrap();
    std::fs::write(dir.join("notes.json"), "{}").unwrap();
    let rows = listed(&dir);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        ["profile-20261120-140000.json", "profile-20261119-101500.json"],
        "files only"
    );
    assert_eq!(rows[0].when, when_made("profile-20261120-140000.json"), "in the user's zone");
    let mut view = RecordingsView::new();
    view.rows = Some(rows);
    view.filter.insert("2026-11-19");
    assert_eq!(view.shown().len(), 1);
    view.drawn = vec![(Rect::new(0.0, 100.0, 400.0, 30.0), 0)];
    match view.click(Point::new(10.0, 110.0)) {
        Some(ViewerAction::OpenFile { path, .. }) => {
            assert!(path.ends_with("profile-20261119-101500.json"))
        }
        other => panic!("{other:?}"),
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// A recording's moment, written in UTC in its name, shown in the user's zone (a name
/// that isn't one shown as it is).
#[test]
fn the_moment_is_shown_in_the_user_s_zone() {
    let local = chrono::DateTime::parse_from_rfc3339("2026-11-20T10:15:00Z")
        .unwrap()
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    assert_eq!(when_made("profile-20261120-101500.json"), local);
    assert_eq!(when_made("profile-20261120-101500-2.json"), local);
    assert_eq!(when_made("profile-x.json"), "", "not a recording's moment");
}

/// More recordings than the page holds: its scrollbar's thumb pressed and dragged down
/// scrolls it, through the view's own press, drag and release.
#[test]
fn the_page_s_thumb_dragged_scrolls_it() {
    let mut view = RecordingsView::new();
    view.rows = Some(
        (0..120)
            .map(|i| Listed {
                path: PathBuf::from(format!("profile-{i}.json")),
                name: format!("profile-{i}.json"),
                when: format!("moment {i}"),
                size: 1024,
            })
            .collect(),
    );
    let icons = icons::Icons::builtin();
    let palette = ui_components::Palette::dark();
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let bounds = Rect::new(0.0, 0.0, 900.0, 400.0);
    view.paint(ViewerPaint {
        bounds,
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    assert!(view.content_h > bounds.size.height, "longer than the page");
    let thumb = Point::new(bounds.max_x() - 4.0, 8.0);
    view.hover(thumb);
    assert!(view.click(thumb).is_none(), "the scrollbar takes the press");
    assert!(view.drag(Point::new(thumb.x, 160.0)), "the thumb moved");
    assert!(view.scroll > 0.0, "dragged down");
    assert!(view.release());
    assert!(!view.drag(Point::new(thumb.x, 300.0)), "let go: the moves don't reach it");
}

/// The filter is a part: a press on it gives it the keys (typed text goes in, one line, the
/// list back at its top), its clear button empties it and keeps them; Esc empties it, then
/// gives them up; Tab does nothing; a press elsewhere takes them away.
#[test]
fn the_filter_has_the_keys_after_a_press_on_it() {
    let mut view = RecordingsView::new();
    view.field = Rect::new(0.0, 0.0, 300.0, 34.0);
    let on = Point::new(20.0, 10.0);
    assert!(!view.wants_text());
    assert!(!view.text_input("x"), "no part has the keys");
    assert!(view.click(on).is_none());
    assert!(view.wants_text());
    view.hover(on);
    assert_eq!(view.cursor_at(on), Some(ViewerCursor::Text));
    view.scroll = 50.0;
    assert!(view.text_input("20\n26"));
    assert_eq!((view.filter.text(), view.scroll), ("2026", 0.0));
    assert!(view.edit_key_with(ViewerKey::Backspace, KeyReach::default()));
    assert_eq!(view.filter.text(), "202");
    assert!(!view.edit_key_with(ViewerKey::Tab, KeyReach::default()), "Tab isn't its");
    assert!(view.wants_text(), "nor does it move the keys");
    view.clear = Some(Rect::new(280.0, 8.0, 16.0, 16.0));
    assert!(view.click(Point::new(288.0, 16.0)).is_none());
    assert_eq!(view.filter.text(), "", "cleared");
    assert!(view.wants_text(), "still typing");
    view.clear = None;
    view.text_input("a");
    assert!(view.edit_key_with(ViewerKey::Escape, KeyReach::default()));
    assert_eq!(view.filter.text(), "");
    assert!(view.wants_text(), "the first Esc empties it");
    assert!(view.edit_key_with(ViewerKey::Escape, KeyReach::default()));
    assert!(!view.wants_text(), "the second gives the keys up");
    view.click(on);
    assert!(view.wants_text());
    view.click(Point::new(20.0, 200.0));
    assert!(!view.wants_text(), "a press elsewhere takes them away");
}
