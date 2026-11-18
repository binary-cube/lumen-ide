use super::*;

#[test]
fn mirrors_get_only_the_changed_text() {
    let source = Document::new("fn mare() { ăă }");
    let mut mirror = Document::new("fn mic() { ăă }");
    sync_text(&source, &mut mirror);
    assert_eq!(mirror.buffer().to_string(), "fn mare() { ăă }");
    // A single undo step, which reverts to the old text.
    mirror.undo();
    assert_eq!(mirror.buffer().to_string(), "fn mic() { ăă }");
}

#[test]
fn mirrors_sync_long_texts_with_multibyte_chars() {
    let tail = "ăîșț ".repeat(3000);
    let source = Document::new(&format!("început{tail}"));
    let mut mirror = Document::new(&format!("inceput{tail}"));
    sync_text(&source, &mut mirror);
    assert_eq!(mirror.buffer().to_string(), source.buffer().to_string());
    let source = Document::new(&format!("{tail}ț"));
    let mut mirror = Document::new(&format!("{tail}ă"));
    sync_text(&source, &mut mirror);
    assert_eq!(mirror.buffer().to_string(), source.buffer().to_string());
}

#[test]
fn verbatim_windows_paths_are_simplified() {
    assert_eq!(simplify_path(PathBuf::from(r"\\?\C:\proiect")), PathBuf::from(r"C:\proiect"));
    assert_eq!(
        simplify_path(PathBuf::from(r"\\?\UNC\server\x")),
        PathBuf::from(r"\\?\UNC\server\x")
    );
    assert_eq!(simplify_path(PathBuf::from("/tmp/p")), PathBuf::from("/tmp/p"));
}

/// A file opened after startup gets the result of the background parse (colors,
/// folding), as in the app.
#[test]
fn files_opened_later_get_syntax_from_the_worker() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let support = Arc::new(support);
    let mut wb = Workbench::new(Arc::clone(&support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let worker = SyntaxWorker::spawn(Arc::clone(&support.syntax), Arc::new(|| {})).unwrap();
    wb.set_worker(worker);
    let dir = std::env::temp_dir().join(format!("syntax-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.php");
    std::fs::write(&file, "<?php\nclass A {\n    function f() {}\n}\n").unwrap();
    wb.open_project(&dir).unwrap();
    let mut text = TextSystem::new();
    let mut scene = Scene::default();
    wb.paint(&mut text, &mut scene);
    wb.open_file(&file).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut ok = false;
    while std::time::Instant::now() < deadline {
        wb.paint(&mut text, &mut scene);
        wb.poll_syntax();
        if wb.tabs[wb.active].syntax.snapshot().is_some() {
            ok = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let _ = std::fs::remove_dir_all(&dir);
    assert!(ok, "the tab didn't get the syntax snapshot");
}

#[test]
fn errors_clear_after_typing_the_missing_token() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let support = Arc::new(support);
    let mut wb = Workbench::new(Arc::clone(&support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let worker = SyntaxWorker::spawn(Arc::clone(&support.syntax), Arc::new(|| {})).unwrap();
    wb.set_worker(worker);
    let dir = std::env::temp_dir().join(format!("ns-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.php");
    std::fs::write(
        &file,
        "<?php\n\nnamespace App\n\nclass Car {\n    public function start() {\n    }\n}\n",
    )
    .unwrap();
    let mut text = TextSystem::new();
    let mut scene = Scene::default();
    wb.open_file(&file).unwrap();
    let settle = |wb: &mut Workbench, text: &mut TextSystem, scene: &mut Scene, before: u64| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            wb.paint(text, scene);
            wb.poll_syntax();
            let tab = &wb.tabs[wb.active];
            // A real parse of the current version (not just the adjusted tree).
            if tab.syntax.parses() > before
                && tab.syntax.snapshot().is_some_and(|s| s.version() == tab.doc.version())
            {
                wb.paint(text, scene);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    };
    settle(&mut wb, &mut text, &mut scene, 0);
    assert!(
        !wb.tabs[wb.active].diagnostics.errors.is_empty(),
        "the missing `;` should be reported"
    );
    // `;` at the end of the `namespace App` line, typed as in the editor.
    let end = "<?php\n\nnamespace App".len();
    wb.edit(&mut text, |doc| commands::click(doc, end, false, false));
    let before = wb.tabs[wb.active].syntax.parses();
    wb.type_text(&mut text, ";");
    settle(&mut wb, &mut text, &mut scene, before);
    let errors: Vec<_> =
        wb.tabs[wb.active].diagnostics.errors.iter().map(|e| e.range.clone()).collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(errors.is_empty(), "errors left: {errors:?}");
}

#[test]
fn session_restores_tabs_and_splits() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let support = Arc::new(support);
    let dir = std::env::temp_dir().join(format!("sess-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for name in ["a.php", "b.php", "c.php"] {
        std::fs::write(dir.join(name), "<?php\n").unwrap();
    }
    let mut text = TextSystem::new();
    let mut scene = Scene::default();
    {
        let mut wb = Workbench::new(Arc::clone(&support), Ports::local());
        wb.forget_projects();
        wb.resize(PhysicalSize::new(1200, 800), 1.0);
        wb.open_project(&dir).unwrap();
        for name in ["a.php", "b.php", "c.php"] {
            wb.open_file(&dir.join(name)).unwrap();
        }
        wb.toggle_pin(0);
        wb.split_tab(2, Split::Down, true);
        wb.paint(&mut text, &mut scene);
    }
    let mut wb = Workbench::new(Arc::clone(&support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.open_project(&dir).unwrap();
    assert!(wb.restore_session());
    let groups = wb.group_count();
    let spec = wb.editors.tree.to_spec();
    let current: Vec<String> = wb.tabs.iter().map(Tab::title).collect();
    let pinned = wb
        .editors
        .groups
        .iter()
        .flat_map(|g| g.tabs.iter())
        .chain(wb.tabs.iter())
        .filter(|t| t.pinned)
        .count();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(groups, 2);
    assert!(spec.starts_with("D("), "{spec}");
    assert_eq!(current, ["c.php"]);
    assert_eq!(pinned, 1);
}

#[test]
fn about_blocks_the_editor_behind_it() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let dir = std::env::temp_dir().join(format!("about-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, "abc").unwrap();
    let mut text = TextSystem::new();
    wb.open_file(&file).unwrap();
    wb.open_about();
    assert!(wb.modal_open());
    wb.type_text(&mut text, "x");
    wb.edit_key(&mut text, EditKey::Backspace { word: false });
    assert!(!wb.scroll(Point::new(400.0, 400.0), 0.0, 100.0));
    assert!(!wb.open_project_search());
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(wb.tabs[wb.active].doc.buffer().to_string(), "abc");
}

/// Typing, then a pause (or leaving the tab): the file is saved on its own.
#[test]
fn autosave_writes_changed_files() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let dir = std::env::temp_dir().join(format!("autosave-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, "x").unwrap();
    wb.open_file(&file).unwrap();
    let mut text = TextSystem::new();
    wb.type_text(&mut text, "y");
    assert!(wb.autosave_deadline().is_some(), "a save is scheduled after the pause");
    assert!(!wb.autosave(false), "not before the pause");
    wb.last_edit = Some(std::time::Instant::now() - crate::autosave::IDLE);
    assert!(wb.autosave(false));
    let saved = std::fs::read_to_string(&file).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    // With the final newline (on by default).
    assert_eq!(saved, "yx\n");
    assert!(!wb.tabs[wb.active].doc.is_dirty());
}

/// The groups that aren't current are saved too (a file changed in a split, then the
/// other split clicked); a file open in two groups is written once, from the current one.
#[test]
fn autosave_covers_every_group() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let dir = std::env::temp_dir().join(format!("autosave-groups-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (a, b) = (dir.join("a.txt"), dir.join("b.txt"));
    std::fs::write(&a, "a").unwrap();
    std::fs::write(&b, "b").unwrap();
    wb.open_file(&a).unwrap();
    wb.open_file(&b).unwrap();
    let mut text = TextSystem::new();
    wb.type_text(&mut text, "y");
    // b moves to a group of its own (the current one), then the first group is clicked.
    assert!(wb.split_tab(wb.active, Split::Right, true));
    assert!(wb.focus_group(0));
    assert!(wb.autosave(true));
    let saved = std::fs::read_to_string(&b).unwrap_or_default();
    let untouched = std::fs::read_to_string(&a).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(saved, "yb\n");
    assert_eq!(untouched, "a", "not changed: not written");
}

/// A module following the project (a project service): what it hears, from its project.
#[derive(Debug, Default)]
struct Hears(Arc<std::sync::Mutex<Vec<String>>>);

impl workbench_model::ProjectService for Hears {
    fn attach(&self, project: &workbench_model::ProjectContext<'_>) -> Vec<kernel::Subscription> {
        let (a, b) = (Arc::clone(&self.0), Arc::clone(&self.0));
        vec![
            project.events.subscribe(move |e: &workspace::events::ProjectOpened| {
                let name = e.root.file_name().unwrap().to_string_lossy().into_owned();
                a.lock().unwrap().push(format!("opened {name}"));
            }),
            project.events.subscribe(move |e: &document::events::FileSaved| {
                let name = e.path.file_name().unwrap().to_string_lossy().into_owned();
                b.lock().unwrap().push(format!("saved {name}"));
            }),
        ]
    }
}

/// Opening a project and saving a file are announced as domain events — on the project's
/// own bus: a module attached to it hears them; once another project opens, the first one's
/// subscriptions are gone with it.
#[test]
fn project_and_saves_are_published_as_events() {
    let (mut support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let hears = Arc::new(Hears::default());
    support.project_services.push(Arc::clone(&hears) as Arc<dyn workbench_model::ProjectService>);
    let dir = std::env::temp_dir().join(format!("events-{}", std::process::id()));
    let other = std::env::temp_dir().join(format!("events-other-{}", std::process::id()));
    for d in [&dir, &other] {
        std::fs::create_dir_all(d).unwrap();
        std::fs::write(d.join("a.txt"), "x").unwrap();
    }
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join("a.txt")).unwrap();
    wb.save_tab(wb.active, None).unwrap();
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(*hears.0.lock().unwrap(), [format!("opened {name}"), "saved a.txt".to_owned()]);
    // The first project's bus is gone: what's published now is the new project's.
    let old_bus = wb.data.events.clone();
    wb.open_project(&other).unwrap();
    hears.0.lock().unwrap().clear();
    old_bus.publish(&document::events::FileSaved { path: dir.join("a.txt") });
    assert!(hears.0.lock().unwrap().is_empty(), "the old project's subscriptions ended");
    wb.data.events.publish(&document::events::FileSaved { path: other.join("a.txt") });
    assert_eq!(*hears.0.lock().unwrap(), ["saved a.txt".to_owned()]);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&other);
}

/// A module's commands: in the menus (under its own menu), run through the workbench port;
/// without a repository the git ones do nothing (and nothing breaks).
#[test]
fn module_commands_appear_in_menus_and_run_through_the_host() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let menus = crate::commands::menus();
    let git = menus.iter().find(|m| m.title == "Git").expect("the git module's menu");
    assert!(git.items.len() >= 4);
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let dir = std::env::temp_dir().join(format!("no-git-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "x").unwrap();
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join("a.txt")).unwrap();
    let head = command_model::CommandId("git.compare_head");
    assert_eq!(wb.run_command(head), Some(false), "no repository: disabled");
    assert_eq!(wb.run_command(command_model::CommandId("no.such.command")), None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A click on the empty screen takes the keyboard from the project panel's file search:
/// typing goes to the home screen's field.
#[test]
fn clicking_the_home_screen_focuses_its_field() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let dir = std::env::temp_dir().join(format!("home-focus-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    wb.open_project(&dir).unwrap();
    let mut text = TextSystem::new();
    let mut scene = Scene::default();
    wb.paint(&mut text, &mut scene);
    assert!(wb.focus_file_search());
    let area = wb.editor_area;
    let center = Point::new(area.min_x() + area.size.width / 2.0, area.min_y() + 40.0);
    wb.mouse_down(&mut text, center, false, false, false, 1);
    assert!(wb.focus == Focus::Editor);
    wb.type_text(&mut text, "abc");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(wb.home.picker.query.text(), "abc", "the home screen's field, not the panel's");
}

/// Renaming a method: the plan found through the language's navigation, applied to the
/// open file (in the editor) and the closed one (on disk), then undone at once.
#[test]
fn renames_a_method_everywhere_and_undoes_it() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let dir = std::env::temp_dir().join(format!("rename-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // As the indexer names the files (macOS: `/private/var/…`).
    let dir = crate::workbench::canonical(&dir);
    let user = "<?php\nclass User\n{\n    public function save(): void {}\n    public function load(): void {}\n}\n";
    let used = "<?php\n$u = new User();\n$u->save();\n$other->save();\nfunction save() {}\n";
    std::fs::write(dir.join("User.php"), user).unwrap();
    std::fs::write(dir.join("use.php"), used).unwrap();
    let other = "<?php\nclass Other\n{\n    public function save(): void {}\n}\n";
    std::fs::write(dir.join("Other.php"), other).unwrap();
    wb.open_project(&dir).unwrap();
    let started = std::time::Instant::now();
    // Until the symbols of the three files are indexed.
    let indexed = |wb: &Workbench| {
        let index = wb.data.symbols.read().unwrap();
        ["User.php", "use.php", "Other.php"].iter().all(|f| index.file(&dir.join(f)).is_some())
    };
    while !indexed(&wb) && started.elapsed() < std::time::Duration::from_secs(20) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    wb.open_file(&dir.join("User.php")).unwrap();
    // The cursor on `save` (no text system: loading the fonts is slow in tests).
    let doc = &mut wb.tabs[wb.active].doc;
    let at = user.find("save").unwrap() + 1;
    document::commands::click(doc, at, false, false);
    let context = wb.rename_context().expect("a method at the cursor");
    assert_eq!(context.target.name, "save");
    // `load` is another method of `User`: a clash; `store` is free; `Other::store` isn't in the way.
    assert_eq!(context.conflicts.conflicts("load").len(), 1);
    assert!(context.conflicts.conflicts("store").is_empty());
    let mut plan = refactor::application::plan_rename(
        &context.target,
        context.sources.as_ref(),
        context.resolver.as_ref(),
        &|| false,
    );
    plan.set_replacement("store");
    let chosen: Vec<(String, u32)> = plan
        .files
        .iter()
        .flat_map(|f| {
            let name = f.path.file_name().unwrap().to_string_lossy().into_owned();
            f.occurrences.iter().filter(|o| o.chosen).map(move |o| (name.clone(), o.line))
        })
        .collect();
    // The declaration and the call on a `User`; not `$other` (`User::save` or
    // `Other::save`: unsure), not the function, not `Other::save`.
    assert_eq!(chosen, [("User.php".to_owned(), 3), ("use.php".to_owned(), 2)]);
    // Announced (whoever indexes follows): the files changed.
    let applied = Arc::new(std::sync::Mutex::new(Vec::new()));
    let heard = Arc::clone(&applied);
    let _applied = wb.data.events.subscribe(move |e: &refactor::events::RefactoringApplied| {
        heard.lock().unwrap().push(e.changed.clone());
    });
    // One batch of changes around it: its followers take the files as one.
    let order = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (started, done, ended) = (Arc::clone(&order), Arc::clone(&order), Arc::clone(&order));
    let _order = [
        wb.data.events.subscribe(move |_: &workspace::events::BulkChangeStarted| {
            started.lock().unwrap().push("started");
        }),
        wb.data.events.subscribe(move |_: &refactor::events::RefactoringApplied| {
            done.lock().unwrap().push("applied");
        }),
        wb.data.events.subscribe(move |_: &workspace::events::BulkChangeEnded| {
            ended.lock().unwrap().push("ended");
        }),
    ];
    assert_eq!(wb.apply_refactoring(&plan), Ok(2));
    assert_eq!(*order.lock().unwrap(), ["started", "applied", "ended"]);
    {
        let mut changed = applied.lock().unwrap()[0].clone();
        changed.sort();
        assert_eq!(changed, [dir.join("User.php"), dir.join("use.php")]);
    }
    // Reindexed at once (the index takes a refactoring without waiting for quiet: within
    // a few ms): the new name is a declaration (go to declaration finds it).
    let symbols = Arc::clone(&wb.data.symbols);
    let declared = |name: &str| {
        let index = symbols.read().unwrap();
        index
            .file(&dir.join("User.php"))
            .is_some_and(|f| f.symbols.iter().any(|s| s.name == name.into()))
    };
    let soon = |done: &dyn Fn() -> bool| {
        let started = std::time::Instant::now();
        while !done() && started.elapsed() < std::time::Duration::from_secs(2) {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        started.elapsed()
    };
    let took = soon(&|| declared("store") && !declared("save"));
    assert!(declared("store") && !declared("save"));
    assert!(took < std::time::Duration::from_millis(500), "at once, not after the quiet: {took:?}");
    let open = wb.tabs[wb.active].doc.buffer().to_string();
    assert!(open.contains("function store()"), "the open file is edited: {open}");
    let disk = std::fs::read_to_string(dir.join("use.php")).unwrap();
    assert!(disk.contains("$u->store();") && disk.contains("$other->save();"), "{disk}");
    assert_eq!(wb.undo_refactoring(), Some(0));
    assert_eq!(wb.tabs[wb.active].doc.buffer().to_string(), user);
    assert_eq!(std::fs::read_to_string(dir.join("use.php")).unwrap(), used);
    assert_eq!(wb.undo_refactoring(), None, "once");
    soon(&|| declared("save") && !declared("store"));
    assert!(declared("save") && !declared("store"));
    // A file changed after the refactoring keeps that work: undo leaves it as it is.
    assert_eq!(wb.apply_refactoring(&plan), Ok(2));
    let later = format!("{}// later\n", std::fs::read_to_string(dir.join("use.php")).unwrap());
    std::fs::write(dir.join("use.php"), &later).unwrap();
    assert_eq!(wb.undo_refactoring(), Some(1), "one file skipped");
    assert_eq!(std::fs::read_to_string(dir.join("use.php")).unwrap(), later, "not overwritten");
    assert_eq!(wb.tabs[wb.active].doc.buffer().to_string(), user, "the unchanged one undone");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Renaming a class changes the comments where they write it as code (a tag's type, a
/// `@see`), not by itself a word of their text: that one is shown, left to choose (plan 75).
#[test]
fn a_class_renamed_in_the_comments_that_write_it_as_code() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let dir = std::env::temp_dir().join(format!("rename-comments-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = crate::workbench::canonical(&dir);
    let user = "<?php\nnamespace App;\n\nclass User\n{\n}\n";
    let used = "<?php\nnamespace App;\n\n/**\n * Saves a User.\n *\n * @param User $user\n * @see User\n */\nfunction save(User $user): void {}\n";
    std::fs::write(dir.join("User.php"), user).unwrap();
    std::fs::write(dir.join("save.php"), used).unwrap();
    wb.open_project(&dir).unwrap();
    let started = std::time::Instant::now();
    let indexed = |wb: &Workbench| {
        let index = wb.data.symbols.read().unwrap();
        ["User.php", "save.php"].iter().all(|f| index.file(&dir.join(f)).is_some())
    };
    while !indexed(&wb) && started.elapsed() < std::time::Duration::from_secs(20) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    wb.open_file(&dir.join("User.php")).unwrap();
    let doc = &mut wb.tabs[wb.active].doc;
    document::commands::click(doc, user.find("User").unwrap() + 1, false, false);
    let context = wb.rename_context().expect("a class at the cursor");
    let plan = refactor::application::plan_rename(
        &context.target,
        context.sources.as_ref(),
        context.resolver.as_ref(),
        &|| false,
    );
    let places: Vec<(u32, bool)> = plan
        .files
        .iter()
        .filter(|f| f.path.ends_with("save.php"))
        .flat_map(|f| f.occurrences.iter().map(|o| (o.line, o.chosen)))
        .collect();
    // The text's „User” (line 4) shown, not chosen; `@param`, `@see` and the code chosen.
    assert_eq!(places, [(4, false), (6, true), (7, true), (9, true)]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Renaming a PHP class's file renames the class (the refactoring's preview opens with
/// the new name); another file is just renamed.
#[test]
fn renaming_a_class_file_renames_the_class() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let dir = std::env::temp_dir().join(format!("class-file-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = crate::workbench::canonical(&dir);
    std::fs::write(dir.join("User.php"), "<?php\nclass User {}\n").unwrap();
    std::fs::write(dir.join("helpers.php"), "<?php\nfunction help() {}\n").unwrap();
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join("helpers.php")).unwrap();
    let helpers = wb.active;
    assert!(!wb.rename_file(helpers, "tools.php"), "no class named after it");
    wb.open_file(&dir.join("User.php")).unwrap();
    let user = wb.active;
    assert!(!wb.rename_file(user, "User.txt"), "not a PHP class name");
    assert!(wb.rename_file(user, "Customer.php"));
    let view = wb.tabs[wb.active].doc.path().unwrap().to_path_buf();
    assert!(view.extension().is_some_and(|e| e == ::refactoring::EXTENSION));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The language's naming rules (PHP's): a `$variable` is renamed within its function, a
/// property from its `$`, and `$this` never.
#[test]
fn renames_php_variables_and_properties_by_the_language_rules() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let dir = std::env::temp_dir().join(format!("rename-vars-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = crate::workbench::canonical(&dir);
    let text = "<?php\nclass A\n{\n    private $count = 0;\n    public function f()\n    {\n        $y = 1;\n        return $this->count + $y;\n    }\n}\nfunction g() { $y = 2; }\n";
    std::fs::write(dir.join("A.php"), text).unwrap();
    wb.open_project(&dir).unwrap();
    let started = std::time::Instant::now();
    let indexed =
        |wb: &Workbench| wb.data.symbols.read().unwrap().file(&dir.join("A.php")).is_some();
    while !indexed(&wb) && started.elapsed() < std::time::Duration::from_secs(20) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    wb.open_file(&dir.join("A.php")).unwrap();
    let at = |wb: &mut Workbench, offset: usize| {
        document::commands::click(&mut wb.tabs[wb.active].doc, offset, false, false);
        wb.rename_context()
    };
    let y = text.find("$y").unwrap() + 1;
    let context = at(&mut wb, y).expect("a variable");
    assert_eq!(context.target.name, "$y");
    let refactor::application::Scope::Local { range, .. } = &context.target.scope else {
        panic!("a local variable");
    };
    assert!(range.contains(&y) && !range.contains(&text.rfind("$y").unwrap()), "f's only");
    let this = text.find("$this").unwrap() + 2;
    assert!(at(&mut wb, this).is_err(), "`$this` isn't renamed");
    let count = text.find("$count").unwrap();
    let context = at(&mut wb, count).expect("a property, from its `$`");
    assert_eq!(context.target.name, "count");
    assert!(matches!(context.target.scope, refactor::application::Scope::Project));
    let _ = std::fs::remove_dir_all(&dir);
}

/// `a.php:3` in the file search finds `a.php` and opens it at that place.
#[test]
fn a_place_typed_after_a_file_name() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let dir = std::env::temp_dir().join(format!("file-place-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = crate::workbench::canonical(&dir);
    std::fs::write(dir.join("a.php"), "<?php\n\necho 1;\n").unwrap();
    wb.open_project(&dir).unwrap();
    let started = std::time::Instant::now();
    while wb.data.indexer.as_ref().and_then(|i| i.peek_files()).is_none()
        && started.elapsed() < std::time::Duration::from_secs(10)
    {
        wb.ensure_index();
        wb.poll_index();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // In the project panel's search (the tree's module): ↵ opens it at that line.
    let mut text = TextSystem::new();
    assert!(wb.focus_file_search());
    wb.paint(&mut text, &mut Scene::default());
    wb.type_text(&mut text, "a.php:3");
    // Found on the search's thread: shown once it comes.
    let started = std::time::Instant::now();
    while !wb.poll_side_panel() && started.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    wb.edit_key(&mut text, EditKey::Enter { shift: false });
    assert_eq!(wb.tabs[wb.active].doc.path(), Some(dir.join("a.php").as_path()));
    assert_eq!(wb.cursor_position().map(|(line, _, _)| line), Some(2));
    assert_eq!(wb.focus, Focus::Editor, "a result opened: the editor has the keyboard");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Change Signature on a method: the declaration and the calls in another file rewritten
/// (parameters swapped, a new one with its value in the calls).
#[test]
fn changes_a_method_signature_everywhere() {
    use refactor::signature::{NewParam, find_calls};
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let dir = std::env::temp_dir().join(format!("signature-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = crate::workbench::canonical(&dir);
    let mailer = "<?php\nclass Mailer\n{\n    public function send(string $to, string $body = ''): void {}\n}\n";
    let used = "<?php\n$m = new Mailer();\n$m->send('a@b.c', 'Hi');\n$m->send(to: 'x@y.z');\n";
    std::fs::write(dir.join("Mailer.php"), mailer).unwrap();
    std::fs::write(dir.join("use.php"), used).unwrap();
    wb.open_project(&dir).unwrap();
    let started = std::time::Instant::now();
    let symbols = Arc::clone(&wb.data.symbols);
    let indexed = || {
        let index = symbols.read().unwrap();
        ["Mailer.php", "use.php"].iter().all(|f| index.file(&dir.join(f)).is_some())
    };
    while !indexed() && started.elapsed() < std::time::Duration::from_secs(20) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    wb.open_file(&dir.join("Mailer.php")).unwrap();
    let at = mailer.find("send").unwrap() + 1;
    document::commands::click(&mut wb.tabs[wb.active].doc, at, false, false);
    let context = wb.signature_context().expect("a method at the cursor");
    let old = context.target.old.clone();
    assert_eq!(old.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["to", "body"]);
    let calls = find_calls(
        &context.target,
        context.syntax.as_ref(),
        context.sources.as_ref(),
        context.resolver.as_ref(),
        &|| false,
    );
    let added =
        NewParam::Added { param: context.syntax.parse("bool $html = false"), value: "true".into() };
    let new = vec![
        NewParam::Kept { from: 1, param: old[1].clone() },
        NewParam::Kept { from: 0, param: old[0].clone() },
        added,
    ];
    let plan = calls.plan(&context.target, &new, context.syntax.as_ref(), &|_, _| None);
    assert!(wb.apply_refactoring(&plan).is_ok());
    let open = wb.tabs[wb.active].doc.buffer().to_string();
    assert!(open.contains("send(string $body = '', string $to, bool $html = false)"), "{open}");
    let disk = std::fs::read_to_string(dir.join("use.php")).unwrap();
    assert!(disk.contains("$m->send('Hi', 'a@b.c', true);"), "{disk}");
    // `to:` by name, `body` missing: its default fills the gap.
    assert!(disk.contains("$m->send('', 'x@y.z', true);"), "{disk}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Views opened below the editor (the refactorings' previews, Find Usages) share one group
/// split under it; the code they open goes to the group above.
#[test]
fn views_below_share_a_group_under_the_editor() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let dir = std::env::temp_dir().join(format!("views-below-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = crate::workbench::canonical(&dir);
    std::fs::write(dir.join("a.php"), "<?php\n").unwrap();
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join("a.php")).unwrap();
    assert!(wb.open_view_below("First", diagrams::EXTENSION, &()));
    assert_eq!((wb.group_count(), wb.editors.current), (2, 1));
    // The next one joins it, from whichever group.
    wb.focus_group(0);
    assert!(wb.open_view_below("Second", diagrams::EXTENSION, &()));
    assert_eq!((wb.group_count(), wb.editors.current, wb.tabs.len()), (2, 1, 2));
    wb.focus_code_group();
    assert_eq!(wb.editors.current, 0, "its code opens above");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Grabbing the project panel's separator without moving keeps its width (it used to jump
/// left by the tool strip's width).
#[test]
fn grabbing_the_panel_separator_keeps_the_width() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let dir = std::env::temp_dir().join(format!("panel-grab-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    wb.open_project(&dir).unwrap();
    wb.resize(PhysicalSize::new(1400, 900), 2.0);
    let before = wb.left_dock_width();
    let mut text = TextSystem::new();
    let at = Point::new(
        wb.tools.sidebar.max_x() + SPLITTER_WIDTH * 2.0 / 2.0,
        wb.tools.sidebar.min_y() + 200.0,
    );
    wb.mouse_down(&mut text, at, false, false, false, 1);
    wb.mouse_drag(&mut text, at);
    wb.mouse_up(&mut text);
    assert_eq!(wb.left_dock_width(), before);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A file changed on disk by someone else (a download, another program): its tab follows
/// when it has no unsaved changes; with them, they're kept and not saved over it unasked.
#[test]
fn tabs_follow_their_files_changed_on_disk() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let dir = std::env::temp_dir().join(format!("reload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\ntwo\n").unwrap();
    wb.open_project(&dir).unwrap();
    wb.open_file(&file).unwrap();
    let file = wb.tabs[wb.active].doc.path().unwrap().to_path_buf();
    // Clean: reloaded.
    std::fs::write(&file, "one\nTWO from the server\n").unwrap();
    assert!(wb.reload_written(std::slice::from_ref(&file)));
    assert_eq!(wb.tabs[wb.active].doc.buffer().to_string(), "one\nTWO from the server\n");
    assert!(!wb.tabs[wb.active].doc.is_dirty());
    // Our own save (the same text): nothing to do.
    assert!(!wb.reload_written(std::slice::from_ref(&file)));
    // Unsaved changes: kept; the automatic save won't write over the file.
    let len = wb.tabs[wb.active].doc.buffer().len();
    wb.tabs[wb.active].doc.apply(
        document::Transaction::single(len..len, "mine\n"),
        None,
        document::EditKind::Command,
    );
    std::fs::write(&file, "changed again\n").unwrap();
    assert!(!wb.reload_written(std::slice::from_ref(&file)));
    assert!(wb.tabs[wb.active].doc.buffer().to_string().ends_with("mine\n"));
    assert!(wb.tabs[wb.active].disk_changed);
    let _ = std::fs::remove_dir_all(&dir);
}

/// With a waker (the app), a changed file is read on a worker: its tab changes once the
/// text is back (a poll), never during the call that heard of it.
#[test]
fn a_changed_file_is_read_again_off_the_ui_thread() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&woken);
    wb.set_waker(Arc::new(move || {
        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }));
    let dir = std::env::temp_dir().join(format!("reload-bg-{:x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("log.txt");
    std::fs::write(&file, "one\n").unwrap();
    wb.open_file(&file).unwrap();
    let file = wb.tabs[wb.active].doc.path().unwrap().to_path_buf();
    std::fs::write(&file, "one\ntwo appended\n").unwrap();
    assert!(!wb.reload_written(std::slice::from_ref(&file)), "nothing read here");
    assert_eq!(wb.tabs[wb.active].doc.buffer().to_string(), "one\n");
    let started = std::time::Instant::now();
    while !wb.poll_reloads() && started.elapsed() < std::time::Duration::from_secs(10) {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(wb.tabs[wb.active].doc.buffer().to_string(), "one\ntwo appended\n");
    assert!(woken.load(std::sync::atomic::Ordering::SeqCst) > 0, "the loop woken");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A watcher that reports what the test says happened on disk: entries made, removed or
/// renamed, files written, a look at everything again. The tests that follow the disk go
/// through it: the system's own (FSEvents) loses events without a word while the disk is
/// very busy (measured: a build writing beside them, 1 of 40 heard), so through it they'd
/// only test the machine's load; the real watcher has its own tests (`fs-local`).
#[derive(Debug, Default, Clone)]
pub(super) struct FakeWatching {
    changed: Arc<std::sync::Mutex<Vec<PathBuf>>>,
    written: Arc<std::sync::Mutex<Vec<PathBuf>>>,
    rescan: Arc<std::sync::atomic::AtomicBool>,
}

impl FakeWatching {
    /// Entries made, removed or renamed (as the watcher reports them).
    pub(super) fn changed(&self, paths: &[PathBuf]) {
        self.changed.lock().unwrap().extend_from_slice(paths);
    }

    /// A file written.
    pub(super) fn written(&self, path: &Path) {
        self.written.lock().unwrap().push(path.to_path_buf());
    }

    /// Watching just started (what changed before may have been missed).
    pub(super) fn rescan(&self) {
        self.rescan.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl workspace::FileWatching for FakeWatching {
    fn watch(
        &self,
        _root: &Path,
        _dirs: workspace::WatchedDirs,
        _wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Option<Box<dyn workspace::FileWatcher>> {
        Some(Box::new(self.clone()))
    }
}

impl workspace::FileWatcher for FakeWatching {
    fn drain_changed(&self) -> Vec<PathBuf> {
        std::mem::take(&mut *self.changed.lock().unwrap())
    }

    fn drain_written(&self) -> Vec<PathBuf> {
        std::mem::take(&mut *self.written.lock().unwrap())
    }

    fn take_rescan(&self) -> bool {
        self.rescan.swap(false, std::sync::atomic::Ordering::Relaxed)
    }

    fn take_vcs_changed(&self) -> bool {
        false
    }
}

/// Recent files kept in memory.
#[derive(Debug, Default)]
struct FakeRecentFiles(std::sync::Mutex<Vec<PathBuf>>);

impl workspace::RecentFiles for FakeRecentFiles {
    fn load(&self, _root: &Path) -> Vec<PathBuf> {
        self.0.lock().unwrap().clone()
    }

    fn save(&self, _root: &Path, files: &[PathBuf]) -> io::Result<()> {
        *self.0.lock().unwrap() = files.to_vec();
        Ok(())
    }
}

/// The workbench reaches the outside through its ports: given fakes, the watcher's report
/// reloads a tab and the recent files are kept there, nothing in the project's folder.
#[test]
fn the_workbench_follows_the_ports_it_is_given() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let watching = FakeWatching::default();
    let recent = Arc::new(FakeRecentFiles::default());
    let ports = Ports {
        watching: Arc::new(watching.clone()),
        recent_files: Arc::clone(&recent) as Arc<dyn workspace::RecentFiles>,
        ..Ports::local()
    };
    let mut wb = Workbench::new(Arc::new(support), ports);
    wb.forget_projects();
    let dir = std::env::temp_dir().join(format!("ports-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\n").unwrap();
    wb.open_project(&dir).unwrap();
    wb.open_file(&file).unwrap();
    let file = wb.tabs[wb.active].doc.path().unwrap().to_path_buf();
    // Written in the background (never on the UI thread): waited for.
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while recent.0.lock().unwrap().is_empty() && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(*recent.0.lock().unwrap(), std::slice::from_ref(&file));
    assert!(!dir.join(kernel::brand::PROJECT_DIR).join("recent-files").exists());
    std::fs::write(&file, "two\n").unwrap();
    watching.written(&file);
    assert!(wb.poll_fs());
    assert_eq!(wb.tabs[wb.active].doc.buffer().to_string(), "two\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Our own save reported late by the watcher, after typing went on: not a change on disk
/// (no message, the automatic save keeps saving the file).
#[test]
fn a_late_report_of_our_own_save_is_not_a_change_on_disk() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let dir = std::env::temp_dir().join(format!("own-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, "one\n").unwrap();
    wb.open_file(&file).unwrap();
    let file = wb.tabs[wb.active].doc.path().unwrap().to_path_buf();
    let mut text = TextSystem::new();
    wb.type_text(&mut text, "x");
    wb.save_tab(wb.active, None).unwrap();
    wb.type_text(&mut text, "y");
    assert!(!wb.reload_written(std::slice::from_ref(&file)));
    assert!(!wb.tabs[wb.active].disk_changed, "ours");
    // Someone else's change still is one.
    std::fs::write(&file, "theirs\n").unwrap();
    wb.reload_written(std::slice::from_ref(&file));
    assert!(wb.tabs[wb.active].disk_changed);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The fingerprint of a text doesn't depend on how it's cut into chunks.
#[test]
fn text_fingerprint_ignores_chunking() {
    let whole = text_fingerprint(["hello world"].into_iter());
    assert_eq!(text_fingerprint(["hel", "lo w", "orld"].into_iter()), whole);
    assert_ne!(text_fingerprint(["hello"].into_iter()), whole);
}

/// Saving a comparison's result: it's done — it closes and the file's tab is in front.
#[test]
fn a_saved_comparison_closes_and_shows_its_file() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let dir = std::env::temp_dir().join(format!("compare-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, "mine\n").unwrap();
    wb.open_project(&dir).unwrap();
    wb.open_file(&file).unwrap();
    let file = wb.tabs[wb.active].doc.path().unwrap().to_path_buf();
    let input = diff::CompareInput {
        left: (file.clone(), "server\n".into()),
        right: (file.clone(), "mine\n".into()),
        labels: [None, None],
        read_only: [true, false],
    };
    assert!(workbench_model::ViewHost::compare(&mut wb, input, "a.txt · server"));
    let tabs = wb.tabs.len();
    assert!(wb.tabs[wb.active].viewer.is_some(), "the comparison in front");
    wb.save_text(&file, "server\nmine\n").unwrap();
    assert_eq!(wb.tabs.len(), tabs - 1, "the comparison closed");
    assert_eq!(wb.tabs[wb.active].doc.path(), Some(file.as_path()));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "server\nmine\n");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A comparison's result of a Windows file (`\r\n`) is saved with its endings as they
/// were, never doubled (`\r\r\n`).
#[test]
fn a_saved_result_keeps_windows_line_endings() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let dir = std::env::temp_dir().join(format!("compare-crlf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, "mine\r\n").unwrap();
    let file = crate::workbench::canonical(&file);
    wb.save_text(&file, "server\r\nmine\r\n").unwrap();
    let saved = std::fs::read(&file).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(String::from_utf8_lossy(&saved), "server\r\nmine\r\n");
}

/// A file icon set drawing every file with one icon of its own.
#[derive(Debug)]
struct OneIconSet;

impl icons::FileIconSet for OneIconSet {
    fn id(&self) -> &'static str {
        "one-icon"
    }
    fn name(&self) -> &'static str {
        "One"
    }
    fn credit(&self) -> &'static str {
        ""
    }
    fn file(&self, _name: &str, _language: &str) -> Option<&'static str> {
        Some("file")
    }
    fn folder(&self, _name: &str, _open: bool) -> Option<&'static str> {
        None
    }
    fn svg(&self, _key: &str) -> Option<std::borrow::Cow<'static, str>> {
        None
    }
}

/// The files' icons are found once per name, and again when the file icon set or the
/// theme changes, or the caches are cleared.
#[test]
fn file_icons_are_found_again_when_the_icon_set_changes() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut icons = Icons::builtin();
    let set = icons.register_file_set(Arc::new(OneIconSet));
    let palette = Palette::default();
    let mut cache = FileIcons::default();
    let ours = cache.get(&support, &icons, &palette, 0, "index.php");
    assert_eq!(ours, file_icon(&support, &icons, &palette, "index.php"));
    assert!(!Icons::is_colored(ours.0));
    assert_eq!(cache.get(&support, &icons, &palette, 0, "index.php"), ours);
    assert_eq!(cache.icons.len(), 1, "found once");
    icons.set_file_set(Some(set));
    let theirs = cache.get(&support, &icons, &palette, 0, "index.php");
    assert!(Icons::is_colored(theirs.0), "the set's icon, not the one remembered");
    icons.set_file_set(None);
    assert_eq!(cache.get(&support, &icons, &palette, 0, "index.php"), ours);
    // A new theme, then "Clear and Rebuild Caches", start over.
    cache.get(&support, &icons, &palette, 1, "a.css");
    assert_eq!(cache.icons.len(), 1, "a new theme: the names found before are gone");
    cache.purge();
    assert!(cache.icons.is_empty());
}

/// A view's tab its module gave an icon (the relations diagram's) shows it whatever the
/// file icon set; another view's tab keeps the icon its title gives.
#[test]
fn a_views_own_icon_wins_over_the_file_icon_set() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut icons = Icons::builtin();
    for icon in &support.icons {
        icons.register(icon.name, icon.svg);
    }
    let set = icons.register_file_set(Arc::new(OneIconSet));
    icons.set_file_set(Some(set));
    let palette = Palette::default();
    let name = format!("shop · Relations.{}", database::diagram::VIEW_EXTENSION);
    let (id, _) = file_icon(&support, &icons, &palette, &name);
    assert_eq!(icons.name(id), Some("relations"));
    // Another view's tab: as its title reads (`left.txt ↔ right.txt`: a text file's).
    let other = file_icon(&support, &icons, &palette, "left.txt ↔ right.txt.view-compare");
    assert_eq!(other, file_icon(&support, &icons, &palette, "left.txt ↔ right.txt"));
    assert!(Icons::is_colored(other.0), "the set's");
}

/// With a project open, modules get its own settings (`<project dir>/project.toml`): what they save
/// there is the project's, and another project starts without it.
#[test]
fn modules_keep_settings_per_project() {
    use workbench_model::WorkbenchHost;
    const FORMATTER: settings_model::Setting<String> =
        settings_model::Setting::new("formatting", "php", String::new);
    let base = std::env::temp_dir().join(format!("project-settings-{}", std::process::id()));
    let (one, two) = (base.join("one"), base.join("two"));
    std::fs::create_dir_all(&one).unwrap();
    std::fs::create_dir_all(&two).unwrap();
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    assert!(wb.project_settings().is_none(), "no project: none");
    wb.open_project(&one).unwrap();
    wb.project_settings().unwrap().set(&FORMATTER, "pint".to_owned()).unwrap();
    let saved = std::fs::read_to_string(one.join(kernel::brand::PROJECT_DIR).join("project.toml"));
    assert!(saved.unwrap().contains("pint"));
    wb.open_project(&two).unwrap();
    assert_eq!(wb.project_settings().unwrap().get(&FORMATTER), "", "another project");
    let _ = std::fs::remove_dir_all(&base);
}

/// The common start of two texts cut into chunks differently.
#[test]
fn common_prefix_spans_chunks() {
    let (a, b) = (["ab", "cdef", "x"], ["abc", "de", "fy"]);
    assert_eq!(common_prefix(a.into_iter(), b.into_iter()), 6);
    assert_eq!(common_prefix(["abc"].into_iter(), ["ab", "c", "d"].into_iter()), 3);
    assert_eq!(common_prefix(["x"].into_iter(), ["y"].into_iter()), 0);
    assert_eq!(common_prefix([""; 0].into_iter(), ["a"].into_iter()), 0);
}

/// A project with a console open (see [`console`]); forgotten and removed when dropped.
pub(crate) struct Console {
    pub(crate) wb: Workbench,
    pub(crate) dir: PathBuf,
    pub(crate) path: PathBuf,
}

impl Drop for Console {
    fn drop(&mut self) {
        self.wb.forget_projects();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A project with a SQLite file as its source, a console of it open.
pub(crate) fn console(name: &str) -> Console {
    let dir = std::env::temp_dir().join(format!("bar-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let dir = crate::workbench::canonical(&dir);
    std::fs::write(dir.join("plain.sql"), "SELECT 1;\n").unwrap();
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.open_project(&dir).unwrap();
    let id = workbench_model::WorkbenchHost::project_id(&wb).unwrap();
    let sessions = database_explorer::sessions(id).unwrap();
    let catalog = sessions.catalog();
    catalog.set_store(Arc::new(database::NoStore));
    catalog.ensure_loaded();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while catalog.list(database::Scope::Project) == database_explorer::catalog::Listed::Loading
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let mut shop = database::DataSource::new("shop", "sqlite", database::Scope::Project);
    shop.file = Some(dir.join("shop.db"));
    shop.secrets = database::SecretPolicy::Never;
    let source = shop.id;
    catalog.add(shop).unwrap();
    let consoles = database_console::consoles::consoles(id).unwrap();
    consoles.set_store(Arc::new(database::console::NoConsoles));
    let spec = consoles.create(source, "SELECT 1;\nSELECT 2;\n").unwrap();
    let path = spec.path(&dir);
    wb.open_file(&path).unwrap();
    Console { wb, dir, path }
}

/// The context menus as shown in a project: the editor's on a file, the tree's on the
/// active file (nothing copied), on another file (text copied) and on a folder — for the
/// golden check (`keymaps.rs`).
pub(crate) fn context_menus() -> Vec<(&'static str, Vec<ui_workbench::MenuEntry>)> {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let dir = std::env::temp_dir().join(format!("context-menus-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let dir = crate::workbench::canonical(&dir);
    std::fs::write(dir.join("a.php"), "<?php\n").unwrap();
    std::fs::write(dir.join("b.php"), "<?php\n").unwrap();
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join("a.php")).unwrap();
    let mut menus = Vec::new();
    menus.push(("editor", wb.context_menu(workbench_model::MenuPlace::Editor)));
    for (name, entry, copied) in [
        ("tree file", "a.php", None),
        ("tree file, another active, text copied", "b.php", Some("x")),
        ("tree folder", "sub", Some("x")),
    ] {
        let copied = copied.map(str::to_owned);
        wb.set_clipboard_reader(std::rc::Rc::new(move || copied.clone()));
        wb.set_menu_target(Some(dir.join(entry)));
        let place = if entry == "sub" {
            workbench_model::MenuPlace::TreeFolder
        } else {
            workbench_model::MenuPlace::TreeFile
        };
        menus.push((name, wb.context_menu(place)));
    }
    wb.clear_menu_target();
    let _ = std::fs::remove_dir_all(&dir);
    menus
}

/// Drags separator `splitter` by `(dx, dy)` (physical pixels), as the mouse does.
fn drag_splitter(wb: &mut Workbench, text: &mut TextSystem, at: Rect, dx: f32, dy: f32) {
    let from = Point::new(at.min_x() + at.size.width / 2.0, at.min_y() + at.size.height / 2.0);
    wb.mouse_down(text, from, false, false, false, 1);
    wb.mouse_drag(text, Point::new(from.x + dx / 2.0, from.y + dy / 2.0));
    wb.mouse_drag(text, Point::new(from.x + dx, from.y + dy));
    wb.mouse_up(text);
}

/// The bottom dock opens, never dragged in the project, at ~45% of the window (a result
/// shows 8–10 rows), the editor keeping at least 30% — whatever the window's height.
#[test]
fn the_bottom_dock_starts_at_almost_half_the_window() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    for (height, scale) in [(900, 1.0), (1400, 2.0), (600, 1.0)] {
        wb.resize(PhysicalSize::new(1400, height), scale);
        if !wb.bottom_shows(services_panel::TOOL) {
            assert!(wb.toggle_tool(services_panel::TOOL));
        }
        let bottom = wb.tools.bottom_bounds;
        let available = bottom.max_y() - wb.card.min_y();
        let share = bottom.size.height / available;
        assert!((0.40..=0.46).contains(&share), "{height}: {share}");
        assert!(wb.card.size.height >= available * 0.30, "{height}: the editor keeps 30%");
        // In a usual window, taller than the 280 px it used to open at (one row of a result).
        if height as f32 / scale >= 850.0 {
            assert!(bottom.size.height / scale > 330.0, "{height}");
        }
    }
    assert_eq!(wb.data.sizes.get(workbench_model::sizes::DOCK_BOTTOM), None);
}

/// One size per dock: every tool window shown there takes it (switching between them never
/// makes the area jump); dragged with one, the others follow.
#[test]
fn switching_tool_windows_in_a_dock_keeps_its_size() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1400, 900), 1.0);
    let mut text = TextSystem::new();
    // The bottom: Services, then the terminal, then Services again.
    assert!(wb.toggle_tool(services_panel::TOOL));
    let services = wb.tools.bottom_bounds;
    assert!(wb.toggle_tool("terminal") && wb.bottom_shows("terminal"));
    assert_eq!(wb.tools.bottom_bounds, services, "the terminal takes Services' height");
    let at = wb.tools.terminal_splitter.bounds();
    drag_splitter(&mut wb, &mut text, at, 0.0, -90.0);
    let dragged = wb.tools.bottom_bounds;
    assert!(dragged.size.height > services.size.height + 60.0);
    assert!(wb.toggle_tool(services_panel::TOOL) && wb.bottom_shows(services_panel::TOOL));
    assert_eq!(wb.tools.bottom_bounds, dragged, "Services takes the terminal's");
    assert_eq!(wb.tools.bottom_panel().map(|s| s.bounds), Some(dragged));
    // The right: each panel registered there as wide as the first.
    let right: Vec<&'static str> =
        wb.docked(ui_workbench::Dock::Right).iter().map(|t| t.id()).collect();
    let mut widths = Vec::new();
    for id in right.iter().copied() {
        if wb.show_side_panel(id)
            && let Some(side) = wb.tools.side_on(ui_workbench::Dock::Right)
        {
            widths.push((id, side.bounds));
        }
    }
    assert!(widths.len() >= 2, "{right:?}");
    assert!(widths.iter().all(|(_, b)| *b == widths[0].1), "{widths:?}");
    // Dragged with the last one, the first takes it too.
    let at = wb.tools.side_splitter.bounds();
    drag_splitter(&mut wb, &mut text, at, -70.0, 0.0);
    let wide = wb.tools.side_on(ui_workbench::Dock::Right).unwrap().bounds;
    assert!(wide.size.width > widths[0].1.size.width + 50.0);
    assert!(wb.show_side_panel(widths[0].0));
    assert_eq!(wb.tools.side_on(ui_workbench::Dock::Right).unwrap().bounds, wide);
}

/// The docks' sizes are the project's: dragged, then the project opened again (another
/// window, as at the next start), they're as they were; another project keeps its own;
/// a double click on a separator gives its default back.
#[test]
fn dock_sizes_come_back_with_the_project_and_each_project_has_its_own() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let support = Arc::new(support);
    let root = std::env::temp_dir().join(format!("dock-sizes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (a, b) = (root.join("a"), root.join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let mut text = TextSystem::new();
    let mut scene = Scene::default();
    let window = |support: &Arc<crate::modules::LanguageSupport>| {
        let mut wb = Workbench::new(Arc::clone(support), Ports::local());
        wb.forget_projects();
        wb.resize(PhysicalSize::new(1400, 900), 1.0);
        wb
    };
    let right = |wb: &Workbench| wb.docked(ui_workbench::Dock::Right)[0].id();
    let (bottom_a, right_a, left_a);
    {
        let mut wb = window(&support);
        wb.open_project(&a).unwrap();
        assert!(wb.toggle_tool(services_panel::TOOL));
        let id = right(&wb);
        assert!(wb.show_side_panel(id));
        wb.paint(&mut text, &mut scene);
        let at = wb.tools.terminal_splitter.bounds();
        drag_splitter(&mut wb, &mut text, at, 0.0, 120.0);
        let at = wb.tools.side_splitter.bounds();
        drag_splitter(&mut wb, &mut text, at, -60.0, 0.0);
        let at = wb.tools.splitter.bounds();
        drag_splitter(&mut wb, &mut text, at, 50.0, 0.0);
        bottom_a = wb.tools.bottom_bounds;
        right_a = wb.tools.side_on(ui_workbench::Dock::Right).unwrap().bounds;
        left_a = wb.left_dock_width();
        assert!(wb.data.sizes.get(workbench_model::sizes::DOCK_BOTTOM).is_some());
        assert!(left_a > TREE_WIDTH);
        wb.freeze_session();
    }
    {
        // Project b: its own (the defaults: never dragged there), then dragged its way.
        let mut wb = window(&support);
        wb.open_project(&b).unwrap();
        assert!(wb.toggle_tool(services_panel::TOOL));
        assert_ne!(wb.tools.bottom_bounds, bottom_a);
        assert_eq!(wb.left_dock_width(), TREE_WIDTH);
        let at = wb.tools.terminal_splitter.bounds();
        drag_splitter(&mut wb, &mut text, at, 0.0, -100.0);
        wb.freeze_session();
    }
    let mut wb = window(&support);
    wb.open_project(&a).unwrap();
    assert!(wb.restore_session());
    assert!(wb.bottom_shows(services_panel::TOOL));
    assert_eq!(wb.tools.bottom_bounds, bottom_a);
    assert_eq!(wb.left_dock_width(), left_a);
    let id = right(&wb);
    assert!(wb.show_side_panel(id));
    assert_eq!(wb.tools.side_on(ui_workbench::Dock::Right).unwrap().bounds, right_a);
    // The same window opening b: b's, nothing of a's.
    wb.freeze_session();
    wb.open_project(&b).unwrap();
    assert_eq!(wb.left_dock_width(), TREE_WIDTH);
    assert!(wb.restore_session());
    assert!(wb.bottom_shows(services_panel::TOOL));
    let bottom_b = wb.tools.bottom_bounds;
    assert!(bottom_b.size.height > bottom_a.size.height + 150.0, "{bottom_b:?} {bottom_a:?}");
    // A double click on its separator: the default again (and saved so).
    let at = wb.tools.terminal_splitter.bounds();
    let center = Point::new(at.min_x() + at.size.width / 2.0, at.min_y() + at.size.height / 2.0);
    wb.mouse_down(&mut text, center, false, false, false, 2);
    wb.mouse_up(&mut text);
    assert_eq!(wb.data.sizes.get(workbench_model::sizes::DOCK_BOTTOM), None);
    let _ = std::fs::remove_dir_all(&root);
}

/// The index of the layer cutting `card`'s corners back (a ring of the window's background
/// whose inner edge is the card's outline), if one was drawn.
fn corner_mask(wb: &Workbench, scene: &Scene, card: Rect) -> Option<usize> {
    let r = (CARD_RADIUS * wb.scale).round();
    scene.layers().iter().position(|l| {
        l.clip.is_some_and(|c| c.min_x() == card.min_x() && c.max_y() == card.max_y())
            && l.quads.iter().any(|q| {
                q.border_color == wb.palette.window_background
                    && q.border_width == r
                    && q.bounds.min_x() == card.min_x() - r
                    && q.bounds.max_y() == card.max_y() + r
            })
    })
}

/// Every card's bottom corners stay rounded whatever its content paints (user images 394
/// vs 395: the Database window's list background, filled to its square edges, cut them):
/// after a panel's content, a layer clipped to the card covers what's past its outline
/// and draws its border again — the side panels on both sides, the bottom window and the
/// editor's card, in both themes.
#[test]
fn every_card_s_corners_are_cut_back_over_what_its_content_draws() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    let dir = std::env::temp_dir().join(format!("card-corners-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.php"), "<?php\n").unwrap();
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join("a.php")).unwrap();
    wb.resize(PhysicalSize::new(1400, 900), 2.0);
    assert!(wb.show_side_panel("database"));
    assert!(wb.toggle_tool(services_panel::TOOL));
    let mut text = TextSystem::new();
    for id in ["default-dark", "default-light"] {
        let mut registry = theming::ThemeRegistry::default();
        for theme in themes_builtin::builtin_themes() {
            registry.register(theme);
        }
        wb.set_theme(registry.resolve(&theming::ThemeId::parse(id)).unwrap());
        let mut scene = Scene::default();
        wb.paint(&mut text, &mut scene);
        let cards = wb.tools.sides.iter().map(|s| s.bounds).chain([wb.card]);
        for card in cards {
            let mask = corner_mask(&wb, &scene, card).unwrap_or_else(|| panic!("{card:?}"));
            // Drawn after everything filled in the card's bottom-left corner.
            let corner = Point::new(card.min_x() + 1.0, card.max_y() - 1.0);
            let last_fill = scene.layers().iter().rposition(|l| {
                l.clip.is_none_or(|c| c.contains(corner))
                    && l.quads.iter().any(|q| q.color.a > 0.0 && q.bounds.contains(corner))
            });
            assert!(last_fill.is_some_and(|f| f < mask), "{card:?}: {last_fill:?} {mask}");
            // The border again, over the content.
            let border = &scene.layers()[mask].quads[1];
            assert_eq!((border.bounds, border.border_color), (card, wb.palette.card_border));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a key typed and the frames cost on the UI thread, in a whole workbench (release):
/// an ordinary PHP class and a generated one of tens of thousands of lines, the caret in a
/// method's body; an idle frame, a key and the frame after it, `$this->` (completion).
/// `cargo test -p app --release --bin lumen measure_typing -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, run on demand"]
#[allow(clippy::disallowed_methods)] // Measured by the test, not the app.
fn measure_typing() {
    use std::time::{Duration, Instant};
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let support = Arc::new(support);
    let dir = std::env::temp_dir().join(format!("measure-typing-{}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let class = |methods: usize| {
        let mut s =
            String::from("<?php\n\nnamespace App;\n\nclass Big\n{\n    private int $n = 0;\n\n");
        for i in 0..methods {
            s.push_str(&format!(
                "    /** Doc {i}. */\n    public function method{i}(int $a, string $b): int\n    {{\n        $x = $a + {i};\n        return $this->n + $x;\n    }}\n\n"
            ));
        }
        s.push_str("}\n");
        s
    };
    for (name, methods) in [("ordinary.php", 60), ("huge.php", 6000)] {
        let file = dir.join(name);
        let text = class(methods);
        std::fs::write(&file, &text).unwrap();
        let mut wb = Workbench::new(Arc::clone(&support), Ports::local());
        wb.forget_projects();
        wb.resize(PhysicalSize::new(2400, 1500), 2.0);
        let worker = SyntaxWorker::spawn(Arc::clone(&support.syntax), Arc::new(|| {})).unwrap();
        wb.set_worker(worker);
        wb.open_project(&dir).unwrap();
        let (mut ts, mut scene) = (TextSystem::new(), Scene::default());
        wb.open_file(&file).unwrap();
        // Parsed and settled first.
        let settled = Instant::now() + Duration::from_secs(20);
        while Instant::now() < settled && wb.tabs[wb.active].syntax.snapshot().is_none() {
            wb.paint(&mut ts, &mut scene);
            wb.poll_syntax();
            std::thread::sleep(Duration::from_millis(5));
        }
        let at = text.find(&format!("$x = $a + {};", methods / 2)).unwrap() + "$x = $a".len();
        wb.edit(&mut ts, |doc| commands::click(doc, at, false, false));
        for _ in 0..5 {
            wb.paint(&mut ts, &mut scene);
            wb.poll_syntax();
        }
        let mut time = |f: &mut dyn FnMut(&mut Workbench, &mut TextSystem, &mut Scene)| {
            let mut worst = Duration::ZERO;
            let mut total = Duration::ZERO;
            for _ in 0..20 {
                let started = Instant::now();
                f(&mut wb, &mut ts, &mut scene);
                let took = started.elapsed();
                worst = worst.max(took);
                total += took;
            }
            (total / 20, worst)
        };
        let idle = time(&mut |wb, ts, scene| {
            wb.paint(ts, scene);
        });
        let key = time(&mut |wb, ts, _| {
            wb.type_text(ts, "y");
        });
        let after = time(&mut |wb, ts, scene| {
            wb.type_text(ts, "z");
            wb.poll_syntax();
            wb.paint(ts, scene);
        });
        let member = time(&mut |wb, ts, scene| {
            wb.type_text(ts, " $this->");
            wb.paint(ts, scene);
        });
        println!(
            "{name} ({} lines): idle frame {:?} (worst {:?}); key {:?} (worst {:?}); key + frame {:?} (worst {:?}); `$this->` + frame {:?} (worst {:?})",
            text.lines().count(),
            idle.0,
            idle.1,
            key.0,
            key.1,
            after.0,
            after.1,
            member.0,
            member.1
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// `$x->` on a receiver of no known type, in a project with libraries
/// (`EDITOR_MEASURE_PROJECT=<folder>`, a copy: a file is written in its `src`): what each key
/// after it costs on the UI thread, the index built first.
/// `EDITOR_MEASURE_PROJECT=… cargo test -p app --release --bin lumen measure_member_typing -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, run on demand"]
#[allow(clippy::disallowed_methods)] // Measured by the test, not the app.
fn measure_member_typing() {
    use std::time::{Duration, Instant};
    let Some(root) = std::env::var_os("EDITOR_MEASURE_PROJECT").map(PathBuf::from) else {
        eprintln!("EDITOR_MEASURE_PROJECT not set: skipped");
        return;
    };
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let support = Arc::new(support);
    let file = root.join("src").join("MeasureMembers.php");
    let source = "<?php\n\nfunction measure($x)\n{\n    $x->\n    new \n    \n}\n";
    std::fs::write(&file, source).unwrap();
    let mut wb = Workbench::new(Arc::clone(&support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(2400, 1500), 2.0);
    let worker = SyntaxWorker::spawn(Arc::clone(&support.syntax), Arc::new(|| {})).unwrap();
    wb.set_worker(worker);
    wb.open_project(&root).unwrap();
    let (mut ts, mut scene) = (TextSystem::new(), Scene::default());
    let deadline = Instant::now() + Duration::from_secs(120);
    while Instant::now() < deadline && !wb.index_ready() {
        wb.poll_indexer();
        wb.paint(&mut ts, &mut scene);
        std::thread::sleep(Duration::from_millis(20));
    }
    wb.open_file(&file).unwrap();
    for _ in 0..5 {
        wb.paint(&mut ts, &mut scene);
        wb.poll_syntax();
    }
    let at = "<?php\n\nfunction measure($x)\n{\n    $x->".len();
    wb.edit(&mut ts, |doc| commands::click(doc, at, false, false));
    let keys = |wb: &mut Workbench,
                ts: &mut TextSystem,
                scene: &mut Scene,
                what: &str,
                chars: &[&str]| {
        for ch in chars {
            let started = Instant::now();
            wb.type_text(ts, ch);
            let key = started.elapsed();
            wb.paint(ts, scene);
            println!("{what} + {ch:?}: {key:?} (the key), {:?} with its frame", started.elapsed());
        }
    };
    keys(&mut wb, &mut ts, &mut scene, "`$x->`", &["g", "e", "t"]);
    // A type's name (`new …`), a function's (a statement's start).
    let now = |wb: &Workbench| wb.tabs[wb.active].doc.buffer().text();
    let at = now(&wb).find("new ").unwrap() + "new ".len();
    wb.edit(&mut ts, |doc| commands::click(doc, at, false, false));
    keys(&mut wb, &mut ts, &mut scene, "`new `", &["R", "e", "q"]);
    let at = now(&wb).rfind("    \n}").unwrap() + 4;
    wb.edit(&mut ts, |doc| commands::click(doc, at, false, false));
    keys(&mut wb, &mut ts, &mut scene, "a statement", &["s", "t", "r"]);
    for _ in 0..3 {
        wb.edit(&mut ts, commands::backspace);
    }
    // To profile (`sample <pid>`): keys typed and taken back for this many seconds.
    if let Some(seconds) = std::env::var("EDITOR_MEASURE_LOOP").ok().and_then(|s| s.parse().ok()) {
        println!("looping, pid {}", std::process::id());
        let until = Instant::now() + Duration::from_secs(seconds);
        while Instant::now() < until {
            wb.type_text(&mut ts, "s");
            wb.edit(&mut ts, commands::backspace);
        }
    }
    let _ = std::fs::remove_file(&file);
}
