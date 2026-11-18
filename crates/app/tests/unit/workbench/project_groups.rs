//! The projects list's groups through the workbench: a group named in its dialog, a
//! project moved into it from the menu, the group collapsed, the project taken out. The
//! list is the test thread's own file (`crate::recent`), never the user's.

use super::*;
use ui_workbench::ProjectsTarget;
use workspace::recent::{Place, RECENT};

/// Two project folders in the recent list, and a workbench with nothing open.
fn setup(name: &str) -> (Workbench, PathBuf, PathBuf, TextSystem) {
    let dir = std::env::temp_dir().join(format!("groups-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (a, b) = (dir.join("a"), dir.join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    crate::recent::update(|list| {
        *list = Default::default();
        list.open(&a);
        list.open(&b);
        true
    });
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let mut text = TextSystem::new();
    wb.paint(&mut text, &mut render_api::Scene::default());
    (wb, a, b, text)
}

#[test]
fn a_group_is_named_filled_collapsed_and_a_project_forgotten() {
    let (mut wb, a, b, mut text) = setup("flow");
    assert_eq!(wb.home.recent.projects.len(), 2);
    // "New group": its name asked in a dialog.
    wb.picker_action(ProjectsAction::NewGroup, false);
    assert!(wb.dialog_open());
    wb.edit_key(&mut text, EditKey::Backspace { word: false });
    wb.type_text(&mut text, "Clients");
    wb.edit_key(&mut text, EditKey::Enter { shift: false });
    assert!(!wb.dialog_open());
    let group =
        wb.home.recent.model.groups.iter().find(|g| g.id != RECENT).map(|g| (g.id, g.name.clone()));
    let Some((id, name)) = group else { panic!("the group is made") };
    assert_eq!(name, "Clients");
    // Moved into it from a project's menu.
    wb.home.recent_target = Some(ProjectsTarget::Project { path: a.clone(), current: false });
    assert!(wb.recent_menu(&format!("move:{:032x}", id.0)));
    assert_eq!(wb.home.recent.model.group_of(&a), id);
    // Collapsed (and kept so).
    wb.picker_action(ProjectsAction::ToggleGroup(id), false);
    assert!(crate::recent::load().group(id).is_some_and(|g| g.collapsed));
    // Dropped back in the recent group, then taken out of the list (its folder stays).
    let back =
        ProjectsAction::MoveProject { path: a.clone(), into: RECENT, place: Place::Anywhere };
    wb.picker_action(back, false);
    assert_eq!(wb.home.recent.model.group_of(&a), RECENT);
    wb.home.recent_target = Some(ProjectsTarget::Project { path: b.clone(), current: false });
    assert!(wb.recent_menu("forget"));
    assert_eq!(wb.home.recent.model.projects, vec![a.clone()]);
    assert!(b.is_dir());
    // Deleting the group keeps the projects.
    wb.home.recent_target = Some(ProjectsTarget::Group(id));
    assert!(wb.recent_menu("delete_group"));
    assert_eq!(wb.home.recent.model.groups.iter().map(|g| g.id).collect::<Vec<_>>(), [RECENT]);
    assert_eq!(wb.home.recent.projects.len(), 1);
    let _ = std::fs::remove_dir_all(a.parent().unwrap());
}

/// ⌦ on a recent project of the list: off the list (its folder stays).
#[test]
fn delete_takes_the_selected_project_off_the_list() {
    let (mut wb, a, b, mut text) = setup("delete");
    for _ in 0..8 {
        if matches!(wb.home.picker.confirm(), Some(ProjectsAction::OpenRecent(_))) {
            break;
        }
        wb.edit_key(&mut text, EditKey::Move { motion: Motion::Down, extend: false });
    }
    let Some(ProjectsAction::OpenRecent(selected)) = wb.home.picker.confirm() else {
        panic!("a recent project selected");
    };
    assert!(wb.edit_key(&mut text, EditKey::Delete));
    let left: Vec<PathBuf> = wb.home.recent.projects.iter().map(|p| p.path.clone()).collect();
    assert_eq!(left.len(), 1);
    assert!(!left.contains(&selected), "the selected one went");
    assert!(a.is_dir() && b.is_dir(), "their folders stay");
    let _ = std::fs::remove_dir_all(a.parent().unwrap());
}
