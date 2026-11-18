use super::*;
use code_intelligence::{CompletionContext, ContextKind, FileSymbols, Symbol, SymbolKind, Target};
use std::sync::atomic::AtomicUsize;

/// Test language: each `class X` line declares class X; counts the extractions.
#[derive(Debug, Default)]
struct Fake(AtomicUsize);

impl LanguageIntelligence for Fake {
    fn language(&self) -> kernel::LanguageId {
        kernel::LanguageId::new_static("fake")
    }
    fn extensions(&self) -> &[&'static str] {
        &["fk"]
    }
    fn extract(&self, text: &str) -> FileSymbols {
        self.0.fetch_add(1, Ordering::SeqCst);
        // `class X` declares X; `method y` a member of the class above it.
        let mut symbols = Vec::new();
        let mut class: Option<std::sync::Arc<str>> = None;
        for line in text.lines() {
            let (name, kind, container) = if let Some(name) = line.strip_prefix("class ") {
                class = Some(name.into());
                (name, SymbolKind::Class, None)
            } else if let Some(name) = line.strip_prefix("method ") {
                (name, SymbolKind::Method, class.clone())
            } else {
                continue;
            };
            symbols.push(Symbol {
                name: name.into(),
                kind,
                fqn: name.into(),
                container,
                parents: Box::default(),
                range: 0..1,
                offset: 0,
                line: 0,
                detail: None,
                is_static: false,
                visibility: Default::default(),
                note: None,
                types: None,
            });
        }
        FileSymbols { symbols, ..Default::default() }
    }
    fn context(&self, _: &str, o: usize) -> CompletionContext {
        CompletionContext { replace: o..o, prefix: String::new(), kind: ContextKind::None }
    }
    fn target(&self, _: &str, _: usize) -> Option<Target> {
        None
    }
    fn locals(&self, _: &str, _: usize) -> Vec<String> {
        Vec::new()
    }
    fn keywords(&self) -> &[&'static str] {
        &[]
    }
}

fn wait(cond: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !cond() {
        assert!(Instant::now() < deadline, "timeout");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn builds_caches_and_follows_file_changes() {
    // The folders the languages' modules register (Composer's, npm's, the builds').
    kernel::paths::register(kernel::ProjectDirs {
        dependencies: &["vendor", "node_modules"],
        unlisted: &["node_modules"],
        generated: &["dist", "build", "storage", "public/build", "var/cache"],
        ..Default::default()
    });
    let root = std::env::temp_dir().join(format!("indexer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("node_modules")).unwrap();
    std::fs::write(root.join("src/a.fk"), "class Alpha\n").unwrap();
    std::fs::write(root.join("node_modules/x.fk"), "class Ignored\n").unwrap();
    let root = root.canonicalize().unwrap();

    let fake = Arc::new(Fake::default());
    let indexer = ProjectIndexer::start(root.clone(), vec![fake], Arc::new(|| {})).unwrap();
    wait(|| !indexer.is_busy());
    let has = |name: &str| indexer.index().read().unwrap().type_by_fqn(name).is_some();
    assert!(has("Alpha") && !has("Ignored"));
    let data = root.join(kernel::brand::PROJECT_DIR);
    assert!(data.join("index/symbols.bin").exists());
    assert_eq!(std::fs::read_to_string(data.join(".gitignore")).unwrap(), "*\n");

    assert!(indexer.take_updated().is_empty(), "a build's files aren't listed");
    // New file: appears in the index without a rebuild, and is said to have changed (once).
    std::fs::write(root.join("src/b.fk"), "class Beta\n").unwrap();
    indexer.changed(vec![root.join("src/b.fk")]);
    wait(|| has("Beta"));
    wait(|| !indexer.take_updated().is_empty());
    assert!(indexer.take_updated().is_empty());
    drop(indexer);

    // Second startup: everything comes from the cache, nothing is reparsed.
    std::thread::sleep(Duration::from_millis(400));
    let fake2 = Arc::new(Fake::default());
    let indexer =
        ProjectIndexer::start(root.clone(), vec![fake2.clone()], Arc::new(|| {})).unwrap();
    wait(|| !indexer.is_busy());
    assert!(indexer.index().read().unwrap().type_by_fqn("Beta").is_some());
    assert_eq!(fake2.0.load(Ordering::SeqCst), 0, "from the cache, nothing extracted");
    drop(indexer);
    let _ = std::fs::remove_dir_all(&root);
}

/// A project not in use lets go of its index and reads it back, what changed meanwhile too.
#[test]
fn resting_empties_the_index_and_waking_brings_it_back() {
    let root = std::env::temp_dir().join(format!("indexer-rest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.fk"), "class Alpha\n").unwrap();
    let root = root.canonicalize().unwrap();
    let fake = Arc::new(Fake::default());
    let indexer = ProjectIndexer::start(root.clone(), vec![fake.clone()], Arc::new(|| {})).unwrap();
    wait(|| !indexer.is_busy());
    let has = |name: &str| indexer.index().read().unwrap().type_by_fqn(name).is_some();
    assert!(has("Alpha"));
    let parsed = fake.0.load(Ordering::SeqCst);

    indexer.rest();
    wait(|| !has("Alpha"));
    let cache = root.join(kernel::brand::PROJECT_DIR).join("index/symbols.bin");
    assert!(std::fs::metadata(&cache).unwrap().len() > 0, "its cache stays whole");
    // Written while it rests, and told: nothing happens until it wakes.
    std::fs::write(root.join("src/b.fk"), "class Beta\n").unwrap();
    indexer.changed(vec![root.join("src/b.fk")]);
    indexer.refresh();

    indexer.wake();
    wait(|| has("Alpha") && has("Beta"));
    assert_eq!(fake.0.load(Ordering::SeqCst), parsed + 1, "only the new file parsed");
    drop(indexer);
    let _ = std::fs::remove_dir_all(&root);
}

/// Closed while it rests: the cache isn't overwritten by what it (doesn't) hold.
#[test]
fn closing_while_resting_keeps_the_cache() {
    let root = std::env::temp_dir().join(format!("indexer-closed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.fk"), "class Alpha\n").unwrap();
    let root = root.canonicalize().unwrap();
    let indexer =
        ProjectIndexer::start(root.clone(), vec![Arc::new(Fake::default())], Arc::new(|| {}))
            .unwrap();
    wait(|| !indexer.is_busy());
    indexer.rest();
    wait(|| indexer.index().read().unwrap().is_empty());
    drop(indexer);
    std::thread::sleep(Duration::from_millis(400));
    let fake = Arc::new(Fake::default());
    let indexer = ProjectIndexer::start(root.clone(), vec![fake.clone()], Arc::new(|| {})).unwrap();
    wait(|| !indexer.is_busy());
    assert!(indexer.index().read().unwrap().type_by_fqn("Alpha").is_some());
    assert_eq!(fake.0.load(Ordering::SeqCst), 0, "from the cache, nothing extracted");
    drop(indexer);
    let _ = std::fs::remove_dir_all(&root);
}

/// The text index stays on disk until someone searches: never waited for, and what changed
/// meanwhile is found.
#[test]
fn the_text_index_is_read_when_asked_and_finds_what_changed_since() {
    let root = std::env::temp_dir().join(format!("indexer-lazy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.php"), "<?php // sends the invoice\n").unwrap();
    let root = root.canonicalize().unwrap();
    let indexer =
        ProjectIndexer::start(root.clone(), vec![Arc::new(Fake::default())], Arc::new(|| {}))
            .unwrap();
    wait(|| !indexer.is_busy() && indexer.residency.is_cold());
    assert_eq!(indexer.search.read().unwrap().len(), 0, "not in memory");
    std::fs::write(root.join("src/b.php"), "<?php // archives the receipt\n").unwrap();
    indexer.changed(vec![root.join("src/b.php")]);

    assert!(indexer.search("invoice", 5).is_empty(), "the first question doesn't wait");
    wait(|| indexer.take_search_loaded());
    assert_eq!(indexer.search("invoice", 5).len(), 1);
    assert_eq!(indexer.search("receipt", 5).len(), 1, "changed while it was on disk");
    drop(indexer);
    let _ = std::fs::remove_dir_all(&root);
}

/// What's excluded isn't indexed; marked again, it's looked at without a restart.
#[test]
fn excluded_folders_stay_out_and_come_back_when_unmarked() {
    use workspace::marks::{PathMark, PathMarks, ProjectMarks};
    let root = std::env::temp_dir().join(format!("indexer-marks-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("old/site")).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.fk"), "class Alpha\n").unwrap();
    std::fs::write(root.join("old/site/b.fk"), "class Beta\n").unwrap();
    let root = root.canonicalize().unwrap();
    let mut excluded = PathMarks::default();
    excluded.set("old", Some(PathMark::Excluded));
    let marks = ProjectMarks::new(excluded);
    let indexer = ProjectIndexer::start_marked(
        root.clone(),
        marks.clone(),
        vec![Arc::new(Fake::default())],
        Arc::new(|| {}),
    )
    .unwrap();
    wait(|| !indexer.is_busy());
    let has = |name: &str| indexer.index().read().unwrap().type_by_fqn(name).is_some();
    assert!(has("Alpha") && !has("Beta"));
    assert!(!indexer.wants(&root.join("old/site/b.fk")));

    marks.replace(PathMarks::default());
    indexer.refresh();
    wait(|| has("Beta"));
    drop(indexer);
    let _ = std::fs::remove_dir_all(&root);
}

/// What changed on disk without being told (a file made before anyone watched) is found
/// when it's asked to look again — only that file parsed.
#[test]
fn a_refresh_finds_what_changed_and_parses_only_that() {
    let root = std::env::temp_dir().join(format!("indexer-refresh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.fk"), "class Alpha\n").unwrap();
    let root = root.canonicalize().unwrap();
    let fake = Arc::new(Fake::default());
    let indexer = ProjectIndexer::start(root.clone(), vec![fake.clone()], Arc::new(|| {})).unwrap();
    wait(|| !indexer.is_busy());
    let has = |name: &str| indexer.index().read().unwrap().type_by_fqn(name).is_some();
    std::fs::write(root.join("src/g.fk"), "class Gamma\n").unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(!has("Gamma"), "it watches nothing itself");
    let parsed = fake.0.load(Ordering::SeqCst);
    indexer.refresh();
    wait(|| has("Gamma"));
    assert_eq!(fake.0.load(Ordering::SeqCst), parsed + 1, "only the new file parsed");
    drop(indexer);
    let _ = std::fs::remove_dir_all(&root);
}

/// What it did, for the Task Manager: the files read, the cache written, its builds and
/// their files, its thread's CPU.
#[test]
fn its_usage_is_counted() {
    let root = std::env::temp_dir().join(format!("indexer-usage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.fk"), "class Alpha\n").unwrap();
    std::fs::write(root.join("src/b.fk"), "class Beta\n").unwrap();
    let root = root.canonicalize().unwrap();
    let fake = Arc::new(Fake::default());
    let indexer = ProjectIndexer::start(root.clone(), vec![fake], Arc::new(|| {})).unwrap();
    wait(|| !indexer.is_busy());
    let usage = indexer.usage();
    let load = |a: &std::sync::atomic::AtomicU64| a.load(Ordering::Relaxed);
    wait(|| load(&usage.built) == 1);
    assert_eq!(load(&usage.builds), 1);
    assert!(load(&usage.read) >= ("class Alpha\n".len() + "class Beta\n".len()) as u64);
    assert!(load(&usage.written) > 0, "the cache was written");
    assert_eq!(load(&usage.files_done), load(&usage.files), "the build went through all");
    // Windows counts a thread's CPU in ~16 ms steps: a build this small may show none.
    assert!(cfg!(windows) || load(&usage.cpu) > 0, "its thread's CPU");
    drop(indexer);
    let _ = std::fs::remove_dir_all(&root);
}

/// A dependency's file kept packed (plan 74): its class found at once, its members when
/// asked for (the walks over every file see only its class), the project's own file as
/// before; and the same after a start from the cache (its block not packed again).
#[test]
fn a_dependency_s_file_is_kept_packed_and_its_members_come_when_asked() {
    kernel::paths::register(kernel::ProjectDirs {
        dependencies: &["vendor", "node_modules"],
        unlisted: &["node_modules"],
        generated: &["dist", "build", "storage", "public/build", "var/cache"],
        ..Default::default()
    });
    let root = std::env::temp_dir().join(format!("indexer-packed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("vendor/lib")).unwrap();
    std::fs::write(root.join("src/a.fk"), "class Own\nmethod mine\n").unwrap();
    std::fs::write(root.join("vendor/lib/b.fk"), "class Builder\nmethod where\nmethod order\n")
        .unwrap();
    let root = root.canonicalize().unwrap();
    let check = |indexer: &ProjectIndexer| {
        let index = indexer.index();
        let index = index.read().unwrap();
        assert!(index.type_by_fqn("Builder").is_some());
        let walked = index.files().find(|(p, _)| p.ends_with("vendor/lib/b.fk")).unwrap().1;
        assert_eq!(walked.symbols.len(), 1, "its class only, in the walks");
        let members: Vec<&str> = index.declared_members("Builder").map(|(_, s)| &*s.name).collect();
        assert_eq!(members, ["where", "order"], "unpacked when asked for");
        assert_eq!(index.declared_members("Own").count(), 1, "the project's own as before");
        assert_eq!(index.file(&root.join("vendor/lib/b.fk")).unwrap().symbols.len(), 3);
    };
    let indexer =
        ProjectIndexer::start(root.clone(), vec![Arc::new(Fake::default())], Arc::new(|| {}))
            .unwrap();
    wait(|| !indexer.is_busy());
    check(&indexer);
    drop(indexer);
    std::thread::sleep(Duration::from_millis(400));
    let fake = Arc::new(Fake::default());
    let indexer = ProjectIndexer::start(root.clone(), vec![fake.clone()], Arc::new(|| {})).unwrap();
    wait(|| !indexer.is_busy());
    check(&indexer);
    assert_eq!(fake.0.load(Ordering::SeqCst), 0, "from the cache, nothing extracted");
    drop(indexer);
    let _ = std::fs::remove_dir_all(&root);
}

/// A manager's file naming its dependency folder (plan 74g): the files there are a library's
/// (kept packed); a folder marked "normal" in a dependency folder is the project's own.
#[test]
fn a_manager_s_folder_is_a_library_and_a_normal_mark_is_not() {
    fn listed(text: &str) -> Vec<String> {
        text.lines().map(str::to_owned).collect()
    }
    kernel::paths::register(kernel::ProjectDirs {
        dependencies: &["vendor"],
        found_by: &[kernel::DependencyFinder { manifest: "deps.txt", folders: listed }],
        ..Default::default()
    });
    let root = std::env::temp_dir().join(format!("indexer-found-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("lib/acme")).unwrap();
    std::fs::create_dir_all(root.join("vendor/mine")).unwrap();
    std::fs::write(root.join("deps.txt"), "lib\n").unwrap();
    std::fs::write(root.join("lib/acme/a.fk"), "class Acme\nmethod run\n").unwrap();
    std::fs::write(root.join("vendor/mine/b.fk"), "class Mine\nmethod go\n").unwrap();
    let root = root.canonicalize().unwrap();
    let mut marks = workspace::marks::PathMarks::default();
    marks.set("vendor/mine", Some(workspace::marks::PathMark::Normal));
    let indexer = ProjectIndexer::start_marked(
        root.clone(),
        ProjectMarks::new(marks),
        vec![Arc::new(Fake::default())],
        Arc::new(|| {}),
    )
    .unwrap();
    wait(|| !indexer.is_busy());
    let index = indexer.index();
    let index = index.read().unwrap();
    let walked = |end: &str| index.files().find(|(p, _)| p.ends_with(end)).unwrap().1.symbols.len();
    assert_eq!(walked("lib/acme/a.fk"), 1, "a library's: its class only in the walks");
    assert_eq!(walked("vendor/mine/b.fk"), 2, "marked normal: the project's own");
    assert_eq!(kernel::paths::found(&root), ["lib"]);
    drop(index);
    drop(indexer);
    kernel::paths::set_found(&root, Vec::new());
    let _ = std::fs::remove_dir_all(&root);
}

/// A build over many files (several batches, read on several threads): every file indexed
/// once, each parsed once, and the next start reads them all from the cache.
#[test]
fn a_build_reads_many_files_on_several_threads() {
    let root = std::env::temp_dir().join(format!("indexer-many-{:x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    let count = READ_BATCH * 2 + 37;
    for i in 0..count {
        std::fs::write(root.join(format!("src/f{i}.fk")), format!("class C{i}\nmethod m{i}\n"))
            .unwrap();
    }
    let root = root.canonicalize().unwrap();
    let fake = Arc::new(Fake::default());
    let indexer = ProjectIndexer::start(root.clone(), vec![fake.clone()], Arc::new(|| {})).unwrap();
    wait(|| !indexer.is_busy());
    {
        let index = indexer.index();
        let index = index.read().unwrap();
        assert!((0..count).all(|i| index.type_by_fqn(&format!("C{i}")).is_some()));
    }
    assert_eq!(fake.0.load(Ordering::SeqCst), count, "each file parsed once");
    assert!(indexer.usage().cpu.load(Ordering::Relaxed) > 0, "its CPU counted");
    drop(indexer);
    std::thread::sleep(Duration::from_millis(400));
    let again = Arc::new(Fake::default());
    let indexer =
        ProjectIndexer::start(root.clone(), vec![again.clone()], Arc::new(|| {})).unwrap();
    wait(|| !indexer.is_busy());
    assert_eq!(again.0.load(Ordering::SeqCst), 0, "all from the cache");
    drop(indexer);
    let _ = std::fs::remove_dir_all(&root);
}
