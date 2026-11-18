use super::*;

fn files() -> Vec<String> {
    [
        "src/Controller/UserController.php",
        "src/Entity/User.php",
        "src/Repository/UserRepository.php",
        "templates/user/index.html.twig",
        "config/services.yaml",
        "composer.json",
    ]
    .map(String::from)
    .to_vec()
}

#[test]
fn folders_match_by_their_own_name() {
    let folders =
        ["crates", "crates/domain", "crates/domain/intelligence", "crates/domain/intelligence/src"]
            .map(String::from);
    let found = search_folders(&folders, "intel", 10);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, "crates/domain/intelligence");
    assert_eq!(found[0].name_positions, [0, 1, 2, 3, 4]);
}

#[test]
fn camel_case_abbreviations_match_names() {
    let found = search_files(&files(), "uctrl", 10);
    assert_eq!(found[0].path, "src/Controller/UserController.php");
    assert_eq!(found[0].name_positions[..2], [0, 4]);
}

#[test]
fn exact_short_names_rank_first() {
    let found = search_files(&files(), "user", 10);
    assert_eq!(found[0].path, "src/Entity/User.php");
    assert!(found.iter().any(|m| m.path.ends_with("index.html.twig")));
}

#[test]
fn path_matches_when_name_does_not() {
    let found = search_files(&files(), "configserv", 10);
    assert_eq!(found[0].path, "config/services.yaml");
    assert_eq!(search_files(&files(), "zzz", 10), Vec::new());
    assert_eq!(search_files(&files(), "  ", 10), Vec::new());
}

#[test]
fn name_and_dir_split() {
    let m = &search_files(&files(), "composer", 1)[0];
    assert_eq!((m.dir(), m.name()), ("", "composer.json"));
}

#[test]
fn one_search_for_folders_and_files() {
    let files = ["src/Controller/UserController.php", "src/Entity/User.php"].map(String::from);
    let folders = ["src", "src/Controller", "src/Entity"].map(String::from);
    let both = search_paths(&files, &folders, &PathQuery { text: "contr", folders: 5, files: 5 });
    assert_eq!(both.folders[0].path, "src/Controller");
    assert_eq!(both.files[0].path, "src/Controller/UserController.php");
    let only_files =
        search_paths(&files, &folders, &PathQuery { text: "contr", folders: 0, files: 5 });
    assert!(only_files.folders.is_empty() && !only_files.files.is_empty());
}

#[test]
fn a_place_after_the_name() {
    assert_eq!(split_location("User.php:12"), ("User.php", Some((11, 0))));
    assert_eq!(split_location("src/User.php:12:5"), ("src/User.php", Some((11, 4))));
    assert_eq!(split_location("User.php"), ("User.php", None));
    assert_eq!(split_location("User.php:"), ("User.php:", None));
    assert_eq!(split_location(":12"), (":12", None));
    assert_eq!(split_location("C:12"), ("C", Some((11, 0))));
}

#[test]
fn one_filter_for_every_listing() {
    // The folders the languages' modules register (Composer's, npm's, the builds').
    kernel::paths::register(kernel::ProjectDirs {
        dependencies: &["vendor", "node_modules"],
        unlisted: &["node_modules"],
        generated: &["dist", "build", "storage", "public/build", "var/cache"],
        ..Default::default()
    });
    let php = FileFilter::code(&["php", "phtml"]);
    assert!(php.accepts("src/User.php") && php.accepts("tpl/a.PHTML"));
    assert!(!php.accepts("vendor/x/A.php"), "dependencies left out");
    assert!(!php.accepts("assets/app.js"));
    let all = FileFilter { dependencies: true, ..FileFilter::default() };
    assert!(all.accepts("vendor/x/A.php") && all.accepts("README"));
    let under = FileFilter { under: Some("src".into()), ..FileFilter::default() };
    assert!(
        under.accepts("src/a.php") && !under.accepts("srcx/a.php") && !under.accepts("lib/a.php")
    );
}

/// A folder typed goes on as a shell's would: the folders after its last `/` whose name starts
/// with what follows (any case), then looser ones under it; nothing outside it.
#[test]
fn a_folder_typed_goes_on_as_a_shell_completes_it() {
    let folders: Vec<String> = [
        "src",
        "src/Controller",
        "src/Command",
        "src/Entity",
        "src/Controller/Admin",
        "migrations",
        "public",
        "public/build",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(complete_folders(&folders, "", 10), ["migrations", "public", "src"]);
    assert_eq!(complete_folders(&folders, "s", 10), ["src"]);
    assert_eq!(
        complete_folders(&folders, "src/", 10),
        ["src/Command", "src/Controller", "src/Entity"]
    );
    assert_eq!(complete_folders(&folders, "src/co", 10), ["src/Command", "src/Controller"]);
    assert_eq!(complete_folders(&folders, "/SRC/Controller/", 10), ["src/Controller/Admin"]);
    // Loosely, under the folder typed: `ctl` is in `Controller`'s name, not at its start.
    let loose = complete_folders(&folders, "src/ctl", 10);
    assert!(loose.contains(&"src/Controller".to_owned()), "{loose:?}");
    assert!(loose.iter().all(|f| f.starts_with("src/")), "{loose:?}");
    assert_eq!(complete_folders(&folders, "src/", 2).len(), 2);
}

/// The disk, plainly (the test's own: the domain has no I/O; the test reads and writes a
/// scratch folder).
struct Disk;

#[allow(clippy::disallowed_methods)]
impl FileSystem for Disk {
    fn read_dir(&self, path: &Path) -> io::Result<Vec<crate::DirEntry>> {
        std::fs::read_dir(path)?
            .map(|e| {
                let e = e?;
                let kind =
                    if e.file_type()?.is_dir() { NodeKind::Directory } else { NodeKind::File };
                Ok(crate::DirEntry::new(e.file_name().to_string_lossy(), kind))
            })
            .collect()
    }

    fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }
}

/// A file `.gitignore` leaves out (`.env.local`) is found by its name, never listed among
/// the project's files (searched in); an ignored folder (`build/`) isn't walked at all.
#[test]
#[allow(clippy::disallowed_methods)]
fn an_ignored_file_is_found_by_name_and_an_ignored_folder_isnt_walked() {
    let root = std::env::temp_dir().join(format!("ignored-{}", kernel::id::new_uuid()));
    std::fs::create_dir_all(root.join("build")).unwrap();
    std::fs::write(root.join(".gitignore"), ".env.local\nbuild/\n").unwrap();
    std::fs::write(root.join(".env"), "A=1\n").unwrap();
    std::fs::write(root.join(".env.local"), "A=2\n").unwrap();
    std::fs::write(root.join("build/out.js"), "x").unwrap();
    let index = index_project(&root, &Disk, 1000).unwrap();
    assert!(!index.files.contains(&".env.local".to_owned()), "not searched in");
    assert_eq!(index.ignored, [".env.local"]);
    assert!(!index.files.iter().chain(&index.ignored).any(|f| f.starts_with("build/")));
    let query = PathQuery { text: ".env.local", folders: 0, files: 5 };
    let found = search_paths_with(&index.files, &index.ignored, &index.folders, &query);
    assert_eq!(found.files.first().map(|m| m.path.as_str()), Some(".env.local"));
    let plain = search_paths(&index.files, &index.folders, &query);
    assert!(plain.files.iter().all(|m| m.path != ".env.local"), "only where asked for");
    let _ = std::fs::remove_dir_all(root);
}
