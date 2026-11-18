use super::*;

fn marks(text: &str) -> PathMarks {
    PathMarks::parse(text)
}

#[test]
fn a_path_takes_its_nearest_marked_ancestors_mark() {
    let m = marks("excluded var\nnormal var/keep\n");
    assert!(m.is_excluded("var/old/a.php"));
    assert_eq!(m.of("var/keep/a.php"), Some(PathMark::Normal));
    assert_eq!(m.of("variable/a.php"), None, "a name that only starts the same");
    assert_eq!(m.of("src/a.php"), None);
}

#[test]
fn the_file_is_read_back_as_written_and_bad_lines_are_left_out() {
    let m = marks("# a comment\nexcluded var/old-wp\nnonsense x\ngenerated build/\nlibrary \n");
    assert_eq!(m.to_text(), "generated build\nexcluded var/old-wp\n", "sorted by path");
    assert_eq!(marks(&m.to_text()), m);
}

#[test]
fn a_path_with_spaces_stays_whole() {
    let m = marks("excluded old site/backup\n");
    assert!(m.is_excluded("old site/backup/a.php"));
}

#[test]
fn a_mark_is_taken_off() {
    let mut m = PathMarks::default();
    m.set("var/", Some(PathMark::Excluded));
    assert_eq!(m.own("var"), Some(PathMark::Excluded));
    m.set("var", None);
    assert!(m.is_empty());
}

#[test]
fn only_excluding_a_library_folder_leaves_names_unknown() {
    kernel::paths::register(kernel::ProjectDirs {
        dependencies: &["vendor"],
        ..Default::default()
    });
    assert!(!marks("excluded var\n").excludes_dependencies());
    assert!(marks("excluded vendor\n").excludes_dependencies());
    assert!(!marks("normal vendor\n").excludes_dependencies());
}
