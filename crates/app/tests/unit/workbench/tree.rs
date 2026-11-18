//! The project panel as it works today (step 6a of plan 14: the safety net before it moves
//! into its module) — what had no test: a new entry, deletion, the disk's changes, a file
//! brought from elsewhere, dragging, the tree's keyboard.

use super::*;
use std::sync::mpsc::Sender;

/// A project in a scratch folder (`a.txt`, `src/b.txt`), opened, its panel laid out.
fn opened(name: &str, support: LanguageSupport) -> (Workbench, PathBuf) {
    opened_with(name, support, Ports::local())
}

/// [`opened`], through `ports`.
fn opened_with(name: &str, support: LanguageSupport, ports: Ports) -> (Workbench, PathBuf) {
    let dir = std::env::temp_dir().join(format!("tree-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("a.txt"), "a").unwrap();
    std::fs::write(dir.join("src/b.txt"), "b").unwrap();
    let dir = crate::workbench::canonical(&dir);
    let mut wb = Workbench::new(Arc::new(support), ports);
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.open_project(&dir).unwrap();
    paint(&mut wb);
    (wb, dir)
}

/// A frame: the panels given the project, laid out.
fn paint(wb: &mut Workbench) {
    wb.paint(&mut TextSystem::new(), &mut render_api::Scene::default());
}

/// The project panel (the tree's module), shown on the left.
fn project_panel(wb: &mut Workbench) -> &mut SidePanel {
    wb.tools.sides.iter_mut().find(|s| s.id == PROJECT_TOOL).expect("the project panel is shown")
}

/// Where row `path` is drawn in the project panel (found by asking the panel what's there;
/// selects it), if it shows.
fn find_row(wb: &mut Workbench, path: &Path) -> Option<Point> {
    paint(wb);
    let side = project_panel(wb);
    let b = side.content;
    let mut y = b.min_y() + 2.0;
    while y < b.max_y() {
        let p = Point::new(b.min_x() + 60.0, y);
        if side.panel.context_at(p).is_some_and(|c| c.path() == Some(path)) {
            return Some(p);
        }
        y += 4.0;
    }
    None
}

fn row(wb: &mut Workbench, path: &Path) -> Point {
    find_row(wb, path).unwrap_or_else(|| panic!("{} isn't shown", path.display()))
}

fn support() -> LanguageSupport {
    crate::modules::load(&extensions::ModuleSelection::default()).0
}

/// Waits (a few seconds at most) for `done`, the app's loop polling meanwhile.
fn until(wb: &mut Workbench, done: impl Fn(&mut Workbench) -> bool) -> bool {
    // Long: with every test running, macOS delivers the disk's events late (up to ~50 s).
    until_for(wb, std::time::Duration::from_secs(60), done)
}

/// [`until`] for at most `wait`.
fn until_for(
    wb: &mut Workbench,
    wait: std::time::Duration,
    done: impl Fn(&mut Workbench) -> bool,
) -> bool {
    let deadline = std::time::Instant::now() + wait;
    while std::time::Instant::now() < deadline {
        wb.poll_fs();
        wb.poll_viewers();
        wb.poll_jobs();
        wb.poll_entry_changes();
        if done(wb) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    false
}

/// The project opens with its panel (the tree's module) on the left; ⌘1 hides it and brings
/// it back as it was (its open folders kept).
#[test]
fn the_project_panel_is_the_tree_s_module_and_keeps_its_state() {
    let (mut wb, dir) = opened("panel", support());
    assert!(wb.tools.side_on(ui_workbench::Dock::Left).is_some_and(|s| s.id == PROJECT_TOOL));
    assert!(wb.reveal_path_in_project(&dir.join("src/b.txt")));
    assert!(find_row(&mut wb, &dir.join("src/b.txt")).is_some());
    assert!(wb.toggle_project_panel());
    assert!(wb.tools.side_on(ui_workbench::Dock::Left).is_none());
    assert!(wb.toggle_project_panel());
    assert!(find_row(&mut wb, &dir.join("src/b.txt")).is_some(), "src still open");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A new file / folder typed in the tree: a name that's empty, has forbidden characters or
/// already exists is refused (nothing made, still typing); ↵ makes it (a file opens), Esc
/// gives up.
#[test]
fn a_new_entry_is_validated_made_or_given_up() {
    let (mut wb, dir) = opened("new", support());
    let mut text = TextSystem::new();
    let typing = |wb: &mut Workbench| project_panel(wb).panel.wants_text();
    assert!(wb.new_entry(false));
    assert_eq!(wb.focus, Focus::Side(ui_workbench::Dock::Left));
    wb.type_text(&mut text, "a.txt");
    wb.edit_key(&mut text, EditKey::Enter { shift: false });
    assert!(typing(&mut wb), "it exists: refused");
    for _ in 0..5 {
        wb.edit_key(&mut text, EditKey::Backspace { word: false });
    }
    wb.type_text(&mut text, "x?.txt");
    wb.edit_key(&mut text, EditKey::Enter { shift: false });
    assert!(typing(&mut wb) && !dir.join("x?.txt").exists(), "a forbidden character");
    for _ in 0..6 {
        wb.edit_key(&mut text, EditKey::Backspace { word: false });
    }
    wb.type_text(&mut text, "c.txt");
    wb.edit_key(&mut text, EditKey::Enter { shift: false });
    assert!(dir.join("c.txt").is_file());
    assert_eq!(wb.tabs[wb.active].doc.path(), Some(dir.join("c.txt").as_path()), "opened");
    // A folder, given up.
    assert!(wb.new_entry(true));
    wb.type_text(&mut text, "later");
    wb.edit_key(&mut text, EditKey::Escape);
    assert!(!typing(&mut wb));
    assert!(!dir.join("later").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Entries deleted (announced, whoever deleted them): the saved tabs of what went close,
/// the tree follows; only entries inside the project count (once).
#[test]
fn a_deletion_closes_the_saved_tabs_and_the_tree_follows() {
    let (mut wb, dir) = opened("delete", support());
    let outside = std::env::temp_dir().join("elsewhere.txt");
    let asked = [dir.join("src"), dir.join("src/b.txt"), outside];
    let roots = workspace::delete::deletion_roots(&asked, &dir);
    assert_eq!(roots, [dir.join("src")], "inside, not twice");
    wb.open_file(&dir.join("src/b.txt")).unwrap();
    wb.reveal_path_in_project(&dir.join("src/b.txt"));
    // As the trash would (without filling the user's): gone, then announced.
    std::fs::remove_dir_all(dir.join("src")).unwrap();
    wb.data.events.publish(&workspace::events::EntriesDeleted { paths: roots });
    assert!(wb.poll_entry_changes());
    assert!(wb.tabs.iter().all(|t| t.doc.path() != Some(dir.join("src/b.txt").as_path())));
    assert!(until(&mut wb, |wb| find_row(wb, &dir.join("src")).is_none()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A deleted file's saved tabs close in every editor group, not only the current one; the
/// group that was current stays current.
#[test]
fn a_deletion_closes_the_tabs_in_every_group() {
    let (mut wb, dir) = opened("delete-groups", support());
    wb.open_file(&dir.join("src/b.txt")).unwrap();
    wb.open_file(&dir.join("a.txt")).unwrap();
    // b.txt to a group on the right, a.txt left where it was.
    let b = wb.tabs.iter().position(|t| t.doc.path() == Some(dir.join("src/b.txt").as_path()));
    assert!(wb.split_tab(b.unwrap(), Split::Right, true));
    assert_eq!(wb.group_count(), 2);
    assert!(wb.focus_group(0));
    std::fs::remove_file(dir.join("src/b.txt")).unwrap();
    let paths = vec![dir.join("src/b.txt")];
    wb.data.events.publish(&workspace::events::EntriesDeleted { paths });
    assert!(wb.poll_entry_changes());
    assert_eq!(wb.group_count(), 1, "the group left empty went");
    assert_eq!(
        wb.tabs.iter().filter_map(|t| t.doc.path()).collect::<Vec<_>>(),
        [dir.join("a.txt")]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Files made / removed on disk by others show in the tree, as the watcher reports them
/// (announced); what it missed, at its look at everything again.
#[test]
fn the_tree_follows_the_disk() {
    let watching = crate::workbench::tests::FakeWatching::default();
    let ports = Ports { watching: Arc::new(watching.clone()), ..Ports::local() };
    let (mut wb, dir) = opened_with("disk", support(), ports);
    let heard: Arc<std::sync::Mutex<Vec<PathBuf>>> = Arc::default();
    let into = Arc::clone(&heard);
    let _listening = wb.data.events.subscribe(move |e: &workspace::events::EntriesChanged| {
        into.lock().unwrap().extend(e.paths.iter().cloned());
    });
    // Made on disk, as the watcher reports it: shown and announced.
    let new = dir.join("new.txt");
    std::fs::write(&new, "n").unwrap();
    watching.changed(std::slice::from_ref(&new));
    assert!(
        until(&mut wb, |wb| find_row(wb, &new).is_some()),
        "the tree shows a file made on disk"
    );
    assert_eq!(*heard.lock().unwrap(), vec![new.clone()], "and it was announced");
    // Removed: let go of.
    std::fs::remove_file(&new).unwrap();
    watching.changed(std::slice::from_ref(&new));
    assert!(
        until(&mut wb, |wb| find_row(wb, &new).is_none()),
        "the tree lets go of a file removed on disk"
    );
    // Made before watching started (unheard): the look at everything again finds it.
    let missed = dir.join("src/missed.txt");
    assert!(wb.reveal_path_in_project(&dir.join("src/b.txt")));
    std::fs::write(&missed, "m").unwrap();
    watching.rescan();
    assert!(until(&mut wb, |wb| find_row(wb, &missed).is_some()), "a look at everything again");
    let _ = std::fs::remove_dir_all(&dir);
}

/// What changed on disk while the window was left, the watcher silent (macOS loses events
/// while the disk is very busy): back in the window, the folders shown and the open files
/// are checked — off this thread — and follow; what didn't change isn't read.
#[test]
fn back_in_the_window_what_changed_meanwhile_shows() {
    let watching = crate::workbench::tests::FakeWatching::default();
    let ports = Ports { watching: Arc::new(watching), ..Ports::local() };
    let (mut wb, dir) = opened_with("away", support(), ports);
    let a = dir.join("a.txt");
    wb.open_file(&a).unwrap();
    let other = wb.tabs.len();
    // Left (only the time kept).
    wb.project_left();
    // Meanwhile, unheard: a file made, the open one written.
    let made = dir.join("made.txt");
    std::fs::write(&made, "m").unwrap();
    std::fs::write(&a, "changed meanwhile").unwrap();
    // Back.
    wb.project_brought_forward();
    assert!(wb.check_open_files(), "the open file read again");
    let tab = wb.tabs.iter().find(|t| t.doc.path() == Some(a.as_path())).unwrap();
    assert_eq!(tab.doc.buffer().to_string(), "changed meanwhile");
    assert!(until(&mut wb, |wb| find_row(wb, &made).is_some()), "the file made meanwhile shows");
    assert_eq!(wb.tabs.len(), other);
    // Back again, nothing changed: nothing read.
    assert!(!wb.check_open_files());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A module that brings `far.txt` to disk when asked (as a remote project does).
#[derive(Debug)]
struct Brings {
    asked: std::sync::Mutex<Option<Sender<Result<(), String>>>>,
}

impl workbench_model::ProjectService for Brings {
    fn attach(&self, _: &workbench_model::ProjectContext<'_>) -> Vec<kernel::Subscription> {
        Vec::new()
    }

    fn fetch(&self, path: &Path) -> Option<std::sync::mpsc::Receiver<Result<(), String>>> {
        if path.file_name()? != "far.txt" {
            return None;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        *self.asked.lock().unwrap() = Some(tx);
        Some(rx)
    }
}

/// A file only elsewhere: opening it asks its module to bring it, and it opens once here.
#[test]
fn a_file_brought_from_elsewhere_opens_once_here() {
    let brings = Arc::new(Brings { asked: std::sync::Mutex::new(None) });
    let mut support = support();
    support.project_services.push(Arc::clone(&brings) as Arc<dyn workbench_model::ProjectService>);
    let (mut wb, dir) = opened("far", support);
    let far = dir.join("far.txt");
    wb.open_file(&far).unwrap();
    assert!(wb.tabs.iter().all(|t| t.doc.path() != Some(far.as_path())), "not yet");
    std::fs::write(&far, "far").unwrap();
    brings.asked.lock().unwrap().take().unwrap().send(Ok(())).unwrap();
    assert!(until(&mut wb, |wb| wb.tabs.iter().any(|t| t.doc.path() == Some(far.as_path()))));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Dragging an entry in the panel onto a folder asks for its move (a refactoring, previewed
/// — or why not); dropped where it already is, nothing.
#[test]
fn a_dragged_entry_goes_only_where_it_can() {
    let (mut wb, dir) = opened("drag", support());
    let mut text = TextSystem::new();
    let (at_a, at_src) = (row(&mut wb, &dir.join("a.txt")), row(&mut wb, &dir.join("src")));
    wb.mouse_down(&mut text, at_a, false, false, false, 1);
    wb.mouse_drag(&mut text, at_src);
    let (tabs, groups) = (wb.tab_count(), wb.group_count());
    assert!(wb.mouse_up(&mut text));
    // Its preview opens below the editor (a new group when a file is open).
    let previewed = wb.tab_count() > tabs || wb.group_count() > groups;
    assert!(previewed || wb.last_notification().is_some(), "the move asked for");
    // A drop where it already is asks nothing.
    assert!(!wb.move_entry(&dir.join("src/b.txt"), &dir.join("src")));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A row clicked gives the panel the keyboard (its commands act on the row); typing goes back
/// to the editor; a dialog over it takes the keyboard away.
#[test]
fn the_tree_has_the_keyboard_after_a_click_on_a_row() {
    use workbench_model::WorkbenchHost;
    let (mut wb, dir) = opened("focus", support());
    let mut text = TextSystem::new();
    let a = row(&mut wb, &dir.join("a.txt"));
    wb.mouse_down(&mut text, a, false, false, false, 1);
    assert_eq!(wb.focus, Focus::Side(ui_workbench::Dock::Left));
    assert_eq!(WorkbenchHost::context(&wb).place, workbench_model::MenuPlace::TreeFile);
    assert_eq!(wb.target_path(), Some(dir.join("a.txt")));
    // A dialog has the keyboard: the tree's row isn't what commands act on (the open file is).
    wb.open_about();
    assert_eq!(WorkbenchHost::context(&wb).place, workbench_model::MenuPlace::Editor);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Copy / Cut / Paste of entries, the module's commands run through the app on what the menu
/// was opened on: a copy gets a free name, a folder is copied whole (never into itself), a
/// cut moves (its open tab follows) once; files copied in the file manager are pasted too.
#[test]
fn copies_and_moves_entries_in_the_background() {
    use workbench_model::SystemHost;
    let (mut wb, dir) = opened("paste", support());
    std::fs::create_dir_all(dir.join("src/lib")).unwrap();
    std::fs::create_dir_all(dir.join("dest")).unwrap();
    std::fs::write(dir.join("src/lib/b.php"), "<?php").unwrap();
    let os: std::rc::Rc<std::cell::RefCell<Vec<PathBuf>>> = std::rc::Rc::default();
    let (read, write) = (std::rc::Rc::clone(&os), std::rc::Rc::clone(&os));
    wb.set_files_clipboard(FilesClipboard {
        read: std::rc::Rc::new(move || read.borrow().clone()),
        write: std::rc::Rc::new(move |paths: &[PathBuf]| *write.borrow_mut() = paths.to_vec()),
    });
    let run = |wb: &mut Workbench, id: &str, on: PathBuf| {
        wb.set_menu_target(Some(on));
        let ran = wb.run_command_named(id);
        wb.clear_menu_target();
        ran == Some(true)
    };
    let pasted = |wb: &mut Workbench| until(wb, |wb| wb.progress.is_empty());
    // A file copied next to itself: `a copy.txt`.
    assert!(run(&mut wb, "tree.copy", dir.join("a.txt")));
    assert!(run(&mut wb, "tree.paste", dir.join("a.txt")));
    assert!(pasted(&mut wb));
    assert!(dir.join("a copy.txt").exists() && dir.join("a.txt").exists());
    // A folder copied whole; not into itself.
    assert!(run(&mut wb, "tree.copy", dir.join("src/lib")));
    assert!(run(&mut wb, "tree.paste", dir.join("dest")));
    assert!(pasted(&mut wb));
    assert!(dir.join("dest/lib/b.php").exists());
    os.borrow_mut().clear();
    assert!(run(&mut wb, "tree.copy", dir.join("src")));
    os.borrow_mut().clear();
    assert!(!run(&mut wb, "tree.paste", dir.join("src/lib")));
    // A cut moves; the open file's tab follows; the cut is used up.
    wb.open_file(&dir.join("a.txt")).unwrap();
    assert!(run(&mut wb, "tree.cut", dir.join("a.txt")));
    os.borrow_mut().clear();
    assert!(run(&mut wb, "tree.paste", dir.join("dest")));
    assert!(pasted(&mut wb));
    assert!(!dir.join("a.txt").exists() && dir.join("dest/a.txt").exists());
    assert!(until(&mut wb, |wb| {
        wb.tabs.iter().any(|t| t.doc.path() == Some(dir.join("dest/a.txt").as_path()))
    }));
    assert!(!run(&mut wb, "tree.paste", dir.join("src")), "a cut is pasted once");
    // Copied in the file manager: pasted (as a copy) even with nothing copied here.
    *os.borrow_mut() = vec![dir.join("dest/lib")];
    assert!(run(&mut wb, "tree.paste", dir.join("src")));
    assert!(pasted(&mut wb));
    assert!(dir.join("src/lib copy/b.php").exists());
    assert!(wb.clipboard_files().len() == 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a panel asks of the app through the host: a question whose "yes" runs (nothing on
/// "cancel"), work shown in the status bar until it ends, the system clipboard's files.
#[test]
fn the_host_asks_shows_work_and_holds_files() {
    use workbench_model::{Progress, Question, SystemHost, WorkbenchHost};
    let (mut wb, dir) = opened("host", support());
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let question = || Question {
        title: "Delete?".into(),
        message: "a.txt".into(),
        icon: "trash",
        yes: "Delete".into(),
        danger: true,
        list: Vec::new(),
    };
    for answer in ["cancel", "yes"] {
        let count = Arc::clone(&asked);
        assert!(wb.confirm(
            question(),
            Box::new(move |_| {
                count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            })
        ));
        assert!(wb.dialog_open());
        wb.answer_dialog(Some(answer));
    }
    assert_eq!(asked.load(std::sync::atomic::Ordering::Relaxed), 1, "only the yes ran");
    // Work in the status bar, replaced by its id, gone when it ends.
    let work = |label: &str| Progress { label: label.into(), icon: "copy", stop: None };
    wb.set_progress("paste", Some(work("1 / 3")));
    wb.set_progress("paste", Some(work("2 / 3")));
    assert_eq!(wb.progress.len(), 1);
    assert_eq!(wb.progress[0].1.label, "2 / 3");
    wb.set_progress("paste", None);
    assert!(wb.progress.is_empty());
    // The system clipboard's files, as the window keeps them.
    let held: std::rc::Rc<std::cell::RefCell<Vec<PathBuf>>> = std::rc::Rc::default();
    let (read, write) = (std::rc::Rc::clone(&held), std::rc::Rc::clone(&held));
    wb.set_files_clipboard(FilesClipboard {
        read: std::rc::Rc::new(move || read.borrow().clone()),
        write: std::rc::Rc::new(move |paths: &[PathBuf]| *write.borrow_mut() = paths.to_vec()),
    });
    wb.set_clipboard_files(&[dir.join("a.txt")]);
    assert_eq!(wb.clipboard_files(), [dir.join("a.txt")]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A panel registered on the left: its side's events, recorded.
#[derive(Debug, Default)]
struct Seen {
    header: Vec<String>,
    commands: Vec<String>,
    keys: usize,
    told: Vec<String>,
    /// What a drag then a release asks for.
    drop: Option<ui_workbench::ToolPanelAction>,
    /// Where the mouse was said to be.
    hovers: Vec<Point>,
}

struct LeftPanel(Arc<std::sync::Mutex<Seen>>);

impl ui_workbench::ToolPanel for LeftPanel {
    fn update(&mut self, _: &ui_workbench::PanelContext<'_>) {}
    fn paint(&mut self, _: ui_workbench::ViewerPaint<'_>) {}
    fn header(&self) -> Option<Vec<ui_workbench::PanelButton>> {
        Some(vec![ui_workbench::PanelButton {
            id: "new",
            icon: "plus",
            tooltip: "New".into(),
            group: 0,
        }])
    }
    fn header_action(&mut self, id: &str) -> Option<ui_workbench::ToolPanelAction> {
        self.0.lock().unwrap().header.push(id.to_owned());
        None
    }
    fn edit_key(&mut self, _: ui_workbench::ViewerKey) -> bool {
        self.0.lock().unwrap().keys += 1;
        true
    }
    fn command(&mut self, id: &str) -> bool {
        self.0.lock().unwrap().commands.push(id.to_owned());
        id != "select_all"
    }
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(told) = data.downcast_ref::<&str>() {
            self.0.lock().unwrap().told.push((*told).to_owned());
        }
    }
    fn release(&mut self) -> bool {
        true
    }
    fn hover(&mut self, p: Point) -> bool {
        self.0.lock().unwrap().hovers.push(p);
        false
    }
    fn take_action(&mut self) -> Option<ui_workbench::ToolPanelAction> {
        self.0.lock().unwrap().drop.take()
    }
}

#[derive(Debug)]
struct LeftTool(Arc<std::sync::Mutex<Seen>>);

impl ui_workbench::ToolWindowProvider for LeftTool {
    fn id(&self) -> &'static str {
        "left-test"
    }
    fn icon(&self) -> &'static str {
        "folder"
    }
    fn title(&self) -> String {
        "Left".into()
    }
    fn dock(&self) -> ui_workbench::Dock {
        ui_workbench::Dock::Left
    }
    fn content(&self) -> ui_workbench::ToolContent {
        ui_workbench::ToolContent::Panel
    }
    fn create(&self) -> Option<Box<dyn ui_workbench::ToolPanel>> {
        Some(Box::new(LeftPanel(Arc::clone(&self.0))))
    }
}

/// A panel registered on the left takes the project panel's place while it's open: the
/// header the app draws for it (its buttons, their tooltips), its clicks, keys and editing
/// commands; Structure stays on the right meanwhile; the project button brings the tree back.
#[test]
fn a_left_panel_takes_the_project_panel_s_place() {
    let seen = Arc::new(std::sync::Mutex::new(Seen::default()));
    let mut support = support();
    support.tools.push(Arc::new(LeftTool(Arc::clone(&seen))));
    let (mut wb, dir) = opened("left", support);
    let mut text = TextSystem::new();
    assert!(wb.toggle_tool("left-test"));
    assert!(wb.toggle_side_panel("structure"), "the right side keeps its own");
    let left = wb.tools.side_on(ui_workbench::Dock::Left).unwrap();
    assert_eq!(left.bounds, wb.tools.sidebar, "in the project panel's place");
    assert!(left.content.min_y() > left.bounds.min_y(), "below the header drawn for it");
    let content = left.content;
    let tool = |wb: &Workbench, id: &str| {
        let t = wb.tool(id).cloned().unwrap();
        wb.tool_button_of(&t, false).active
    };
    assert!(tool(&wb, "left-test") && !tool(&wb, PROJECT_TOOL));
    // Painted once (the header lays out its buttons), then its button clicked and hovered.
    wb.paint(&mut text, &mut render_api::Scene::default());
    let button =
        wb.tools.side_on(ui_workbench::Dock::Left).unwrap().header.bounds_of("new").unwrap();
    let middle =
        |r: Rect| Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0);
    wb.mouse_down(&mut text, middle(button), false, false, false, 1);
    assert_eq!(seen.lock().unwrap().header, ["new"]);
    assert_eq!(wb.side_button_at(middle(button)).map(|(_, t)| t), Some("New".to_owned()));
    // A click in it gives it the keyboard: its keys, the editing commands it takes.
    wb.mouse_down(&mut text, middle(content), false, false, false, 1);
    assert_eq!(wb.focus, Focus::Side(ui_workbench::Dock::Left));
    wb.edit_key(&mut text, EditKey::Move { motion: Motion::Down, extend: false });
    assert_eq!(seen.lock().unwrap().keys, 1);
    assert!(wb.side_command(&mut text, "copy"));
    assert!(!wb.side_command(&mut text, "select_all"), "not taken: it goes where it would");
    assert_eq!(seen.lock().unwrap().commands, ["copy", "select_all"]);
    // The project button: the tree back in its place; Structure still open.
    assert!(wb.toggle_project_panel());
    assert!(wb.tools.side_on(ui_workbench::Dock::Left).is_some_and(|s| s.id == PROJECT_TOOL));
    assert!(tool(&wb, PROJECT_TOOL) && !tool(&wb, "left-test"));
    assert!(wb.tools.side_on(ui_workbench::Dock::Right).is_some());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A module's knowledge of the targets: every one is "seen".
#[derive(Debug)]
struct Knows;

impl workbench_model::ContextProvider for Knows {
    fn add_facts(
        &self,
        _: Option<(kernel::ProjectId, &Path)>,
        context: &mut workbench_model::ActionContext,
    ) {
        for path in context.paths() {
            context.add(&path, "test.seen");
        }
    }
}

/// What commands act on, as modules see it: the right-clicked entry (its place, its mark,
/// the modules' facts) while its menu is open, else the keyboard's — the tree's selection,
/// else the active file.
#[test]
fn commands_know_what_they_act_on_and_what_modules_know_of_it() {
    use workbench_model::{MenuPlace, WorkbenchHost};
    let mut support = support();
    support.context_providers.push(Arc::new(Knows));
    let (mut wb, dir) = opened("context", support);
    let mut text = TextSystem::new();
    // A right click on a file: the tree's menu, on it.
    let a = row(&mut wb, &dir.join("a.txt"));
    let context = wb.entries_menu_at(a).unwrap();
    assert_eq!(context.place, MenuPlace::TreeFile);
    assert_eq!(context.path(), Some(dir.join("a.txt").as_path()));
    assert!(context.all("test.seen"));
    assert_eq!(WorkbenchHost::context(&wb), context);
    assert_eq!(wb.target_path(), Some(dir.join("a.txt")));
    // The menu gone: the keyboard's, the folder clicked in the tree.
    wb.clear_menu_target();
    let src = row(&mut wb, &dir.join("src"));
    wb.mouse_down(&mut text, src, false, false, false, 1);
    let context = WorkbenchHost::context(&wb);
    assert_eq!(context.place, MenuPlace::TreeFolder);
    assert_eq!(context.path(), Some(dir.join("src").as_path()));
    assert!(context.all("test.seen"));
    // The editor's: the active file.
    wb.open_file(&dir.join("src/b.txt")).unwrap();
    wb.focus = Focus::Editor;
    let context = WorkbenchHost::context(&wb);
    assert_eq!(
        (context.place, context.path()),
        (MenuPlace::Editor, Some(dir.join("src/b.txt").as_path()))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a module asks of its panel through the host: told something (shown first, the
/// keyboard given if asked), an entry dropped moved as a refactoring; the system's clipboard
/// and file manager asked of the window.
#[test]
fn a_module_tells_its_panel_and_a_drop_moves_an_entry() {
    use workbench_model::{SystemHost, ViewHost};
    let seen = Arc::new(std::sync::Mutex::new(Seen::default()));
    let mut support = support();
    support.tools.push(Arc::new(LeftTool(Arc::clone(&seen))));
    let (mut wb, dir) = opened("tell", support);
    let mut text = TextSystem::new();
    assert!(wb.tell_panel("left-test", &"new file", true));
    assert_eq!(seen.lock().unwrap().told, ["new file"]);
    assert_eq!(wb.focus, Focus::Side(ui_workbench::Dock::Left));
    assert!(!wb.tell_panel("nothing-here", &"x", false));
    // A press in it, a drag, a release asking for a move: the move's preview (or why not).
    let content = wb.tools.side_on(ui_workbench::Dock::Left).unwrap().content;
    wb.mouse_down(
        &mut text,
        Point::new(content.min_x() + 20.0, content.min_y() + 20.0),
        false,
        false,
        false,
        1,
    );
    let from = dir.join("a.txt");
    seen.lock().unwrap().drop =
        Some(ui_workbench::ToolPanelAction::Move { from: from.clone(), into: dir.join("src") });
    let tabs = wb.tab_count();
    assert!(wb.mouse_up(&mut text));
    assert!(wb.tab_count() > tabs || wb.last_notification().is_some(), "the move was asked for");
    // The window's part: the clipboard's text, the file manager.
    wb.set_clipboard_text("x");
    assert!(matches!(wb.take_request(), Some(MouseResult::CopyText(t)) if t == "x"));
    wb.reveal_in_file_manager(&from);
    assert!(matches!(wb.take_request(), Some(MouseResult::Reveal(p)) if p == from));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The project panel's header buttons, clicked as the user does: collapse all closes the
/// opened folders, expand all opens the ones already read, locate shows the active file.
#[test]
fn the_project_panel_header_buttons_work() {
    let (mut wb, dir) = opened("header", support());
    let mut text = TextSystem::new();
    let click = |wb: &mut Workbench, text: &mut TextSystem, id: &str| {
        paint(wb);
        let r = project_panel(wb).header.bounds_of(id).unwrap_or_else(|| panic!("no {id}"));
        let p = Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0);
        wb.mouse_down(text, p, false, false, false, 1);
        wb.mouse_up(text);
        paint(wb);
    };
    let b = dir.join("src/b.txt");
    // Nothing read yet: expand all reads and opens the next level (src).
    assert!(find_row(&mut wb, &b).is_none());
    click(&mut wb, &mut text, "expand");
    assert!(find_row(&mut wb, &b).is_some(), "expand all opens src, even unread");
    click(&mut wb, &mut text, "collapse");
    assert!(find_row(&mut wb, &b).is_none(), "collapse all closes src");
    click(&mut wb, &mut text, "expand");
    assert!(find_row(&mut wb, &b).is_some(), "expand all opens src again (it was read)");
    click(&mut wb, &mut text, "collapse");
    wb.open_file(&b).unwrap();
    click(&mut wb, &mut text, "locate");
    assert!(until(&mut wb, |wb| find_row(wb, &b).is_some()), "locate shows the active file");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A project is known by its identity (a UUID v4 at each opening), never its name or path:
/// the same folder opened again is another project, and its panel is given the new one
/// (its events), not left with the old.
#[test]
fn the_same_folder_opened_again_is_another_project() {
    let (mut wb, dir) = opened("identity", support());
    let first = wb.data.id;
    let given = |wb: &mut Workbench| project_panel(wb).project.flatten().map(|k| k.0);
    assert_eq!(given(&mut wb), Some(first), "the panel has this project");
    wb.open_project(&dir).unwrap();
    paint(&mut wb);
    let second = wb.data.id;
    assert_ne!(first, second, "another opening, another identity");
    assert_eq!(given(&mut wb), Some(second), "the panel follows the new one");
    assert_eq!(workbench_model::WorkbenchHost::project_id(&wb), Some(second));
    let _ = std::fs::remove_dir_all(&dir);
}

/// For the Task Manager: a save is written to the project's disk count, on its "open files"
/// part (by the project's identity).
#[test]
fn a_save_counts_as_written_for_its_project() {
    let (mut wb, dir) = opened("saves", support());
    let file = dir.join("a.txt");
    wb.open_file(&file).unwrap();
    let mut text = TextSystem::new();
    wb.type_text(&mut text, "hello ");
    let i = wb.active;
    wb.save_tab(i, None).unwrap();
    let written = std::fs::metadata(&file).unwrap().len();
    let parts = wb.app_parts();
    let open = parts.iter().find(|p| p.name == i18n::t("tasks.files")).expect("open files");
    assert_eq!(open.project, Some(wb.data.id));
    assert_eq!(open.written, Some(written));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The Task Manager's two views: a window's own (its project's parts and the app's), and
/// every window's (each project by its identity — two named alike stay two — the app's
/// own parts once).
#[test]
fn the_task_manager_sees_this_project_or_every_open_one() {
    use workbench_model::ResourceSource;
    let base = std::env::temp_dir().join(format!("task-views-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let open = |dir: &Path| {
        std::fs::create_dir_all(dir).unwrap();
        let mut wb = Workbench::new(Arc::new(support()), Ports::local());
        wb.forget_projects();
        wb.resize(PhysicalSize::new(1200, 800), 1.0);
        wb.open_project(dir).unwrap();
        wb
    };
    // Two projects named alike, in two windows.
    let (first, second) = (open(&base.join("a/shop")), open(&base.join("b/shop")));
    let source = Arc::clone(&first.task_data);
    first.refresh_task_data();
    second.refresh_task_data();
    let own = source.snapshot();
    // The other window fills in its parts once the view of all is asked (as it does on its
    // next event while that view is open).
    let _ = source.snapshot_all();
    second.refresh_task_data();
    assert_eq!(own.projects.iter().map(|p| p.id).collect::<Vec<_>>(), [first.data.id]);
    assert!(own.parts.iter().all(|p| p.project.is_none() || p.project == Some(first.data.id)));
    let all = source.snapshot_all();
    let ids: Vec<_> = all.projects.iter().map(|p| p.id).collect();
    assert!(ids.contains(&first.data.id) && ids.contains(&second.data.id));
    // (Other tests' windows may be open too: only these two are looked at.)
    let ours = |id| all.projects.iter().find(|p| p.id == id).map(|p| p.name.as_str());
    assert_eq!(ours(first.data.id), Some("shop"));
    assert_eq!(ours(second.data.id), Some("shop"), "named alike, two projects");
    let interface = i18n::t("tasks.ui");
    assert_eq!(all.parts.iter().filter(|p| p.name == interface).count(), 1, "the app's once");
    let files = i18n::t("tasks.files");
    let projects_files: Vec<_> =
        all.parts.iter().filter(|p| p.name == files).map(|p| p.project).collect();
    assert!(projects_files.contains(&Some(first.data.id)));
    assert!(projects_files.contains(&Some(second.data.id)));
    drop((first, second));
    let _ = std::fs::remove_dir_all(&base);
}

/// A module's job finished after its project was closed: its completion never reaches the
/// project opened meanwhile (it's dropped with the old one); in its own project, it arrives.
#[test]
fn a_job_never_reaches_another_project() {
    use workbench_model::{Completion, WorkbenchHost};
    let (mut wb, dir) = opened("jobs", support());
    wb.forget_projects();
    let job = |wb: &mut Workbench, said: &'static str, gate: std::sync::mpsc::Receiver<()>| {
        WorkbenchHost::spawn(
            wb,
            Box::new(move || {
                let _ = gate.recv();
                Box::new(move |host: &mut dyn WorkbenchHost| {
                    host.notify(workbench_model::Notification::info(said))
                }) as Completion
            }),
        );
    };
    let said = |wb: &Workbench| wb.last_notification();
    // In its own project.
    let (open, gate) = std::sync::mpsc::channel();
    job(&mut wb, "same project", gate);
    open.send(()).unwrap();
    assert!(until(&mut wb, |wb| said(wb).as_deref() == Some("same project")));
    // Another project opened before it ends.
    let (open, gate) = std::sync::mpsc::channel();
    job(&mut wb, "old project", gate);
    let other = dir.join("src");
    wb.open_project(&other).unwrap();
    wb.forget_projects();
    // Not started yet, it was cancelled (dropped with its gate); started, it ends now.
    let _ = open.send(());
    for _ in 0..25 {
        wb.poll_jobs();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_ne!(said(&wb).as_deref(), Some("old project"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A side panel is told when the mouse leaves it (the point outside): what it lit, a
/// scrollbar it held shown, go — not only the panel under the mouse hears it move.
#[test]
fn a_side_panel_hears_the_mouse_leave() {
    let seen = Arc::new(std::sync::Mutex::new(Seen::default()));
    let mut support = support();
    support.tools.push(Arc::new(LeftTool(Arc::clone(&seen))));
    let (mut wb, dir) = opened("leave", support);
    assert!(wb.toggle_tool("left-test"));
    paint(&mut wb);
    let content = wb.tools.side_on(ui_workbench::Dock::Left).unwrap().content;
    let inside = Point::new(content.min_x() + 10.0, content.min_y() + 10.0);
    let outside = Point::new(wb.editor_area.min_x() + 50.0, wb.editor_area.min_y() + 50.0);
    wb.hover(inside);
    wb.hover(outside);
    let hovers = seen.lock().unwrap().hovers.clone();
    assert_eq!(hovers, [inside, outside], "told it left, once");
    wb.hover(outside);
    assert_eq!(seen.lock().unwrap().hovers.len(), 2, "not again while away");
    let _ = std::fs::remove_dir_all(dir);
}

/// A watcher that only says "look again", once.
#[derive(Debug)]
struct Dropped(std::sync::atomic::AtomicBool);
impl workspace::FileWatcher for Dropped {
    fn drain_changed(&self) -> Vec<PathBuf> {
        Vec::new()
    }
    fn drain_written(&self) -> Vec<PathBuf> {
        Vec::new()
    }
    fn take_rescan(&self) -> bool {
        self.0.swap(false, std::sync::atomic::Ordering::Relaxed)
    }
    fn take_vcs_changed(&self) -> bool {
        false
    }
}

/// Events dropped (the system too busy: "look again"): an open file another program changed
/// meanwhile is read again too, though its write was never heard.
#[test]
fn a_rescan_reads_again_the_open_files_changed_meanwhile() {
    let (mut wb, dir) = opened("rescan-tabs", support());
    let file = dir.join("a.txt");
    wb.open_file(&file).unwrap();
    // Changed by another program, a moment later (its stamp moves), unheard.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&file, "changed elsewhere, a longer text").unwrap();
    wb.tree_watcher = Some(Box::new(Dropped(std::sync::atomic::AtomicBool::new(true))));
    wb.poll_fs();
    assert!(until(&mut wb, |wb| {
        wb.tabs.iter().any(|t| t.doc.buffer().text() == "changed elsewhere, a longer text")
    }));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A look at everything again (the system dropped its events): a file gone from an open
/// folder meanwhile goes from the tree too, though its removal was never heard.
#[test]
fn a_rescan_reads_again_every_open_folder() {
    let (mut wb, dir) = opened("rescan-dirs", support());
    let b = dir.join("src/b.txt");
    assert!(wb.reveal_path_in_project(&b));
    assert!(until(&mut wb, |wb| find_row(wb, &b).is_some()), "src open, b.txt shown");
    // Nothing heard from now on (the real watcher gone): only the look again tells.
    wb.tree_watcher = Some(Box::new(Dropped(std::sync::atomic::AtomicBool::new(false))));
    std::fs::remove_file(&b).unwrap();
    wb.data.events.publish(&workspace::events::EntriesRescan { root: dir.clone() });
    assert!(until(&mut wb, |wb| find_row(wb, &b).is_none()), "gone from the open folder");
    let _ = std::fs::remove_dir_all(&dir);
}
