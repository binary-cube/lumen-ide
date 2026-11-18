use super::*;

/// One port for every question about the project: without the files index yet, the
/// project is walked once; paths, filtered files and declarations answer through it.
#[test]
fn one_port_for_paths_files_and_declarations() {
    // `vendor` a dependency folder (the modules register it; the registry is the
    // process's, and a test runs in a process of its own).
    kernel::paths::register(kernel::ProjectDirs {
        dependencies: &["vendor"],
        ..Default::default()
    });
    let dir = std::env::temp_dir().join(format!("queries-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("vendor/x")).unwrap();
    std::fs::write(dir.join("src/Mailer.php"), "").unwrap();
    std::fs::write(dir.join("vendor/x/Lib.php"), "").unwrap();
    let mut index = SymbolIndex::default();
    let mut file = code_intelligence::FileSymbols { language: "php".into(), ..Default::default() };
    file.symbols.push(code_intelligence::Symbol {
        name: "Mailer".into(),
        kind: code_intelligence::SymbolKind::Class,
        fqn: "App\\\\Mailer".into(),
        container: None,
        parents: Box::default(),
        range: 0..0,
        offset: 0,
        line: 0,
        detail: None,
        is_static: false,
        visibility: Default::default(),
        note: None,
        types: None,
    });
    index.set_file(&dir.join("src/Mailer.php"), file);
    let queries = QueriesSnapshot {
        marks: Default::default(),
        root: dir.clone(),
        disk: Arc::new(fs_local::LocalFileSystem),
        files: None,
        folders: None,
        ignored: None,
        symbols: Arc::new(RwLock::new(index)),
        text: None,
        walked: OnceLock::new(),
        language_of: Arc::new(|_: &std::path::Path| "php".to_owned()),
        generation: 7,
        list_dir: Arc::new(|_: &std::path::Path| Ok(Vec::new())),
        file_icon: Arc::new(|_: &str| ("file", String::new())),
        folder_color: Arc::new(|_: &str| None),
        intelligence: Vec::new(),
    };
    let found = queries.paths(&PathQuery { text: "mail", folders: 2, files: 2 });
    assert_eq!(found.files.first().map(|f| f.path.as_str()), Some("src/Mailer.php"));
    assert_eq!(queries.files(&FileFilter::code(&["php"])), [dir.join("src/Mailer.php")]);
    let hits = queries.symbols(&SymbolQuery { text: "mlr", limit: 5, ..Default::default() });
    assert_eq!(hits.first().map(|h| h.name.as_str()), Some("Mailer"));
    assert!(queries.text(&SearchQuery::new("mail", 5)).is_empty(), "no text index: nothing");
    let _ = std::fs::remove_dir_all(&dir);
}
