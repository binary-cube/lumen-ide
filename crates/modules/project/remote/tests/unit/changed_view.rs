use super::*;

fn file(path: &str, modified: i64) -> RecentFile {
    RecentFile { path: path.into(), modified, size: 1, sent: true }
}

#[test]
fn a_span_up_to_now_moves_on_and_days_are_whole() {
    let now = 1_800_000_000;
    assert_eq!(Span::DEFAULT, Span::Hours(4), "the last 4 hours at first");
    assert_eq!(Span::Hours(4).bounds(now), (now - 4 * 3600, None));
    let rows = [file("a", now - 60), file("b", now - 3 * 3600), file("c", now - 5 * 3600)];
    assert_eq!(in_span(&rows, Span::Hours(4), now), [0, 1]);
    // Later, what was read ages out without reading again.
    assert_eq!(in_span(&rows, Span::Hours(4), now + 3600 + 1), [0]);
    // Days picked: from the first's midnight to the end of the last.
    let day = Date::new(2027, 1, 15).unwrap();
    let (from, to) = Span::Days(day, day).bounds(now);
    let to = to.unwrap();
    assert!((23 * 3600..=25 * 3600).contains(&(to - from)));
    let rows = [file("in", from), file("late", to), file("early", from - 1)];
    assert_eq!(in_span(&rows, Span::Days(day, day), now), [0]);
    // Today: from its local midnight.
    let (start, end) = Span::Today.bounds(now);
    assert_eq!((start, end), (local_time::day_start(local_time::today()).unwrap(), None));
}

#[test]
fn the_quick_spans_are_named() {
    let labels: Vec<String> = Span::QUICK.iter().map(|s| s.label()).collect();
    assert_eq!(labels[0], tr!("remote.changed_hours", count = 1));
    assert_eq!(labels[1], tr!("remote.changed_hours", count = 4));
    assert_eq!(labels[2], t("remote.changed_today"));
    assert_eq!(labels[3], tr!("remote.changed_hours", count = 24));
    assert_eq!(labels[4], tr!("remote.changed_days", count = 7));
}

#[test]
fn a_quick_choice_and_the_calendar_change_the_span() {
    let mut view = ChangedFiles {
        chips: vec![
            (Rect::new(0.0, 0.0, 10.0, 10.0), Chip::Quick(2)),
            (Rect::new(20.0, 0.0, 10.0, 10.0), Chip::Pick),
        ],
        ..ChangedFiles::default()
    };
    view.click(Point::new(5.0, 5.0));
    assert_eq!(view.span, Span::Today);
    view.click(Point::new(25.0, 5.0));
    assert!(view.picker.is_some() && view.wants_text(), "the calendar takes the keys");
    let day = Date::new(2026, 11, 2).unwrap();
    view.picker_event(PickerEvent::Apply(day, day.add_days(3)));
    assert_eq!(view.span, Span::Days(day, day.add_days(3)));
    assert!(view.picker.is_none());
    // The menu copies the shown rows' paths.
    view.rows = vec![file("a.php", 2), file("src/b.php", 1)];
    view.shown = vec![0, 1];
    view.menu = Some("a.php".into());
    assert!(view.command(COPY_ALL));
    assert_eq!(
        view.take_action(),
        Some(ViewerAction::Request(AppRequest::CopyText("a.php\nsrc/b.php".into())))
    );
}

/// Clicked into, the search takes the keys and narrows the list by the paths (the count
/// says how many of the span's); Esc empties it, then gives the keys back.
#[test]
fn the_search_narrows_the_files_by_their_paths() {
    let day = Date::new(2026, 11, 15).unwrap();
    let rows = vec![file("admin/a.php", 3), file("admin/b.css", 2), file("src/c.php", 1)];
    let mut view = ChangedFiles {
        stamps: rows.iter().map(timeline::Stamp::of).collect(),
        rows,
        field: Rect::new(0.0, 0.0, 200.0, 28.0),
        ..ChangedFiles::default()
    };
    // Every stamp on one day (whatever the zone): one heading.
    for s in &mut view.stamps {
        s.day = day;
    }
    view.make_list(vec![0, 1, 2]);
    assert_eq!(view.items.len(), 4);
    assert!(!view.wants_text());
    view.click(Point::new(10.0, 10.0));
    assert!(view.wants_text(), "clicked into: the keys");
    view.text_input("PHP");
    view.make_list(vec![0, 1, 2]);
    assert_eq!(view.shown, [0, 2]);
    assert_eq!(view.in_span, 3);
    assert_eq!(view.items[0], timeline::Item::Day { day, count: 2 });
    view.text_input(" admin");
    view.make_list(vec![0, 1, 2]);
    assert_eq!(view.shown, [0]);
    assert!(view.edit_key_with(ViewerKey::Escape, KeyReach::default()));
    view.make_list(vec![0, 1, 2]);
    assert_eq!(view.shown, [0, 1, 2], "emptied");
    assert!(view.wants_text());
    assert!(view.edit_key_with(ViewerKey::Escape, KeyReach::default()));
    assert!(!view.wants_text(), "the keys given back");
}

/// Scrolled into a day, a click on its pinned heading finds no file (the one under it is
/// hidden); below it, the file there.
#[test]
fn the_pinned_day_takes_its_clicks() {
    let day = Date::new(2026, 11, 15).unwrap();
    let rows: Vec<RecentFile> = (0..10).map(|i| file(&format!("f{i}.php"), 100 - i)).collect();
    let mut view = ChangedFiles {
        stamps: rows.iter().map(timeline::Stamp::of).collect(),
        rows,
        list: Rect::new(0.0, 0.0, 400.0, 200.0),
        ..ChangedFiles::default()
    };
    for s in &mut view.stamps {
        s.day = day;
    }
    view.make_list((0..10).collect());
    // Scrolled past the heading: three files above the top.
    view.scroll = timeline::PAD_TOP + timeline::DAY_H + 3.0 * timeline::FILE_H;
    assert_eq!(view.row_at(Point::new(50.0, 5.0)), None, "the pinned heading");
    let below = timeline::DAY_H + 1.0;
    let at = view.row_at(Point::new(50.0, below)).unwrap();
    assert_eq!(at, 4, "the fifth file, under the heading's height");
}
