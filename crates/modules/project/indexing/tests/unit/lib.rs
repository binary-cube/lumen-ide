use super::*;

thread_local! {
    /// Every wait to rest, on this test's thread (none: the module's own).
    pub(super) static WAIT: std::cell::Cell<Option<std::time::Duration>> =
        const { std::cell::Cell::new(None) };
}

#[test]
fn the_module_brings_the_index_and_its_report() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(IndexingModule));
    let (mut contributions, reports) = registry.activate(&extensions::ModuleSelection::default());
    assert!(reports.iter().all(|r| r.state == extensions::ModuleState::Active));
    assert_eq!(contributions.take::<Arc<dyn IndexProvider>>().len(), 1);
    assert_eq!(contributions.take::<Arc<dyn ResourceReporter>>().len(), 1);
}

/// A language declaring classes in `.fk` files (`class Name` per line).
#[derive(Debug, Default)]
struct Fake;

impl code_intelligence::LanguageIntelligence for Fake {
    fn language(&self) -> kernel::LanguageId {
        kernel::LanguageId::new_static("fake")
    }
    fn extensions(&self) -> &[&'static str] {
        &["fk"]
    }
    fn extract(&self, text: &str) -> code_intelligence::FileSymbols {
        use code_intelligence::{Symbol, SymbolKind};
        let symbols = text
            .lines()
            .filter_map(|l| l.strip_prefix("class "))
            .map(|name| Symbol {
                name: name.into(),
                kind: SymbolKind::Class,
                fqn: name.into(),
                container: None,
                parents: Box::default(),
                range: 0..1,
                offset: 0,
                line: 0,
                detail: None,
                is_static: false,
                visibility: Default::default(),
                note: None,
                types: None,
            })
            .collect();
        code_intelligence::FileSymbols { symbols, ..Default::default() }
    }
    fn context(&self, _: &str, o: usize) -> code_intelligence::CompletionContext {
        code_intelligence::CompletionContext {
            replace: o..o,
            prefix: String::new(),
            kind: code_intelligence::ContextKind::None,
        }
    }
    fn target(&self, _: &str, _: usize) -> Option<code_intelligence::Target> {
        None
    }
    fn locals(&self, _: &str, _: usize) -> Vec<String> {
        Vec::new()
    }
    fn keywords(&self) -> &[&'static str] {
        &[]
    }
}

/// The index opened for a project: its declarations, its paths, followed through its events,
/// and what the Task Manager is told of it.
#[test]
fn an_index_opens_follows_its_project_and_reports_itself() {
    let dir = std::env::temp_dir().join(format!("indexing-module-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.fk"), "class Alpha\n").unwrap();
    let dir = dir.canonicalize().unwrap();
    let events = kernel::EventBus::new();
    let id = kernel::ProjectId::new();
    let index = Provider
        .open(IndexContext {
            marks: Default::default(),
            id,
            root: &dir,
            events: &events,
            wake: Arc::new(|| {}),
            intelligence: vec![Arc::new(Fake)],
        })
        .unwrap();
    let wait = |done: &dyn Fn() -> bool| {
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_secs(10) {
            if done() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        false
    };
    let declares = |name: &str| {
        index
            .symbols()
            .read()
            .unwrap()
            .files()
            .any(|(_, f)| f.symbols.iter().any(|s| s.name == name.into()))
    };
    assert!(wait(&|| declares("Alpha") && !index.is_busy()));
    // Saved: followed through the project's events.
    std::fs::write(dir.join("b.fk"), "class Beta\n").unwrap();
    events.publish(&document::events::FileSaved { path: dir.join("b.fk") });
    assert!(wait(&|| declares("Beta")));
    assert!(index.ensure_paths());
    assert!(wait(&|| index.take_paths_built()));
    assert!(
        index.find_paths(&workspace::PathQuery { text: "b.fk", folders: 0, files: 5 }).files.len()
            == 1
    );
    // The Task Manager's part: its project (by identity), its files, its queue, what it
    // read, its CPU, its build as an operation (ended).
    let parts = Reporter.report();
    let part = parts.iter().find(|p| p.project == Some(id)).expect("its part");
    let name = i18n::tr!(
        "tasks.project_index",
        project = dir.file_name().unwrap().to_string_lossy().into_owned()
    );
    assert_eq!(part.name, name);
    assert_eq!(part.owner, "indexing");
    assert_eq!(part.group, workbench_model::resources::INDEXING, "listed under Indexing");
    assert!(part.threads.contains(&"index-queue"));
    assert!(!part.detail.is_empty());
    assert!(part.read.unwrap() >= "class Alpha\n".len() as u64);
    assert!(part.cpu.is_some());
    let build = part.activities.iter().find(|a| a.kind == ActivityKind::Index).expect("its build");
    assert_eq!((build.project, build.running()), (Some(id), false));
    drop(index);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_selection_on_one_line_fills_the_search() {
    assert_eq!(search_seed("  cart total   ").as_deref(), Some("cart total"));
    assert_eq!(search_seed("two\nlines"), None);
    assert_eq!(search_seed("   "), None);
}

/// A project whose window was left lets its index go after a while, back when it comes
/// forward.
#[test]
fn a_left_project_rests_until_it_comes_forward() {
    let dir = std::env::temp_dir().join(format!("indexing-left-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.fk"), "class Alpha\n").unwrap();
    let dir = dir.canonicalize().unwrap();
    let events = kernel::EventBus::new();
    // Not brought forward after it opens: left, it rests (soon, here).
    WAIT.with(|w| w.set(Some(std::time::Duration::from_millis(300))));
    let index = Provider
        .open(IndexContext {
            marks: Default::default(),
            id: kernel::ProjectId::new(),
            root: &dir,
            events: &events,
            wake: Arc::new(|| {}),
            intelligence: vec![Arc::new(Fake)],
        })
        .unwrap();
    let declares = || {
        index
            .symbols()
            .read()
            .unwrap()
            .files()
            .any(|(_, f)| f.symbols.iter().any(|s| s.name == "Alpha".into()))
    };
    let wait = |done: &dyn Fn() -> bool| {
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_secs(10) {
            if done() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
    };
    assert!(wait(&|| declares() && !index.is_busy()), "built");
    assert!(wait(&|| !declares()), "rested once left");
    events.publish(&workspace::events::ProjectBroughtForward);
    assert!(wait(&declares), "back once forward");
    // Left again and back before the wait ends: nothing goes.
    WAIT.with(|w| w.set(Some(std::time::Duration::from_secs(60))));
    events.publish(&workspace::events::ProjectLeft);
    events.publish(&workspace::events::ProjectBroughtForward);
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(declares());
    drop(index);
    let _ = std::fs::remove_dir_all(&dir);
}
