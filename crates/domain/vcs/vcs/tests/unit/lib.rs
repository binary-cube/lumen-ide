use super::*;

#[test]
fn changes_against_the_committed_text() {
    let base = "a\nb\nc\nd\n";
    let current = "a\nB\nc\nnew\n";
    let changes = line_changes(base, current);
    assert_eq!(
        changes,
        [
            LineChange { lines: 1..2, kind: ChangeKind::Modified, base: 1..2 },
            LineChange { lines: 3..4, kind: ChangeKind::Modified, base: 3..4 },
        ]
    );
    let added = line_changes("a\nb\n", "a\nx\ny\nb\n");
    assert_eq!(added, [LineChange { lines: 1..3, kind: ChangeKind::Added, base: 1..1 }]);
    let deleted = line_changes("a\nb\nc\n", "a\nc\n");
    assert_eq!(deleted, [LineChange { lines: 1..1, kind: ChangeKind::Deleted, base: 1..2 }]);
    assert!(line_changes("same\n", "same\n").is_empty());
}

#[test]
fn relative_paths_use_slashes() {
    let root = Path::new("/w/project");
    assert_eq!(
        relative_path(root, Path::new("/w/project/src/A.php")).as_deref(),
        Some("src/A.php")
    );
    assert_eq!(relative_path(root, Path::new("/elsewhere/a")), None);
    let commit =
        Commit { id: "abcdef123456".into(), author: "A".into(), time: 0, summary: "s".into() };
    assert_eq!(commit.short(), "abcdef1");
}

#[test]
fn blame_follows_lines_moved_by_local_edits() {
    let commit =
        |id: &str| Commit { id: id.into(), author: "A".into(), time: 0, summary: id.into() };
    // Committed: a b c d (a, b from commit 1; c, d from commit 2).
    let blame = vec![
        BlameHunk { lines: 0..2, commit: commit("one") },
        BlameHunk { lines: 2..4, commit: commit("two") },
    ];
    // Now: a NEW b C d — a line added after `a`, `c` changed.
    let changes = line_changes("a\nb\nc\nd\n", "a\nNEW\nb\nC\nd\n");
    let lines = blame_lines(&changes, &blame, 5);
    assert_eq!(lines, [Some(0), None, Some(0), None, Some(1)]);
    // A deletion shifts the rest up.
    let changes = line_changes("a\nb\nc\nd\n", "a\nc\nd\n");
    assert_eq!(blame_lines(&changes, &blame, 3), [Some(0), Some(1), Some(1)]);
}

#[test]
fn dates_from_unix_seconds() {
    assert_eq!(date(0), (1970, 1, 1));
    assert_eq!(date(951_782_400), (2000, 2, 29));
    assert_eq!(date(1_792_454_400), (2026, 10, 20));
}

#[test]
fn reference_names_follow_git_s_rules() {
    for ok in ["main", "feature/login", "v1.2.3", "fix-42", "a.b/c_d", "release@2"] {
        assert_eq!(check_ref_name(ok), Ok(()), "{ok}");
    }
    assert_eq!(check_ref_name(""), Err(RefNameError::Empty));
    for bad in ["a b", "a~b", "a^b", "a:b", "a?b", "a*b", "a[b", "a\\b", "a\u{7}b"] {
        assert_eq!(check_ref_name(bad), Err(RefNameError::BadCharacter), "{bad}");
    }
    for bad in
        ["a..b", "a@{b", "a//b", "-a", "/a", "a/", "a.", "a.lock", ".a", "a/.b", "@", "a/b.lock/c"]
    {
        assert_eq!(check_ref_name(bad), Err(RefNameError::BadShape), "{bad}");
    }
}

#[test]
fn stashes_and_submodules_as_git_lists_them() {
    let stashes =
        parse_stashes("stash@{0}\u{1f}On main: wip\nstash@{1}\u{1f}WIP on x: 4b3685a style\n");
    assert_eq!(stashes.len(), 2);
    assert_eq!(stashes[0], Stash { name: "stash@{0}".into(), message: "On main: wip".into() });
    assert!(parse_stashes("").is_empty());
    let modules = "submodule.libs/b.path libs/b\nsubmodule.a.path vendor/a\n";
    assert_eq!(parse_submodules(modules), ["libs/b", "vendor/a"]);
}
