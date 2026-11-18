use super::*;
use std::path::Path;

/// The files' generations follow the project's events, whoever changed the files: a write
/// counts for the files only, a new entry for both; the repository's folder, our own data
/// and what the code generates don't count; the counting ends with its subscriptions.
#[test]
fn generations_follow_the_project_s_events() {
    kernel::paths::register(kernel::ProjectDirs {
        dependencies: &["vendor"],
        unlisted: &[],
        generated: &["var/cache"],
        ..Default::default()
    });
    let events = kernel::EventBus::default();
    let generations = Arc::new(Generations::default());
    let root = Path::new("/p");
    let subscriptions = generations.follow(&events, root, vec![".git"]);
    let written = |path: &str| {
        events.publish(&workspace::events::FilesWritten { paths: vec![root.join(path)] });
    };
    written("src/A.php");
    assert_eq!((generations.files(), generations.entries()), (1, 0));
    written("var/cache/x.php");
    written(".git/index");
    written(&format!("{}/session.toml", kernel::brand::PROJECT_DIR));
    assert_eq!(generations.files(), 1, "not project files");
    events.publish(&workspace::events::EntriesChanged { paths: vec![root.join("src/B.php")] });
    assert_eq!((generations.files(), generations.entries()), (2, 1));
    events.publish(&document::events::FileSaved { path: root.join("src/B.php") });
    assert_eq!((generations.files(), generations.entries()), (3, 1));
    drop(subscriptions);
    written("src/A.php");
    assert_eq!(generations.files(), 3);
}
