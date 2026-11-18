//! The editor area taking the window: the side windows hidden, then shown again as they
//! were; nothing to hide, nothing done.

use super::*;

#[test]
fn the_editor_area_takes_the_window_and_gives_it_back() {
    let dir = std::env::temp_dir().join(format!("editor-full-{}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = crate::workbench::canonical(&dir);
    let support = crate::modules::load(&extensions::ModuleSelection::default()).0;
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.open_project(&dir).unwrap();
    wb.paint(&mut TextSystem::new(), &mut render_api::Scene::default());
    if !wb.tools.sides.iter().any(|s| s.id == PROJECT_TOOL) {
        assert!(wb.toggle_project_panel());
    }
    let before: Vec<&str> = wb.tools.sides.iter().map(|s| s.id).collect();
    assert!(!before.is_empty());
    assert!(wb.toggle_editor_full(true, true));
    assert!(wb.editor_full());
    assert!(
        !wb.tools
            .sides
            .iter()
            .any(|s| matches!(s.dock, ui_workbench::Dock::Left | ui_workbench::Dock::Right)),
        "no side window"
    );
    assert!(wb.navigation().maximized, "its button says it goes back");
    assert!(wb.toggle_editor_full(true, true));
    assert!(!wb.editor_full());
    let after: Vec<&str> = wb.tools.sides.iter().map(|s| s.id).collect();
    assert_eq!(after, before, "as they were");
    // The height only, with nothing at the bottom: nothing to do.
    assert!(!wb.toggle_editor_full(false, true));
    assert!(!wb.editor_full());
    let _ = std::fs::remove_dir_all(dir);
}
