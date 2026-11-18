//! The project's indexing as it works today (step 15a of plan 15: the safety net before it
//! becomes a module): the declarations and the files follow saves, the disk, renames and
//! deletions, a bulk paste, a rebuild; another project starts clean.

use super::*;

/// A project in a scratch folder (`a.php`: class `Alpha`), opened, its first index done.
fn opened(name: &str) -> (Workbench, PathBuf) {
    opened_with(name, Ports::local())
}

/// [`opened`], through `ports`.
fn opened_with(name: &str, ports: Ports) -> (Workbench, PathBuf) {
    let dir = std::env::temp_dir().join(format!("indexing-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.php"), "<?php\nclass Alpha {}\n").unwrap();
    let dir = crate::workbench::canonical(&dir);
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), ports);
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.open_project(&dir).unwrap();
    // The watcher starts on its own thread; once it watches, it asks for a look at
    // everything again (what changed meanwhile). Waited for too: later in a test it would
    // mark the indexes out of date.
    let watching = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let heard = Arc::clone(&watching);
    let _watching = wb.data.events.subscribe(move |_: &workspace::events::EntriesRescan| {
        heard.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    assert!(settle(&mut wb, |wb| declares(wb, "Alpha")), "the first index");
    assert!(
        settle(&mut wb, |_| watching.load(std::sync::atomic::Ordering::Relaxed)),
        "watching the project"
    );
    (wb, dir)
}

/// A declaration named `name` is in the project's index.
fn declares(wb: &Workbench, name: &str) -> bool {
    let Ok(index) = wb.data.symbols.read() else { return false };
    let query = code_intelligence::SymbolQuery { text: name, limit: 20, ..Default::default() };
    code_intelligence::search_symbols(&index, &query).iter().any(|h| h.name == name)
}

/// The project's file search finds `name`.
fn finds_file(wb: &mut Workbench, name: &str) -> bool {
    let found = wb.search_paths(name, 0, 20);
    found.files.iter().any(|f| f.path.rsplit('/').next() == Some(name))
}

/// Waits (20 s at most) for `done`, the app's loop polling meanwhile.
fn settle(wb: &mut Workbench, done: impl Fn(&mut Workbench) -> bool) -> bool {
    // Generous: the machine may be busy with every other test.
    settle_for(wb, 90, done)
}

fn settle_for(wb: &mut Workbench, secs: u64, done: impl Fn(&mut Workbench) -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < deadline {
        wb.poll_fs();
        wb.poll_index();
        wb.poll_indexer();
        wb.poll_symbols();
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

/// A file saved, made on disk by others, renamed and deleted (as the watcher reports them):
/// the declarations follow, and the file search finds what's there.
#[test]
fn the_index_follows_saves_and_the_disk() {
    let watching = crate::workbench::tests::FakeWatching::default();
    // Watching started: what's on disk looked at (as the real one asks once it's live).
    watching.rescan();
    let (mut wb, dir) =
        opened_with("follow", Ports { watching: Arc::new(watching.clone()), ..Ports::local() });
    // Saved from its tab.
    wb.open_file(&dir.join("a.php")).unwrap();
    let len = wb.tabs[wb.active].doc.buffer().len();
    assert!(workbench_model::EditorHost::edit_active(&mut wb, 0..len, "<?php\nclass Gamma {}\n"));
    wb.save_tab(wb.active, None).unwrap();
    watching.written(&dir.join("a.php"));
    assert!(settle(&mut wb, |wb| declares(wb, "Gamma") && !declares(wb, "Alpha")));
    // Made on disk by another program.
    let beta = dir.join("b.php");
    std::fs::write(&beta, "<?php\nclass Beta {}\n").unwrap();
    watching.changed(std::slice::from_ref(&beta));
    watching.written(&beta);
    assert!(
        settle(&mut wb, |wb| declares(wb, "Beta") && finds_file(wb, "b.php")),
        "a file made on disk: declared {} and found {}",
        declares(&wb, "Beta"),
        finds_file(&mut wb, "b.php")
    );
    // Renamed on disk.
    let gamma = dir.join("c.php");
    std::fs::rename(&beta, &gamma).unwrap();
    watching.changed(&[beta, gamma.clone()]);
    assert!(settle(&mut wb, |wb| finds_file(wb, "c.php") && !finds_file(wb, "b.php")));
    assert!(declares(&wb, "Beta"));
    // Deleted.
    std::fs::remove_file(&gamma).unwrap();
    watching.changed(std::slice::from_ref(&gamma));
    assert!(settle(&mut wb, |wb| !declares(wb, "Beta") && !finds_file(wb, "c.php")));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A folder of 40 files pasted at once: indexed in the end (the indexer resumed after it).
#[test]
fn a_bulk_paste_ends_indexed() {
    let (mut wb, dir) = opened("paste");
    let from = std::env::temp_dir().join(format!("indexing-paste-src-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&from);
    std::fs::create_dir_all(from.join("lib")).unwrap();
    for i in 0..40 {
        std::fs::write(
            from.join(format!("lib/C{i}.php")),
            format!("<?php\nclass Pasted{i} {{}}\n"),
        )
        .unwrap();
    }
    let os: std::rc::Rc<std::cell::RefCell<Vec<PathBuf>>> = std::rc::Rc::default();
    let read = std::rc::Rc::clone(&os);
    wb.set_files_clipboard(FilesClipboard {
        read: std::rc::Rc::new(move || read.borrow().clone()),
        write: std::rc::Rc::new(|_: &[PathBuf]| {}),
    });
    *os.borrow_mut() = vec![from.join("lib")];
    let stats = |wb: &Workbench| wb.data.indexer.as_ref().unwrap().stats();
    let before = stats(&wb);
    wb.set_menu_target(Some(dir.clone()));
    assert_eq!(wb.run_command_named("tree.paste"), Some(true));
    wb.clear_menu_target();
    assert!(settle(&mut wb, |wb| (0..40).all(|i| declares(wb, &format!("Pasted{i}")))));
    assert!(settle(&mut wb, |wb| !wb.indexing()), "not left paused or busy");
    // One batch of changes: a few reindexes, not one per file (the watcher's word on the
    // 40 files, coming after, merges into one more at most).
    std::thread::sleep(std::time::Duration::from_millis(500));
    settle_for(&mut wb, 1, |_| false);
    let after = stats(&wb);
    let batches = after.batches - before.batches;
    assert!(batches <= 3, "{batches} reindexes for one paste: {before:?} → {after:?}");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&from);
}

/// Clear and Rebuild Caches builds the index again; another project sees nothing of this one.
#[test]
fn a_rebuild_and_another_project_start_clean() {
    let (mut wb, dir) = opened("rebuild");
    // The first indexing over before listening: on a slow machine it could still be going
    // and be heard finishing.
    assert!(settle(&mut wb, |wb| finds_file(wb, "a.php")
        && declares(wb, "Alpha")
        && !wb.indexing()));
    // Announced: each index starting, then up to date.
    let heard = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (started, finished) = (Arc::clone(&heard), Arc::clone(&heard));
    let _s = wb.data.events.subscribe(move |e: &workspace::events::IndexingStarted| {
        started.lock().unwrap().push((e.index, true));
    });
    let _f = wb.data.events.subscribe(move |e: &workspace::events::IndexingFinished| {
        finished.lock().unwrap().push((e.index, false));
    });
    wb.purge();
    finds_file(&mut wb, "a.php");
    // Waited for the announcements themselves: the index may still be the old one (and
    // idle) for a moment after the purge.
    let symbols_done = {
        let heard = Arc::clone(&heard);
        move || {
            let heard = heard.lock().unwrap();
            heard.iter().filter(|(i, _)| *i == workspace::events::IndexKind::Symbols).count() >= 2
        }
    };
    assert!(settle(&mut wb, |wb| symbols_done() && declares(wb, "Alpha") && !wb.indexing()));
    assert!(settle(&mut wb, |wb| finds_file(wb, "a.php")));
    {
        use workspace::events::IndexKind::{Paths, Symbols};
        let heard = heard.lock().unwrap();
        for index in [Paths, Symbols] {
            let order: Vec<bool> =
                heard.iter().filter(|(i, _)| *i == index).map(|(_, s)| *s).collect();
            assert_eq!(order, [true, false], "{index:?} started, then finished");
        }
    }
    let other = std::env::temp_dir().join(format!("indexing-other-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&other);
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("o.php"), "<?php\nclass Omega {}\n").unwrap();
    wb.open_project(&other).unwrap();
    assert!(!declares(&wb, "Alpha"), "nothing of the previous project");
    assert!(settle(&mut wb, |wb| declares(wb, "Omega")));
    assert!(!finds_file(&mut wb, "a.php"));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&other);
}

// ─── The engines' comparison (`tools/lsp-compare`) ───

/// The built-in engine's answers to the engines' comparison (`tools/lsp-compare/run.sh`):
/// the probes prepared by it (`EDITOR_PROBES`, JSON) asked of the app as the editor asks
/// them, PHP on the built-in engine whatever the settings say; the answers and how long
/// each took go to `EDITOR_PROBE_ANSWERS`. Ignored, and nothing done without them.
#[test]
#[ignore = "the engines' comparison: run by tools/lsp-compare/run.sh"]
fn the_comparisons_probes_answered_by_the_built_in_engine() {
    let (Some(probes), Some(out)) =
        (std::env::var_os("EDITOR_PROBES"), std::env::var_os("EDITOR_PROBE_ANSWERS"))
    else {
        return;
    };
    let probes: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(probes).unwrap()).unwrap();
    let mut chosen = language_servers::settings::config();
    chosen.engines = vec![(
        "php".into(),
        language_servers::settings::Choice {
            engine: code_intelligence::servers::Engine::BuiltIn,
            server: None,
        },
    )];
    let _pinned = language_servers::settings::pin(chosen);
    let mut text = TextSystem::new();
    let mut answers = serde_json::Map::new();
    let mut projects = serde_json::Map::new();
    for (name, root) in probes["projects"].as_object().unwrap() {
        let root = crate::workbench::canonical(&PathBuf::from(root.as_str().unwrap()));
        let (mut wb, took) = probed_project(&root);
        projects.insert(name.clone(), serde_json::json!({ "index_seconds": took }));
        for probe in probes["probes"].as_array().unwrap() {
            if probe["project"].as_str() == Some(name.as_str()) {
                let id = probe["id"].as_str().unwrap().to_owned();
                answers.insert(id, probe_answer(&mut wb, &mut text, &root, probe));
            }
        }
    }
    let written = serde_json::json!({ "engine": "ours", "projects": projects, "answers": answers });
    std::fs::write(out, serde_json::to_string_pretty(&written).unwrap()).unwrap();
}

/// A probed project opened and indexed (its libraries too): the workbench, and how long
/// the first index took (seconds).
fn probed_project(root: &Path) -> (Workbench, f64) {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1400, 900), 1.0);
    let started = std::time::Instant::now();
    wb.open_project(root).unwrap();
    // The project's own bus (made with it): its first index announced done — or, had it
    // ended before anyone listened, seen busy and then idle.
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let heard = Arc::clone(&done);
    let _done = wb.data.events.subscribe(move |e: &workspace::events::IndexingFinished| {
        if e.index == workspace::events::IndexKind::Symbols {
            heard.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    });
    let busy = std::cell::Cell::new(false);
    let indexed = settle_for(&mut wb, 600, |wb| {
        busy.set(busy.get() || wb.indexing());
        let idle = !wb.indexing() && wb.data.symbols.read().is_ok_and(|i| !i.is_empty());
        (done.load(std::sync::atomic::Ordering::Relaxed) || busy.get()) && idle
    });
    assert!(indexed, "{} indexed", root.display());
    (wb, started.elapsed().as_secs_f64())
}

/// One probe asked: its file open with the probe's text (a probe's code inserted), the
/// cursor at its place; the question asked four times (the first cold, the others warm),
/// the text given back after.
fn probe_answer(
    wb: &mut Workbench,
    text: &mut TextSystem,
    root: &Path,
    probe: &serde_json::Value,
) -> serde_json::Value {
    let path = root.join(probe["file"].as_str().unwrap());
    wb.open_file(&path).unwrap();
    let original = wb.tabs[wb.active].doc.buffer().text();
    let asked = probe["text"].as_str().unwrap_or(&original).to_owned();
    let changed = asked != original;
    if changed {
        wb.edit(text, |doc| {
            let end = doc.buffer().len();
            commands::replace_range(doc, 0..end, &asked);
        });
    }
    let offset = probe["offset"].as_u64().unwrap_or(0) as usize;
    wb.edit(text, |doc| commands::click(doc, offset, false, false));
    let category = probe["category"].as_str().unwrap();
    let mut times = Vec::new();
    let mut result = serde_json::Value::Null;
    let asks = if matches!(category, "diagnostics" | "noise") { 1 } else { 4 };
    for _ in 0..asks {
        let started = std::time::Instant::now();
        result = probe_ask(wb, text, root, category, offset);
        times.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    if changed {
        wb.edit(text, |doc| {
            let end = doc.buffer().len();
            commands::replace_range(doc, 0..end, &original);
        });
    }
    serde_json::json!({ "result": result, "ms": times })
}

/// A file's place as the probes say it: relative to the project, its line (0-based).
fn probe_place(root: &Path, path: &Path, line: u32) -> serde_json::Value {
    let relative = path.strip_prefix(root).unwrap_or(path);
    serde_json::json!([relative.to_string_lossy().replace('\\', "/"), line])
}

fn probe_ask(
    wb: &mut Workbench,
    text: &mut TextSystem,
    root: &Path,
    category: &str,
    offset: usize,
) -> serde_json::Value {
    use serde_json::json;
    match category {
        "completion" => {
            wb.update_completion(text);
            let labels: Vec<String> =
                wb.completion.items().iter().map(|i| i.label.clone()).collect();
            wb.completion.close();
            json!(labels)
        }
        "definition" => wb
            .definition_at(offset)
            .map_or(json!([]), |l| json!([probe_place(root, &l.path, l.line)])),
        "hover" => wb.code_hover_card(offset).map_or(json!(null), |card| {
            let mut shown = Vec::new();
            shown.extend(card.title.map(|(t, _)| t));
            shown.extend(card.subtitle);
            shown.push(card.code);
            shown.extend(card.doc);
            shown.extend(card.notes.into_iter().map(|(n, _)| n));
            for p in card.params {
                shown.push(format!("{} {}", p.ty.unwrap_or_default(), p.name));
            }
            json!(shown.join("\n"))
        }),
        "signature" => wb
            .param_hint_card(offset)
            .map_or(json!(null), |c| json!({ "label": c.code, "active": c.active })),
        "references" | "rename" => {
            let context =
                if category == "rename" { wb.rename_context() } else { wb.usages_context() };
            let Ok(context) = context else { return json!([]) };
            let plan = refactor::application::plan_rename(
                &context.target,
                context.sources.as_ref(),
                context.resolver.as_ref(),
                &|| false,
            );
            let places: Vec<_> = plan
                .files
                .iter()
                .flat_map(|f| {
                    f.occurrences
                        .iter()
                        .filter(|o| o.chosen)
                        .map(|o| probe_place(root, &f.path, o.line))
                })
                .collect();
            json!(places)
        }
        "diagnostics" | "noise" => {
            // Until the file's analyses are in: the index's checks come after the syntax's.
            let mut last = String::new();
            let mut steady = std::time::Instant::now();
            let started = std::time::Instant::now();
            while started.elapsed() < std::time::Duration::from_secs(20) {
                wb.poll_servers();
                wb.poll_diagnostics();
                wb.poll_symbols();
                wb.poll_jobs();
                wb.poll_index();
                // The frameworks' problems (found in the background, as the app asks).
                wb.poll_frameworks();
                wb.paint(text, &mut Scene::default());
                let now = format!("{:?}", wb.tabs[wb.active].diagnostics.marks);
                if now != last {
                    last = now;
                    steady = std::time::Instant::now();
                } else if steady.elapsed() > std::time::Duration::from_secs(3) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            let tab = &wb.tabs[wb.active];
            let source = tab.text();
            let marks: Vec<_> = tab
                .diagnostics
                .marks
                .iter()
                .map(|m| {
                    let severity = if m.weak {
                        "hint"
                    } else if m.warning {
                        "warning"
                    } else {
                        "error"
                    };
                    let message = match &m.code {
                        Some(code) => format!("{} {code}", m.message),
                        None => m.message.clone(),
                    };
                    json!({
                        "line": kernel::text::line_of(&source, m.range.start),
                        "message": message,
                        "severity": severity,
                    })
                })
                .collect();
            json!(marks)
        }
        other => panic!("unknown category {other}"),
    }
}
