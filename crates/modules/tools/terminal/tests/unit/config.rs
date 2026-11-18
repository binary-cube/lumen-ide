use super::*;

#[test]
fn a_command_is_its_program_and_arguments_a_quoted_path_kept_whole() {
    assert_eq!(
        split_command("/usr/local/bin/nu -l --stdin"),
        Some(("/usr/local/bin/nu".into(), vec!["-l".into(), "--stdin".into()]))
    );
    assert_eq!(
        split_command(r#""C:\Program Files\Git\bin\bash.exe" --login -i"#),
        Some((r"C:\Program Files\Git\bin\bash.exe".into(), vec!["--login".into(), "-i".into()]))
    );
    assert_eq!(split_command("  fish  "), Some(("fish".into(), vec![])));
    assert_eq!(split_command("   "), None);
    assert_eq!(split_command(r#""unclosed"#), None);
}

/// "Other…" starts its command; written empty, the default shell; a shell id, that one.
#[test]
fn a_new_tab_starts_the_shell_chosen() {
    let custom = TerminalConfig {
        shell: CUSTOM.into(),
        custom: "/opt/shell -i".into(),
        ..TerminalConfig::default()
    };
    let profile = profile(&custom, None, None);
    assert_eq!(profile.program.as_deref(), Some("/opt/shell"));
    assert_eq!(profile.args, ["-i"]);
    let empty = TerminalConfig { shell: CUSTOM.into(), ..TerminalConfig::default() };
    assert_eq!(super::profile(&empty, None, None).program, pty::profile("", None).program);
}

/// In a project, zsh keeps the project's own history (a file of the app's, for that
/// project); the setting off, or a shell keeping none (cmd, an unknown one), as it was.
#[test]
fn a_project_s_shell_keeps_its_own_history() {
    files::places::isolate(
        std::env::temp_dir().join(format!("terminal-history-{}", std::process::id())),
    );
    let zsh = TerminalConfig {
        shell: CUSTOM.into(),
        custom: "/bin/zsh -l".into(),
        ..TerminalConfig::default()
    };
    let root = std::path::Path::new("/work/shop");
    let kept = profile(&zsh, None, Some(root));
    let file = std::path::PathBuf::from(&kept.env["HISTFILE"]);
    assert!(file.ends_with("zsh_history") && file.exists(), "made, ready for the shell");
    assert_ne!(
        profile(&zsh, None, Some(std::path::Path::new("/work/blog"))).env["HISTFILE"],
        kept.env["HISTFILE"]
    );
    let off = TerminalConfig { project_history: false, ..zsh.clone() };
    assert!(profile(&off, None, Some(root)).env.is_empty());
    assert!(profile(&zsh, None, None).env.is_empty(), "outside a project: the usual one");
    let nu = TerminalConfig { custom: "/usr/local/bin/nu".into(), ..zsh };
    assert!(profile(&nu, None, Some(root)).env.is_empty());
}

/// A program a module asked for starts directly (no shell around it), in its own folder or
/// else the one given, with its environment added.
#[test]
fn a_launched_program_starts_as_asked() {
    let mut launch = agents::Launch::new("claude", &["--resume", "id 1"], None);
    launch.env.push(("FORCE_COLOR".into(), "1".into()));
    let profile = launched(&launch, Some(std::path::PathBuf::from("/project")));
    assert_eq!(profile.program.as_deref(), Some("claude"));
    assert_eq!(profile.args, ["--resume", "id 1"]);
    assert_eq!(profile.cwd.as_deref(), Some(std::path::Path::new("/project")));
    assert_eq!(profile.env.get("FORCE_COLOR").map(String::as_str), Some("1"));
    launch.cwd = Some("/elsewhere".into());
    let own = launched(&launch, Some(std::path::PathBuf::from("/project")));
    assert_eq!(own.cwd.as_deref(), Some(std::path::Path::new("/elsewhere")), "its own first");
}

/// By hand (`--ignored`, a Mac or Linux with zsh): a real zsh, with a home of the test's (the
/// user's files never read nor written), keeps a command typed in the project's file — the
/// system's `/etc/zshrc` (macOS sets `HISTFILE` there) read on the way.
#[test]
#[ignore = "runs the system's zsh"]
fn a_real_zsh_keeps_the_project_s_history() {
    use std::io::Write as _;
    let base = std::env::temp_dir().join(format!("terminal-history-{}", std::process::id()));
    files::places::isolate(base.clone());
    let home = base.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let zsh = TerminalConfig {
        shell: CUSTOM.into(),
        custom: "/bin/zsh".into(),
        ..TerminalConfig::default()
    };
    let started = profile(&zsh, None, Some(std::path::Path::new("/work/shop")));
    let file = std::path::PathBuf::from(&started.env[terminal_session::shell_history::HISTORY_VAR]);
    let mut shell = std::process::Command::new("/bin/zsh")
        .arg("-i")
        .env_clear()
        .env("HOME", &home)
        .env("PATH", "/usr/bin:/bin")
        .envs(&started.env)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    // As typed: history kept (the default zsh keeps none without a size).
    let typed = "HISTSIZE=100; SAVEHIST=100; echo typed-in-the-project\nexit\n";
    shell.stdin.take().unwrap().write_all(typed.as_bytes()).unwrap();
    shell.wait().unwrap();
    let kept = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(kept.contains("typed-in-the-project"), "{}: {kept:?}", file.display());
    assert!(!home.join(".zsh_history").exists(), "never the usual one");
}
