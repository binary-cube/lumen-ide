//! What a watcher hears of the project: the repositories' folders and the app's own folder
//! apart, but for the user's files in it (a module's: the consoles').

use super::*;

#[test]
fn the_app_s_own_folder_is_unheard_but_for_the_user_s_files_in_it() {
    let dirs = WatchedDirs { vcs: vec![".git"], own: vec!["consoles"] };
    let own = |rest: &str| Path::new(kernel::brand::PROJECT_DIR).join(rest);
    assert!(!dirs.internal(Path::new("src/a.php")));
    assert!(dirs.internal(Path::new(".git/HEAD")));
    assert!(dirs.internal(Path::new("vendor/lib/.git/index")));
    assert!(dirs.internal(Path::new(kernel::brand::PROJECT_DIR)));
    assert!(dirs.internal(&own("session.toml")));
    assert!(dirs.internal(&own("index/symbols.bin")));
    assert!(!dirs.internal(&own("consoles")));
    assert!(!dirs.internal(&own("consoles/3a/console.sql")));
    // Without the modules' word, all of it is the app's.
    assert!(WatchedDirs::default().internal(&own("consoles/3a/console.sql")));
}

#[test]
fn only_a_file_right_inside_the_project_s_own_folder_is_its_own() {
    let own = kernel::brand::PROJECT_DIR;
    assert!(WatchedDirs::own_file(Path::new(&format!("{own}/path-marks"))));
    assert!(!WatchedDirs::own_file(Path::new(&format!("{own}/index/symbols"))));
    assert!(!WatchedDirs::own_file(Path::new("src/path-marks")));
    assert!(!WatchedDirs::own_file(Path::new(own)));
}
