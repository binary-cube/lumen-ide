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
    let profile = profile(&custom, None);
    assert_eq!(profile.program.as_deref(), Some("/opt/shell"));
    assert_eq!(profile.args, ["-i"]);
    let empty = TerminalConfig { shell: CUSTOM.into(), ..TerminalConfig::default() };
    assert_eq!(super::profile(&empty, None).program, pty::profile("", None).program);
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
