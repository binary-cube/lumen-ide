//! A module's view over the window: one drawing all of it (Relax mode) leaves nothing
//! behind it drawn and the views behind told hidden; it alone gets the keys; Esc closes it
//! and the window is back as it was.

use super::*;

#[test]
fn a_view_covering_the_window_hides_what_is_behind_until_esc() {
    let dir = std::env::temp_dir().join(format!("modal-view-cover-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "abc\n".repeat(40)).unwrap();
    let dir = crate::workbench::canonical(&dir);
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join("a.txt")).unwrap();
    let mut text = TextSystem::new();
    let mut scene = Scene::default();
    wb.paint(&mut text, &mut scene);
    let has_card = |scene: &Scene, card: Rect| {
        scene.layers().iter().any(|l| l.quads.iter().any(|q| q.bounds == card))
    };
    let (card, sides) = (wb.card, wb.tools.sides.iter().map(|s| s.id).collect::<Vec<_>>());
    assert!(has_card(&scene, card), "the editor's card drawn");

    assert_eq!(wb.run_command(command_model::CommandId("relax.start")), Some(true));
    assert!(wb.modal_open(), "a modal: nothing behind gets anything");
    scene.reset(Rgba::BLACK);
    wb.paint(&mut text, &mut scene);
    assert!(!has_card(&scene, card), "nothing behind it drawn");
    assert!(wb.modal_view_deadline().is_some(), "it asks its frames at its own pace");
    // Typing goes nowhere behind it.
    wb.type_text(&mut text, "x");
    assert!(wb.modal_open());

    wb.edit_key(&mut text, EditKey::Escape);
    assert!(!wb.has_modal_view(), "Esc closed it");
    assert_eq!(wb.modal_view_deadline(), None, "no frames asked once closed");
    scene.reset(Rgba::BLACK);
    wb.paint(&mut text, &mut scene);
    assert!(has_card(&scene, wb.card), "the window drawn again");
    assert_eq!(wb.card, card);
    assert_eq!(
        wb.tools.sides.iter().map(|s| s.id).collect::<Vec<_>>(),
        sides,
        "panels as they were"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Tab reaches a module's view (a modal's, a panel's) with its ⇧: back, as the views read
/// it (the reach extended), the same as on the tabs.
#[test]
fn shift_tab_reaches_a_view_as_back() {
    use ui_workbench::{KeyReach, ViewerKey};
    let back = KeyReach { word: false, extend: true };
    assert_eq!(
        super::editing::panel_key(&EditKey::Tab { shift: true }),
        Some((ViewerKey::Tab, back))
    );
    assert_eq!(
        super::editing::panel_key(&EditKey::Tab { shift: false }),
        Some((ViewerKey::Tab, KeyReach::default()))
    );
}

/// A module's window with a text field (a data source's form): ⌘C copies the field's
/// selection, ⌘X cuts it, ⌘Z / ⇧⌘Z undo and redo in it — never the document behind it.
#[test]
fn a_modal_s_field_takes_copy_cut_and_undo() {
    /// A view that's one text field.
    struct Field(ui_kit::TextInput);
    impl ui_workbench::Viewer for Field {
        fn paint(&mut self, _ctx: ui_workbench::ViewerPaint<'_>) {}
        fn wants_text(&self) -> bool {
            true
        }
        fn text_input(&mut self, typed: &str) -> bool {
            self.0.insert(typed);
            true
        }
        fn selected_text(&self) -> Option<String> {
            Some(self.0.selected_text().to_owned())
        }
        fn command(&mut self, id: &str) -> bool {
            match id {
                "undo" => self.0.undo(),
                "redo" => self.0.redo(),
                "cut" => {
                    self.0.insert("");
                    true
                }
                "select_all" => {
                    self.0.select_all();
                    true
                }
                _ => false,
            }
        }
    }
    let dir = std::env::temp_dir().join(format!("modal-view-field-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "the document\n").unwrap();
    let dir = crate::workbench::canonical(&dir);
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join("a.txt")).unwrap();
    let mut text = TextSystem::new();
    wb.select_all(&mut text);
    wb.modal_view = Some(ModalView {
        viewer: Box::new(Field(ui_kit::TextInput::new("postgresql://admin@host/db"))),
        frame: None,
        popover: false,
        dragging: false,
        extension: "view-test-field".into(),
    });
    assert!(wb.select_all(&mut text), "⌘A in the field");
    assert_eq!(
        wb.copy().as_deref(),
        Some("postgresql://admin@host/db"),
        "the field's, not the document's"
    );
    assert_eq!(wb.cut(&mut text).as_deref(), Some("postgresql://admin@host/db"));
    assert_eq!(wb.modal_view.as_ref().unwrap().viewer.selected_text().as_deref(), Some(""));
    assert_eq!(wb.viewer_undo(false), Some(true), "⌘Z in the field");
    assert_eq!(
        wb.copy().as_deref(),
        Some("postgresql://admin@host/db"),
        "the text back, selected as it was before the cut"
    );
    assert_eq!(wb.viewer_undo(true), Some(true), "⇧⌘Z in the field");
    assert_eq!(wb.copy(), None, "cut again: nothing selected, nothing copied");
    assert_eq!(wb.tabs[wb.active].doc.buffer().text(), "the document\n", "the document untouched");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A module's window whose view zooms (a diagram's): a press reaches it with its clicks
/// (a double click, a drag from there), the pinch, ⌘+ / ⌘− and ⌘0 zoom it, its footer
/// shows the zoom; nothing behind it zooms.
#[test]
fn a_modal_view_that_zooms_gets_the_zoom_and_its_presses() {
    use std::sync::Mutex;
    #[derive(Default)]
    struct Seen {
        zoom: f32,
        presses: Vec<u32>,
    }
    struct Zooming(Arc<Mutex<Seen>>);
    impl ui_workbench::Viewer for Zooming {
        fn paint(&mut self, _ctx: ui_workbench::ViewerPaint<'_>) {}
        fn zoom_level(&self) -> Option<f32> {
            Some(self.0.lock().unwrap().zoom)
        }
        fn zoom(&mut self, factor: f32, _at: Point) -> bool {
            self.0.lock().unwrap().zoom *= factor;
            true
        }
        fn reset_zoom(&mut self) -> bool {
            self.0.lock().unwrap().zoom = 1.0;
            true
        }
        fn press(&mut self, _p: Point, clicks: u32) -> Option<ui_workbench::ViewerAction> {
            self.0.lock().unwrap().presses.push(clicks);
            None
        }
    }
    let dir = std::env::temp_dir().join(format!("modal-view-zoom-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "text\n").unwrap();
    let dir = crate::workbench::canonical(&dir);
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join("a.txt")).unwrap();
    let editor_zoom = wb.zoom_level();
    let seen = Arc::new(Mutex::new(Seen { zoom: 1.0, presses: Vec::new() }));
    wb.modal_view = Some(ModalView {
        viewer: Box::new(Zooming(Arc::clone(&seen))),
        frame: Some((Default::default(), "Diagram".into())),
        popover: false,
        dragging: false,
        extension: "view-test-zoom".into(),
    });
    if let Some((frame, _)) = wb.modal_view.as_mut().and_then(|m| m.frame.as_mut()) {
        frame.open();
    }
    let mut text = TextSystem::new();
    let mut scene = Scene::default();
    wb.paint(&mut text, &mut scene);
    let content = wb.modal_content();
    wb.click_modal_view(&mut text, content.center(), 2);
    assert_eq!(seen.lock().unwrap().presses, [2], "a double click, as a press of two");
    assert!(wb.zoom_by(2.0, content.center()), "the pinch");
    assert!(wb.zoom_step(true));
    assert!((seen.lock().unwrap().zoom - 2.5).abs() < 1e-4);
    assert!(wb.zoom_reset());
    assert_eq!(seen.lock().unwrap().zoom, 1.0);
    assert_eq!(wb.zoom_level(), editor_zoom, "nothing behind it zoomed");
    assert!(wb.modal_view_zooms());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A view over everything without a field gets a typed character as its key (a diagram's
/// `0` fits it, `1` is 100%), as in a tab.
#[test]
fn a_modal_view_without_a_field_gets_its_keys() {
    use std::sync::Mutex;
    struct Keys(Arc<Mutex<Vec<char>>>);
    impl ui_workbench::Viewer for Keys {
        fn paint(&mut self, _ctx: ui_workbench::ViewerPaint<'_>) {}
        fn key(&mut self, key: char) -> bool {
            self.0.lock().unwrap().push(key);
            true
        }
    }
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let keys = Arc::new(Mutex::new(Vec::new()));
    wb.modal_view = Some(ModalView {
        viewer: Box::new(Keys(Arc::clone(&keys))),
        frame: None,
        popover: false,
        dragging: false,
        extension: "view-test-keys".into(),
    });
    let mut text = TextSystem::new();
    assert_eq!(wb.modal_view_text(&mut text, "0"), Some(true));
    assert_eq!(wb.modal_view_text(&mut text, "1"), Some(true));
    assert_eq!(*keys.lock().unwrap(), ['0', '1']);
}
