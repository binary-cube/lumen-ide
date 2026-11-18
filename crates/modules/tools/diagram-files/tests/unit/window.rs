use super::*;
use render_api::Scene;
use text_layout::TextSystem;

/// A diagrams folder in a fresh scratch folder (removed at the end).
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn folder() -> Scratch {
    let dir = std::env::temp_dir()
        .join(format!("diagrams-window-{}", kernel::id::uuid_text(kernel::id::new_uuid())));
    for (path, text) in [
        ("arch.drawio", "<mxfile/>"),
        ("Flows/login.drawio", "<mxfile/>"),
        ("Flows/notes.txt", "not a diagram"),
        ("api.DRAWIO", "<mxfile/>"),
        (".hidden.drawio", "<mxfile/>"),
    ] {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    Scratch(dir)
}

fn paint(panel: &mut DiagramsPanel) {
    let (palette, icons) = (ui_components::Palette::dark(), icons::Icons::builtin());
    let mut scene = Scene::default();
    panel.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 300.0, 600.0),
        scale: 1.0,
        text: &mut TextSystem::new(),
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: false,
        waker: None,
    });
}

/// What the panel was doing done: polled until its list (read on a worker) is in, at most
/// a few seconds — a busy machine takes longer than one poll.
fn settle(panel: &mut DiagramsPanel) {
    settle_until(panel, |_| true);
}

/// [`settle`], also until `done` holds (a job's outcome is in: a folder made, a diagram
/// opened).
fn settle_until(panel: &mut DiagramsPanel, done: impl Fn(&DiagramsPanel) -> bool) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    panel.poll();
    while (panel.listing || !done(panel)) && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(2));
        panel.poll();
    }
}

/// The folders first, then the diagrams (by name, case aside); nothing else, nothing hidden.
#[test]
fn the_folder_s_diagrams_are_listed() {
    let dir = folder();
    let names: Vec<(usize, String)> = list(&dir.0).into_iter().map(|e| (e.depth, e.name)).collect();
    assert_eq!(
        names,
        [
            (0, "Flows".to_owned()),
            (1, "login.drawio".to_owned()),
            (0, "api.DRAWIO".to_owned()),
            (0, "arch.drawio".to_owned())
        ]
    );
    assert!(list(&dir.0.join("missing")).is_empty());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&dir.0, dir.0.join("loop")).unwrap();
        assert_eq!(list(&dir.0).len(), 4, "a link isn't followed");
    }
}

/// Shown: a diagram clicked opens; a folder clicked folds (what's in it hidden) and unfolds;
/// something changed in the folder: listed again.
#[test]
fn a_diagram_opens_and_a_folder_folds() {
    let dir = folder();
    let mut panel = DiagramsPanel::default();
    panel.show(dir.0.clone());
    paint(&mut panel);
    settle(&mut panel);
    paint(&mut panel);
    assert_eq!(panel.rows.len(), 4);
    let row = |panel: &DiagramsPanel, name: &str| {
        let i = panel.entries.iter().position(|e| e.name == name).unwrap();
        panel.rows.iter().find(|(_, at)| *at == i).map(|(r, _)| r.center())
    };
    let arch = row(&panel, "arch.drawio").unwrap();
    assert_eq!(
        panel.click(arch),
        Some(ToolPanelAction::Open { path: dir.0.join("arch.drawio"), line: 0 })
    );
    let flows = row(&panel, "Flows").unwrap();
    assert_eq!(panel.click(flows), None);
    paint(&mut panel);
    assert_eq!(panel.rows.len(), 3, "its diagram hidden");
    assert!(row(&panel, "login.drawio").is_none());
    panel.click(row(&panel, "Flows").unwrap());
    paint(&mut panel);
    assert_eq!(panel.rows.len(), 4);
    std::fs::write(dir.0.join("new.drawio"), "<mxfile/>").unwrap();
    panel.stale.store(true, Ordering::Release);
    settle(&mut panel);
    assert_eq!(panel.entries.len(), 5);
}

/// No diagrams yet: the folder they go in, said.
#[test]
fn no_diagrams_says_where_they_go() {
    let dir = folder();
    let mut panel = DiagramsPanel::default();
    panel.show(dir.0.join("empty"));
    paint(&mut panel);
    settle(&mut panel);
    paint(&mut panel);
    assert!(panel.rows.is_empty() && panel.dir.is_some() && !panel.listing);
}

/// A panel on a project in a scratch folder (its diagrams folder made, empty).
fn project() -> (DiagramsPanel, Scratch) {
    let root = std::env::temp_dir()
        .join(format!("diagrams-window-project-{}", kernel::id::uuid_text(kernel::id::new_uuid())));
    let dir = root.join(kernel::brand::PROJECT_DIR).join(FOLDER);
    std::fs::create_dir_all(&dir).unwrap();
    let mut panel = DiagramsPanel { root: Some(root.clone()), ..DiagramsPanel::default() };
    panel.show(dir);
    paint(&mut panel);
    settle(&mut panel);
    (panel, Scratch(root))
}

/// New Diagram: made and opened; New Folder: made, where the next goes.
#[test]
fn new_diagrams_and_folders_from_the_header() {
    let (mut panel, _root) = project();
    assert_eq!(panel.header().unwrap().len(), 2);
    panel.header_action("new-folder");
    settle_until(&mut panel, |p| p.current.is_some());
    let folder = panel.current.clone().unwrap();
    assert!(folder.ends_with("New folder"));
    panel.header_action("new-diagram");
    settle_until(&mut panel, |p| p.action.is_some());
    let Some(ToolPanelAction::Open { path, .. }) = panel.take_action() else { panic!("opened") };
    assert_eq!(path, folder.join("Untitled.drawio"), "in the folder made last");
    assert!(path.exists());
    assert!(panel.entries.iter().any(|e| e.path == path), "listed");
}

/// A row's menu: renamed in its row (↩: the app renames it, `.drawio` kept; Esc: nothing),
/// moved to a folder, deleted offered.
#[test]
fn an_entry_is_renamed_and_moved() {
    let (mut panel, _root) = project();
    panel.header_action("new-folder");
    settle_until(&mut panel, |p| p.current.is_some());
    panel.current = None;
    panel.header_action("new-diagram");
    settle_until(&mut panel, |p| p.action.is_some());
    let _ = panel.take_action();
    paint(&mut panel);
    let row = |panel: &DiagramsPanel, name: &str| {
        let i = panel.entries.iter().position(|e| e.name == name).unwrap();
        panel.rows.iter().find(|(_, at)| *at == i).map(|(r, _)| r.center()).unwrap()
    };
    let at = row(&panel, "Untitled.drawio");
    let menu = panel.menu_at(at).unwrap();
    assert!(
        menu.iter()
            .any(|e| matches!(e, ui_components::ViewMenuEntry::Item { id, .. } if id == "delete"))
    );
    assert!(panel.menu_command("rename"));
    assert!(panel.wants_text());
    panel.text_input("Architecture");
    assert!(panel.edit_key(ui_components::ViewerKey::Enter));
    let Some(ToolPanelAction::Rename { from, to }) = panel.take_action() else { panic!("renamed") };
    assert_eq!(to, from.with_file_name("Architecture.drawio"));
    panel.menu_at(at);
    panel.menu_command("rename");
    panel.text_input("Nope");
    panel.edit_key(ui_components::ViewerKey::Escape);
    assert!(panel.take_action().is_none() && !panel.wants_text());
    let folder = panel.entries.iter().find(|e| e.is_dir).unwrap().path.clone();
    panel.menu_at(at);
    assert!(panel.menu_command(&format!("move:{}", folder.display())));
    assert_eq!(panel.take_action(), Some(ToolPanelAction::Move { from, into: folder }));
}

/// A row's menu acts on the entry it was opened on, even after the list was made again
/// (another diagram sorted before it), and `.drawio` is known in any case.
#[test]
fn a_row_s_menu_follows_its_entry() {
    let (mut panel, _root) = project();
    panel.header_action("new-diagram");
    settle_until(&mut panel, |p| p.action.is_some());
    let _ = panel.take_action();
    paint(&mut panel);
    let i = panel.entries.iter().position(|e| e.name == "Untitled.drawio").unwrap();
    let at = panel.rows.iter().find(|(_, r)| *r == i).map(|(r, _)| r.center()).unwrap();
    panel.menu_at(at);
    let dir = panel.dir.clone().unwrap();
    std::fs::write(dir.join("A first.drawio"), "").unwrap();
    panel.list();
    settle(&mut panel);
    assert!(panel.menu_command("rename"));
    panel.text_input("Plan.DRAWIO");
    panel.edit_key(ui_components::ViewerKey::Enter);
    let Some(ToolPanelAction::Rename { from, to }) = panel.take_action() else { panic!("renamed") };
    assert!(from.ends_with("Untitled.drawio"));
    assert!(to.ends_with("Plan.DRAWIO"), "no second extension");
    assert_eq!(crate::window_actions::stem("x.DrawIO"), "x");
    assert_eq!(crate::window_actions::stem(".drawio"), ".drawio");
}

/// A row pressed and let go is a click (a diagram opens on release); dragged, its card
/// follows the mouse and it goes into the folder it's let go over — never where it is.
#[test]
fn a_diagram_is_dragged_into_a_folder() {
    let dir = folder();
    let mut panel = DiagramsPanel::default();
    panel.show(dir.0.clone());
    paint(&mut panel);
    settle(&mut panel);
    paint(&mut panel);
    let row = |panel: &DiagramsPanel, name: &str| {
        let i = panel.entries.iter().position(|e| e.name == name).unwrap();
        panel.rows.iter().find(|(_, at)| *at == i).map(|(r, _)| r.center()).unwrap()
    };
    let arch = row(&panel, "arch.drawio");
    assert_eq!(panel.press(arch, 1), None, "nothing on the press");
    assert!(panel.release());
    assert_eq!(
        panel.take_action(),
        Some(ToolPanelAction::Open { path: dir.0.join("arch.drawio"), line: 0 })
    );
    // Over its own folder: nowhere to go.
    panel.press(arch, 1);
    assert!(!panel.drag(Point::new(arch.x + 2.0, arch.y)), "a few pixels aren't a drag");
    let api = row(&panel, "api.DRAWIO");
    assert!(panel.drag(api));
    assert_eq!(panel.dragging.as_ref().unwrap().into, None, "where it is already");
    let flows = row(&panel, "Flows");
    assert!(panel.drag(flows));
    assert_eq!(panel.dragging.as_ref().unwrap().into, Some(dir.0.join("Flows")));
    paint(&mut panel);
    assert!(panel.release());
    assert_eq!(
        panel.take_action(),
        Some(ToolPanelAction::Move { from: dir.0.join("arch.drawio"), into: dir.0.join("Flows") })
    );
    // A folder isn't dropped into itself; a diagram of a folder goes up below the rows.
    panel.press(flows, 1);
    panel.drag(row(&panel, "login.drawio"));
    assert_eq!(panel.dragging.as_ref().unwrap().into, None);
    panel.release();
    assert_eq!(panel.take_action(), None);
    panel.press(row(&panel, "login.drawio"), 1);
    panel.drag(Point::new(50.0, 590.0));
    panel.release();
    assert_eq!(
        panel.take_action(),
        Some(ToolPanelAction::Move { from: dir.0.join("Flows/login.drawio"), into: dir.0.clone() })
    );
}
