use super::*;

fn shell(program: &str, args: &[&str]) -> ShellProfile {
    ShellProfile {
        name: "x".into(),
        program: Some(program.into()),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        ..Default::default()
    }
}

/// zsh: our startup files (each reading the user's own), `ZDOTDIR` to them; the user's
/// `ZDOTDIR` kept; its arguments unchanged.
#[test]
fn zsh_gets_startup_files_that_read_the_user_s_first() {
    let themed = themed(&shell("/bin/zsh", &["-l"]), "/tmp/p", Some("/home/u/zsh")).unwrap();
    assert_eq!(themed.profile.env["ZDOTDIR"], "/tmp/p");
    assert_eq!(themed.profile.env["EDITOR_USER_ZDOTDIR"], "/home/u/zsh");
    assert_eq!(themed.profile.args, ["-l"]);
    let names: Vec<&str> = themed.files.iter().map(|(n, _)| *n).collect();
    assert_eq!(names, [".zshenv", ".zprofile", ".zshrc"]);
    let zshrc = &themed.files[2].1;
    let reads = zshrc.find(r#". "$ZDOTDIR/.zshrc""#).unwrap();
    let prompt = zshrc.find("PROMPT=").unwrap();
    let restores = zshrc.find("unset EDITOR_ZDOTDIR").unwrap();
    assert!(reads < prompt && prompt < restores, "theirs, then the prompt, then ZDOTDIR back");
}

#[test]
fn each_known_shell_its_way_others_left_alone() {
    let bash = themed(&shell("/bin/bash", &["-l"]), "/tmp/p", None).unwrap();
    assert_eq!(bash.profile.args, ["--rcfile", "/tmp/p/bashrc", "-i"]);
    assert!(bash.files[0].1.contains(".bash_profile") && bash.files[0].1.contains("PS1="));
    let git_bash =
        themed(&shell(r"C:\Program Files\Git\bin\bash.exe", &["--login", "-i"]), "C:/t", None);
    assert!(git_bash.is_some_and(|t| t.profile.args[0] == "--rcfile"));
    let fish = themed(&shell("/opt/homebrew/bin/fish", &["-l"]), "/tmp/p", None).unwrap();
    assert_eq!(fish.profile.args[..2], ["-l", "-C"]);
    assert!(fish.files.is_empty());
    let pwsh = themed(&shell(r"C:\pwsh\pwsh.exe", &["-NoLogo"]), "C:/t", None).unwrap();
    assert_eq!(pwsh.profile.args[..3], ["-NoLogo", "-NoExit", "-EncodedCommand"]);
    // The project's history kept first (a no-op without one), then the prompt.
    let command = format!("{}; {POWERSHELL_PROMPT}", crate::shell_history::POWERSHELL_HISTORY);
    assert_eq!(pwsh.profile.args[3], encoded(&command));
    let cmd = themed(&shell(r"C:\Windows\System32\cmd.exe", &[]), "C:/t", None).unwrap();
    assert!(cmd.profile.env["PROMPT"].contains("$P"));
    assert_eq!(themed(&shell("/usr/local/bin/nu", &[]), "/tmp/p", None), None);
    assert_eq!(themed(&ShellProfile::default(), "/tmp/p", None), None, "no program: the system's");
}

/// PowerShell's encoded command: UTF-16LE in base64, as `-EncodedCommand` reads it.
#[test]
fn a_powershell_command_is_encoded_as_it_expects() {
    // "dir" → 64 00 69 00 72 00
    assert_eq!(encoded("dir"), "ZABpAHIA");
    assert_eq!(encoded("ls"), "bABzAA==");
    assert_eq!(encoded(""), "");
}
