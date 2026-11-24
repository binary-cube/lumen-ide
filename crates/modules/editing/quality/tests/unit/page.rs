use std::path::{Path, PathBuf};

use code_quality::{Choice, Target};
use kernel::EventBus;

use super::*;
use crate::settings::tests::Memory;

/// A tool of the tests: a formatter or a checker for PHP, found at `vendor/bin/<id>`.
#[derive(Debug)]
struct Tool(&'static str, Kind);

impl QualityTool for Tool {
    fn id(&self) -> &'static str {
        self.0
    }
    fn name(&self) -> &'static str {
        self.0
    }
    fn kind(&self) -> Kind {
        self.1
    }
    fn languages(&self) -> &'static [&'static str] {
        &["php"]
    }
    fn candidates(&self) -> &'static [&'static str] {
        match self.0 {
            "fmt" => &["vendor/bin/fmt"],
            _ => &["vendor/bin/lint"],
        }
    }
    fn args(&self, target: &Target, _root: &Path) -> Vec<String> {
        match target {
            Target::File(file) => vec!["--quiet".into(), file.to_string_lossy().into_owned()],
            Target::Project => Vec::new(),
        }
    }
}

/// A project in a temp folder whose `vendor/bin/fmt` exists, its runs known to the module.
fn project(name: &str) -> (PathBuf, Arc<ProjectRuns>, Arc<SettingsService>) {
    let root = std::env::temp_dir().join(format!("quality-page-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("vendor/bin")).unwrap();
    std::fs::write(root.join("vendor/bin/fmt"), "").unwrap();
    let settings = Arc::new(SettingsService::new(Arc::new(Memory::default())));
    let events = EventBus::default();
    let tools: Vec<Arc<dyn QualityTool>> =
        vec![Arc::new(Tool("fmt", Kind::Formatter)), Arc::new(Tool("lint", Kind::Checker))];
    let context = workbench_model::ProjectContext {
        marks: Default::default(),
        id: kernel::ProjectId::new(),
        root: &root,
        storage: &root,
        events: &events,
        wake: Arc::new(|| {}),
        settings: Some(Arc::clone(&settings)),
        language_of: Arc::new(|_: &Path| Some("php".to_owned())),
        quality_tools: tools,
        text_formatters: Vec::new(),
        notifier: std::sync::Arc::new(workbench_model::NoNotifier),
    };
    let runs = Arc::new(ProjectRuns::new(&context));
    crate::PROJECTS.insert(context.id, &runs);
    (root, runs, settings)
}

/// What the block sees, the Languages page showing PHP.
fn scope(project: &SettingsService) -> SettingsScope<'_> {
    SettingsScope {
        settings: project,
        project: Some(project),
        project_data: None,
        wake: None,
        language: Some("php"),
    }
}

fn statuses(block: &SettingsBlock) -> Vec<String> {
    let rows = block.rows.iter();
    rows.filter_map(|r| match r {
        SettingsRow::Status { text, .. } => Some(text.clone()),
        _ => None,
    })
    .collect()
}

fn keys_of(block: &SettingsBlock) -> Vec<&'static str> {
    let rows = block.rows.iter();
    rows.filter_map(|r| match r {
        SettingsRow::Choices { key, .. }
        | SettingsRow::Toggle { key, .. }
        | SettingsRow::Text { key, .. } => Some(*key),
        _ => None,
    })
    .collect()
}

#[test]
fn without_a_project_the_page_says_to_open_one() {
    let page = QualitySettings;
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let block = looked_block(
        &page,
        SettingsScope {
            settings: &settings,
            project: None,
            project_data: None,
            wake: None,
            language: None,
        },
    );
    assert_eq!(block.rows.len(), 1);
    assert!(matches!(block.rows[0], SettingsRow::Status { .. }));
}

/// A tool chosen: what it would run shown, its on-save switch; a binary or a command: its
/// field, the tool it follows, kept while its value is still to be written.
#[test]
fn the_choices_are_saved_in_the_project_and_shown() {
    let (root, _runs, settings) = project("choices");
    let page = QualitySettings;
    let scope = scope(&settings);
    assert_eq!(keys_of(&looked_block(&page, scope)), ["formatter", "checker"]);
    // The page's chooser lists the tools' languages; another language: nothing shown.
    let ids: Vec<String> = page.languages(scope).into_iter().map(|l| l.id).collect();
    assert_eq!(ids, ["php"]);
    assert!(looked_block(&page, SettingsScope { language: Some("sql"), ..scope }).rows.is_empty());

    page.choose(scope, "formatter", 1);
    page.choose(scope, "format_on_save", 1);
    assert_eq!(crate::settings::choice(&settings, "php").formatter, Choice::Tool("fmt".into()));
    assert!(crate::settings::choice(&settings, "php").format_on_save);
    let block = looked_block(&page, scope);
    assert_eq!(keys_of(&block), ["formatter", "format_on_save", "checker"]);
    assert!(statuses(&block)[0].contains("vendor/bin/fmt --quiet {file}"), "{block:?}");

    // The checker isn't installed: said so.
    page.choose(scope, "checker", 1);
    assert!(statuses(&looked_block(&page, scope))[1].contains("lint"));

    // A command, its line still to be written: chosen, asked for.
    page.choose(scope, "checker", COMMAND);
    let block = looked_block(&page, scope);
    let checker = &keys_of(&block)[2..];
    assert_eq!(checker, ["checker", "checker_value", "checker_reads", "check_on_save"]);
    assert_eq!(crate::settings::choice(&settings, "php").checker, Choice::None);
    page.set_text(scope, "checker_value", " ./scripts/cs.sh {file} ");
    page.choose(scope, "checker_reads", 1);
    let line = "./scripts/cs.sh {file}".to_owned();
    let expected = Choice::Command { line, reads_as: Some("lint".into()) };
    assert_eq!(crate::settings::choice(&settings, "php").checker, expected);
    // The same way chosen again keeps what was written; another one doesn't.
    page.choose(scope, "checker", COMMAND);
    assert_eq!(crate::settings::choice(&settings, "php").checker, expected);
    page.choose(scope, "checker", BINARY);
    assert_eq!(crate::settings::saved(&settings, "php", Kind::Checker).value, "");
    page.set_text(scope, "checker_value", "tools/missing");
    assert!(statuses(&looked_block(&page, scope))[1].contains("tools/missing"));

    page.choose(scope, "formatter", NONE);
    assert_eq!(crate::settings::choice(&settings, "php").formatter, Choice::None);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn languages_follow_the_tools_and_show_their_names() {
    let tools: Vec<Arc<dyn QualityTool>> =
        vec![Arc::new(Tool("fmt", Kind::Formatter)), Arc::new(Tool("lint", Kind::Checker))];
    assert_eq!(languages(&tools), ["php"]);
    assert_eq!(language_name("javascript"), "JavaScript");
    assert_eq!(language_name("elixir"), "elixir");
}

/// ESLint as the JavaScript module describes it: a Node package of the project.
#[derive(Debug)]
struct Lint;

impl QualityTool for Lint {
    fn id(&self) -> &'static str {
        "eslint"
    }
    fn name(&self) -> &'static str {
        "ESLint"
    }
    fn kind(&self) -> Kind {
        Kind::Checker
    }
    fn languages(&self) -> &'static [&'static str] {
        &["javascript"]
    }
    fn candidates(&self) -> &'static [&'static str] {
        &["node_modules/.bin/eslint", "eslint"]
    }
    fn scripts(&self) -> &'static [&'static str] {
        &["node_modules/eslint/bin/eslint.js"]
    }
    fn args(&self, _target: &Target, _root: &Path) -> Vec<String> {
        Vec::new()
    }
}

/// The project's own ESLint (its package, its version) offered while nothing is chosen;
/// chosen through the question, it's saved and allowed to run.
#[test]
fn a_tool_the_project_has_is_offered_and_chosen() {
    let root = std::env::temp_dir().join(format!("quality-page-offer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("node_modules/eslint/bin")).unwrap();
    let settings = Arc::new(SettingsService::new(Arc::new(Memory::default())));
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
        quality_tools: vec![Arc::new(Lint)],
        text_formatters: Vec::new(),
        notifier: std::sync::Arc::new(workbench_model::NoNotifier),
    };
    let runs = Arc::new(ProjectRuns::new(&context));
    crate::PROJECTS.insert(context.id, &runs);
    let page = QualitySettings;
    let scope = || SettingsScope { language: Some("javascript"), ..scope(&settings) };
    // Not there: nothing offered.
    assert!(statuses(&looked_block(&page, scope())).is_empty());
    std::fs::write(root.join("node_modules/eslint/bin/eslint.js"), "").unwrap();
    std::fs::write(
        root.join("node_modules/eslint/package.json"),
        "{\"name\": \"eslint\", \"version\": \"9.12.0\"}",
    )
    .unwrap();
    // Looked at again (the block would after a few seconds).
    FOUND.forget();
    let found = in_project(&root, &Lint).unwrap();
    assert_eq!(
        (found.place.as_str(), found.version.as_deref()),
        ("node_modules/eslint", Some("9.12.0"))
    );
    let block = looked_block(&page, scope());
    let offer = block.rows.iter().find_map(|r| match r {
        SettingsRow::Status { text, button: Some((action, _)) } => Some((text.clone(), *action)),
        _ => None,
    });
    let (text, action) = offer.expect("the offer");
    assert!(text.contains("ESLint 9.12.0") && text.contains("node_modules/eslint"), "{text}");
    assert_eq!(action, "use-checker");
    // Asked first (it runs the project's code); yes: chosen and allowed.
    let (question, _then) = page.press(scope(), action).expect("a question");
    assert!(question.danger);
    use_found(&runs, "javascript", Kind::Checker, &found);
    assert_eq!(chosen(&settings, "javascript", Kind::Checker), Choice::Tool("eslint".into()));
    assert!(runs.allows(&Choice::Tool("eslint".into())));
    let _ = std::fs::remove_dir_all(&root);
}

/// The block once what it shows from the disk is looked at (in the background: a block is
/// built on the UI thread).
fn looked_block(page: &QualitySettings, scope: SettingsScope<'_>) -> SettingsBlock {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let block = page.block(scope);
        if !crate::looked::busy() {
            let again = page.block(scope);
            if !crate::looked::busy() && format!("{again:?}") == format!("{block:?}") {
                return again;
            }
        }
        assert!(std::time::Instant::now() < until, "the block's looks are made");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
