use super::*;

#[test]
fn dependency_paths() {
    // The folders the languages' modules register (Composer's, npm's, the builds').
    crate::paths::register(crate::ProjectDirs {
        dependencies: &["vendor", "node_modules"],
        unlisted: &["node_modules"],
        generated: &["dist", "build", "storage", "public/build", "var/cache"],
        ..Default::default()
    });
    assert!(in_dependencies(Path::new("vendor/symfony/x.php")));
    assert!(in_dependencies(Path::new("web/node_modules/a.js")));
    assert!(!in_dependencies(Path::new("src/vendors/x.php")));
    assert!(in_dependencies_or_git(Path::new(".git/HEAD")));
}

#[test]
fn home_is_shown_as_tilde() {
    let home = std::env::temp_dir().join("home-of-ana");
    let sep = std::path::MAIN_SEPARATOR;
    assert_eq!(
        tildify(&home.join("work").join("shop"), Some(&home)),
        format!("~{sep}work{sep}shop")
    );
    assert_eq!(tildify(&home, Some(&home)), "~");
    let elsewhere = std::env::temp_dir().join("other");
    assert_eq!(tildify(&elsewhere, Some(&home)), elsewhere.display().to_string());
    assert_eq!(tildify(Path::new("src/a.php"), Some(&home)), "src/a.php", "relative: as it is");
    assert_eq!(tildify(&elsewhere, None), elsewhere.display().to_string());
}

#[test]
fn what_the_code_generates_is_known_by_its_place() {
    register(ProjectDirs {
        generated: &["var/cache", "storage", "dist"],
        ..ProjectDirs::default()
    });
    for generated in ["var/cache/dev/a.php", "storage/framework/x", "web/dist/app.js", "dist"] {
        assert!(in_generated(Path::new(generated)), "{generated}");
    }
    for own in ["src/Cache.php", "var/log/dev.log", "distance.php", "src/storage.php"] {
        assert!(!in_generated(Path::new(own)), "{own}");
    }
}

#[test]
fn globs_match_as_gitignore_reads_them() {
    assert!(glob_match("src/**/*.php", "src/Controller/Blog.php"));
    assert!(glob_match("src/**/*.php", "src/Kernel.php"), "`**/` matches nothing too");
    assert!(!glob_match("src/*.php", "src/Controller/Blog.php"), "`*` stays in a component");
    assert!(glob_match("config/**", "config/routes/admin.yaml"));
    assert!(glob_match("translations/*.[xy]*", "translations/messages.yaml"));
    assert!(!glob_match("config/**", "src/config.php"));
}

#[test]
fn a_folder_marked_as_a_library_counts_as_a_dependency_until_forgotten() {
    // Absolute on every system (`/projects` has no drive on Windows: not absolute there).
    let root = &std::env::temp_dir().join("marked-library");
    set_libraries(root, vec![root.join("third_party")]);
    assert!(in_dependencies(&root.join("third_party/a.php")));
    assert!(!in_dependencies(&root.join("src/a.php")));
    set_libraries(root, Vec::new());
    assert!(!in_dependencies(&root.join("third_party/a.php")));
}

/// A dependency in a project: through a folder a module names (only the project's own part
/// of the path), in one its managers' files name; and whether it's that folder itself.
#[test]
fn a_dependency_is_told_from_the_project_s_own_path() {
    crate::paths::register(crate::ProjectDirs { dependencies: &["vendor"], ..Default::default() });
    let root = Path::new("/work/vendor/shop");
    assert_eq!(dependency_at(root, Path::new("src/App.php")), None, "the root's own `vendor`");
    assert_eq!(dependency_at(root, Path::new("vendor")), Some(true));
    assert_eq!(dependency_at(root, Path::new("vendor/acme/x.php")), Some(false));
    set_found(root, vec!["lib/composer".into()]);
    assert_eq!(dependency_at(root, Path::new("lib/composer")), Some(true));
    assert_eq!(dependency_at(root, Path::new("lib/composer/acme/y.php")), Some(false));
    assert_eq!(dependency_at(root, Path::new("lib/other.php")), None);
    assert!(in_dependencies(&root.join("lib/composer/acme/y.php")), "what everyone asks knows it");
    set_found(root, Vec::new());
    assert_eq!(dependency_at(root, Path::new("lib/composer")), None, "forgotten");
}

/// A generated folder is found as a whole part of a path (at its start or inside it, a name
/// or several), never as a piece of a name.
#[test]
fn a_folder_is_a_whole_part_of_a_path() {
    assert!(has_folder("var/cache/dev/a.php", "var/cache"));
    assert!(has_folder("app/var/cache", "var/cache"));
    assert!(has_folder("x/dist/y.js", "dist"));
    assert!(has_folder("dist", "dist"));
    assert!(!has_folder("distant/y.js", "dist"));
    assert!(!has_folder("src/predist/y.js", "dist"));
    assert!(!has_folder("a/var/cachex/b", "var/cache"));
    assert!(!has_folder("a", ""));
}

/// A path through a dependency folder (any of the registered names, as a whole name) is one;
/// a name only containing one isn't.
#[test]
fn a_path_through_a_dependency_folder_is_found_by_its_names() {
    register(ProjectDirs { dependencies: &["vendor", "node_modules"], ..Default::default() });
    assert!(through_dependency_dir(Path::new("/p/vendor/acme/a.php")));
    assert!(through_dependency_dir(Path::new("web/node_modules/x/index.js")));
    assert!(!through_dependency_dir(Path::new("/p/vendors/a.php")));
    assert!(!through_dependency_dir(Path::new("/p/src/my_vendor/a.php")));
}
