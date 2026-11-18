use super::*;
use crate::catalog::Catalog;
use crate::fixture::{Project, TempDir, language_of, register_reads, wait_until};
use refactor::application::plan_rename;

/// A project with a `.env` file, PHP and YAML reading its key, and a comment naming it.
fn project() -> Project {
    let dir = TempDir::new("rename");
    dir.write(".env", "APP_ENV=dev\nURL=http://${APP_ENV}/\n");
    dir.write(".env.example", "APP_ENV=\n");
    dir.write("src/Kernel.php", "<?php\n// APP_ENV is read below.\n$env = getenv('APP_ENV');\n");
    dir.write("config/services.yaml", "parameters:\n  env: '%env(APP_ENV)%'\n");
    dir.write("vendor/lib/x.php", "<?php getenv('APP_ENV');\n");
    Project { dir }
}

/// The context at `needle` in `relative` (its text from disk), the project's catalog read.
fn context_at(project: &Arc<Project>, relative: &str, needle: &str) -> Option<RenameContext> {
    register_reads();
    // Composer's, as PHP's module says.
    kernel::paths::register(kernel::ProjectDirs {
        dependencies: &["vendor"],
        ..Default::default()
    });
    let root = project.dir.path().to_path_buf();
    let id = kernel::ProjectId::new();
    let catalog = Catalog::new(root.clone(), language_of(), Arc::new(|| {}));
    wait_until(|| !catalog.busy());
    let _kept = catalog::keep(id, catalog);
    let path = root.join(relative);
    let text = std::fs::read_to_string(&path).unwrap();
    let language = language_of()(&path).unwrap();
    let at = CursorContext {
        project: Some(id),
        root: Some(&root),
        queries: Some(Arc::clone(project) as Arc<dyn ProjectQueries>),
        open: Arc::default(),
        path: &path,
        language: &language,
        text: &text,
        offset: text.find(needle)? + 1,
    };
    Keys.rename_context(&at)
}

/// The places a plan finds: (file, line text) of each occurrence.
fn places(context: &RenameContext) -> Vec<(String, String)> {
    let plan = plan_rename(&context.target, &*context.sources, &*context.resolver, &|| false);
    let root = context.sources.root().unwrap().to_path_buf();
    plan.files
        .iter()
        .flat_map(|f| {
            let file = f.path.strip_prefix(&root).unwrap().to_string_lossy().into_owned();
            f.occurrences.iter().map(move |o| (file.clone(), o.line_text.clone()))
        })
        .collect()
}

#[test]
fn a_keys_places_from_a_usage_in_code() {
    let project = Arc::new(project());
    let context = context_at(&project, "src/Kernel.php", "APP_ENV')").unwrap();
    assert_eq!(context.target.name, "APP_ENV");
    let mut found = places(&context);
    found.sort();
    assert_eq!(
        found,
        [
            (".env".to_owned(), "APP_ENV=dev".to_owned()),
            (".env".to_owned(), "URL=http://${APP_ENV}/".to_owned()),
            (".env.example".to_owned(), "APP_ENV=".to_owned()),
            ("config/services.yaml".to_owned(), "  env: '%env(APP_ENV)%'".to_owned()),
            ("src/Kernel.php".to_owned(), "$env = getenv('APP_ENV');".to_owned()),
        ],
        "not the comment, not the library's"
    );
}

#[test]
fn the_same_places_from_its_definition() {
    let project = Arc::new(project());
    let context = context_at(&project, ".env", "APP_ENV=").unwrap();
    assert_eq!(places(&context).len(), 5);
}

#[test]
fn a_new_name_already_a_key_conflicts() {
    let project = Arc::new(project());
    let context = context_at(&project, ".env", "APP_ENV=").unwrap();
    let conflicts = context.conflicts.conflicts("URL");
    assert_eq!(conflicts.len(), 1);
    assert_eq!((conflicts[0].line, conflicts[0].line_text.as_str()), (1, "URL=http://${APP_ENV}/"));
    assert!(context.conflicts.conflicts("FRESH").is_empty());
}

#[test]
fn nothing_where_no_key_is() {
    let project = Arc::new(project());
    assert!(context_at(&project, "src/Kernel.php", "is read").is_none());
}
