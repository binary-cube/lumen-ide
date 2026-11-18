use super::*;
use crate::fixture::{TempDir, language_of, register_reads, wait_until};
use std::sync::atomic::{AtomicUsize, Ordering};

/// The diagnostics said, the last per file.
type Said = Arc<Mutex<HashMap<PathBuf, Vec<Diagnostic>>>>;

/// The diagnostics said on `events`.
fn listen(events: &EventBus) -> (Said, Subscription) {
    let said: Said = Arc::default();
    let into = Arc::clone(&said);
    let subscription = events.subscribe(move |e: &DiagnosticsFound| {
        if let DiagnosticsScope::File(file) = &e.scope {
            lock(&into).insert(file.clone(), e.diagnostics.clone());
        }
    });
    (said, subscription)
}

fn attach(dir: &TempDir, events: &EventBus, woken: &Arc<AtomicUsize>) -> Vec<Subscription> {
    let woken = Arc::clone(woken);
    let storage = dir.path().join(".storage");
    let context = ProjectContext {
        marks: Default::default(),
        id: kernel::ProjectId::new(),
        root: dir.path(),
        storage: &storage,
        events,
        wake: Arc::new(move || {
            woken.fetch_add(1, Ordering::Relaxed);
        }),
        settings: None,
        language_of: language_of(),
        quality_tools: Vec::new(),
        text_formatters: Vec::new(),
        notifier: Arc::new(workbench_model::NoNotifier),
    };
    EnvService::default().attach(&context)
}

#[test]
fn a_variable_no_env_file_defines_is_flagged_as_its_document_is_typed() {
    register_reads();
    let dir = TempDir::new("service");
    let env = dir.write(".env", "A=1\n");
    dir.write(".env.example", "DOCUMENTED=\n");
    let events = EventBus::new();
    let woken = Arc::new(AtomicUsize::new(0));
    let (said, _listening) = listen(&events);
    let _attached = attach(&dir, &events, &woken);
    let path = dir.path().join("src/a.php");
    let text = "<?php getenv('A'); getenv('B'); getenv('DOCUMENTED');\n";
    let document = document::Document::new(text);
    events.publish(&DocumentOpened {
        document: kernel::DocumentId::next(),
        path,
        language: "php".into(),
        version: 1,
        text: document.buffer().snapshot(),
    });
    let relative = PathBuf::from("src/a.php");
    wait_until(|| lock(&said).get(&relative).is_some_and(|d| !d.is_empty()));
    let found = lock(&said)[&relative].clone();
    assert_eq!(found.len(), 1);
    let column = text.find("B'").unwrap() as u32;
    assert_eq!(
        (found[0].line, found[0].column, found[0].severity),
        (0, Some(column), Severity::Warning)
    );
    assert_eq!(found[0].end, Some((0, column + 1)));
    assert!(woken.load(Ordering::Relaxed) > 0);
    // `B` defined: cleared once the file is read again.
    std::fs::write(&env, "A=1\nB=2\n").unwrap();
    events.publish(&FileSaved { path: env });
    wait_until(|| lock(&said).get(&relative).is_some_and(Vec::is_empty));
}

#[test]
fn a_project_without_env_files_flags_nothing() {
    register_reads();
    let dir = TempDir::new("service-none");
    let events = EventBus::new();
    let (said, _listening) = listen(&events);
    let attached = attach(&dir, &events, &Arc::new(AtomicUsize::new(0)));
    let document = document::Document::new("<?php getenv('B');");
    events.publish(&DocumentOpened {
        document: kernel::DocumentId::next(),
        path: dir.path().join("a.php"),
        language: "php".into(),
        version: 1,
        text: document.buffer().snapshot(),
    });
    // Its checks ran (the catalog read): nothing said.
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(lock(&said).values().all(Vec::is_empty));
    drop(attached);
}

#[test]
fn keys_no_code_reads_are_noted_while_the_setting_is_on() {
    register_reads();
    let dir = TempDir::new("service-unused");
    dir.write(".env", "READ=1\nUNREAD=2\nURL=http://$READ\n");
    dir.write("src/a.php", "<?php getenv('READ'); getenv('URL');");
    let events = EventBus::new();
    let (said, _listening) = listen(&events);
    let _attached = attach(&dir, &events, &Arc::new(AtomicUsize::new(0)));
    settings::set_unused_keys(true);
    let env = PathBuf::from(".env");
    wait_until(|| lock(&said).get(&env).is_some_and(|d| !d.is_empty()));
    let found = lock(&said)[&env].clone();
    assert_eq!(
        found.iter().map(|d| (d.line, d.severity)).collect::<Vec<_>>(),
        [(1, Severity::Info)]
    );
    // Off: cleared.
    settings::set_unused_keys(false);
    wait_until(|| lock(&said).get(&env).is_some_and(Vec::is_empty));
}
