use super::*;

fn repo(name: &str, config: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("git-trust-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join(".git/config"), config).unwrap();
    root
}

/// A repository whose own config runs nothing: git runs there freely.
#[test]
fn a_plain_repository_needs_no_trust() {
    let root = repo("plain", "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = x\n");
    assert!(risky(&root).is_empty());
    assert_eq!(check(&root), Ok(()));
    let _ = std::fs::remove_dir_all(root);
}

/// A filter (run by `git status`), a hooks folder, a diff driver: refused until trusted;
/// trusted, it runs; changed after, refused again.
#[test]
fn programs_in_its_config_run_only_once_trusted() {
    let root = repo(
        "risky",
        "[filter \"lfs\"]\n\tclean = evil %f\n[core]\n\thooksPath = /tmp/h\n[diff.bin]\n\ttextconv = x\n",
    );
    let lines = check(&root).unwrap_err();
    assert_eq!(lines.len(), 3, "{lines:?}");
    trust(&root).unwrap();
    assert_eq!(check(&root), Ok(()));
    std::fs::write(root.join(".git/config"), "[filter \"lfs\"]\n\tclean = other\n").unwrap();
    assert!(check(&root).is_err(), "changed: asked again");
    let _ = std::fs::remove_dir_all(root);
}

/// Run from a folder inside it, the repository's config still counts.
#[test]
fn a_folder_inside_the_repository_is_the_repository() {
    let root = repo("inside", "[filter \"x\"]\n\tsmudge = evil\n");
    std::fs::create_dir_all(root.join("src/deep")).unwrap();
    assert!(check(&root.join("src/deep")).is_err());
    trust(&root.join("src")).unwrap();
    assert_eq!(check(&root.join("src/deep")), Ok(()), "trusted as the repository");
    let _ = std::fs::remove_dir_all(root);
}
