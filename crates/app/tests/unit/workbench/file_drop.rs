//! Files and folders dropped from the file manager: how the items are sorted (what exists,
//! the limits), what a drop does (folders as projects here or in new windows, files as tabs,
//! a modal refusing it), what the overlay says, and the drag over a real window (the group
//! under the pointer, a modal refusing, its end). Only scratch folders are used.

use super::*;

fn paths(names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(PathBuf::from).collect()
}

/// `dir/…` is a folder, `gone/…` isn't there, anything else is a file.
fn kind(path: &Path) -> Option<Dropped> {
    let text = path.to_string_lossy();
    if text.starts_with("gone/") {
        None
    } else if text.starts_with("dir/") {
        Some(Dropped::Folder)
    } else {
        Some(Dropped::File)
    }
}

fn plan(names: &[&str]) -> DropPlan {
    plan_drop(&paths(names), kind)
}

fn opened(action: DropAction) -> DropOpen {
    match action {
        DropAction::Open(open) => open,
        other => panic!("expected something to open, got {other:?}"),
    }
}

#[test]
fn the_items_are_sorted_in_order_and_the_missing_ones_ignored() {
    let p = plan(&["a.php", "dir/shop", "gone/x", "b.png", "dir/blog"]);
    assert_eq!(p.folders, paths(&["dir/shop", "dir/blog"]));
    assert_eq!(p.files, paths(&["a.php", "b.png"]));
    assert_eq!((p.missing, p.skipped), (1, 0));
}

#[test]
fn a_huge_drop_opens_only_up_to_the_limits() {
    let mut names: Vec<String> = (0..MAX_FILES + 7).map(|i| format!("f{i}.txt")).collect();
    names.extend((0..MAX_PROJECTS + 2).map(|i| format!("dir/p{i}")));
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let p = plan(&names);
    assert_eq!(p.files.len(), MAX_FILES);
    assert_eq!(p.folders.len(), MAX_PROJECTS);
    assert_eq!(p.skipped, 7 + 2);
    assert_eq!(p.files.last(), Some(&PathBuf::from(format!("f{}.txt", MAX_FILES - 1))));
    // Thousands: the disk isn't asked about every one, all the rest are counted.
    let many: Vec<PathBuf> = (0..5000).map(|i| PathBuf::from(format!("f{i}"))).collect();
    let asked = std::cell::Cell::new(0);
    let p = plan_drop(&many, |path| {
        asked.set(asked.get() + 1);
        kind(path)
    });
    assert_eq!(p.files.len() + p.skipped, 5000);
    assert!(asked.get() <= 1000, "asked about {} items", asked.get());
}

#[test]
fn a_modal_refuses_the_drop() {
    assert_eq!(drop_action(plan(&["a.php"]), true, false), DropAction::Refused);
    assert_eq!(drop_action(plan(&["dir/shop"]), true, true), DropAction::Refused);
}

#[test]
fn nothing_left_on_disk_opens_nothing() {
    assert_eq!(drop_action(plan(&["gone/a", "gone/b"]), false, true), DropAction::Nothing);
}

#[test]
fn a_folder_opens_as_the_project_here_as_open_project_does() {
    // An empty window: here, straight.
    let open = opened(drop_action(plan(&["dir/shop"]), false, false));
    assert_eq!(open.here, Some("dir/shop".into()));
    assert!(open.elsewhere.is_empty() && open.files.is_empty());
    // A window with something open: through "Open Project…"'s question (here or new).
    let open = opened(drop_action(plan(&["dir/shop"]), false, true));
    assert_eq!(open.here, Some("dir/shop".into()));
}

#[test]
fn several_folders_each_get_their_own_project() {
    let open = opened(drop_action(plan(&["dir/a", "dir/b", "dir/c"]), false, false));
    assert_eq!(open.here, Some("dir/a".into()), "the first in the empty window");
    assert_eq!(open.elsewhere, paths(&["dir/b", "dir/c"]));
    let open = opened(drop_action(plan(&["dir/a", "dir/b"]), false, true));
    assert_eq!(open.here, None, "this window keeps its project");
    assert_eq!(open.elsewhere, paths(&["dir/a", "dir/b"]));
}

#[test]
fn files_open_here_as_tabs() {
    for has_content in [false, true] {
        let open = opened(drop_action(plan(&["a.php", "b.json"]), false, has_content));
        assert_eq!(open.files, paths(&["a.php", "b.json"]));
        assert_eq!((open.here.as_ref(), open.elsewhere.len()), (None, 0));
    }
}

#[test]
fn mixed_folders_become_projects_and_files_tabs() {
    // An empty window: the folder is its project, the files open in it.
    let open = opened(drop_action(plan(&["a.php", "dir/shop"]), false, false));
    assert_eq!(open.here, Some("dir/shop".into()));
    assert_eq!(open.files, paths(&["a.php"]));
    // Something open: the folder in a new window (asking "this window" would close the
    // files just dropped), the files here.
    let open = opened(drop_action(plan(&["a.php", "dir/shop"]), false, true));
    assert_eq!((open.here, open.elsewhere), (None, paths(&["dir/shop"])));
    assert_eq!(open.files, paths(&["a.php"]));
}

#[test]
fn the_overlay_says_what_the_drop_opens() {
    let text = drop_text(&drop_action(plan(&["dir/shop"]), false, false));
    assert_eq!(text.title, i18n::t("drop.title"));
    assert_eq!(text.lines, [i18n::tr!("drop.project", name = "shop")]);
    assert!(text.lines[0].contains("shop"));
    assert_eq!(text.icon, "folder-input");

    let text = drop_text(&drop_action(plan(&["a.php", "b.php", "c.php"]), false, true));
    assert_eq!(text.lines, [i18n::tr!("drop.files", count = 3)]);
    assert!(text.lines[0].contains('3'));

    let text = drop_text(&drop_action(plan(&["a.php", "dir/shop"]), false, true));
    let expected = format!(
        "{} · {}",
        i18n::tr!("drop.project_new_window", name = "shop"),
        i18n::tr!("drop.file", name = "a.php")
    );
    assert_eq!(text.lines, [expected]);

    let text = drop_text(&drop_action(plan(&["dir/a", "dir/b"]), false, false));
    assert_eq!(text.lines, [i18n::tr!("drop.projects", count = 2)]);

    let text = drop_text(&DropAction::Refused);
    assert_eq!(text.title, i18n::t("drop.refused_title"));
    assert_eq!(text.icon, "lock");
    assert_eq!(drop_text(&DropAction::Nothing).title, i18n::t("drop.nothing_title"));

    // Over the limits: said under it.
    let names: Vec<String> = (0..MAX_FILES + 3).map(|i| format!("f{i}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let text = drop_text(&drop_action(plan(&names), false, true));
    assert_eq!(text.lines.len(), 2);
    assert!(text.lines[1].contains('3'), "{:?}", text.lines);
    assert!(skipped_message(3).contains("50"));
}

#[test]
fn every_message_is_in_both_languages() {
    let catalogs = [
        ("ro", include_str!("../../../../domain/core/i18n/locales/ro.toml")),
        ("en", include_str!("../../../../domain/core/i18n/locales/en.toml")),
    ];
    for (locale, catalog) in catalogs {
        let section = catalog.split("\n[drop]\n").nth(1).expect("a [drop] section");
        let section = section.split("\n[").next().unwrap_or(section);
        for name in [
            "title",
            "project",
            "project_new_window",
            "projects",
            "file",
            "files",
            "over_limit",
            "skipped",
            "refused_title",
            "refused_detail",
            "nothing_title",
            "nothing_detail",
        ] {
            let line = format!("{name} = ");
            assert!(
                section.lines().any(|l| l.starts_with(&line)),
                "drop.{name} missing in {locale}"
            );
        }
    }
}

/// A scratch folder with two files and a sub-folder, the workbench showing it, laid out.
fn window(name: &str) -> (Workbench, PathBuf, TextSystem) {
    let dir = std::env::temp_dir().join(format!("file-drop-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("shop")).unwrap();
    std::fs::write(dir.join("a.txt"), "abc\n").unwrap();
    std::fs::write(dir.join("b.txt"), "def\n").unwrap();
    let dir = crate::workbench::canonical(&dir);
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let mut text = TextSystem::new();
    wb.paint(&mut text, &mut render_api::Scene::default());
    (wb, dir, text)
}

#[test]
fn the_disk_sorts_a_real_drop() {
    let (wb, dir, _) = window("plan");
    let p = wb.plan_drop(&[dir.join("a.txt"), dir.join("gone.txt"), dir.join("shop")]);
    assert_eq!(p.files, [dir.join("a.txt")]);
    assert_eq!(p.folders, [dir.join("shop")]);
    assert_eq!(p.missing, 1);
    // A link to a folder: its real place is the project's root (as "Open Project…").
    #[cfg(unix)]
    {
        let link = dir.join("link");
        std::os::unix::fs::symlink(dir.join("shop"), &link).unwrap();
        assert_eq!(wb.plan_drop(&[link]).folders, [dir.join("shop")]);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_drag_outlines_the_group_under_the_pointer() {
    let (mut wb, dir, mut text) = window("groups");
    wb.open_file(&dir.join("a.txt")).unwrap();
    wb.open_file(&dir.join("b.txt")).unwrap();
    assert!(wb.split_tab(wb.active, Split::Right, true));
    wb.layout();
    assert_eq!(wb.group_count(), 2);
    let left = wb.editors.groups[0].bounds;
    let right = wb.editors.groups[1].bounds;
    let center = |r: Rect| Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + 100.0);

    wb.drag_files_over(&[dir.join("a.txt")], center(left));
    assert!(wb.file_drag_active());
    assert!(wb.overlay_is_open(Overlay::FileDrop));
    assert!(!wb.modal_open(), "the overlay blocks nothing");
    let (action, group) = wb.file_drop_shown().unwrap();
    assert!(matches!(action, DropAction::Open(_)));
    assert_eq!(group, Some(0));
    wb.paint(&mut text, &mut render_api::Scene::default());

    assert!(wb.move_file_drag(center(right)), "another group: redrawn");
    assert_eq!(wb.file_drop_shown().unwrap().1, Some(1));
    assert!(!wb.move_file_drag(center(right)), "the same one");
    wb.focus_drop_group(center(right));
    assert_eq!(wb.editors.current, 1);

    // A folder alone outlines nothing.
    wb.drag_files_over(&[dir.join("shop")], center(left));
    assert_eq!(wb.file_drop_shown().unwrap().1, None);

    assert!(wb.end_file_drag());
    assert!(!wb.end_file_drag());
    assert!(!wb.overlay_is_open(Overlay::FileDrop));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_modal_open_refuses_the_drag() {
    let (mut wb, dir, mut text) = window("modal");
    wb.open_about();
    wb.drag_files_over(&[dir.join("a.txt")], Point::new(600.0, 400.0));
    assert_eq!(wb.file_drop_shown().unwrap(), (DropAction::Refused, None));
    assert_eq!(wb.active_overlay(), Some(Overlay::About), "the modal keeps the keyboard");
    wb.paint(&mut text, &mut render_api::Scene::default());
    let plan = wb.plan_drop(&[dir.join("a.txt")]);
    assert_eq!(wb.drop_action(plan), DropAction::Refused);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_window_takes_the_dropped_folder_as_its_project() {
    let (wb, dir, _) = window("empty");
    assert!(!wb.has_content());
    let plan = wb.plan_drop(&[dir.join("shop"), dir.join("a.txt")]);
    let open = opened(wb.drop_action(plan));
    assert_eq!(open.here, Some(dir.join("shop")));
    assert_eq!(open.files, [dir.join("a.txt")]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file dropped in from elsewhere is marked as outside the project; the project's own, a
/// view's tab and anything with no project open aren't.
#[test]
fn a_file_from_elsewhere_is_marked_outside_the_project() {
    use crate::workbench::paint::outside_project;
    use std::path::Path;
    let root = Some(Path::new("/work/shop"));
    assert!(outside_project(Path::new("/tmp/notes.php"), root));
    assert!(outside_project(Path::new("/work/shop-old/a.php"), root), "a sibling with its prefix");
    assert!(!outside_project(Path::new("/work/shop/src/a.php"), root));
    assert!(!outside_project(Path::new("/tmp/canvas.view-canvas"), root), "a view, not a file");
    assert!(!outside_project(Path::new("/tmp/notes.php"), None), "no project open");
}
