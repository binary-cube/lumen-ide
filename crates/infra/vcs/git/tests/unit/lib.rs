use super::*;

/// The tests read this project's own repository (always a git checkout).
fn this_repository() -> Option<GitRepository> {
    GitRepository::discover(Path::new(env!("CARGO_MANIFEST_DIR")))
}

#[test]
fn reads_files_history_and_blame_of_this_repository() {
    let Some(repo) = this_repository() else {
        eprintln!("not a git checkout: skipped");
        return;
    };
    assert!(repo.root().join("Cargo.toml").is_file());
    assert!(repo.branch().is_some() && repo.head_id().is_some());
    let head = repo.text_at(&Revision::Head, "Cargo.toml").unwrap();
    assert!(head.contains("[workspace]"));
    assert!(repo.text_at(&Revision::Head, "no/such/file.txt").is_none());
    let history = repo.history("Cargo.toml", 3);
    assert!(!history.is_empty() && history.len() <= 3);
    assert_eq!(history[0].id.len(), 40);
    let lines = head.lines().count();
    let blame = repo.blame("Cargo.toml");
    let covered: usize = blame.iter().map(|h| h.lines.len()).sum();
    assert_eq!(covered, lines, "blame covers every committed line");
}

#[test]
fn parses_porcelain_status() {
    let out = " M src/a.php\0?? new.txt\0D  gone.php\0R  b.php\0old_b.php\0UU conflict.php\0A  added.js\0";
    let found = parse_status(out);
    let states: Vec<(&str, FileState)> = found.iter().map(|f| (f.path.as_str(), f.state)).collect();
    assert_eq!(
        states,
        [
            ("src/a.php", FileState::Modified),
            ("new.txt", FileState::Untracked),
            ("gone.php", FileState::Deleted),
            ("b.php", FileState::Renamed),
            ("conflict.php", FileState::Conflicted),
            ("added.js", FileState::Added),
        ]
    );
}

#[test]
fn status_of_this_repository_or_a_missing_tool() {
    let Some(repo) = this_repository() else { return };
    // Works with git installed; without it, it says so (and nothing breaks).
    match repo.status() {
        Ok(_) | Err(VcsError::ToolMissing(_)) => {}
        Err(e) => panic!("status failed: {e}"),
    }
}

#[test]
fn the_log_has_parents_references_and_details() {
    let Some(repo) = this_repository() else { return };
    let all = LogFilter::default();
    let page = repo.log(&all, 0, 20);
    assert!(!page.is_empty());
    // Newest first, `HEAD` labelled somewhere in the first page (unless far behind).
    assert!(page.windows(2).all(|w| w[0].commit.time >= w[1].commit.time));
    let next = repo.log(&all, 20, 5);
    assert!(next.iter().all(|e| !page.iter().any(|p| p.commit.id == e.commit.id)));
    let head = repo.head_id().unwrap();
    // One reference's line starts at it.
    assert_eq!(
        repo.log(&only("HEAD"), 0, 1).first().map(|e| e.commit.id.as_str()),
        Some(head.as_str())
    );
    let refs = repo.refs();
    assert!(refs.windows(2).all(|w| w[0].kind <= w[1].kind), "by kind");
    if let Some(branch) = refs.iter().find(|r| r.kind == version_control::RefKind::Branch) {
        assert!(!repo.log(&only(&branch.name), 0, 1).is_empty());
    }
    let details = repo.commit_details(&head).unwrap();
    assert_eq!(details.commit.id, head);
    assert!(!details.message.is_empty());
    assert!(details.files.iter().all(|f| !f.path.is_empty()));
    assert!(repo.sync_state().is_some_and(|s| !s.branch.is_empty()) || repo.branch().is_some());
}

/// A filter with only `reference`'s history.
fn only(reference: &str) -> LogFilter {
    LogFilter { references: vec![reference.to_owned()], ..LogFilter::default() }
}

/// `git` run in `dir` as `author` at `date` (its own HOME: nothing of the user's is read);
/// its output when it succeeded.
fn run_git(
    dir: &std::path::Path,
    args: &[&str],
    (author, date): (&str, &str),
) -> Option<std::process::Output> {
    std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", author)
        .env("GIT_AUTHOR_EMAIL", format!("{}@example.com", author.to_lowercase()))
        .env("GIT_COMMITTER_NAME", author)
        .env("GIT_COMMITTER_EMAIL", "c@example.com")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .ok()
        .filter(|o| o.status.success())
}

/// A repository made in a temporary folder with `git` (fixed authors and dates; its own
/// HOME: nothing of the user's is read): `None` without `git`.
fn sample_repository(name: &str) -> Option<(GitRepository, std::path::PathBuf)> {
    let dir = std::env::temp_dir().join(format!("git-log-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src/chat")).ok()?;
    let git = |args: &[&str], who: (&str, &str)| run_git(&dir, args, who);
    let ana = ("Ana", "2026-01-01T10:00:00Z");
    git(&["init", "-q", "-b", "main"], ana)?;
    git(&["config", "user.name", "Ana"], ana)?;
    // Its email too: the app's own commits on it read only this (Windows' git guesses none).
    git(&["config", "user.email", "ana@example.com"], ana)?;
    // Its files as written (Windows' git turns `\n` into `\r\n` on checkout by default).
    git(&["config", "core.autocrlf", "false"], ana)?;
    std::fs::write(dir.join("a.php"), "a").ok()?;
    git(&["add", "-A"], ana)?;
    git(&["commit", "-q", "-m", "First: the shop"], ana)?;
    std::fs::write(dir.join("src/chat/bot.php"), "bot").ok()?;
    git(&["add", "-A"], ("Bob", "2026-01-05T10:00:00Z"))?;
    git(&["commit", "-q", "-m", "Chat bot"], ("Bob", "2026-01-05T10:00:00Z"))?;
    git(&["checkout", "-q", "-b", "feature"], ana)?;
    std::fs::write(dir.join("a.php"), "b").ok()?;
    git(&["commit", "-q", "-am", "Cart fix"], ("Ana", "2026-01-09T10:00:00Z"))?;
    git(&["checkout", "-q", "main"], ana)?;
    let repo = GitRepository::discover(&dir)?;
    Some((repo, dir))
}

#[test]
fn the_log_s_filters_combine_on_a_sample_repository() {
    let Some((repo, dir)) = sample_repository("filters") else { return };
    let summaries = |filter: &LogFilter| -> Vec<String> {
        repo.log(filter, 0, 50).into_iter().map(|e| e.commit.summary).collect()
    };
    assert_eq!(summaries(&LogFilter::default()), ["Cart fix", "Chat bot", "First: the shop"]);
    assert_eq!(summaries(&only("main")), ["Chat bot", "First: the shop"]);
    let by = |author: &str| LogFilter { authors: vec![author.into()], ..LogFilter::default() };
    assert_eq!(summaries(&by("Bob")), ["Chat bot"]);
    assert_eq!(summaries(&by("ana@example.com")), ["Cart fix", "First: the shop"]);
    let text = |text: &str, regex| LogFilter {
        text: Some(version_control::TextFilter { text: text.into(), regex, match_case: false }),
        ..LogFilter::default()
    };
    assert_eq!(summaries(&text("CART", false)), ["Cart fix"]);
    assert_eq!(summaries(&text("^(first|chat)", true)), ["Chat bot", "First: the shop"]);
    let head = repo.head_id().unwrap();
    assert_eq!(summaries(&text(&head[..8], false)), ["Chat bot"], "a hash");
    let since = LogFilter {
        since: version_control::parse_day("2026-01-04"),
        until: version_control::parse_day("2026-01-06"),
        ..LogFilter::default()
    };
    assert_eq!(summaries(&since), ["Chat bot"]);
    let paths = |p: &str| LogFilter { paths: vec![p.into()], ..LogFilter::default() };
    assert_eq!(summaries(&paths("src")), ["Chat bot"], "a folder");
    assert_eq!(summaries(&paths("a.php")), ["Cart fix", "First: the shop"]);
    let combined = LogFilter { references: vec!["main".into()], ..paths("a.php") };
    assert_eq!(summaries(&combined), ["First: the shop"]);
    // Paged: the filtered commits skipped, not the walked ones.
    assert_eq!(repo.log(&paths("a.php"), 1, 5)[0].commit.summary, "First: the shop");
    let topo = LogFilter { order: version_control::LogOrder::Topological, ..LogFilter::default() };
    assert_eq!(summaries(&topo).len(), 3);
    assert_eq!(repo.authors(100), ["Ana", "Bob"]);
    assert_eq!(repo.user_name().as_deref(), Some("Ana"));
    let mut files = repo.tracked_files();
    files.sort();
    assert_eq!(files, ["a.php", "src/chat/bot.php"]);
    assert!(repo.branch_states().is_empty(), "nothing tracks a remote");
    let first = repo.log(&text("first", false), 0, 1)[0].commit.id.clone();
    assert_eq!(repo.containing_branches(&first), ["feature", "main"]);
    let fix = repo.log(&text("cart fix", false), 0, 1)[0].commit.id.clone();
    assert_eq!(repo.containing_branches(&fix), ["feature"]);
    let _ = std::fs::remove_dir_all(dir);
}

/// A sample repository with a bare one beside it as `origin` (both made for the test).
fn with_a_remote(name: &str) -> Option<(GitRepository, std::path::PathBuf, std::path::PathBuf)> {
    let (repo, dir) = sample_repository(name)?;
    let bare = dir.with_extension("bare.git");
    let _ = std::fs::remove_dir_all(&bare);
    let who = ("Ana", "2026-01-01T10:00:00Z");
    run_git(dir.parent()?, &["init", "-q", "--bare", bare.to_str()?], who)?;
    run_git(&dir, &["remote", "add", "origin", bare.to_str()?], who)?;
    Some((repo, dir, bare))
}

fn out_of(dir: &std::path::Path, args: &[&str]) -> Option<String> {
    run_git(dir, args, ("Ana", "2026-01-01T10:00:00Z"))
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
}

fn plan(branches: Vec<version_control::PushedBranch>) -> PushPlan {
    PushPlan { remote: "origin".into(), branches, tags: false, force: false }
}

fn branch(local: &str, remote: &str, track: bool) -> version_control::PushedBranch {
    version_control::PushedBranch { local: local.into(), remote: remote.into(), track }
}

#[test]
fn the_remotes_are_listed_with_where_they_lead() {
    let Some((repo, dir, bare)) = with_a_remote("remotes") else { return };
    let remotes = repo.remotes();
    assert_eq!(remotes.len(), 1);
    assert_eq!(remotes[0].name, "origin");
    assert_eq!(remotes[0].url, bare.to_str().unwrap());
    let _ = (std::fs::remove_dir_all(dir), std::fs::remove_dir_all(bare));
}

#[test]
fn each_branch_goes_under_its_name_and_only_the_chosen_ones_track() {
    let Some((repo, dir, bare)) = with_a_remote("plan") else { return };
    let branches = vec![branch("main", "main", true), branch("feature", "shared/cart", false)];
    let outcome = repo.push_plan(&plan(branches)).unwrap();
    assert!(!outcome.up_to_date);
    let main = out_of(&dir, &["rev-parse", "main"]);
    assert_eq!(out_of(&bare, &["rev-parse", "main"]), main);
    let feature = out_of(&dir, &["rev-parse", "feature"]);
    assert_eq!(out_of(&bare, &["rev-parse", "shared/cart"]), feature);
    assert_eq!(out_of(&dir, &["config", "branch.main.remote"]).as_deref(), Some("origin"));
    assert_eq!(out_of(&dir, &["config", "branch.main.merge"]).as_deref(), Some("refs/heads/main"));
    assert_eq!(out_of(&dir, &["config", "branch.feature.remote"]), None);
    let _ = (std::fs::remove_dir_all(dir), std::fs::remove_dir_all(bare));
}

#[test]
fn only_the_tags_can_be_sent() {
    let Some((repo, dir, bare)) = with_a_remote("tags") else { return };
    out_of(&dir, &["tag", "v1"]).unwrap();
    repo.push_plan(&PushPlan { tags: true, ..plan(Vec::new()) }).unwrap();
    assert!(out_of(&bare, &["rev-parse", "refs/tags/v1"]).is_some());
    assert_eq!(out_of(&bare, &["for-each-ref", "refs/heads"]).as_deref(), Some(""));
    let _ = (std::fs::remove_dir_all(dir), std::fs::remove_dir_all(bare));
}

#[test]
fn rewritten_history_goes_only_when_forced_and_never_over_what_was_not_seen() {
    let Some((repo, dir, bare)) = with_a_remote("force") else { return };
    let main = || vec![branch("main", "main", true)];
    repo.push_plan(&plan(main())).unwrap();
    let who = ("Ana", "2026-02-01T10:00:00Z");
    run_git(&dir, &["commit", "-q", "--amend", "-m", "Rewritten"], who).unwrap();
    assert!(repo.push_plan(&plan(main())).is_err(), "not without force");
    repo.push_plan(&PushPlan { force: true, ..plan(main()) }).unwrap();
    assert_eq!(out_of(&bare, &["rev-parse", "main"]), out_of(&dir, &["rev-parse", "main"]));
    // The remote moved and this repository hasn't read it: the lease refuses.
    let theirs = out_of(&bare, &["rev-parse", "main~1"]).unwrap();
    out_of(&bare, &["update-ref", "refs/heads/main", &theirs]).unwrap();
    run_git(&dir, &["commit", "-q", "--amend", "-m", "Again"], who).unwrap();
    assert!(repo.push_plan(&PushPlan { force: true, ..plan(main()) }).is_err());
    let _ = (std::fs::remove_dir_all(dir), std::fs::remove_dir_all(bare));
}

/// Branches, tags and an amended commit, on a repository made for the test (never a real
/// one): each write as git does it, read back.
#[test]
fn branches_tags_and_amend_on_a_sample_repository() {
    let Some((repo, dir)) = sample_repository("writes") else { return };
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&dir)
            .env("HOME", &dir)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    let all = LogFilter::default();
    let first = repo.log(&all, 0, 10).last().unwrap().commit.id.clone();
    // A branch at an older commit, not checked out; then one checked out.
    repo.create_branch("fix/old", Some(&first), false).unwrap();
    assert_eq!(git(&["rev-parse", "fix/old"]).as_deref(), Some(first.as_str()));
    assert_eq!(repo.branch().as_deref(), Some("main"));
    repo.create_branch("topic", None, true).unwrap();
    assert_eq!(repo.branch().as_deref(), Some("topic"));
    assert!(repo.create_branch("topic", None, false).is_err(), "it exists");
    repo.rename_branch("fix/old", "fix/older").unwrap();
    assert!(repo.branches().contains(&"fix/older".to_owned()));
    assert!(!repo.branches().contains(&"fix/old".to_owned()));
    // Tags: light, and annotated (its message kept).
    repo.create_tag("v1", None, None).unwrap();
    repo.create_tag("v0", Some(&first), Some("The first")).unwrap();
    assert_eq!(git(&["cat-file", "-t", "v0"]).as_deref(), Some("tag"));
    assert_eq!(git(&["cat-file", "-t", "v1"]).as_deref(), Some("commit"));
    // Amend: the last message, a new one; not pushed until a remote branch holds it.
    assert_eq!(repo.last_commit_message().as_deref(), Some("Chat bot"));
    assert!(!repo.head_pushed());
    let before = repo.head_id().unwrap();
    let after = repo.amend(&[], "Chat bot, amended").unwrap();
    assert_ne!(before, after);
    assert_eq!(repo.last_commit_message().as_deref(), Some("Chat bot, amended"));
    git(&["update-ref", "refs/remotes/origin/topic", "HEAD"]).unwrap();
    assert!(repo.head_pushed());
    let _ = std::fs::remove_dir_all(dir);
}

/// A rebase left with a conflict (as SourceTree or the terminal leaves it): `HEAD` detached,
/// the operation said — the branch being rebased, its step; given up, nothing in progress.
#[test]
fn a_rebase_left_in_progress_is_said() {
    let Some((repo, dir)) = sample_repository("rebase") else { return };
    let ana = ("Ana", "2026-02-01T10:00:00Z");
    assert!(repo.operation().is_none() && !repo.detached());
    std::fs::write(dir.join("a.php"), "c").unwrap();
    run_git(&dir, &["commit", "-q", "-am", "Main's cart"], ana).unwrap();
    run_git(&dir, &["checkout", "-q", "feature"], ana).unwrap();
    // It stops on the conflict (a failure for git: no output expected).
    assert!(run_git(&dir, &["rebase", "main"], ana).is_none(), "a conflict stops it");
    let op = repo.operation().expect("a rebase in progress");
    assert_eq!(op.kind, version_control::OperationKind::Rebase);
    assert_eq!(op.branch.as_deref(), Some("feature"));
    assert_eq!(op.step, Some((1, 1)));
    assert!(repo.detached());
    run_git(&dir, &["rebase", "--abort"], ana).unwrap();
    assert!(repo.operation().is_none());
    assert_eq!(repo.branch().as_deref(), Some("feature"));
    let _ = std::fs::remove_dir_all(dir);
}

/// The sample repository stopped by a rebase of `feature` onto `main` on `a.php`'s conflict
/// (main: `c`, feature: `b`); its own identity, so the commits git makes need nothing of the
/// user's.
fn stopped_rebase(name: &str) -> Option<(GitRepository, std::path::PathBuf)> {
    let (repo, dir) = sample_repository(name)?;
    let ana = ("Ana", "2026-02-01T10:00:00Z");
    run_git(&dir, &["config", "user.email", "ana@example.com"], ana)?;
    run_git(&dir, &["config", "commit.gpgsign", "false"], ana)?;
    std::fs::write(dir.join("a.php"), "c").ok()?;
    run_git(&dir, &["commit", "-q", "-am", "Main's cart"], ana)?;
    run_git(&dir, &["checkout", "-q", "feature"], ana)?;
    run_git(&dir, &["rebase", "main"], ana).is_none().then_some(())?;
    Some((repo, dir))
}

#[test]
fn a_conflict_keeps_its_versions_and_is_resolved_by_taking_a_side() {
    let Some((repo, dir)) = stopped_rebase("take-side") else { return };
    let conflicted = || version_control::application::conflicted_files(&repo).unwrap();
    assert_eq!(conflicted(), ["a.php"]);
    let versions = version_control::application::conflict_versions(&repo, "a.php");
    // During a rebase, ours is what it's rebased onto; theirs, the commit replayed.
    assert_eq!(versions.base.as_deref(), Some("a"));
    assert_eq!(versions.ours.as_deref(), Some("c"));
    assert_eq!(versions.theirs.as_deref(), Some("b"));
    let merged =
        version_control::Merged::parse(&std::fs::read_to_string(dir.join("a.php")).unwrap());
    assert_eq!(merged.unresolved(), 1);

    repo.take_side(&["a.php".into()], Side::Theirs).unwrap();
    assert!(conflicted().is_empty());
    assert_eq!(std::fs::read_to_string(dir.join("a.php")).unwrap(), "b");
    assert_eq!(repo.proceed(OperationStep::Continue), Ok(Proceeded::Finished));
    assert!(repo.operation().is_none());
    assert_eq!(repo.branch().as_deref(), Some("feature"));
    assert_eq!(repo.text_at(&Revision::Head, "a.php").as_deref(), Some("b"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_merge_resolved_by_hand_is_committed_and_one_can_be_given_up() {
    let Some((repo, dir)) = stopped_rebase("merge") else { return };
    let ana = ("Ana", "2026-02-02T10:00:00Z");
    assert_eq!(repo.proceed(OperationStep::Abort), Ok(Proceeded::Finished));
    assert!(repo.operation().is_none());
    // The same conflict, merged this time.
    assert!(run_git(&dir, &["merge", "main"], ana).is_none(), "a conflict stops it");
    let operation = repo.operation().unwrap();
    assert_eq!(operation.kind, OperationKind::Merge);
    assert_eq!(operation_args(OperationKind::Merge, OperationStep::Skip), None);
    std::fs::write(dir.join("a.php"), "b and c").unwrap();
    repo.mark_resolved(&["a.php".into()]).unwrap();
    assert_eq!(repo.proceed(OperationStep::Continue), Ok(Proceeded::Finished));
    assert!(repo.operation().is_none());
    assert_eq!(repo.text_at(&Revision::Head, "a.php").as_deref(), Some("b and c"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_status_is_split_by_the_staging_area() {
    let found = parse_staged(
        "MM both.txt\0A  new.txt\0 D gone.txt\0?? mine.txt\0R  to.txt\0from.txt\0UU clash.txt\0",
    );
    let state =
        |path: &str| found.iter().find(|f| f.path == path).map(|f| (f.staged, f.unstaged)).unwrap();
    assert_eq!(state("both.txt"), (Some(FileState::Modified), Some(FileState::Modified)));
    assert_eq!(state("new.txt"), (Some(FileState::Added), None));
    assert_eq!(state("gone.txt"), (None, Some(FileState::Deleted)));
    assert_eq!(state("mine.txt"), (None, Some(FileState::Untracked)));
    assert_eq!(state("to.txt"), (Some(FileState::Renamed), None));
    assert_eq!(state("clash.txt"), (None, Some(FileState::Conflicted)));
    assert_eq!(found.len(), 6);
}

#[test]
fn some_lines_of_a_hunk_go_on_the_staging_area_and_come_back() {
    use version_control::staging::hunks;
    use version_control::{PatchTarget, Revision};
    let Some((repo, dir)) = sample_repository("lines") else { return };
    let path = "lines.txt".to_owned();
    std::fs::write(dir.join(&path), "a\nb\nc\n").unwrap();
    repo.stage(std::slice::from_ref(&path)).unwrap();
    repo.commit_staged("lines").unwrap();
    std::fs::write(dir.join(&path), "a\nB\nC\n").unwrap();
    let found = hunks("a\nb\nc\n", "a\nB\nC\n", 1);
    let lines = &found[0].lines;
    let wanted: Vec<usize> =
        (0..lines.len()).filter(|i| ["b\n", "B\n"].contains(&lines[*i].text.as_str())).collect();
    let forward = found[0].patch_lines(&path, |i| wanted.contains(&i), false);
    repo.apply_patch(&forward, PatchTarget::Staged, false).unwrap();
    assert_eq!(repo.text_at(&Revision::Index, &path).unwrap(), "a\nB\nc\n");
    let staged = hunks("a\nb\nc\n", "a\nB\nc\n", 1);
    let back = staged[0].patch_lines(&path, |_| true, true);
    repo.apply_patch(&back, PatchTarget::Staged, true).unwrap();
    assert_eq!(repo.text_at(&Revision::Index, &path).unwrap(), "a\nb\nc\n");
    assert_eq!(std::fs::read_to_string(dir.join(&path)).unwrap(), "a\nB\nC\n", "disk untouched");
}

#[test]
fn a_reset_of_several_files_takes_tracked_and_new_ones_together() {
    let Some((repo, dir)) = sample_repository("reset-many") else { return };
    let tracked = repo.tracked_files().into_iter().next().unwrap();
    let before = std::fs::read_to_string(dir.join(&tracked)).unwrap();
    std::fs::write(dir.join(&tracked), "changed\n").unwrap();
    std::fs::write(dir.join("new.txt"), "new\n").unwrap();
    let names = [tracked, "new.txt".to_owned()];
    repo.stage(&names[..1]).unwrap();
    repo.unstage(&names).unwrap();
    repo.discard(&names).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join(&names[0])).unwrap(), before);
    assert!(!dir.join("new.txt").exists());
}

#[test]
fn a_file_goes_back_to_what_it_was_at_a_commit() {
    let Some((repo, dir)) = sample_repository("reset-to") else { return };
    let tracked = repo.tracked_files().into_iter().next().unwrap();
    let before = std::fs::read_to_string(dir.join(&tracked)).unwrap();
    let names = [tracked];
    let first = repo.head_id().unwrap();
    std::fs::write(dir.join(&names[0]), "second\n").unwrap();
    repo.stage(&names).unwrap();
    repo.commit_staged("second").unwrap();
    std::fs::write(dir.join(&names[0]), "not committed\n").unwrap();
    repo.reset_to(&names, &first).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join(&names[0])).unwrap(), before);
    let files = repo.staged_files().unwrap();
    let file = files.iter().find(|f| f.path == names[0]).unwrap();
    assert_eq!((file.staged, file.unstaged), (Some(FileState::Modified), None));
}

#[test]
fn stage_unstage_discard_and_commit_what_is_staged() {
    let Some((repo, dir)) = sample_repository("staging") else { return };
    let tracked = repo.tracked_files().into_iter().next().unwrap();
    std::fs::write(dir.join(&tracked), "changed\n").unwrap();
    std::fs::write(dir.join("[new] file.txt"), "new\n").unwrap();
    let names = [tracked, "[new] file.txt".to_owned()];
    repo.stage(&names).unwrap();
    assert!(
        repo.staged_files().unwrap().iter().all(|f| f.staged.is_some() && f.unstaged.is_none())
    );
    repo.unstage(&names[1..]).unwrap();
    let files = repo.staged_files().unwrap();
    let new = files.iter().find(|f| f.path == names[1]).unwrap();
    assert_eq!((new.staged, new.unstaged), (None, Some(FileState::Untracked)));
    repo.discard(&names[1..]).unwrap();
    assert!(!dir.join(&names[1]).exists(), "a new file discarded is gone");
    let id = repo.commit_staged("only what is staged").unwrap();
    assert_eq!(repo.head_id(), Some(id));
    assert!(repo.staged_files().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_part_of_a_file_is_staged_and_taken_back() {
    let Some((repo, dir)) = sample_repository("patch") else { return };
    std::fs::write(dir.join("part.txt"), "one\ntwo\n").unwrap();
    repo.stage(&["part.txt".to_owned()]).unwrap();
    repo.commit_staged("base").unwrap();
    std::fs::write(dir.join("part.txt"), "one\nTWO\n").unwrap();
    let patch = "--- a/part.txt\n+++ b/part.txt\n@@ -1,2 +1,2 @@\n one\n-two\n+TWO\n";
    repo.apply_patch(patch, PatchTarget::Staged, false).unwrap();
    assert_eq!(repo.text_at(&Revision::Index, "part.txt").as_deref(), Some("one\nTWO\n"));
    repo.apply_patch(patch, PatchTarget::Staged, true).unwrap();
    assert_eq!(repo.text_at(&Revision::Index, "part.txt").as_deref(), Some("one\ntwo\n"));
    repo.apply_patch(patch, PatchTarget::WorkingTree, true).unwrap();
    assert_eq!(std::fs::read_to_string(dir.join("part.txt")).unwrap(), "one\ntwo\n");
    let _ = std::fs::remove_dir_all(dir);
}

/// A change stashed is listed (newest first, with its message); a repository without
/// `.gitmodules` has no submodules.
#[test]
fn stashes_are_listed_and_submodules_come_from_gitmodules() {
    let Some((repo, dir)) = sample_repository("stashes") else { return };
    assert!(repo.stashes().is_empty());
    assert!(repo.submodules().is_empty());
    std::fs::write(dir.join("a.php"), "changed").unwrap();
    run_git(&dir, &["stash", "push", "-q", "-m", "wip cart"], ("Ana", "2026-01-10T10:00:00Z"))
        .expect("stashed");
    let stashes = repo.stashes();
    assert_eq!(stashes.len(), 1);
    assert_eq!(stashes[0].name, "stash@{0}");
    assert!(stashes[0].message.ends_with("wip cart"), "{:?}", stashes[0].message);
    std::fs::write(dir.join(".gitmodules"), "[submodule \"x\"]\n\tpath = libs/x\n\turl = ../x\n")
        .unwrap();
    assert_eq!(repo.submodules(), ["libs/x"]);
    let _ = std::fs::remove_dir_all(&dir);
}
