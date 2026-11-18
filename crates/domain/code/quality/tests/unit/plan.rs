use super::*;

/// A formatter known by a module: `vendor/bin/pint`, else `pint` in PATH.
#[derive(Debug)]
struct Pint;

impl QualityTool for Pint {
    fn id(&self) -> &'static str {
        "pint"
    }
    fn name(&self) -> &'static str {
        "Pint"
    }
    fn kind(&self) -> Kind {
        Kind::Formatter
    }
    fn languages(&self) -> &'static [&'static str] {
        &["php"]
    }
    fn candidates(&self) -> &'static [&'static str] {
        &["vendor/bin/pint", "pint"]
    }
    fn args(&self, target: &Target, _root: &Path) -> Vec<String> {
        match target {
            Target::File(file) => vec![file.to_string_lossy().into_owned()],
            Target::Project => Vec::new(),
        }
    }
}

fn tools() -> Vec<Arc<dyn QualityTool>> {
    vec![Arc::new(Pint)]
}

fn file() -> Target {
    Target::File("src/A.php".into())
}

/// A known tool: found in the project first, else in PATH; not found: said by name.
#[test]
fn a_known_tool_is_found_in_the_project_then_in_path() {
    let root = Path::new("/p");
    let in_project = |p: &Path| p == Path::new("/p/vendor/bin/pint");
    let nowhere = |_: &Path| false;
    let path_has = |name: &str| (name == "pint").then(|| PathBuf::from("/usr/bin/pint"));
    let no_path = |_: &str| None;
    let choice = Choice::Tool("pint".into());
    let run = plan(
        &choice,
        Kind::Formatter,
        &tools(),
        &file(),
        root,
        &Lookup { exists: &in_project, find: &no_path, node: &|| None },
    )
    .unwrap();
    assert_eq!(run.program, Program::Exec("/p/vendor/bin/pint".into()));
    assert_eq!(run.args, ["src/A.php"]);
    assert_eq!(run.reads_as.as_deref(), Some("pint"));
    let run = plan(
        &choice,
        Kind::Formatter,
        &tools(),
        &file(),
        root,
        &Lookup { exists: &nowhere, find: &path_has, node: &|| None },
    )
    .unwrap();
    assert_eq!(run.program, Program::Exec("/usr/bin/pint".into()));
    let missing = plan(
        &choice,
        Kind::Formatter,
        &tools(),
        &file(),
        root,
        &Lookup { exists: &nowhere, find: &no_path, node: &|| None },
    );
    assert_eq!(missing, Err(PlanError::NotFound("Pint".into())));
    let other = plan(
        &Choice::Tool("gone".into()),
        Kind::Formatter,
        &tools(),
        &file(),
        root,
        &Lookup { exists: &nowhere, find: &no_path, node: &|| None },
    );
    assert_eq!(other, Err(PlanError::UnknownTool("gone".into())));
    assert_eq!(
        plan(
            &Choice::None,
            Kind::Formatter,
            &tools(),
            &file(),
            root,
            &Lookup { exists: &nowhere, find: &no_path, node: &|| None }
        ),
        Err(PlanError::Nothing)
    );
}

/// The project's command: with `{file}`, for the file (given in a variable); without, for the
/// whole project — as `./scripts/linux/shell.sh composer cs:fix`.
#[test]
fn a_command_runs_on_the_file_or_the_whole_project() {
    let root = Path::new("/p");
    let lookup = Lookup { exists: &|_| false, find: &|_| None, node: &|| None };
    let script =
        Choice::Command { line: "./scripts/linux/shell.sh composer cs:fix".into(), reads_as: None };
    let run = plan(&script, Kind::Formatter, &tools(), &file(), root, &lookup).unwrap();
    assert_eq!(run.program, Program::Shell("./scripts/linux/shell.sh composer cs:fix".into()));
    assert_eq!(run.target, Target::Project, "no {{file}}: the whole project");
    let per_file = Choice::Command { line: "phpcs --report=emacs {file}".into(), reads_as: None };
    let spaced = Target::File("src/My File.php".into());
    let run = plan(&per_file, Kind::Checker, &tools(), &spaced, root, &lookup).unwrap();
    let variable = if cfg!(windows) { "\"%EDITOR_FILE%\"" } else { "\"$EDITOR_FILE\"" };
    assert_eq!(run.program, Program::Shell(format!("phpcs --report=emacs {variable}")));
    assert_eq!(run.target, spaced);
}

/// A binary: relative to the project; its arguments the format's tool's, else the file.
#[test]
fn a_binary_with_or_without_a_known_format() {
    let root = Path::new("/p");
    let lookup = Lookup { exists: &|_| false, find: &|_| None, node: &|| None };
    let plain = Choice::Binary { path: "tools/fix".into(), reads_as: None };
    let run = plan(&plain, Kind::Formatter, &tools(), &file(), root, &lookup).unwrap();
    assert_eq!(run.program, Program::Exec("/p/tools/fix".into()));
    assert_eq!(run.args, ["src/A.php"]);
    let as_pint = Choice::Binary { path: "/opt/pint".into(), reads_as: Some("pint".into()) };
    let run = plan(&as_pint, Kind::Formatter, &tools(), &Target::Project, root, &lookup).unwrap();
    assert_eq!(run.program, Program::Exec("/opt/pint".into()));
    assert!(run.args.is_empty(), "pint's arguments for the project");
}

/// A checker that's a Node script in the project (ESLint's), reading the file on stdin.
#[derive(Debug)]
struct Lint;

impl QualityTool for Lint {
    fn id(&self) -> &'static str {
        "lint"
    }
    fn name(&self) -> &'static str {
        "Lint"
    }
    fn kind(&self) -> Kind {
        Kind::Checker
    }
    fn languages(&self) -> &'static [&'static str] {
        &["javascript"]
    }
    fn candidates(&self) -> &'static [&'static str] {
        &["node_modules/.bin/lint", "lint"]
    }
    fn scripts(&self) -> &'static [&'static str] {
        &["node_modules/lint/bin/lint.js"]
    }
    fn stdin(&self) -> bool {
        true
    }
    fn args(&self, target: &Target, _root: &Path) -> Vec<String> {
        match target {
            Target::File(file) => vec!["--stdin".into(), file.to_string_lossy().into_owned()],
            Target::Project => vec![".".into()],
        }
    }
}

/// The project's script is run by a Node (the system's, else the app's), the file on its
/// stdin; without a Node, the `.bin` shim; without the package, `PATH`.
#[test]
fn a_node_script_of_the_project_runs_on_a_node() {
    let root = Path::new("/p");
    let tools: Vec<Arc<dyn QualityTool>> = vec![Arc::new(Lint)];
    let choice = Choice::Tool("lint".into());
    let target = Target::File("src/a.js".into());
    let all = |_: &Path| true;
    let node = || Some(PathBuf::from("/app/node"));
    let lookup = Lookup { exists: &all, find: &|_| None, node: &node };
    let run = plan(&choice, Kind::Checker, &tools, &target, root, &lookup).unwrap();
    assert_eq!(run.program, Program::Exec("/app/node".into()));
    let script = root.join("node_modules/lint/bin/lint.js").display().to_string();
    assert_eq!(run.args, [script.as_str(), "--stdin", "src/a.js"]);
    assert!(run.stdin);
    // The whole project: no stdin.
    let run = plan(&choice, Kind::Checker, &tools, &Target::Project, root, &lookup).unwrap();
    assert!(!run.stdin);
    // No Node anywhere: the shim.
    let lookup = Lookup { exists: &all, find: &|_| None, node: &|| None };
    let run = plan(&choice, Kind::Checker, &tools, &target, root, &lookup).unwrap();
    assert_eq!(run.program, Program::Exec("/p/node_modules/.bin/lint".into()));
    // Not in the project: PATH's.
    let path = |name: &str| (name == "lint").then(|| PathBuf::from("/usr/bin/lint"));
    let lookup = Lookup { exists: &|_| false, find: &path, node: &node };
    let run = plan(&choice, Kind::Checker, &tools, &target, root, &lookup).unwrap();
    assert_eq!(run.program, Program::Exec("/usr/bin/lint".into()));
}
