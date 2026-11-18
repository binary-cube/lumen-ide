use super::*;

use history::Retention;
use history::application::keep;

fn view_with(name: &str) -> (HistoryView, PathBuf) {
    let base = std::env::temp_dir().join(format!("history-view-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let file = base.join("a.php");
    std::fs::write(&file, "v2").unwrap();
    let store = Arc::new(DiskHistory::new(base.join("history")));
    keep(store.as_ref(), &Retention::default(), "a.php", b"v1", RevisionKind::BeforeSave, 1_000)
        .unwrap();
    keep(store.as_ref(), &Retention::default(), "a.php", b"v2", RevisionKind::Saved, 2_000)
        .unwrap();
    let mut view = HistoryView::default();
    view.set_data(&HistoryData { store, path: "a.php".into(), file });
    read(&mut view);
    (view, base)
}

/// The versions' read (on a worker) waited for.
fn read(view: &mut HistoryView) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while view.reading.is_some() && std::time::Instant::now() < deadline {
        view.take_read();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// The fixed tab follows the active file: its versions read; one outside the project (or
/// no project): said so, no list; a view's tab in front changes nothing.
#[test]
fn the_fixed_tab_follows_the_active_file() {
    let (_, base) = view_with("follow");
    let project = kernel::ProjectId::new();
    let store = Arc::new(DiskHistory::new(base.join("history")));
    let mut view = HistoryView::default();
    view.set_data(&FollowData { project: Some(project), root: base.clone(), store: Some(store) });
    assert_eq!(view.shown, None, "nothing asked yet");
    // Drawn (it shows): the project's poll asks the active file.
    assert!(crate::follow::poll(project, || Some(base.join("a.php"))));
    assert!(view.follow_active());
    read(&mut view);
    assert_eq!(view.revisions().len(), 2);
    assert!(crate::follow::poll(project, || Some(PathBuf::from("/elsewhere/b.php"))));
    assert!(view.follow_active());
    assert_eq!((view.shown, view.revisions().len()), (Some(Shown::Outside), 0));
    // A view's tab in front (a version opened to compare): the file stays followed.
    assert!(!crate::follow::poll(project, || None));
    assert!(!view.follow_active());
    assert_eq!(view.shown, Some(Shown::Outside));
    let mut none = HistoryView::default();
    none.set_data(&FollowData { project: None, root: PathBuf::new(), store: None });
    assert_eq!(none.shown, Some(Shown::NoFile), "no project: said so");
    crate::follow::forget(project);
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn versions_compare_with_the_file_and_come_back() {
    let (mut view, base) = view_with("compare");
    assert_eq!(view.revisions().len(), 2);
    // The newest matches the file now.
    assert_eq!(view.current, Some(view.revisions()[0].hash));
    let Some(ViewerAction::CompareBeside(input, title)) = compared(&mut view, 1) else {
        panic!("no comparison")
    };
    assert_eq!((input.left.1.as_str(), input.right.1.as_str()), ("v1", "v2"));
    assert!(title.starts_with("a.php · 1970-01-01"));
    let Some(ViewerAction::SetText { text, .. }) = view.revert(1) else { panic!("no revert") };
    assert_eq!(text, "v1");
    // The keyboard: ↓ selects, Enter compares.
    assert!(view.edit_key(ViewerKey::Down));
    assert!(view.edit_key(ViewerKey::Enter));
    assert!(matches!(prepared(&mut view), Some(ViewerAction::CompareBeside(..))));
    // Its action (its key, `r`, is the keymap's): the versions read again.
    assert!(view.command("reload"));
    read(&mut view);
    assert_eq!(view.revisions().len(), 2);
    assert!(!view.key('r'));
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn a_row_scrolled_under_the_header_takes_no_clicks_there() {
    let (mut view, base) = view_with("clip");
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut paint = |view: &mut HistoryView, text: &mut text_layout::TextSystem| {
        view.paint(ViewerPaint {
            bounds: Rect::new(0.0, 0.0, 900.0, 600.0),
            scale: 1.0,
            text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: false,
            waker: None,
        })
    };
    paint(&mut view, &mut text);
    // The first row, scrolled half out of the list.
    view.scroll = 20.0;
    paint(&mut view, &mut text);
    let list = view.list;
    let above = Point::new(list.min_x() + 40.0, list.min_y() - 4.0);
    assert_eq!(view.hit(above), None);
    assert_eq!(view.hit(Point::new(above.x, list.min_y() + 4.0)), Some(Hit::Row(0)));
    let _ = std::fs::remove_dir_all(base);
}

/// What the view prepared in the background (a comparison being read).
fn prepared(view: &mut HistoryView) -> Option<ViewerAction> {
    let started = std::time::Instant::now();
    while view.opening.is_busy() && started.elapsed() < std::time::Duration::from_secs(10) {
        view.poll();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    view.take_action()
}

fn compared(view: &mut HistoryView, i: usize) -> Option<ViewerAction> {
    view.compare(i);
    prepared(view)
}
