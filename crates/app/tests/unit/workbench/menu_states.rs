use super::*;
use command_model::Requires;

fn workbench() -> Workbench {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb
}

/// A scratch folder with a file (a Git repository of its own when `git`).
fn folder(name: &str, git: bool) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("menu-states-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.php"), "<?php\necho 1;\n").unwrap();
    if git {
        let ok = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&dir)
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "git init in a scratch folder");
    }
    crate::workbench::canonical(&dir)
}

/// The project opened, and its repository looked for (in the background).
fn open(wb: &mut Workbench, dir: &std::path::Path) {
    wb.open_project(dir).unwrap();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !wb.repository_known() {
        assert!(std::time::Instant::now() < until, "the repository was never looked for");
        wb.poll_decorations();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn state(wb: &Workbench, id: &str) -> RowState {
    wb.row_state(&wb.menu_context(), id)
}

#[test]
fn with_nothing_open_the_project_and_file_rows_are_greyed_out_with_why() {
    let wb = workbench();
    let context = wb.menu_context();
    // Nothing open; the window itself can be kept on top (a test's window says so).
    let bare = command_model::MenuContext { keeps_on_top: true, ..Default::default() };
    assert_eq!(context, bare);
    // Git: every row, its window's too, for want of a project.
    for id in ["git.commit", "git.pull", "git.annotate", "git.compare_head", "git.rollback"] {
        assert_eq!(state(&wb, id), RowState::Disabled(Some(Requires::Project)), "{id}");
    }
    assert_eq!(state(&wb, "tool-window:git"), RowState::Disabled(Some(Requires::Project)));
    assert_eq!(state(&wb, "project.search"), RowState::Disabled(Some(Requires::Project)));
    assert_eq!(state(&wb, "file.save"), RowState::Disabled(Some(Requires::Tab)));
    assert_eq!(state(&wb, "code.comment"), RowState::Disabled(Some(Requires::TextFile)));
    assert_eq!(state(&wb, "tab.reopen_closed"), RowState::Disabled(Some(Requires::ClosedTab)));
    assert_eq!(state(&wb, "navigate.back"), RowState::Disabled(Some(Requires::BackHistory)));
    // What needs nothing stays.
    for id in ["file.new", "project.open", "app.settings", "view.zoom_in", "edit.copy"] {
        assert_eq!(state(&wb, id), RowState::Enabled, "{id}");
    }
    // The menus' rows greyed out: Git's among them, the always-there ones not.
    let disabled = wb.disabled_rows(&context);
    assert!(disabled.iter().any(|(id, _)| id == "git.commit"));
    assert!(!disabled.iter().any(|(id, _)| id == "file.new"));
    // Their keys do nothing either.
    assert!(!wb.command_applies(crate::commands::SAVE));
    assert!(wb.command_applies(crate::commands::NEW_FILE));
}

#[test]
fn a_project_without_git_greys_out_git_alone_and_a_file_brings_the_editors_rows() {
    let dir = folder("plain", false);
    let mut wb = workbench();
    open(&mut wb, &dir);
    let context = wb.menu_context();
    assert!(context.project && !context.repository && context.tabs == 0);
    assert_eq!(state(&wb, "git.commit"), RowState::Disabled(Some(Requires::Repository)));
    assert_eq!(state(&wb, "tool-window:git"), RowState::Disabled(Some(Requires::Repository)));
    assert_eq!(state(&wb, "project.search"), RowState::Enabled);
    assert_eq!(state(&wb, "file.save"), RowState::Disabled(Some(Requires::Tab)));
    wb.open_file(&dir.join("a.php")).unwrap();
    let context = wb.menu_context();
    assert!(context.text_file && context.file_on_disk && context.tabs == 1);
    assert_eq!(context.language.as_deref(), Some("php"));
    for id in ["file.save", "code.comment", "code.fold", "navigate.line", "file.reveal_in_project"]
    {
        assert_eq!(state(&wb, id), RowState::Enabled, "{id}");
    }
    // One tab: no other to go to; SQL's reformat not for PHP.
    assert_eq!(state(&wb, "tab.next"), RowState::Disabled(Some(Requires::OtherTabs)));
    assert!(matches!(state(&wb, "sql.format_statement"), RowState::Disabled(_)));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn in_a_git_project_with_a_file_its_rows_apply() {
    let dir = folder("git", true);
    let mut wb = workbench();
    open(&mut wb, &dir);
    wb.open_file(&dir.join("a.php")).unwrap();
    let context = wb.menu_context();
    assert!(context.project && context.repository);
    for id in ["git.commit", "git.changes", "git.log", "git.fetch", "tool-window:git"] {
        assert_eq!(state(&wb, id), RowState::Enabled, "{id}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// File › Close Project: greyed out (its keys doing nothing) without a project, there with
/// one, in File beside Open Project.
#[test]
fn close_project_needs_a_project() {
    use crate::commands as cmd;
    let mut wb = workbench();
    assert_eq!(state(&wb, "project.close"), RowState::Disabled(Some(Requires::Project)));
    assert!(!wb.command_applies(cmd::CLOSE_PROJECT));
    let dir = folder("close-project", false);
    open(&mut wb, &dir);
    assert_eq!(state(&wb, "project.close"), RowState::Enabled);
    assert!(wb.command_applies(cmd::CLOSE_PROJECT));
    assert_eq!(cmd::menu_path(cmd::CLOSE_PROJECT), cmd::menu_path(cmd::OPEN_PROJECT));
    assert!(!cmd::menu_path(cmd::CLOSE_PROJECT).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
