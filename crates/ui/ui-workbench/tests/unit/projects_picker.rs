use std::path::Path;

use super::*;
use crate::palette::Palette;

fn project(path: &str) -> RecentProject {
    let name = Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned());
    RecentProject {
        name: name.unwrap_or_default(),
        display: path.to_owned(),
        path: PathBuf::from(path),
        kind: None,
    }
}

/// `/w/a` in group "Clients" › "Inner", `/w/b` and `/w/d` in "Clients", `/w/c` in the
/// recent group.
fn sample() -> (RecentList, GroupId, GroupId) {
    let paths = ["/w/a", "/w/b", "/w/c", "/w/d"];
    let mut model = RecentProjects::of(paths.iter().map(PathBuf::from).collect());
    let clients = model.new_group("Clients", None);
    let inner = model.new_group("Inner", Some(clients));
    model.move_to(Path::new("/w/a"), inner, Place::Anywhere);
    model.move_to(Path::new("/w/b"), clients, Place::Anywhere);
    model.move_to(Path::new("/w/d"), clients, Place::Anywhere);
    let projects = paths.iter().map(|p| project(p)).collect();
    (RecentList { model, projects, sessions: Vec::new() }, clients, inner)
}

/// The list as the empty screen shows it, tall enough for every row.
fn paint(picker: &mut ProjectsPicker, recent: &RecentList) {
    paint_in(picker, recent, 2000.0);
}

/// The list as the empty screen shows it, `height` tall.
fn paint_in(picker: &mut ProjectsPicker, recent: &RecentList, height: f32) {
    let palette = Palette::dark();
    let ctx = Ctx::new(&palette, 1.0);
    let painter = Painter {
        text: &mut TextSystem::new(),
        scene: &mut Scene::default(),
        palette: &palette,
        icons: &Icons::builtin(),
    };
    picker.rebuild(None, recent);
    picker.paint_content(Rect::new(0.0, 0.0, 800.0, height), painter, &ctx);
}

/// A point at `part` of row `r`'s height.
fn at(r: Rect, part: f32) -> Point {
    Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height * part)
}

/// Drags the row of `from` to `to` and drops it.
fn drop_on(picker: &mut ProjectsPicker, from: &str, to: Point) -> Option<PickerAction> {
    picker.hit(center(row_of(picker, from)));
    picker.drag(to);
    picker.release()
}

fn moved(path: &str, into: GroupId, place: Place) -> Option<PickerAction> {
    Some(PickerAction::MoveProject { path: PathBuf::from(path), into, place })
}

/// The history's rows, as (depth, name), groups marked with `+` / `-`.
fn tree(picker: &ProjectsPicker) -> Vec<(usize, String)> {
    picker
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Group { name, depth, collapsed, .. } => {
                Some((*depth, format!("{}{name}", if *collapsed { "+" } else { "-" })))
            }
            Item::Project { project, depth, .. } => Some((*depth, project.name.clone())),
            _ => None,
        })
        .collect()
}

fn row_of(picker: &ProjectsPicker, name: &str) -> Rect {
    let at = picker.items.iter().position(|i| match i {
        Item::Group { name: n, .. } => n == name,
        Item::Project { project, .. } => project.name == name,
        _ => false,
    });
    let found = picker.rows.iter().find(|(_, i)| Some(*i) == at).map(|(r, _)| *r);
    found.unwrap_or_else(|| panic!("no row for {name}"))
}

fn center(r: Rect) -> Point {
    Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
}

#[test]
fn groups_show_nested_with_the_recent_one() {
    let (mut recent, clients, _) = sample();
    let mut picker = ProjectsPicker::default();
    paint(&mut picker, &recent);
    let open = format!("-{}", i18n::t("home.recent_group"));
    let expected = [
        (0, open.as_str()),
        (1, "c"),
        (0, "-Clients"),
        (1, "-Inner"),
        (2, "a"),
        (1, "b"),
        (1, "d"),
    ];
    let expected: Vec<_> = expected.iter().map(|(d, n)| (*d, (*n).to_owned())).collect();
    assert_eq!(tree(&picker), expected);
    // Collapsed: what it holds is hidden; it still counts it.
    recent.model.set_collapsed(clients, true);
    paint(&mut picker, &recent);
    assert_eq!(tree(&picker)[2..], [(0, "+Clients".to_owned())]);
    let count = picker.items.iter().find_map(|i| match i {
        Item::Group { id, count, .. } if *id == clients => Some(*count),
        _ => None,
    });
    assert_eq!(count, Some(3));
}

#[test]
fn a_click_on_a_group_toggles_it_on_release() {
    let (recent, clients, _) = sample();
    let mut picker = ProjectsPicker::default();
    paint(&mut picker, &recent);
    let point = center(row_of(&picker, "Clients"));
    assert_eq!(picker.hit(point), None, "a press waits for the release");
    assert!(picker.pressed());
    assert_eq!(picker.release(), Some(PickerAction::ToggleGroup(clients)));
    assert!(!picker.pressed());
    assert_eq!(picker.selected_group(), Some((clients, false)));
}

#[test]
fn dragging_a_project_into_a_group_or_between_two() {
    let (recent, clients, inner) = sample();
    let mut picker = ProjectsPicker::default();
    paint(&mut picker, &recent);
    // Onto a group's row: into it, where its order puts it.
    let to = center(row_of(&picker, "Inner"));
    assert_eq!(drop_on(&mut picker, "c", to), moved("/w/c", inner, Place::Anywhere));
    // Onto the history's title: the recent group.
    let to = center(picker.history.unwrap_or_default());
    assert_eq!(drop_on(&mut picker, "a", to), moved("/w/a", RECENT, Place::Anywhere));
    // The upper half of a project: before it; the lower half: before the next one, or at
    // the end.
    let b = row_of(&picker, "b");
    let before_b = Place::Before(PathBuf::from("/w/b"));
    assert_eq!(drop_on(&mut picker, "c", at(b, 0.3)), moved("/w/c", clients, before_b));
    let before_d = Place::Before(PathBuf::from("/w/d"));
    assert_eq!(drop_on(&mut picker, "c", at(b, 0.7)), moved("/w/c", clients, before_d));
    let d = row_of(&picker, "d");
    assert_eq!(drop_on(&mut picker, "c", at(d, 0.7)), moved("/w/c", clients, Place::End));
    // Onto itself: nowhere.
    assert_eq!(drop_on(&mut picker, "b", at(b, 0.7)), None, "onto itself");
    // Onto a project of the recent group: just into it (always by opening).
    let c = row_of(&picker, "c");
    assert_eq!(drop_on(&mut picker, "a", at(c, 0.3)), moved("/w/a", RECENT, Place::Anywhere));
}

#[test]
fn dragging_a_group_before_after_or_into_another() {
    let (recent, clients, inner) = sample();
    let mut picker = ProjectsPicker::default();
    paint(&mut picker, &recent);
    let row = row_of(&picker, "Clients");
    let group = |id, parent, before| Some(PickerAction::MoveGroup { id, parent, before });
    // Its top quarter: before it, as a sibling.
    assert_eq!(drop_on(&mut picker, "Inner", at(row, 0.1)), group(inner, None, Some(clients)));
    // Its middle: into it (an expanded one's bottom too).
    let recent_row = row_of(&picker, &i18n::t("home.recent_group"));
    assert_eq!(drop_on(&mut picker, "Inner", at(row, 0.9)), group(inner, Some(clients), None));
    // Never into itself or its subgroups.
    let inner_row = row_of(&picker, "Inner");
    assert_eq!(drop_on(&mut picker, "Clients", center(inner_row)), None);
    // The recent group takes no subgroups: below its top quarter, after it.
    assert_eq!(
        drop_on(&mut picker, "Inner", center(recent_row)),
        group(inner, None, Some(clients))
    );
    // The recent group itself moves.
    assert_eq!(
        drop_on(&mut picker, &i18n::t("home.recent_group"), at(row, 0.5)),
        group(RECENT, Some(clients), None)
    );
}

#[test]
fn a_drag_near_an_edge_scrolls_the_list() {
    let paths: Vec<String> = (0..20).map(|i| format!("/w/p{i}")).collect();
    let model = RecentProjects::of(paths.iter().map(PathBuf::from).collect());
    let projects = paths.iter().map(|p| project(p)).collect();
    let recent = RecentList { model, projects, sessions: Vec::new() };
    let mut picker = ProjectsPicker::default();
    paint_in(&mut picker, &recent, 400.0);
    assert!(!picker.is_animating());
    picker.hit(center(row_of(&picker, "p0")));
    let bottom = Point::new(200.0, picker.list.max_y() - 4.0);
    picker.drag(bottom);
    assert!(picker.is_animating(), "held at the bottom edge, it keeps scrolling");
    paint_in(&mut picker, &recent, 400.0);
    std::thread::sleep(std::time::Duration::from_millis(30));
    paint_in(&mut picker, &recent, 400.0);
    assert!(picker.scroll.offset() > 0.0);
    // Away from the edges, it stops.
    picker.drag(Point::new(200.0, picker.list.min_y() + picker.list.size.height / 2.0));
    assert!(!picker.is_animating());
    picker.release();
}

#[test]
fn filtering_shows_a_flat_list_with_the_group_path() {
    let (recent, _, _) = sample();
    let mut picker = ProjectsPicker::default();
    picker.type_text("inner");
    paint(&mut picker, &recent);
    let found: Vec<_> = picker
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Project { project, depth, place, .. } => {
                Some((project.name.clone(), *depth, place.clone()))
            }
            Item::Group { .. } => panic!("no groups while filtering"),
            _ => None,
        })
        .collect();
    assert_eq!(found, vec![("a".to_owned(), 0, Some("Clients / Inner".to_owned()))]);
}

#[test]
fn right_click_targets() {
    let (recent, clients, _) = sample();
    let mut picker = ProjectsPicker::default();
    paint(&mut picker, &recent);
    let target = picker.target_at(center(row_of(&picker, "Clients")));
    assert_eq!(target, Some(PickerTarget::Group(clients)));
    assert_eq!(
        picker.target_at(center(row_of(&picker, "b"))),
        Some(PickerTarget::Project { path: PathBuf::from("/w/b"), current: false })
    );
    let title = picker.history.unwrap_or_default();
    let corner = Point::new(title.min_x() + 4.0, title.min_y() + 4.0);
    assert_eq!(picker.target_at(corner), Some(PickerTarget::History));
    let button = picker.new_group.unwrap_or_default();
    assert_eq!(picker.hit(center(button)), Some(PickerAction::NewGroup));
}

/// The top bar's list offers closing the window's project, only while one is open.
#[test]
fn the_list_closes_the_current_project_only_when_there_is_one() {
    let (recent, ..) = sample();
    let closes = |picker: &ProjectsPicker| {
        picker
            .items
            .iter()
            .any(|i| matches!(i, Item::Action { action: PickerAction::CloseProject, .. }))
    };
    let mut picker = ProjectsPicker::default();
    picker.rebuild(None, &recent);
    assert!(!closes(&picker));
    picker.rebuild(Some(&project("/w/c")), &recent);
    assert!(closes(&picker));
}

/// The current project's kind offers its settings (a remote project's connection) as an
/// action of the list; a local project offers none.
#[test]
fn the_current_project_s_kind_offers_its_settings() {
    let (recent, ..) = sample();
    let edits = |picker: &ProjectsPicker| {
        picker.items.iter().find_map(|i| match i {
            Item::Action { label, action: PickerAction::EditProject(path), .. } => {
                Some((label.clone(), path.clone()))
            }
            _ => None,
        })
    };
    let mut picker = ProjectsPicker::default();
    picker.rebuild(Some(&project("/w/c")), &recent);
    assert_eq!(edits(&picker), None, "a local project");
    let mut remote = project("/w/c");
    remote.kind = Some(workbench_model::ProjectKind {
        icon: "refresh",
        label: "FTP".into(),
        detail: None,
        edit: Some(workbench_model::ProjectEdit {
            label: "Remote Connection…".into(),
            menu_label: "Edit Connection…".into(),
            icon: "gear",
        }),
    });
    picker.rebuild(Some(&remote), &recent);
    assert_eq!(edits(&picker), Some(("Remote Connection…".into(), PathBuf::from("/w/c"))));
}

fn sample_sessions() -> Vec<HomeSession> {
    let file = |path: &str| HomeFile {
        name: Path::new(path).file_name().unwrap().to_string_lossy().into_owned(),
        detail: String::new(),
        path: PathBuf::from(path),
        icon: None,
    };
    let window = |name: &str, project: bool, files: &[&str]| HomeSessionWindow {
        name: name.to_owned(),
        detail: String::new(),
        project,
        path: project.then(|| PathBuf::from("/w").join(name)),
        files: files.iter().map(|f| file(f)).collect(),
    };
    vec![
        HomeSession {
            id: 1,
            when: "2 hours ago".into(),
            crashed: true,
            windows: vec![window("shop", true, &["/w/shop/a.php"]), window("alone", false, &[])],
        },
        HomeSession { id: 2, when: "2 days ago".into(), crashed: false, windows: Vec::new() },
    ]
}

#[test]
fn past_sessions_show_as_a_collapsed_tree_that_expands_and_reopens() {
    let (mut recent, ..) = sample();
    recent.sessions = sample_sessions();
    let mut picker = ProjectsPicker::default();
    paint(&mut picker, &recent);
    let sessions = || {
        |i: &Item| {
            matches!(i, Item::Session { .. } | Item::SessionWindow { .. } | Item::SessionFile(_))
        }
    };
    assert_eq!(picker.items.iter().filter(|i| sessions()(i)).count(), 2, "collapsed at first");
    // A click on the session's row expands it (its windows below).
    let (row, _) = *picker
        .rows
        .iter()
        .find(|(_, i)| matches!(picker.items[*i], Item::Session { id: 1, .. }))
        .unwrap();
    let action = picker.hit(center(row)).expect("a session's row");
    assert_eq!(action, PickerAction::ToggleSession(1));
    assert!(action.keeps_open() && picker.toggle_session(&action));
    paint(&mut picker, &recent);
    let windows: Vec<&Item> =
        picker.items.iter().filter(|i| matches!(i, Item::SessionWindow { .. })).collect();
    assert_eq!(windows.len(), 2);
    // A window with files expands to them; one without has nothing to expand.
    let alone =
        picker.items.iter().find(|i| matches!(i, Item::SessionWindow { index: 1, .. })).unwrap();
    assert_eq!(alone.action(), None);
    // A project's window: its row opens the project, its chevron shows its files.
    let (shop, _) = *picker
        .rows
        .iter()
        .find(|(_, i)| matches!(picker.items[*i], Item::SessionWindow { index: 0, .. }))
        .unwrap();
    assert_eq!(picker.hit(center(shop)), Some(PickerAction::OpenRecent(PathBuf::from("/w/shop"))));
    let chevron = Point::new(shop.min_x() + 18.0, center(shop).y);
    let action = picker.hit(chevron).expect("its chevron");
    assert_eq!(action, PickerAction::ToggleSessionWindow(1, 0));
    assert!(picker.toggle_session(&action));
    paint(&mut picker, &recent);
    let file = picker.items.iter().find(|i| matches!(i, Item::SessionFile(_))).unwrap();
    assert_eq!(file.action(), Some(PickerAction::OpenPath(PathBuf::from("/w/shop/a.php"))));
    // Its "Reopen" button.
    let (button, id) = picker.restore_buttons[0];
    assert_eq!(id, 1);
    assert_eq!(picker.hit(center(button)), Some(PickerAction::RestoreSession(1)));
    assert!(!PickerAction::RestoreSession(1).keeps_open());
}

#[test]
fn past_sessions_are_not_listed_while_filtering() {
    let (mut recent, ..) = sample();
    recent.sessions = sample_sessions();
    let mut picker = ProjectsPicker::default();
    picker.type_text("w");
    paint(&mut picker, &recent);
    assert!(!picker.items.iter().any(|i| matches!(i, Item::Session { .. })));
}

#[test]
fn arrows_fold_a_past_session_and_its_windows() {
    let (mut recent, ..) = sample();
    recent.sessions = sample_sessions();
    let mut picker = ProjectsPicker::default();
    paint(&mut picker, &recent);
    picker.selected =
        picker.items.iter().position(|i| matches!(i, Item::Session { id: 1, .. })).unwrap();
    assert!(picker.fold_selected_session(false), "→ expands");
    paint(&mut picker, &recent);
    assert!(picker.items.iter().any(|i| matches!(i, Item::SessionWindow { .. })));
    assert!(picker.fold_selected_session(false), "→ again: stays expanded");
    paint(&mut picker, &recent);
    assert!(picker.items.iter().any(|i| matches!(i, Item::SessionWindow { .. })));
    picker.selected = picker
        .items
        .iter()
        .position(|i| matches!(i, Item::SessionWindow { index: 0, .. }))
        .unwrap();
    assert!(picker.fold_selected_session(false));
    paint(&mut picker, &recent);
    assert!(picker.items.iter().any(|i| matches!(i, Item::SessionFile(_))), "its files");
    picker.selected =
        picker.items.iter().position(|i| matches!(i, Item::Session { id: 1, .. })).unwrap();
    assert!(picker.fold_selected_session(true), "← collapses");
    paint(&mut picker, &recent);
    assert!(!picker.items.iter().any(|i| matches!(i, Item::SessionWindow { .. })));
    picker.selected = picker.items.iter().position(|i| matches!(i, Item::Action { .. })).unwrap();
    assert!(!picker.fold_selected_session(true), "not a session: the key goes on");
}
