use super::*;

fn shell(program: &str, args: &[&str]) -> ShellProfile {
    ShellProfile {
        name: program.into(),
        program: Some(program.into()),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        ..ShellProfile::default()
    }
}

/// zsh and bash keep it in `HISTFILE`, named again for the themed prompt's files.
#[test]
fn zsh_and_bash_are_told_by_histfile() {
    for program in ["/bin/zsh", "/opt/homebrew/bin/bash"] {
        let moved = per_project(&shell(program, &["-l"]), "/data/h/x", "k", None).unwrap();
        assert_eq!(moved.env["HISTFILE"], "/data/h/x");
        assert_eq!(moved.env[HISTORY_VAR], "/data/h/x");
        assert_eq!(moved.args, ["-l"], "started as before");
    }
}

/// Git Bash / MSYS2: the file in their form.
#[test]
fn git_bash_reads_the_path_in_its_own_form() {
    let bash = shell(r"C:\Program Files\Git\bin\bash.exe", &["--login", "-i"]);
    let file = r"C:\Users\Ana Pop\AppData\Roaming\x\terminal-history\k\bash_history";
    let moved = per_project(&bash, file, "k", None).unwrap();
    assert_eq!(
        moved.env["HISTFILE"],
        "/c/Users/Ana Pop/AppData/Roaming/x/terminal-history/k/bash_history"
    );
    assert_eq!(msys_path("relative/x"), None);
    assert_eq!(msys_path("C:x"), None, "a drive's current folder isn't a path of it");
}

/// fish: a session of the project's name; the file is fish's own.
#[test]
fn fish_gets_a_session_of_its_own() {
    let moved = per_project(&shell("/usr/bin/fish", &["-l"]), "", "00ab", None).unwrap();
    assert_eq!(moved.env["fish_history"], "project_00ab");
    assert!(!moved.env.contains_key("HISTFILE"));
}

/// PowerShell: PSReadLine's path set after the profile (never skipped), the shell kept open.
#[test]
fn powershell_sets_psreadline_s_path_after_its_profile() {
    let moved = per_project(&shell("pwsh.exe", &["-NoLogo"]), r"C:\h\ps.txt", "k", None).unwrap();
    assert_eq!(moved.env[HISTORY_VAR], r"C:\h\ps.txt");
    assert_eq!(moved.args[..3], ["-NoLogo", "-NoExit", "-EncodedCommand"]);
    assert_eq!(moved.args[3], crate::prompt::encoded(POWERSHELL_HISTORY));
    assert!(!moved.args.iter().any(|a| a.eq_ignore_ascii_case("-NoProfile")));
}

/// WSL: `HISTFILE` passed into Linux (its path translated), beside what `WSLENV` had.
#[test]
fn wsl_is_passed_the_file_by_wslenv() {
    let wsl = shell(r"C:\Windows\System32\wsl.exe", &[]);
    let moved = per_project(&wsl, r"C:\h\bash_history", "k", None).unwrap();
    assert_eq!(moved.env["WSLENV"], "HISTFILE/p");
    let kept = per_project(&wsl, r"C:\h\bash_history", "k", Some("USERPROFILE/p")).unwrap();
    assert_eq!(kept.env["WSLENV"], "USERPROFILE/p:HISTFILE/p");
    let twice = per_project(&wsl, r"C:\h\bash_history", "k", Some("HISTFILE/p")).unwrap();
    assert_eq!(twice.env["WSLENV"], "HISTFILE/p", "not named twice");
}

/// cmd keeps none between sessions; an unknown shell, or none named, stays as it is.
#[test]
fn a_shell_without_a_history_of_its_own_is_left_alone() {
    assert_eq!(per_project(&shell("cmd.exe", &[]), "x", "k", None), None);
    assert_eq!(per_project(&shell("/usr/local/bin/nu", &[]), "x", "k", None), None);
    assert_eq!(per_project(&ShellProfile::default(), "x", "k", None), None);
    assert_eq!(file_name(kind("cmd.exe")), None);
}

/// A project's key: the same for its root every time, another for another root.
#[test]
fn each_project_has_its_own_key() {
    let shop = project_key("/Users/ana/work/shop");
    assert_eq!(shop, project_key("/Users/ana/work/shop"));
    assert_ne!(shop, project_key("/Users/ana/work/blog"));
    assert_eq!(shop.len(), 16);
    assert!(shop.chars().all(|c| c.is_ascii_hexdigit()), "a name fish takes too");
}
