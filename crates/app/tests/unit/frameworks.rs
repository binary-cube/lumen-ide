use super::*;
use std::sync::atomic::AtomicUsize;

#[derive(Debug, Default)]
struct NoChoices;

impl ProjectFrameworksStore for NoChoices {
    fn load(&self, _: &Path) -> Vec<(String, bool)> {
        Vec::new()
    }
    fn save(&self, _: &Path, _: &[(String, bool)]) -> std::io::Result<()> {
        Ok(())
    }
}

/// Watches `src/**/*.php`; counts its models made (by generation); says "nope" where a
/// document holds `nope`.
#[derive(Debug, Default)]
struct Watching {
    prepared: Mutex<Vec<u64>>,
    calls: AtomicUsize,
}

impl FrameworkSupport for Watching {
    fn watches(&self) -> &'static [&'static str] {
        &["src/**/*.php"]
    }
    fn prepare(&self, at: &FrameworkContext<'_>) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.prepared.lock().unwrap().push(at.own);
    }
    fn diagnostics(&self, at: &FrameworkContext<'_>) -> Vec<code_intelligence::FrameworkProblem> {
        at.text
            .match_indices("nope")
            .map(|(i, _)| code_intelligence::FrameworkProblem {
                range: i..i + 4,
                level: code_intelligence::ProblemLevel::Warning,
                message: "nope".into(),
            })
            .collect()
    }
}

#[derive(Debug)]
struct Fw(Arc<Watching>);

impl Framework for Fw {
    fn id(&self) -> &'static str {
        "fw"
    }
    fn name(&self) -> &'static str {
        "Fw"
    }
    fn detect(&self, _: &Project<'_>) -> Option<String> {
        Some("test".into())
    }
    fn activate(&self, _: &Project<'_>) -> Arc<dyn FrameworkSupport> {
        Arc::clone(&self.0) as Arc<dyn FrameworkSupport>
    }
}

fn wait(what: &str, done: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "waited for {what}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Opened: its model made in the background; only a change to a file it watches counts
/// (its own generation) and makes it again; a document checked: its problems announced.
#[test]
fn a_framework_follows_only_its_files_and_checks_documents_in_the_background() {
    let root = PathBuf::from("/project");
    let support = Arc::new(Watching::default());
    let frameworks: Vec<Arc<dyn Framework>> = vec![Arc::new(Fw(Arc::clone(&support)))];
    let mut project = ProjectFrameworks::load(&NoChoices, &root, &frameworks, &Project::none());
    let events = kernel::EventBus::new();
    project.follow(
        &events,
        Background {
            root: root.clone(),
            levels: Vec::new(),
            languages: Vec::new(),
            generations: Arc::new(|| (0, 0)),
            index: Arc::default(),
        },
    );
    wait("its first model", || support.calls.load(Ordering::Relaxed) == 1);
    let written = |path: &str| workspace::events::FilesWritten { paths: vec![root.join(path)] };
    events.publish(&written("public/app.css"));
    assert_eq!(project.active()[0].own(), 0, "not its file");
    events.publish(&written("src/Controller/Blog.php"));
    assert_eq!(project.active()[0].own(), 1);
    wait("its model made again", || support.prepared.lock().unwrap().last() == Some(&1));

    let found = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&found);
    let _subscription = events.subscribe(move |e: &workbench_model::DiagnosticsFound| {
        heard.lock().unwrap().push(e.clone());
    });
    let doc = Document {
        path: root.join("src/A.php"),
        language: "php".into(),
        text: "<?php\n// nope\n".into(),
    };
    project.check(doc.clone(), &events);
    wait("the document checked", || !found.lock().unwrap().is_empty());
    {
        let found = found.lock().unwrap();
        assert_eq!(found[0].source, "framework-fw");
        assert_eq!(
            found[0].scope,
            workbench_model::DiagnosticsScope::File(PathBuf::from("src/A.php"))
        );
        let d = &found[0].diagnostics[0];
        assert_eq!((d.line, d.column, d.end), (1, Some(3), Some((1, 7))));
        assert_eq!(d.source, "Fw");
    }
    // The same problems again (a keystroke elsewhere): nothing told, no marks rebuilt.
    let edited = Document {
        text: "<?php
// nope
$a = 1;
"
        .into(),
        ..doc.clone()
    };
    project.check(edited, &events);
    let cleaned = Document {
        text: "<?php
"
        .into(),
        ..doc
    };
    project.check(cleaned, &events);
    wait("the clean document told", || found.lock().unwrap().len() == 2);
    let found = found.lock().unwrap();
    assert!(found[1].diagnostics.is_empty(), "its problems gone: told once, as none");
}
