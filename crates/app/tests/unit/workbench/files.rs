use super::*;
use workbench_model::ResourceSource;

#[test]
fn file_urls_are_encoded() {
    assert_eq!(file_url(Path::new("/a b/ș.php")), "file:///a%20b/%C8%99.php");
}

/// A scratch project with two files.
fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("close-project-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.php"), "<?php\necho 1;\n").unwrap();
    std::fs::write(dir.join("b.php"), "<?php\necho 2;\n").unwrap();
    crate::workbench::canonical(&dir)
}

fn workbench() -> Workbench {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb
}

/// Waits (a few seconds at most) for `done`.
fn eventually(mut done: impl FnMut() -> bool) -> bool {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < until {
        if done() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    done()
}

/// Closing the project (its tabs closed first, as the app does) lets go of everything it
/// held — its data, its services (its language servers' registry), its watcher, its windows
/// and panels, the Task Manager's row — and the window shows the start screen, titled with
/// the app's name alone.
#[test]
fn closing_the_project_lets_go_of_it_and_shows_the_start_screen() {
    let dir = project("drop");
    let mut wb = workbench();
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join("a.php")).unwrap();
    let id = wb.data.id;
    let generations = Arc::downgrade(&wb.data.generations);
    let symbols = Arc::downgrade(&wb.data.symbols);
    let settings = wb.data.settings.as_ref().map(Arc::downgrade).unwrap();
    assert!(language_servers::projects::project(id).is_some());
    assert!(wb.tree_watcher.is_some());
    assert!(!wb.tools.sides.is_empty(), "the project panel shows");
    wb.refresh_task_data();
    assert!(wb.task_data.snapshot().projects.iter().any(|p| p.id == id));

    wb.freeze_session();
    while wb.tab_count() > 0 {
        wb.close_tab(wb.tab_count() - 1);
    }
    wb.close_project();

    assert!(wb.project_root().is_none());
    assert!(generations.upgrade().is_none(), "the project's data is dropped");
    assert!(settings.upgrade().is_none(), "its own settings are dropped");
    assert!(language_servers::projects::project(id).is_none(), "its servers are let go");
    assert!(eventually(|| symbols.upgrade().is_none()), "its symbol index is let go");
    assert_ne!(wb.data.id, id);
    assert!(wb.tree_watcher.is_none());
    assert!(wb.tools.sides.is_empty() && wb.tools.parked.is_empty());
    assert!(wb.tools.windows.is_empty() && wb.tools.bottom.is_none());
    assert!(wb.task_data.snapshot().projects.is_empty(), "the Task Manager no longer lists it");
    assert!(wb.home_visible());
    assert_eq!(wb.title(), kernel::brand::NAME);
    assert!(!wb.command_applies(crate::commands::CLOSE_PROJECT));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The session saved on closing comes back when the project is opened again.
#[test]
fn a_closed_project_reopens_with_its_tabs() {
    let dir = project("session");
    let mut wb = workbench();
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join("a.php")).unwrap();
    wb.open_file(&dir.join("b.php")).unwrap();
    wb.freeze_session();
    while wb.tab_count() > 0 {
        wb.close_tab(wb.tab_count() - 1);
    }
    wb.close_project();
    assert_eq!(wb.tab_count(), 0);

    wb.open_project(&dir).unwrap();
    assert!(wb.restore_session());
    let titles: Vec<String> = (0..wb.tab_count()).map(|i| wb.tab_title(i)).collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(titles, ["a.php", "b.php"]);
}

/// The tree's Rename…: a file renamed in its folder takes its open tab along (its title,
/// its path), a folder renamed moves the tabs of the files under it, and a name another
/// entry has is refused, nothing renamed.
#[test]
fn renaming_an_entry_takes_its_tabs_along_and_refuses_a_taken_name() {
    let dir = project("rename");
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(dir.join("lib/c.txt"), "c\n").unwrap();
    let mut wb = workbench();
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join("a.php")).unwrap();
    wb.open_file(&dir.join("lib/c.txt")).unwrap();

    assert!(wb.rename_entry(&dir.join("b.php"), &dir.join("d.php")), "an unopened file");
    assert!(dir.join("d.php").is_file() && !dir.join("b.php").exists());

    assert!(wb.rename_entry(&dir.join("lib"), &dir.join("src")), "a folder");
    assert!(dir.join("src/c.txt").is_file());
    let paths: Vec<_> = (0..wb.tab_count()).filter_map(|i| wb.tab_path(i)).collect();
    assert!(paths.contains(&dir.join("src/c.txt")), "{paths:?}");

    assert!(!wb.rename_entry(&dir.join("a.php"), &dir.join("d.php")), "taken");
    assert!(dir.join("a.php").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}
