//! What each step of the start-up holds of the heap (its live bytes, counted by the tests'
//! allocator): the fonts, the language and keymaps, the modules, the themes, the first
//! window's workbench, a project with one `index.php` opened in it (the app's allocator
//! counts them, `kernel::memory::Counting`). Run on demand:
//! `cargo test -p app startup_memory -- --ignored --nocapture`.

use std::time::Duration;

use super::*;

/// The loop's wakes for `time` (what background work finished, taken as the app does).
fn wakes(workbench: &mut Workbench, time: Duration) {
    let until = std::time::Instant::now() + time;
    while std::time::Instant::now() < until {
        workbench.poll_syntax();
        workbench.poll_index();
        workbench.poll_symbols();
        workbench.poll_indexer();
        workbench.poll_fs();
        workbench.poll_viewers();
        workbench.poll_decorations();
        workbench.poll_jobs();
        workbench.poll_entry_changes();
        workbench.poll_diagnostics();
        workbench.poll_servers();
        workbench.poll_frameworks();
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The heap's live bytes once the threads a step started had time to settle.
fn settled() -> usize {
    std::thread::sleep(Duration::from_millis(1500));
    kernel::memory::live_heap().unwrap_or(0)
}

fn mb(bytes: isize) -> String {
    format!("{:>8.1} MB", bytes as f64 / 1_048_576.0)
}

#[test]
#[ignore = "a measurement, run on demand"]
fn startup_memory() {
    if kernel::memory::live_heap().is_none() {
        eprintln!("no counting allocator: skipped");
        return;
    }
    let mut steps: Vec<(&str, usize)> = vec![("before", settled())];
    let locale = i18n::locale();
    let mut text = TextSystem::with_locale(&locale);
    let family = text.first_available(CODE_FONTS, FontFamily::Monospace);
    text.warm_up(&[
        text_layout::Font::new(FontFamily::SansSerif, 13.0),
        text_layout::Font::new(family, 13.0),
    ]);
    steps.push(("fonts", settled()));
    language::load();
    keymaps::load();
    steps.push(("language, keymaps", settled()));
    let (support, _) = modules::load(&extensions::ModuleSelection::default());
    steps.push(("modules registered", settled()));
    let themes = Themes::load(&support.themes);
    steps.push(("themes", settled()));
    let support = std::sync::Arc::new(support);
    // What the start does next: the modules' data read ahead, the environment looked at.
    support.manager.start();
    steps.push(("modules' data ahead", settled()));
    for intel in &support.intelligence {
        intel.warm();
    }
    steps.push(("intelligence warmed", settled()));
    let mut workbench = Workbench::new(std::sync::Arc::clone(&support), workbench::Ports::local());
    steps.push(("workbench", settled()));
    let project = std::env::temp_dir().join(format!("startup-memory-{}", std::process::id()));
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("index.php"), "<?php\n\necho 'hello';\n").unwrap();
    workbench.open_path(&project).unwrap();
    steps.push(("project opened", settled()));
    workbench.open_path(&project.join("index.php")).unwrap();
    steps.push(("index.php opened", settled()));
    wakes(&mut workbench, Duration::from_secs(10));
    steps.push(("10 s of wakes", settled()));

    let mut last = 0usize;
    eprintln!("\n{:<20} {:>11} {:>11}", "step", "added", "live");
    for (name, live) in &steps {
        eprintln!("{name:<20} {} {}", mb(*live as isize - last as isize), mb(*live as isize));
        last = *live;
    }
    drop((workbench, themes, text));
    let _ = std::fs::remove_dir_all(project);
}

/// What working holds over time: two projects opened in turn, a file opened and closed in
/// each, ten rounds. Flat after the first rounds, or something of a project (a tab) stays
/// after it's gone. Run on demand:
/// `cargo test -p app memory_over_rounds -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, run on demand"]
fn memory_over_rounds() {
    if kernel::memory::live_heap().is_none() {
        eprintln!("no counting allocator: skipped");
        return;
    }
    let (support, _) = modules::load(&extensions::ModuleSelection::default());
    let support = std::sync::Arc::new(support);
    support.manager.start();
    let mut workbench = Workbench::new(std::sync::Arc::clone(&support), workbench::Ports::local());
    let root = std::env::temp_dir().join(format!("memory-rounds-{}", std::process::id()));
    let projects: Vec<std::path::PathBuf> = ["one", "two"]
        .iter()
        .map(|name| {
            let dir = root.join(name);
            std::fs::create_dir_all(dir.join("src")).unwrap();
            std::fs::write(dir.join("index.php"), "<?php\n\necho 'hello';\n").unwrap();
            let class = "<?php\n\nnamespace App;\n\nfinal class Cart\n{\n    public function total(): int\n    {\n        return 0;\n    }\n}\n";
            std::fs::write(dir.join("src/Cart.php"), class).unwrap();
            dir
        })
        .collect();
    let mut rounds = Vec::new();
    for _ in 0..10 {
        for project in &projects {
            workbench.open_path(project).unwrap();
            workbench.open_path(&project.join("src/Cart.php")).unwrap();
            wakes(&mut workbench, Duration::from_millis(600));
            while workbench.tab_count() > 0 {
                workbench.close_tab(0);
            }
            wakes(&mut workbench, Duration::from_millis(300));
        }
        rounds.push(settled());
    }
    eprintln!();
    for (i, live) in rounds.iter().enumerate() {
        let added = *live as isize - rounds[i.saturating_sub(1)] as isize;
        eprintln!("round {:>2} {} {}", i + 1, mb(added), mb(*live as isize));
    }
    drop(workbench);
    let _ = std::fs::remove_dir_all(root);
}

/// What a real project's indexes hold, by elimination: the heap once its indexing ends, then
/// with its symbol index emptied (what the symbols held), then with another (empty) project
/// in its place (what the rest held: the text search, the file lists, the tree). The project
/// is `EDITOR_MEASURE_PROJECT` (a copy: it's indexed and its session written). Run on demand:
/// `EDITOR_MEASURE_PROJECT=/path cargo test -p app --release project_index_memory -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, run on demand"]
#[allow(clippy::disallowed_methods)] // Measured by the test, not the app.
fn project_index_memory() {
    let Some(project) = std::env::var_os("EDITOR_MEASURE_PROJECT").map(std::path::PathBuf::from)
    else {
        eprintln!("EDITOR_MEASURE_PROJECT not set: skipped");
        return;
    };
    if kernel::memory::live_heap().is_none() {
        eprintln!("no counting allocator: skipped");
        return;
    }
    let (support, _) = modules::load(&extensions::ModuleSelection::default());
    let support = std::sync::Arc::new(support);
    support.manager.start();
    let mut workbench = Workbench::new(std::sync::Arc::clone(&support), workbench::Ports::local());
    let before = settled();
    let started = std::time::Instant::now();
    workbench.open_path(&project).unwrap();
    // Indexed: busy, then idle for a while (the symbols, then the text search).
    let mut idle_since = None;
    while started.elapsed() < Duration::from_secs(600) {
        wakes(&mut workbench, Duration::from_millis(200));
        let busy = workbench.indexing();
        idle_since = if busy { None } else { idle_since.or(Some(std::time::Instant::now())) };
        if idle_since.is_some_and(|t| t.elapsed() > Duration::from_secs(5)) {
            break;
        }
    }
    let took = started.elapsed();
    let indexed = settled();
    let symbols = workbench.symbol_index().unwrap();
    let estimate = symbols.read().unwrap().memory();
    let files = symbols.read().unwrap().files().count();
    let by_folder = symbols.read().unwrap().memory_by_folder(&project, 1);
    let parts = symbol_parts(&symbols.read().unwrap());
    let tables = symbols.read().unwrap().memory_parts();
    *symbols.write().unwrap() = code_intelligence::SymbolIndex::default();
    drop(symbols);
    let without_symbols = settled();
    let empty = std::env::temp_dir().join(format!("index-memory-{}", std::process::id()));
    std::fs::create_dir_all(&empty).unwrap();
    workbench.open_path(&empty).unwrap();
    wakes(&mut workbench, Duration::from_secs(3));
    let gone = settled();
    let delta = |a: usize, b: usize| a as isize - b as isize;
    eprintln!("\nproject: {} ({files} files with symbols, indexed in {took:?})", project.display());
    eprintln!("heap before the project   {}", mb(before as isize));
    eprintln!(
        "indexed                   {} (+{})",
        mb(indexed as isize),
        mb(delta(indexed, before))
    );
    eprintln!(
        "the symbol index          {} (its estimate {})",
        mb(delta(indexed, without_symbols)),
        mb(estimate as isize)
    );
    eprintln!("the rest of the project   {}", mb(delta(without_symbols, gone)));
    let mut folders: Vec<_> = by_folder.into_iter().collect();
    folders.sort_by_key(|f| std::cmp::Reverse(f.1));
    for (folder, bytes) in folders.iter().take(8) {
        eprintln!("  symbols in {folder:<20} {}", mb(*bytes as isize));
    }
    eprintln!("the symbol index by part (estimates):");
    for (part, bytes) in tables {
        eprintln!("  {part:<34} {}", mb(bytes as isize));
    }
    eprintln!("what the symbols are made of:");
    for (part, bytes) in &parts {
        eprintln!("  {part:<34} {}", mb(*bytes as isize));
    }
    drop(workbench);
    let _ = std::fs::remove_dir_all(empty);
}

/// What the symbols' heap is made of (estimates by part, largest first): the records, their
/// texts, their lists, the files' imports; with how many there are.
fn symbol_parts(index: &code_intelligence::SymbolIndex) -> Vec<(String, u64)> {
    use std::collections::HashMap;
    let mut parts: HashMap<String, u64> = HashMap::new();
    let mut add =
        |part: &str, bytes: usize| *parts.entry(part.to_owned()).or_default() += bytes as u64;
    let (mut symbols, mut members, mut files) = (0usize, 0usize, 0usize);
    for (_, file) in index.files() {
        files += 1;
        let record = std::mem::size_of::<code_intelligence::Symbol>();
        add("records (Symbol, by capacity)", file.symbols.capacity() * record);
        add(
            "imports (UseImport)",
            file.uses.capacity() * std::mem::size_of::<code_intelligence::UseImport>(),
        );
        for s in &file.symbols {
            symbols += 1;
            members += usize::from(s.container.is_some());
            add("names", s.name.len());
            add(
                if s.container.is_some() { "fqn of members" } else { "fqn of types / functions" },
                s.fqn.len(),
            );
            add("signatures (detail)", s.detail.as_ref().map_or(0, |d| d.len()));
            add("doc types (types)", s.types.as_ref().map_or(0, |t| t.len()));
            add("notes (attributes)", s.note.as_ref().map_or(0, |n| n.len()));
            add("parents (lists)", s.parents.len() * std::mem::size_of::<std::sync::Arc<str>>());
        }
    }
    // The texts' and lists' own allocations: how many, and what the allocator rounds them up
    // to (small blocks in 16-byte steps) beyond what they ask — not in the counted heap.
    let (mut allocations, mut rounding) = (0u64, 0u64);
    let mut count = |bytes: usize| {
        if bytes > 0 {
            allocations += 1;
            rounding += (bytes.div_ceil(16) * 16 - bytes) as u64;
        }
    };
    for (_, file) in index.files() {
        count(file.symbols.capacity() * std::mem::size_of::<code_intelligence::Symbol>());
        count(file.uses.capacity() * std::mem::size_of::<code_intelligence::UseImport>());
        for s in &file.symbols {
            count(s.name.len());
            count(s.fqn.len());
            count(s.detail.as_ref().map_or(0, |d| d.len()));
            count(s.types.as_ref().map_or(0, |t| t.len()));
            count(s.note.as_ref().map_or(0, |n| n.len()));
            count(s.parents.len() * std::mem::size_of::<std::sync::Arc<str>>());
        }
    }
    let mut out: Vec<(String, u64)> = parts.into_iter().collect();
    out.sort_by_key(|p| std::cmp::Reverse(p.1));
    out.push((format!("({files} files, {symbols} symbols, {members} members)"), 0));
    out.push((format!("({allocations} allocations; rounding them up adds)"), rounding));
    out
}

/// Two projects using the same libraries (two copies of one project), in two windows of one
/// process: what the second one adds, with how many library files every index holds once
/// (plan 76b). `EDITOR_MEASURE_PROJECTS=/a:/b` (copies: they're indexed and their sessions
/// written). Run on demand, as [`project_index_memory`].
#[test]
#[ignore = "a measurement, run on demand"]
#[allow(clippy::disallowed_methods)] // Measured by the test, not the app.
fn two_projects_share_their_libraries() {
    let Some(projects) = std::env::var("EDITOR_MEASURE_PROJECTS").ok() else {
        eprintln!("EDITOR_MEASURE_PROJECTS not set: skipped");
        return;
    };
    if kernel::memory::live_heap().is_none() {
        eprintln!("no counting allocator: skipped");
        return;
    }
    let (support, _) = modules::load(&extensions::ModuleSelection::default());
    let support = std::sync::Arc::new(support);
    support.manager.start();
    let indexed = |workbench: &mut Workbench, project: &str| {
        let started = std::time::Instant::now();
        workbench.open_path(std::path::Path::new(project)).unwrap();
        let mut idle_since = None;
        while started.elapsed() < Duration::from_secs(600) {
            wakes(workbench, Duration::from_millis(200));
            let busy = workbench.indexing();
            idle_since = if busy { None } else { idle_since.or(Some(std::time::Instant::now())) };
            if idle_since.is_some_and(|t| t.elapsed() > Duration::from_secs(5)) {
                break;
            }
        }
    };
    let before = settled();
    let mut windows = Vec::new();
    let mut last = before;
    for (i, project) in projects.split(':').enumerate() {
        let mut workbench =
            Workbench::new(std::sync::Arc::clone(&support), workbench::Ports::local());
        indexed(&mut workbench, project);
        windows.push(workbench);
        let now = settled();
        eprintln!(
            "project {} indexed: heap {} (+{}), library files held once: {}",
            i + 1,
            mb(now as isize),
            mb(now as isize - last as isize),
            code_intelligence::library_files_held()
        );
        last = now;
    }
    // The last one's symbol index emptied (the libraries stay, the others hold them): what
    // its own part of it held.
    if std::env::var_os("EDITOR_MEASURE_SYMBOLS").is_some()
        && let Some(index) = windows.last().and_then(Workbench::symbol_index)
    {
        let parts = index.read().unwrap().memory_parts();
        *index.write().unwrap() = code_intelligence::SymbolIndex::default();
        drop(index);
        let now = settled();
        eprintln!("its symbol index emptied: {}", mb(now as isize - last as isize));
        for (part, bytes) in parts {
            eprintln!("  {part:<20} {} (estimate, the shared counted)", mb(bytes as isize));
        }
        return;
    }
    // `EDITOR_MEASURE_REST=1`: the last one brought forward, the others rest (after the
    // module's wait): what their indexes held, apart from the rest of their projects.
    if std::env::var_os("EDITOR_MEASURE_REST").is_some()
        && let Some(front) = windows.last()
    {
        front.project_brought_forward();
        for workbench in &mut windows {
            wakes(workbench, Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_secs(130));
        for workbench in &mut windows {
            wakes(workbench, Duration::from_millis(500));
        }
        let now = settled();
        eprintln!(
            "the others' indexes rested: heap {} ({}), library files held once: {}",
            mb(now as isize),
            mb(now as isize - last as isize),
            code_intelligence::library_files_held()
        );
    }
    drop(windows);
}
