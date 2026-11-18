use super::*;

/// A system with these files, this login shell and these programs in `PATH`.
struct Fake {
    platform: Platform,
    login: Option<&'static str>,
    files: &'static [&'static str],
    path: &'static [(&'static str, &'static str)],
}

impl ShellProbe for Fake {
    fn platform(&self) -> Platform {
        self.platform
    }
    fn login_shell(&self) -> Option<String> {
        self.login.map(str::to_owned)
    }
    fn exists(&self, path: &str) -> bool {
        self.files.contains(&path)
    }
    fn which(&self, program: &str) -> Option<String> {
        self.path.iter().find(|(p, _)| *p == program).map(|(_, at)| (*at).to_owned())
    }
}

fn ids(shells: &[Shell]) -> Vec<&str> {
    shells.iter().map(|s| s.id.as_str()).collect()
}

#[test]
fn unix_the_user_s_shell_first_then_the_usual_ones_each_once_as_login_shells() {
    let mac = Fake {
        platform: Platform::MacOs,
        login: Some("/opt/homebrew/bin/fish"),
        files: &["/bin/zsh", "/bin/bash", "/opt/homebrew/bin/fish"],
        path: &[],
    };
    let shells = available(&mac);
    assert_eq!(ids(&shells), ["fish", "zsh", "bash"]);
    assert_eq!(shells[0].program, "/opt/homebrew/bin/fish");
    assert!(shells.iter().all(|s| s.args == ["-l"]), "login shells");
    assert_eq!(default_shell(&shells).map(|s| s.id.as_str()), Some("fish"));
    // Found in `PATH` when not in the usual places; a login shell that's gone is skipped.
    let linux = Fake {
        platform: Platform::Linux,
        login: Some("/gone/zsh"),
        files: &["/usr/bin/bash"],
        path: &[("zsh", "/nix/bin/zsh")],
    };
    let shells = available(&linux);
    assert_eq!(ids(&shells), ["zsh", "bash"]);
    assert_eq!(shells[0].program, "/nix/bin/zsh");
}

#[test]
fn windows_powershell_7_first_then_windows_powershell_cmd_and_git_bash() {
    let windows = Fake {
        platform: Platform::Windows,
        login: None,
        files: &[
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
            r"C:\Windows\System32\cmd.exe",
            r"D:\Tools\Git\bin\bash.exe",
        ],
        path: &[("pwsh.exe", r"D:\pwsh\pwsh.exe"), ("git.exe", r"D:\Tools\Git\cmd\git.exe")],
    };
    let shells = available(&windows);
    assert_eq!(ids(&shells), ["pwsh", "powershell", "cmd", "git-bash"]);
    let git_bash = &shells[3];
    assert_eq!(git_bash.program, r"D:\Tools\Git\bin\bash.exe", "next to Git");
    assert_eq!(git_bash.args, ["--login", "-i"]);
    // Without PowerShell 7: Windows PowerShell is the default.
    let plain = Fake {
        platform: Platform::Windows,
        login: None,
        files: &[r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"],
        path: &[],
    };
    assert_eq!(default_shell(&available(&plain)).map(|s| s.id.as_str()), Some("powershell"));
}

#[test]
fn a_chosen_shell_by_its_id_else_the_default_one() {
    let mac = Fake {
        platform: Platform::MacOs,
        login: Some("/bin/zsh"),
        files: &["/bin/zsh", "/bin/bash"],
        path: &[],
    };
    let shells = available(&mac);
    assert_eq!(chosen(&shells, "bash").map(|s| s.id.as_str()), Some("bash"));
    assert_eq!(
        chosen(&shells, "gone").map(|s| s.id.as_str()),
        Some("zsh"),
        "uninstalled: the default"
    );
    let profile = shells[1].profile(Some("/p".into()));
    assert_eq!(profile.program.as_deref(), Some("/bin/bash"));
    assert_eq!(profile.args, ["-l"]);
}
