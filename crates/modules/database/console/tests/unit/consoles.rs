//! A project's consoles: made with their files, named after the others, found back from
//! their path, following the disk (a file gone leaves, one put there joins), their list
//! written in the background, a broken list never written over; idle ones swept; the
//! project's close rolling back a transaction left open (said).

use super::*;
use crate::fake::{FakeAccess, Manual, Project, consoles};
use std::sync::Arc;

#[derive(Default)]
struct Memory {
    saved: Mutex<Vec<Vec<ConsoleSpec>>>,
    broken: bool,
}

impl ConsoleStore for Memory {
    fn load(&self) -> Result<Vec<ConsoleSpec>, String> {
        if self.broken { Err("broken".into()) } else { Ok(Vec::new()) }
    }

    fn save(&self, consoles: &[ConsoleSpec]) -> Result<(), String> {
        self.saved.lock().unwrap().push(consoles.to_vec());
        Ok(())
    }
}

#[test]
fn consoles_are_made_with_their_files_and_named_after_the_others() {
    let project = Project::new("make");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let store = Arc::new(Memory::default());
    consoles.set_store(store.clone());
    let one = consoles.create(access.shop, "SELECT 1;").unwrap();
    let two = consoles.create(access.shop, "").unwrap();
    assert_eq!((one.name.as_str(), two.name.as_str()), ("console", "console_1"));
    let path = one.path(&project.root);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "SELECT 1;");
    assert_eq!(consoles.spec_of_path(&path).map(|s| s.id), Some(one.id));
    assert_eq!(consoles.consoles_of(access.shop).len(), 2);
    assert!(consoles.consoles_of(access.people).is_empty());
    // Written in the background, the newest list last.
    manual.run_all();
    let saved = store.saved.lock().unwrap().last().cloned().unwrap();
    assert_eq!(saved.len(), 2);
    // Changed: written again.
    consoles.update(one.id, |s| s.mode = database::console::RunMode::Script);
    manual.run_all();
    let saved = store.saved.lock().unwrap().last().cloned().unwrap();
    assert_eq!(saved[0].mode, database::console::RunMode::Script);
    // Another project's file isn't one.
    assert!(consoles.spec_of_path(&project.root.join("a.sql")).is_none());
}

#[test]
fn the_list_follows_the_disk() {
    let project = Project::new("disk");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let made = consoles.create(access.shop, "").unwrap();
    std::fs::remove_file(made.path(&project.root)).unwrap();
    let folder = database::console::folder(&project.root).join(access.shop.to_string());
    std::fs::write(folder.join("mine.sql"), "SELECT 2;").unwrap();
    let names: Vec<String> =
        consoles.consoles_of(access.shop).into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["mine"]);
    // A file opened from there by hand is a console too.
    let other = folder.join("other.sql");
    std::fs::write(&other, "").unwrap();
    assert_eq!(consoles.spec_of_path(&other).map(|s| s.name), Some("other".to_owned()));
}

#[test]
fn a_broken_list_is_never_written_over() {
    let project = Project::new("broken");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let store = Arc::new(Memory { broken: true, ..Memory::default() });
    consoles.set_store(store.clone());
    consoles.create(access.shop, "").unwrap();
    manual.run_all();
    assert!(store.saved.lock().unwrap().is_empty());
}

#[test]
fn names_sort_with_their_numbers() {
    let mut names = vec!["console_10", "console_2", "console", "abc"];
    names.sort_by_key(|n| natural(n));
    assert_eq!(names, ["abc", "console", "console_2", "console_10"]);
}

/// The project closes: a transaction left open is rolled back, and said.
#[test]
fn the_project_s_close_rolls_back_what_was_left_open() {
    use workbench_model::Notifier;
    #[derive(Default)]
    struct Told(Mutex<Vec<String>>);
    impl Notifier for Told {
        fn show(&self, notification: workbench_model::Notification) {
            self.0.lock().unwrap().push(notification.title);
        }
        fn update(&self, notification: workbench_model::Notification) {
            self.show(notification);
        }
        fn dismiss(&self, _id: workbench_model::NotificationId) {}
    }
    let project = Project::new("closing");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let told = Arc::new(Told::default());
    consoles.set_notifier(told.clone());
    let spec = consoles.create(access.shop, "").unwrap();
    let runner = consoles.runner(spec.id, spec.source);
    let _lease = runner.lease();
    let connector = access.connector(access.shop, &Default::default()).unwrap();
    let statements = match database::console::plan(&database::console::PlanInput {
        text: "BEGIN; DELETE FROM users;",
        mode: database::console::RunMode::Script,
        ..Default::default()
    }) {
        database::console::RunPlan::Run(s) => s,
        other => panic!("{other:?}"),
    };
    runner.run(crate::runner::RunRequest {
        statements,
        document: "".into(),
        settings: Default::default(),
        connector,
        source_name: "shop".into(),
        detached: false,
    });
    manual.run_all();
    assert!(runner.in_transaction());
    let guard = register(Arc::clone(&consoles));
    drop(guard);
    manual.run_all();
    assert!(!runner.connected());
    assert_eq!(told.0.lock().unwrap().len(), 1);
    assert_eq!(project.look("SELECT count(*) FROM users"), [[database::Value::Int(3)]]);
}

/// What's said on `consoles`' bus of the entries (a rename, a deletion), kept.
fn heard(consoles: &Consoles) -> (Arc<Mutex<Vec<String>>>, Vec<kernel::Subscription>) {
    let heard: Arc<Mutex<Vec<String>>> = Arc::default();
    let (renamed, deleted) = (Arc::clone(&heard), Arc::clone(&heard));
    let name = |p: &Path| p.file_name().unwrap().to_string_lossy().into_owned();
    let subscriptions = vec![
        consoles.events().subscribe(move |e: &workspace::events::FileRenamed| {
            renamed.lock().unwrap().push(format!("{} → {}", name(&e.from), name(&e.to)));
        }),
        consoles.events().subscribe(move |e: &workspace::events::EntriesDeleted| {
            let names: Vec<String> = e.paths.iter().map(|p| name(p)).collect();
            deleted.lock().unwrap().push(format!("deleted {}", names.join(", ")));
        }),
    ];
    (heard, subscriptions)
}

/// A console's file renamed (said on the project's bus, whoever did it: its tab, the tree,
/// a refactoring): the same console under the file's new name — its id, its schema, its Tx
/// mode, its parameters' names, its runner told its new file and name (drawn again) —, the
/// list written; its bar telling it again changes nothing.
#[test]
fn a_renamed_file_is_the_same_console_under_its_new_name() {
    let project = Project::new("renamed");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let store = Arc::new(Memory::default());
    consoles.set_store(store.clone());
    let _follow = consoles.follow();
    let spec = consoles.create(access.shop, "SELECT 1;").unwrap();
    consoles.update(spec.id, |s| {
        s.schema = Some("main".into());
        s.tx = database::console::TxMode::Manual;
        s.params = vec!["id".into()];
    });
    let runner = consoles.runner(spec.id, spec.source);
    let generation = runner.generation();
    let from = spec.path(&project.root);
    let to = from.with_file_name("my-events.sql");
    std::fs::rename(&from, &to).unwrap();
    consoles.events().publish(&workspace::events::FileRenamed { from, to: to.clone() });
    let after = consoles.spec(spec.id).expect("the same console");
    assert_eq!(after.name, "my-events");
    assert_eq!(after.path(&project.root), to);
    assert_eq!(after.schema.as_deref(), Some("main"));
    assert_eq!(after.tx, database::console::TxMode::Manual);
    assert_eq!(after.params, ["id"]);
    assert_eq!(consoles.spec_of_path(&to).map(|s| s.id), Some(spec.id));
    assert_eq!(consoles.consoles_of(access.shop).len(), 1, "no other one for the new name");
    assert_eq!(runner.name(), "my-events");
    assert_eq!(runner.file().as_deref(), Some(to.as_path()));
    assert!(runner.generation() > generation, "drawn again");
    assert_eq!(consoles.runners().len(), 1);
    manual.run_all();
    let saved = store.saved.lock().unwrap().last().cloned().unwrap();
    assert_eq!((saved.len(), saved[0].id, saved[0].name.as_str()), (1, spec.id, "my-events"));
    assert_eq!(consoles.follow_file(spec.id, &to).map(|s| s.id), Some(spec.id));
    // Its bar told first (the bus not heard yet): followed all the same.
    let again = to.with_file_name("again.sql");
    std::fs::rename(&to, &again).unwrap();
    assert_eq!(consoles.follow_file(spec.id, &again).map(|s| s.name), Some("again".into()));
    assert_eq!(consoles.specs().len(), 1);
}

/// Moved into another source's folder, it's that source's console (the same one, its
/// database and schema let go, its old connection closed); a folder holding consoles moved
/// takes them along.
#[test]
fn a_console_moved_to_another_source_is_that_source_s() {
    let project = Project::new("other-source");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let _follow = consoles.follow();
    let spec = consoles.create(access.shop, "").unwrap();
    consoles.update(spec.id, |s| s.schema = Some("main".into()));
    consoles.runner(spec.id, spec.source);
    let folder = database::console::folder(&project.root);
    std::fs::create_dir_all(folder.join(access.people.to_string())).unwrap();
    let from = spec.path(&project.root);
    let to = folder.join(access.people.to_string()).join("moved.sql");
    std::fs::rename(&from, &to).unwrap();
    consoles.events().publish(&workspace::events::FileRenamed { from, to });
    let after = consoles.spec(spec.id).unwrap();
    assert_eq!((after.source, after.name.as_str(), after.schema), (access.people, "moved", None));
    assert!(consoles.runner_of(spec.id).is_none(), "the other source's connection let go");
    assert_eq!(consoles.runner(spec.id, after.source).source(), access.people);
}

/// A console of `project` with a transaction open (a `DELETE` waiting for its Commit), its
/// runner's lease held by `lease`.
fn console_in_transaction(
    project: &Project,
    manual: &Manual,
    access: &Arc<FakeAccess>,
    consoles: &Arc<Consoles>,
) -> (ConsoleSpec, Runner, crate::runner::Lease) {
    let spec = consoles.create(access.shop, "").unwrap();
    let runner = consoles.runner(spec.id, spec.source);
    let lease = runner.lease();
    let connector = access.connector(access.shop, &Default::default()).unwrap();
    let database::console::RunPlan::Run(statements) =
        database::console::plan(&database::console::PlanInput {
            text: "BEGIN; DELETE FROM users;",
            mode: database::console::RunMode::Script,
            ..Default::default()
        })
    else {
        panic!("planned")
    };
    runner.run(crate::runner::RunRequest {
        statements,
        document: "".into(),
        settings: Default::default(),
        connector,
        source_name: "shop".into(),
        detached: false,
    });
    manual.run_all();
    assert!(runner.in_transaction());
    let _ = project;
    (spec, runner, lease)
}

/// Moved out of the consoles' folder into the project (39n), it keeps its source as an
/// attached SQL file: the same id, settings and runner — its transaction still open, its
/// connection kept —, its list now the attached files' (said once).
#[test]
fn a_console_moved_into_the_project_stays_attached_to_its_source() {
    let project = Project::new("moved-out");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let files = Arc::new(Memory::default());
    consoles.set_files_store(files.clone());
    let told = Arc::new(crate::fake::Told::default());
    consoles.set_notifier(told.clone());
    let _follow = consoles.follow();
    let (spec, runner, _lease) = console_in_transaction(&project, &manual, &access, &consoles);
    consoles.update(spec.id, |s| s.mode = database::console::RunMode::Script);
    let from = spec.path(&project.root);
    let to = project.root.join("db").join("queries.sql");
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::rename(&from, &to).unwrap();
    consoles.events().publish(&workspace::events::FileRenamed { from, to: to.clone() });
    manual.run_all();
    assert!(consoles.specs().is_empty(), "no console any more");
    let kept = consoles.spec(spec.id).expect("the same one");
    assert!(kept.is_attached());
    assert_eq!((kept.source, kept.name.as_str()), (access.shop, "queries.sql"));
    assert_eq!(kept.file, std::path::Path::new("db").join("queries.sql"));
    assert_eq!(kept.mode, database::console::RunMode::Script, "its settings kept");
    assert_eq!(kept.path(&project.root), to);
    assert_eq!(consoles.spec_of_path(&to).map(|s| s.id), Some(spec.id));
    assert_eq!(consoles.attached_files().len(), 1);
    let saved = files.saved.lock().unwrap().last().cloned().unwrap_or_default();
    assert_eq!(saved.iter().map(|s| s.id).collect::<Vec<_>>(), [spec.id], "written");
    let same = consoles.runner_of(spec.id).expect("its runner kept");
    assert_eq!(same.console(), runner.console());
    assert!(runner.in_transaction() && runner.connected(), "its transaction kept");
    assert_eq!(runner.name(), "queries.sql");
    let said: Vec<String> = told.0.lock().unwrap().iter().map(|n| n.title.clone()).collect();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("db/queries.sql") && said[0].contains("shop"), "{said:?}");
}

/// Moved out of the consoles' folder and out of the project's files (into its own folder),
/// it's a plain file: it leaves the list, its transaction rolled back and its connection
/// closed (both said).
#[test]
fn a_console_moved_out_of_its_folder_is_a_plain_file() {
    let project = Project::new("moved-away");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let told = Arc::new(crate::fake::Told::default());
    consoles.set_notifier(told.clone());
    let _follow = consoles.follow();
    let (spec, runner, _lease) = console_in_transaction(&project, &manual, &access, &consoles);
    let from = spec.path(&project.root);
    let to = project.root.join(kernel::brand::PROJECT_DIR).join("queries.sql");
    std::fs::rename(&from, &to).unwrap();
    consoles.events().publish(&workspace::events::FileRenamed { from, to });
    manual.run_all();
    assert!(consoles.spec(spec.id).is_none() && consoles.specs().is_empty());
    assert!(consoles.attached_files().is_empty());
    assert!(consoles.runner_of(spec.id).is_none());
    assert!(!runner.connected());
    assert_eq!(project.look("SELECT count(*) FROM users"), [[database::Value::Int(3)]]);
    let said: Vec<String> = told.0.lock().unwrap().iter().map(|n| n.title.clone()).collect();
    assert_eq!(said.len(), 2, "{said:?}");
    assert!(said[0].contains("console"), "the transaction rolled back: {said:?}");
    assert!(said[1].contains("queries.sql"), "a plain file now: {said:?}");
}

/// Deleted (said on the bus), it leaves the list with its runner.
#[test]
fn a_deleted_console_leaves_the_list() {
    let project = Project::new("deleted");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let _follow = consoles.follow();
    let kept = consoles.create(access.shop, "").unwrap();
    let gone = consoles.create(access.shop, "").unwrap();
    consoles.runner(gone.id, gone.source);
    std::fs::remove_file(gone.path(&project.root)).unwrap();
    let paths = vec![gone.path(&project.root)];
    consoles.events().publish(&workspace::events::EntriesDeleted { paths });
    assert_eq!(consoles.specs().iter().map(|s| s.id).collect::<Vec<_>>(), [kept.id]);
    assert!(consoles.runner_of(gone.id).is_none());
}

/// Another program's changes (the disk's watcher): a console's file gone with one `.sql`
/// new beside it is a rename, said as one (the tabs and the tree follow) and followed; one
/// gone alone is a deletion, said too; moved out of the consoles' folder, alone, it's
/// followed there; the list's own file written changes nothing.
#[test]
fn another_program_s_rename_is_followed() {
    let project = Project::new("disk-rename");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let _follow = consoles.follow();
    let (heard, _subscriptions) = heard(&consoles);
    let one = consoles.create(access.shop, "").unwrap();
    let two = consoles.create(access.shop, "").unwrap();
    let folder = database::console::folder(&project.root);
    // The list's file written: nothing looked at.
    let list = folder.join(database::console::LIST_FILE);
    consoles.events().publish(&workspace::events::EntriesChanged { paths: vec![list] });
    assert!(heard.lock().unwrap().is_empty());
    // Renamed by another program.
    let from = one.path(&project.root);
    let to = from.with_file_name("renamed.sql");
    std::fs::rename(&from, &to).unwrap();
    consoles.events().publish(&workspace::events::EntriesChanged { paths: vec![from, to.clone()] });
    assert_eq!(heard.lock().unwrap().as_slice(), ["console.sql → renamed.sql"]);
    assert_eq!(consoles.spec(one.id).map(|s| s.name), Some("renamed".into()));
    // Deleted by another program.
    std::fs::remove_file(two.path(&project.root)).unwrap();
    let paths = vec![two.path(&project.root)];
    consoles.events().publish(&workspace::events::EntriesChanged { paths });
    assert_eq!(heard.lock().unwrap().last().unwrap(), "deleted console_1.sql");
    assert!(consoles.spec(two.id).is_none());
    // Moved out by another program.
    let out = project.root.join("kept.sql");
    std::fs::rename(&to, &out).unwrap();
    consoles.events().publish(&workspace::events::EntriesChanged { paths: vec![to, out] });
    assert_eq!(heard.lock().unwrap().last().unwrap(), "renamed.sql → kept.sql");
    assert!(consoles.specs().is_empty());
    // Two gone and two new in the same folder: not told apart, deletions.
    let a = consoles.create(access.shop, "").unwrap();
    let b = consoles.create(access.shop, "").unwrap();
    let (pa, pb) = (a.path(&project.root), b.path(&project.root));
    let (na, nb) = (pa.with_file_name("x.sql"), pb.with_file_name("y.sql"));
    std::fs::rename(&pa, &na).unwrap();
    std::fs::rename(&pb, &nb).unwrap();
    consoles.events().publish(&workspace::events::EntriesChanged { paths: vec![pa, pb, na, nb] });
    assert!(heard.lock().unwrap().last().unwrap().starts_with("deleted "));
    let names: Vec<String> =
        consoles.consoles_of(access.shop).into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["x", "y"], "the new files joined by themselves");
}

/// A project's SQL file `rel` of `project`, written with `text`.
fn sql_file(project: &Project, rel: &str, text: &str) -> PathBuf {
    let path = rel.split('/').fold(project.root.clone(), |p, part| p.join(part));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    path
}

/// A project's SQL file attached to a source (39n): found back from its path, kept in its
/// own list (written in the background), the same attachment when attached again; attached
/// to another source its database and schema let go and its connection closed; detached,
/// it leaves the list. Nothing else is attachable: a console's file, another project's.
#[test]
fn a_project_s_sql_file_is_attached_and_detached() {
    let project = Project::new("attach");
    let manual = Manual::new();
    let mut fake = FakeAccess::new(&project);
    fake.archive = Some(DataSourceId::new());
    let access = Arc::new(fake);
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let files = Arc::new(Memory::default());
    let list = Arc::new(Memory::default());
    consoles.set_store(list.clone());
    consoles.set_files_store(files.clone());
    let path = sql_file(&project, "db/report.sql", "SELECT 1;");
    assert!(consoles.spec_of_path(&path).is_none(), "not attached yet");
    let seen = consoles.attachments();
    let spec = consoles.attach(&path, access.shop).expect("attached");
    assert!(consoles.attachments() > seen, "its bars look again");
    assert!(spec.is_attached());
    assert_eq!(spec.name, "report.sql");
    assert_eq!(consoles.spec_of_path(&path).map(|s| s.id), Some(spec.id));
    assert_eq!(consoles.spec(spec.id).map(|s| s.source), Some(access.shop));
    assert!(consoles.specs().is_empty(), "never a console");
    assert!(consoles.consoles_of(access.shop).is_empty(), "nor in the source's consoles");
    manual.run_all();
    let saved = files.saved.lock().unwrap().last().cloned().unwrap();
    assert_eq!(saved, std::slice::from_ref(&spec));
    assert!(list.saved.lock().unwrap().is_empty(), "the consoles' list untouched");
    // The same source again: the same attachment.
    assert_eq!(consoles.attach(&path, access.shop).map(|s| s.id), Some(spec.id));
    // Its settings changed, kept in its list.
    consoles.update(spec.id, |s| s.schema = Some("main".into()));
    manual.run_all();
    let saved = files.saved.lock().unwrap().last().cloned().unwrap();
    assert_eq!(saved[0].schema.as_deref(), Some("main"));
    // Another source: the same attachment on it, its schema let go, its runner gone.
    let runner = consoles.runner(spec.id, spec.source);
    let archive = access.archive.unwrap();
    let moved = consoles.attach(&path, archive).unwrap();
    assert_eq!((moved.id, moved.source, moved.schema.as_deref()), (spec.id, archive, None));
    assert!(consoles.runner_of(spec.id).is_none() && !runner.connected());
    // Detached: gone from its list.
    let gone = consoles.detach(&path).expect("detached");
    assert_eq!(gone.id, spec.id);
    assert!(consoles.spec_of_path(&path).is_none() && consoles.spec(spec.id).is_none());
    assert!(consoles.detach(&path).is_none(), "nothing left to detach");
    manual.run_all();
    assert_eq!(files.saved.lock().unwrap().last().cloned().unwrap(), []);
    // Not attachable: a console's file, the project's own folder, another project's file.
    let console = consoles.create(access.shop, "").unwrap();
    assert!(consoles.attach(&console.path(&project.root), access.shop).is_none());
    let own = project.root.join(kernel::brand::PROJECT_DIR).join("x.sql");
    assert!(consoles.attach(&own, access.shop).is_none());
    assert!(consoles.attach(&std::env::temp_dir().join("elsewhere.sql"), access.shop).is_none());
    assert!(consoles.attach(&project.root.join("notes.txt"), access.shop).is_none());
}

/// The attached files' list is read once from its store, as written.
#[test]
fn attached_files_are_read_back() {
    struct Kept(Vec<ConsoleSpec>);
    impl ConsoleStore for Kept {
        fn load(&self) -> Result<Vec<ConsoleSpec>, String> {
            Ok(self.0.clone())
        }
        fn save(&self, _: &[ConsoleSpec]) -> Result<(), String> {
            Ok(())
        }
    }
    let project = Project::new("attached-read");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let mut spec = ConsoleSpec::attached(access.shop, ["db", "a.sql"].iter().collect());
    spec.tx = database::console::TxMode::Manual;
    consoles.set_files_store(Arc::new(Kept(vec![spec.clone()])));
    let path = project.root.join("db").join("a.sql");
    assert_eq!(consoles.spec_of_path(&path), Some(spec.clone()));
    assert_eq!(consoles.attached_files(), [spec]);
}

/// An attached file follows its file (39n): renamed or moved in the project it's the same
/// attachment (its runner told the new name); moved into a source's consoles' folder it's
/// that source's console; moved out of the project's files it's detached (said); deleted, it
/// leaves the list.
#[test]
fn an_attached_file_follows_its_file() {
    let project = Project::new("attached-moves");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let told = Arc::new(crate::fake::Told::default());
    consoles.set_notifier(told.clone());
    let _follow = consoles.follow();
    let rename = |from: &Path, to: &Path| {
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::rename(from, to).unwrap();
        consoles
            .events()
            .publish(&workspace::events::FileRenamed { from: from.into(), to: to.into() });
    };
    let path = sql_file(&project, "a.sql", "SELECT 1;");
    let spec = consoles.attach(&path, access.shop).unwrap();
    let runner = consoles.runner(spec.id, spec.source);
    // Renamed, then its folder moved.
    let renamed = project.root.join("b.sql");
    rename(&path, &renamed);
    assert_eq!(consoles.spec_of_path(&renamed).map(|s| s.id), Some(spec.id));
    assert!(consoles.spec_of_path(&path).is_none());
    assert_eq!(runner.name(), "b.sql");
    let folder = project.root.join("sql");
    rename(&renamed, &folder.join("b.sql"));
    let old_folder = folder;
    let new_folder = project.root.join("queries");
    rename(&old_folder, &new_folder);
    let inside = new_folder.join("b.sql");
    let kept = consoles.spec_of_path(&inside).expect("followed with its folder");
    assert_eq!(kept.id, spec.id);
    assert_eq!(kept.file, Path::new("queries").join("b.sql"));
    assert!(told.0.lock().unwrap().is_empty(), "nothing said for a rename");
    // Into the source's consoles' folder: its console, the same one.
    let console =
        database::console::folder(&project.root).join(access.shop.to_string()).join("b.sql");
    rename(&inside, &console);
    let now = consoles.spec_of_path(&console).expect("a console now");
    assert_eq!((now.id, now.is_attached(), now.name.as_str()), (spec.id, false, "b"));
    assert!(consoles.attached_files().is_empty());
    assert_eq!(consoles.consoles_of(access.shop).len(), 1);
    assert_eq!(consoles.runner_of(spec.id).map(|r| r.console()), Some(spec.id));
    // Another attached file moved out of the project's files: detached, said.
    let other = sql_file(&project, "c.sql", "");
    let attached = consoles.attach(&other, access.shop).unwrap();
    let away = project.root.join("c.txt");
    rename(&other, &away);
    assert!(consoles.spec(attached.id).is_none() && consoles.attached_files().is_empty());
    let said: Vec<String> = told.0.lock().unwrap().iter().map(|n| n.title.clone()).collect();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("c.sql"), "{said:?}");
    // Deleted: it leaves the list, with its runner.
    let doomed = sql_file(&project, "d.sql", "");
    let spec = consoles.attach(&doomed, access.shop).unwrap();
    consoles.runner(spec.id, spec.source);
    std::fs::remove_file(&doomed).unwrap();
    consoles.events().publish(&workspace::events::EntriesDeleted { paths: vec![doomed] });
    assert!(consoles.spec(spec.id).is_none() && consoles.runner_of(spec.id).is_none());
}

/// Another program renaming an attached file (the disk's changes): one gone with one new SQL
/// file in its folder is followed; one gone with none stays attached (it may come back).
#[test]
fn an_attached_file_follows_the_disk() {
    let project = Project::new("attached-disk");
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = consoles(&project, &manual, Arc::clone(&access));
    let _follow = consoles.follow();
    let path = sql_file(&project, "db/a.sql", "");
    let spec = consoles.attach(&path, access.shop).unwrap();
    let to = project.root.join("db").join("renamed.sql");
    std::fs::rename(&path, &to).unwrap();
    let paths = vec![path, to.clone()];
    consoles.events().publish(&workspace::events::EntriesChanged { paths });
    assert_eq!(consoles.spec_of_path(&to).map(|s| s.id), Some(spec.id));
    // Gone with nothing new beside it: kept (a branch checked out may bring it back).
    std::fs::remove_file(&to).unwrap();
    consoles.events().publish(&workspace::events::EntriesChanged { paths: vec![to.clone()] });
    assert_eq!(consoles.spec(spec.id).map(|s| s.id), Some(spec.id));
    std::fs::write(&to, "SELECT 2;").unwrap();
    assert_eq!(consoles.spec_of_path(&to).map(|s| s.id), Some(spec.id));
}
