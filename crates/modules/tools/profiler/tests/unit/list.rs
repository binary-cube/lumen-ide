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
