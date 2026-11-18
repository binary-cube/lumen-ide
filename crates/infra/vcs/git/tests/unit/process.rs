use super::*;

#[test]
fn errors_are_sorted_out_from_what_git_said() {
    let auth =
        "remote: Invalid username or password.\nfatal: Authentication failed for 'https://x'";
    assert!(matches!(classify("push", auth), VcsError::Authentication(m) if m.contains("fatal")));
    assert!(matches!(
        classify(
            "push",
            "git@x: Permission denied (publickey).\nfatal: Could not read from remote"
        ),
        VcsError::Authentication(_)
    ));
    assert_eq!(classify("push", "fatal: No configured push destination."), VcsError::NoRemote);
    assert_eq!(classify("pull", ""), VcsError::Failed("git pull failed".into()));
    assert_eq!(
        classify("pull", "CONFLICT (content)"),
        VcsError::Failed("CONFLICT (content)".into())
    );
}

#[test]
fn the_summary_is_the_last_line_said() {
    let out = Output {
        stdout: "Updating a..b\nFast-forward\n a.php | 2 +-\n".into(),
        stderr: String::new(),
    };
    assert_eq!(out.summary(), "a.php | 2 +-");
    let out = Output { stdout: String::new(), stderr: "Everything up-to-date\n".into() };
    assert_eq!(out.summary(), "Everything up-to-date");
}

#[test]
fn a_command_that_isnt_git_reports_why() {
    // A directory that doesn't exist: spawning fails without panicking.
    let result = run(Path::new("/no/such/dir/for/git"), &["status"], Duration::from_secs(5), true);
    assert!(result.is_err());
}
