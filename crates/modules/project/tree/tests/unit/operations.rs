use super::*;
use workbench_model::{ActionContext, MenuPlace, Target};
use workbench_model::{EditorHost, RefactoringHost, ServicesHost, SystemHost, ViewHost};

/// A workbench as the tree's commands see it: the targets chosen by the test, a question
/// answered "yes", background work run at once, what was told recorded.
struct Host {
    root: PathBuf,
    /// The project's identity (its operations go by it).
    id: Option<kernel::ProjectId>,
    context: ActionContext,
    events: kernel::EventBus,
    notes: Vec<String>,
    relisted: Vec<PathBuf>,
    told: Vec<String>,
    clipboard_text: Option<String>,
    progress: Vec<Option<String>>,
}

impl Host {
    fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            id: None,
            context: ActionContext::empty(MenuPlace::TreeFile),
            events: kernel::EventBus::new(),
            notes: Vec::new(),
            relisted: Vec::new(),
            told: Vec::new(),
            clipboard_text: None,
            progress: Vec::new(),
        }
    }

    /// The commands act on `path` (as right-clicked in the tree).
    fn on(&mut self, path: &Path) {
        let is_dir = path.is_dir();
        let place = if is_dir { MenuPlace::TreeFolder } else { MenuPlace::TreeFile };
        self.context = ActionContext::new(place, vec![Target::new(path.to_path_buf(), is_dir)]);
    }
}

impl WorkbenchHost for Host {
    fn project_root(&self) -> Option<PathBuf> {
        Some(self.root.clone())
    }
    fn project_id(&self) -> Option<kernel::ProjectId> {
        self.id
    }
    fn active_file(&self) -> Option<workbench_model::ActiveFile> {
        None
    }
    fn context(&self) -> ActionContext {
        self.context.clone()
    }
    fn repository(&self) -> Option<Arc<dyn version_control::Repository>> {
        None
    }
    fn choose(&mut self, _: workbench_model::ChoiceList, _: workbench_model::OnChoice) -> bool {
        false
    }
    fn confirm(&mut self, _: Question, then: workbench_model::OnYes) -> bool {
        then(self);
        true
    }
    fn spawn(&mut self, job: workbench_model::Job) {
        job()(self);
    }
    fn notify(&mut self, notification: workbench_model::Notification) {
        self.notes.push(notification.title);
    }
    fn set_progress(&mut self, _: &'static str, progress: Option<Progress>) {
        self.progress.push(progress.map(|p| p.label));
    }
    fn relist(&mut self, dir: &Path) {
        self.relisted.push(dir.to_path_buf());
    }
    fn events(&self) -> &kernel::EventBus {
        &self.events
    }
    fn project_storage(&self) -> Option<PathBuf> {
        None
    }
}

impl ViewHost for Host {
    fn open_view(&mut self, _: &str, _: &str, _: &dyn std::any::Any) -> bool {
        false
    }
    fn tell_panel(&mut self, tool: &str, data: &dyn std::any::Any, _: bool) -> bool {
        let told = match data.downcast_ref::<EntriesRequest>() {
            Some(EntriesRequest::NewEntry { is_dir }) => {
                format!("{tool}: new {}", if *is_dir { "folder" } else { "file" })
            }
            Some(EntriesRequest::Reveal { path, .. }) => {
                format!("{tool}: reveal {}", kernel::paths::file_name(path))
            }
            _ => return false,
        };
        self.told.push(told);
        true
    }
}

impl SystemHost for Host {
    fn set_clipboard_text(&mut self, text: &str) {
        self.clipboard_text = Some(text.to_owned());
    }
}

impl EditorHost for Host {}
impl RefactoringHost for Host {}
impl ServicesHost for Host {}

/// A scratch project: `a.txt`, `src/b.txt`.
/// The tests copying, cutting and pasting take turns: the clipboard is the process's.
fn alone() -> std::sync::MutexGuard<'static, ()> {
    static CLIPBOARD_TESTS: Mutex<()> = Mutex::new(());
    CLIPBOARD_TESTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn project(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("tree-ops-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("a.txt"), "a").unwrap();
    std::fs::write(root.join("src/b.txt"), "b").unwrap();
    root.canonicalize().unwrap()
}

fn command(id: CommandId, operations: &Arc<Operations>) -> Command {
    Command { id, operations: Arc::clone(operations) }
}

/// Polls (as the app does after a wake) until the paste is done.
fn finish_paste(operations: &Operations, host: &mut Host) {
    // Long enough for a loaded machine (the whole gate running beside it): the deadline only
    // stops a paste that never ends.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while operations.paste.lock().unwrap().is_some() && std::time::Instant::now() < deadline {
        operations.poll(host);
        std::thread::yield_now();
    }
}

/// Copy then paste into a folder: placed under the folder, the folders listed again, the
/// pasted entry shown in the panel, announced as one batch of changes, the progress cleared.
/// Cut then paste: moved, announced (`FileRenamed`), the cut used up.
#[test]
fn copied_or_cut_entries_are_pasted_where_the_menu_was_opened() {
    let _alone = alone();
    let root = project("paste");
    let operations = Arc::new(Operations::default());
    let mut host = Host::new(&root);
    // Announced as one batch of changes each time.
    let batches = Arc::new(Mutex::new(Vec::new()));
    let (started, ended) = (Arc::clone(&batches), Arc::clone(&batches));
    let _s = host.events.subscribe(move |e: &workspace::events::BulkChangeStarted| {
        started.lock().unwrap().push(("start", e.id));
    });
    let _e = host.events.subscribe(move |e: &workspace::events::BulkChangeEnded| {
        ended.lock().unwrap().push(("end", e.id));
    });
    host.on(&root.join("a.txt"));
    assert!(command(COPY, &operations).run(&mut host));
    host.on(&root.join("src"));
    assert!(command(PASTE, &operations).enabled(&host));
    assert!(command(PASTE, &operations).run(&mut host));
    finish_paste(&operations, &mut host);
    assert_eq!(std::fs::read_to_string(root.join("src/a.txt")).unwrap(), "a");
    assert!(root.join("a.txt").exists(), "a copy");
    assert!(host.relisted.contains(&root.join("src")));
    assert_eq!(host.told.last().map(String::as_str), Some("project: reveal a.txt"));
    {
        let seen = batches.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!((seen[0].0, seen[1].0), ("start", "end"));
        assert_eq!(seen[0].1, seen[1].1, "the same batch");
    }
    assert_eq!(host.progress.last(), Some(&None));
    // Cut, pasted on a file: into its folder, moved and announced.
    let renamed = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&renamed);
    let _sub = host.events.subscribe(move |e: &workspace::events::FileRenamed| {
        heard.lock().unwrap().push(e.to.clone());
    });
    host.on(&root.join("src/b.txt"));
    assert!(command(CUT, &operations).run(&mut host));
    host.on(&root.join("a.txt"));
    assert!(command(PASTE, &operations).run(&mut host));
    finish_paste(&operations, &mut host);
    assert!(root.join("b.txt").exists() && !root.join("src/b.txt").exists());
    assert_eq!(*renamed.lock().unwrap(), [root.join("b.txt")]);
    // Used up: nothing more to paste.
    host.on(&root.join("src"));
    assert!(!command(PASTE, &operations).run(&mut host));
    let _ = std::fs::remove_dir_all(&root);
}

/// Delete asks, then moves the entry to the trash: announced (`EntriesDeleted`), its folder
/// listed again. Copy Path, a new entry, and what can't act on an entry not on disk.
#[test]
fn delete_asks_and_the_rest_act_on_the_targets() {
    let _alone = alone();
    let root = project("delete");
    let operations = Arc::new(Operations::default());
    let mut host = Host::new(&root);
    // Copy Path: the target's.
    host.on(&root.join("src/b.txt"));
    command(COPY_PATH, &operations).run(&mut host);
    assert_eq!(host.clipboard_text, Some(root.join("src/b.txt").to_string_lossy().into_owned()));
    // New Folder: the panel told, with the keyboard.
    assert!(command(NEW_FOLDER, &operations).run(&mut host));
    assert_eq!(host.told.last().map(String::as_str), Some("project: new folder"));
    // A file only on the server: nothing to cut, copy, delete or reveal there.
    host.context = ActionContext::new(
        MenuPlace::TreeFile,
        vec![Target::new(root.join("far.txt"), false).with_mark(workspace::Mark::None, true)],
    );
    for id in [CUT, COPY, DELETE, REVEAL] {
        assert!(!command(id, &operations).enabled(&host), "{id:?}");
    }
    // Deleted (to the trash: skipped where the system has none, as on a CI runner).
    let deleted = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&deleted);
    let _sub = host.events.subscribe(move |e: &workspace::events::EntriesDeleted| {
        heard.lock().unwrap().extend(e.paths.clone());
    });
    host.on(&root.join("a.txt"));
    assert!(command(DELETE, &operations).run(&mut host));
    if !root.join("a.txt").exists() {
        assert_eq!(*deleted.lock().unwrap(), [root.join("a.txt")]);
        assert!(host.relisted.contains(&root));
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// For the Task Manager: a paste is an operation of its project (by identity), counted in
/// files, ended; what it copied read and written; gone when the project closes.
#[test]
fn a_paste_is_an_operation_of_its_project() {
    let _alone = alone();
    let root = project("operation");
    let operations = Arc::new(Operations::default());
    let mut host = Host::new(&root);
    let id = kernel::ProjectId::new();
    host.id = Some(id);
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
    let context = workbench_model::ProjectContext {
        marks: Default::default(),
        id,
        root: &root,
        storage: &root,
        events: &host.events.clone(),
        wake,
        settings: None,
        language_of: std::sync::Arc::new(|_: &std::path::Path| None),
        quality_tools: Vec::new(),
        text_formatters: Vec::new(),
        notifier: std::sync::Arc::new(workbench_model::NoNotifier),
    };
    let held = workbench_model::ProjectService::attach(&Service(Arc::clone(&operations)), &context);
    host.on(&root.join("a.txt"));
    assert!(command(COPY, &operations).run(&mut host));
    host.on(&root.join("src"));
    assert!(command(PASTE, &operations).run(&mut host));
    finish_paste(&operations, &mut host);
    let parts = Reporter.report();
    let part = parts.iter().find(|p| p.project == Some(id)).expect("its part");
    let copy = part.activities.iter().find(|a| a.kind == ActivityKind::Copy).expect("the paste");
    assert_eq!((copy.done, copy.total), (1, Some(1)));
    assert_eq!(copy.ended.as_ref().map(|(_, o)| o.clone()), Some(Outcome::Done));
    assert_eq!((part.read, part.written), (Some(1), Some(1)), "\"a\" read and written");
    // The project closed: its record goes.
    drop(held);
    assert!(Reporter.report().iter().all(|p| p.project != Some(id)));
    let _ = std::fs::remove_dir_all(&root);
}

/// A paste ends in its own project's window: another project's window polling first leaves
/// it (its batch would end on the wrong project's events, the right one's staying open).
/// Its project closed during the paste: another window ends it (on the project's own
/// events: the uploads it held let go), and a new paste can start.
#[test]
fn a_paste_whose_project_closed_is_ended_by_any_window() {
    let _alone = alone();
    let root = project("paste-orphan");
    let operations = Arc::new(Operations::default());
    let mut host = Host::new(&root);
    let id = kernel::ProjectId::new();
    host.id = Some(id);
    let mut other = Host::new(&root);
    other.id = Some(kernel::ProjectId::new());
    let open = Arc::new(());
    let files = Arc::new(ProjectFiles {
        project: id,
        name: "orphan".into(),
        disk: DiskIo::new(),
        activities: Activities::new(std::time::Instant::now),
        wake: Arc::new(|| {}),
        open: Arc::downgrade(&open),
    });
    PROJECTS.lock().unwrap().push(Arc::downgrade(&files));
    let ended = Arc::new(Mutex::new(0));
    let heard = Arc::clone(&ended);
    let _e = host.events.subscribe(move |_: &workspace::events::BulkChangeEnded| {
        *heard.lock().unwrap() += 1;
    });
    host.on(&root.join("a.txt"));
    assert!(command(COPY, &operations).run(&mut host));
    host.on(&root.join("src"));
    assert!(command(PASTE, &operations).run(&mut host));
    operations.poll(&mut other);
    assert!(operations.paste.lock().unwrap().is_some(), "still its own window's");
    drop(open);
    operations.poll(&mut other);
    assert!(operations.paste.lock().unwrap().is_none(), "ended by another window");
    assert_eq!(*ended.lock().unwrap(), 1, "its batch ended on its project's events");
    drop(files);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_paste_ends_in_its_own_project_s_window() {
    let _alone = alone();
    let root = project("paste-own");
    let operations = Arc::new(Operations::default());
    let mut host = Host::new(&root);
    host.id = Some(kernel::ProjectId::new());
    let mut other = Host::new(&root);
    other.id = Some(kernel::ProjectId::new());
    let ended = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&ended);
    let _e = host.events.subscribe(move |e: &workspace::events::BulkChangeEnded| {
        heard.lock().unwrap().push(e.id);
    });
    host.on(&root.join("a.txt"));
    assert!(command(COPY, &operations).run(&mut host));
    host.on(&root.join("src"));
    assert!(command(PASTE, &operations).run(&mut host));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        operations.poll(&mut other);
        std::thread::yield_now();
    }
    assert!(operations.paste.lock().unwrap().is_some(), "left for its own window");
    finish_paste(&operations, &mut host);
    assert_eq!(ended.lock().unwrap().len(), 1, "its batch ended on its project's events");
    let _ = std::fs::remove_dir_all(&root);
}

/// Many deleted are listed from the project's root, `/`-separated.
#[test]
fn the_deleted_are_listed_from_the_root() {
    let root = Path::new("/p");
    let paths =
        [PathBuf::from("/p/src/a.php"), PathBuf::from("/p/b"), PathBuf::from("/elsewhere/c")];
    assert_eq!(listed(&paths, root), ["src/a.php", "b", "/elsewhere/c"]);
}
