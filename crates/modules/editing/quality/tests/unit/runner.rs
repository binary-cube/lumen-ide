#![cfg(unix)]

use std::time::{Duration, Instant};

use code_quality::{Choice, Severity};

use super::*;
use kernel::EventBus;

/// A project in a temp folder, its settings in memory-like TOML there, its runs.
fn project(name: &str) -> (PathBuf, Arc<ProjectRuns>, EventBus) {
    let root = std::env::temp_dir().join(format!("quality-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/a.php"), "<?php\n$x = 1;\n").unwrap();
    let store = settings_test::Memory::default();
    let settings = Arc::new(SettingsService::new(Arc::new(store)));
    let events = EventBus::default();
    let context = workbench_model::ProjectContext {
        marks: Default::default(),
        id: kernel::ProjectId::new(),
        root: &root,
        storage: &root,
        events: &events,
        wake: Arc::new(|| {}),
        settings: Some(settings),
        language_of: Arc::new(|p: &Path| {
            (p.extension().is_some_and(|e| e == "php")).then(|| "php".to_owned())
        }),
        quality_tools: Vec::new(),
        text_formatters: Vec::new(),
        notifier: std::sync::Arc::new(workbench_model::NoNotifier),
    };
    let runs = Arc::new(ProjectRuns::new(&context));
    (root, runs, events)
}

fn wait_done(runs: &ProjectRuns) -> Vec<Done> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let done = runs.take_done();
        if !done.is_empty() {
            return done;
        }
        assert!(Instant::now() < deadline, "the run didn't end");
        std::thread::sleep(Duration::from_millis(20));
    }
}

use crate::settings::tests as settings_test;

/// What the project's settings choose doesn't run until the user allows it (they may have
/// come with the project); a file's name is given in a variable, never run.
#[test]
fn a_command_runs_once_allowed_and_names_its_file_safely() {
    let (root, runs, _events) = project("trust");
    let settings = runs.settings.clone().unwrap();
    let marker = root.join("ran");
    let format = Choice::Command { line: "printf '<?php\\n' > {file}".into(), reads_as: None };
    crate::settings::save_choice(&settings, "php", Kind::Formatter, &format).unwrap();
    let target = Target::File("src/a.php".into());
    assert!(matches!(
        runs.request(Kind::Formatter, target, "php", false),
        Err(PlanError::NotAllowed(_))
    ));
    runs.allow(&format);
    // A name that would run code if the shell read it.
    // (It runs in the project's folder: `ran` would appear there.)
    let tricky = "src/$(touch ran).php".to_owned();
    std::fs::write(root.join(&tricky), "x").unwrap();
    runs.request(Kind::Formatter, Target::File(tricky.clone().into()), "php", false).unwrap();
    let done = wait_done(&runs);
    assert!(done[0].outcome.is_ok(), "{done:?}");
    assert!(!marker.exists(), "the name wasn't run");
    assert_eq!(std::fs::read_to_string(root.join(&tricky)).unwrap(), "<?php\n");
    let _ = std::fs::remove_dir_all(&root);
}

/// The project's checker script: its lines read as problems, the paths relative to the
/// project; a formatter script changes the file; a script that fails says why.
#[test]
fn a_project_s_scripts_check_and_format() {
    let (root, runs, _events) = project("scripts");
    let settings = runs.settings.clone().unwrap();
    let check = Choice::Command {
        line: format!(
            "printf '{}/src/a.php:2:1: Missing docblock\\nsrc/a.php:1: warning: old\\n'",
            root.display()
        ),
        reads_as: None,
    };
    crate::settings::save_choice(&settings, "php", Kind::Checker, &check).unwrap();
    runs.allow(&check);
    runs.request(Kind::Checker, Target::File("src/a.php".into()), "php", false).unwrap();
    let done = wait_done(&runs);
    let problems = done[0].outcome.as_ref().unwrap();
    assert_eq!(problems.len(), 2);
    assert_eq!(problems[0].path, PathBuf::from("src/a.php"), "made relative to the project");
    assert_eq!((problems[0].line, problems[1].severity), (1, Severity::Warning));
    assert_eq!(done[0].target, Target::Project, "a command without {{file}}: the project");
    let format = Choice::Command { line: "printf '<?php\\n' > {file}".into(), reads_as: None };
    crate::settings::save_choice(&settings, "php", Kind::Formatter, &format).unwrap();
    runs.allow(&format);
    runs.request(Kind::Formatter, Target::File("src/a.php".into()), "php", false).unwrap();
    assert!(wait_done(&runs)[0].outcome.is_ok());
    assert_eq!(std::fs::read_to_string(root.join("src/a.php")).unwrap(), "<?php\n");
    let broken =
        Choice::Command { line: "echo 'config missing' >&2; exit 2".into(), reads_as: None };
    crate::settings::save_choice(&settings, "php", Kind::Formatter, &broken).unwrap();
    runs.allow(&broken);
    runs.request(Kind::Formatter, Target::Project, "php", false).unwrap();
    assert_eq!(wait_done(&runs)[0].outcome, Err("config missing".into()));
    let _ = std::fs::remove_dir_all(&root);
}

/// On save: the formatter, then (as asked) the checker; nothing chosen: nothing runs.
#[test]
fn on_save_the_formatter_then_the_checker() {
    let (root, runs, _events) = project("save");
    let settings = runs.settings.clone().unwrap();
    runs.saved(&root.join("src/a.php"));
    std::thread::sleep(Duration::from_millis(100));
    assert!(runs.take_done().is_empty(), "nothing chosen");
    let ok = Choice::Command { line: "true".into(), reads_as: None };
    crate::settings::save_choice(&settings, "php", Kind::Formatter, &ok).unwrap();
    runs.allow(&ok);
    crate::settings::save_choice(&settings, "php", Kind::Checker, &ok).unwrap();
    runs.allow(&ok);
    crate::settings::save_on_save(&settings, "php", Kind::Formatter, true).unwrap();
    crate::settings::save_on_save(&settings, "php", Kind::Checker, true).unwrap();
    runs.saved(&root.join("src/a.php"));
    let done = wait_done(&runs);
    assert_eq!(done[0].kind, Kind::Formatter);
    assert!(done[0].then_check, "the checker comes after it (from the module's poll)");
    let _ = std::fs::remove_dir_all(&root);
}

/// Saves heard during a batch of changes (a refactoring, a paste) wait for its end, then
/// run once per file — planned and run on the pool, never more than [`AT_ONCE`] at a time.
#[test]
fn saves_in_a_batch_run_once_each_after_it() {
    let (root, runs, _events) = project("batch");
    let settings = runs.settings.clone().unwrap();
    // Each run notes itself, waits a little, then notes it ended: overlaps are counted.
    let log = root.join("log");
    let check = Choice::Command {
        line: format!("echo start >> '{0}'; sleep 0.2; echo end >> '{0}'", log.display()),
        reads_as: None,
    };
    crate::settings::save_choice(&settings, "php", Kind::Checker, &check).unwrap();
    runs.allow(&check);
    crate::settings::save_on_save(&settings, "php", Kind::Checker, true).unwrap();
    for i in 0..4 {
        std::fs::write(root.join(format!("src/f{i}.php")), "<?php\n").unwrap();
    }
    runs.hold(7);
    for i in 0..4 {
        runs.heard_save(&root.join(format!("src/f{i}.php")));
        runs.heard_save(&root.join(format!("src/f{i}.php")));
    }
    std::thread::sleep(Duration::from_millis(150));
    assert!(runs.take_done().is_empty() && !log.exists(), "held until the batch ends");
    runs.release(7);
    // The command has no {file}: one target (the project); the saves asked for it while it
    // ran merge into one more run after it.
    assert!(!wait_done(&runs).is_empty());
    std::thread::sleep(Duration::from_millis(800));
    let lines = std::fs::read_to_string(&log).unwrap();
    let mut running = 0i32;
    let mut most = 0;
    for line in lines.lines() {
        running += if line == "start" { 1 } else { -1 };
        most = most.max(running);
    }
    assert!(most as usize <= AT_ONCE, "{lines}");
    assert!(lines.lines().filter(|l| *l == "start").count() <= 2, "merged: {lines}");
    let _ = std::fs::remove_dir_all(&root);
}

/// A formatter a language built in.
#[derive(Debug)]
struct Own;

impl code_quality::TextFormatter for Own {
    fn languages(&self) -> &'static [&'static str] {
        &["sql"]
    }
    fn format(&self, _: &code_quality::FormatRequest) -> code_quality::Formatted {
        code_quality::Formatted::Nothing
    }
}

/// A language's own formatter is the one used while the project chose none for it; a tool
/// chosen comes first.
#[test]
fn a_built_in_formatter_serves_when_no_tool_is_chosen() {
    let root = std::env::temp_dir();
    let settings = Arc::new(SettingsService::new(Arc::new(settings_test::Memory::default())));
    let events = EventBus::default();
    let context = workbench_model::ProjectContext {
        marks: Default::default(),
        id: kernel::ProjectId::new(),
        root: &root,
        storage: &root,
        events: &events,
        wake: Arc::new(|| {}),
        settings: Some(Arc::clone(&settings)),
        language_of: Arc::new(|_: &Path| Some("sql".to_owned())),
        quality_tools: Vec::new(),
        text_formatters: vec![Arc::new(Own)],
        notifier: std::sync::Arc::new(workbench_model::NoNotifier),
    };
    let runs = ProjectRuns::new(&context);
    assert!(runs.builtin("sql").is_some());
    assert!(runs.builtin("php").is_none(), "none for another language");
    let chosen = Choice::Command { line: "sqlfluff fix {file}".into(), reads_as: None };
    crate::settings::save_choice(&settings, "sql", Kind::Formatter, &chosen).unwrap();
    assert!(runs.builtin("sql").is_none(), "the project's tool runs instead");
}

/// A checker and a formatter that are Node packages of the project (as ESLint and Prettier
/// are): their script in `node_modules`, the file on stdin.
#[derive(Debug)]
struct NodeTool(Kind);

impl QualityTool for NodeTool {
    fn id(&self) -> &'static str {
        match self.0 {
            Kind::Checker => "lint",
            Kind::Formatter => "pretty",
        }
    }
    fn name(&self) -> &'static str {
        self.id()
    }
    fn kind(&self) -> Kind {
        self.0
    }
    fn languages(&self) -> &'static [&'static str] {
        &["javascript"]
    }
    fn candidates(&self) -> &'static [&'static str] {
        match self.0 {
            Kind::Checker => &["node_modules/.bin/lint"],
            Kind::Formatter => &["node_modules/.bin/pretty"],
        }
    }
    fn scripts(&self) -> &'static [&'static str] {
        match self.0 {
            Kind::Checker => &["node_modules/lint/bin/lint.js"],
            Kind::Formatter => &["node_modules/pretty/bin/pretty.cjs"],
        }
    }
    fn stdin(&self) -> bool {
        true
    }
    fn args(&self, target: &Target, _root: &Path) -> Vec<String> {
        match target {
            Target::File(file) => vec!["--stdin-filename".into(), file.display().to_string()],
            Target::Project => vec![".".into()],
        }
    }
}

/// The project's Node packages run on a Node (a stand-in here: a shell running the
/// "script"), the saved file on their stdin: the checker's problems read from what it
/// printed, the formatter's output written back to the file.
#[test]
fn a_projects_node_packages_check_and_format_through_stdin() {
    let root = std::env::temp_dir().join(format!("quality-node-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("node_modules/lint/bin")).unwrap();
    std::fs::create_dir_all(root.join("node_modules/pretty/bin")).unwrap();
    std::fs::create_dir_all(root.join("tools")).unwrap();
    std::fs::write(root.join("src/a.js"), "let a = 1;\ndebugger;\n").unwrap();
    // The checker: a problem for each `debugger` it reads, named after the file it's told.
    std::fs::write(
        root.join("node_modules/lint/bin/lint.js"),
        "file=\"$2\"\ngrep -n debugger | while IFS=: read -r line rest; do echo \"$file:$line:1: error Unexpected debugger\"; done\n",
    )
    .unwrap();
    // The formatter: what it reads, upper-cased.
    std::fs::write(root.join("node_modules/pretty/bin/pretty.cjs"), "tr a-z A-Z\n").unwrap();
    let node = root.join("tools/node");
    std::fs::write(&node, "#!/bin/sh\nexec /bin/sh \"$@\"\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o755)).unwrap();
    let store = settings_test::Memory::default();
    let settings = Arc::new(SettingsService::new(Arc::new(store)));
    let events = EventBus::default();
    let context = workbench_model::ProjectContext {
        marks: Default::default(),
        id: kernel::ProjectId::new(),
        root: &root,
        storage: &root,
        events: &events,
        wake: Arc::new(|| {}),
        settings: Some(Arc::clone(&settings)),
        language_of: Arc::new(|_: &Path| Some("javascript".to_owned())),
        quality_tools: vec![Arc::new(NodeTool(Kind::Checker)), Arc::new(NodeTool(Kind::Formatter))],
        text_formatters: Vec::new(),
        notifier: std::sync::Arc::new(workbench_model::NoNotifier),
    };
    let mut runs = ProjectRuns::new(&context);
    let stand_in = node.clone();
    runs.use_node(Arc::new(move || Some(stand_in.clone())));
    let runs = Arc::new(runs);
    for (kind, id) in [(Kind::Checker, "lint"), (Kind::Formatter, "pretty")] {
        let choice = Choice::Tool(id.into());
        crate::settings::save_choice(&settings, "javascript", kind, &choice).unwrap();
        runs.allow(&choice);
    }
    let file = Target::File("src/a.js".into());
    let planned = runs.preview(Kind::Checker, "javascript").unwrap();
    assert_eq!(planned.program, code_quality::Program::Exec(node));
    assert!(planned.stdin);
    runs.request(Kind::Checker, file.clone(), "javascript", false).unwrap();
    let done = wait_done(&runs);
    let problems = done[0].outcome.as_ref().unwrap();
    assert_eq!(problems.len(), 1, "{done:?}");
    assert_eq!((problems[0].path.as_path(), problems[0].line), (Path::new("src/a.js"), 1));
    runs.request(Kind::Formatter, file, "javascript", false).unwrap();
    let done = wait_done(&runs);
    assert!(done[0].outcome.is_ok(), "{done:?}");
    assert_eq!(std::fs::read_to_string(root.join("src/a.js")).unwrap(), "LET A = 1;\nDEBUGGER;\n");
    let _ = std::fs::remove_dir_all(&root);
}
