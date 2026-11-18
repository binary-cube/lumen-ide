//! The shells a terminal can start: which ones a system has (the user's own first, then the
//! usual ones), how each starts (a login shell on Unix, Git Bash with `--login -i`), and the
//! default. Pure: what's on disk is asked through [`ShellProbe`].

use std::path::Path;

use crate::ShellProfile;

/// The system the shells are looked for on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Linux,
    Windows,
}

/// What the system has, asked by the detection (implemented in infrastructure).
pub trait ShellProbe {
    fn platform(&self) -> Platform;
    /// The user's login shell (`$SHELL`, else the account's), on Unix.
    fn login_shell(&self) -> Option<String>;
    /// `path` is an existing file.
    fn exists(&self, path: &str) -> bool;
    /// `program`'s path, found in `PATH`.
    fn which(&self, program: &str) -> Option<String>;
}

/// A shell found on the system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    /// Stable across starts (saved in the settings): `zsh`, `bash`, `fish`, `pwsh`,
    /// `powershell`, `cmd`, `git-bash`… — the program's name, not its path.
    pub id: String,
    /// As shown (`zsh`, `PowerShell 7`, `Git Bash`).
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
}

impl Shell {
    /// How a tab starts it, in `cwd`.
    pub fn profile(&self, cwd: Option<std::path::PathBuf>) -> ShellProfile {
        ShellProfile {
            name: self.name.clone(),
            program: Some(self.program.clone()),
            args: self.args.clone(),
            cwd,
            env: Default::default(),
        }
    }
}

/// Where the usual Unix shells live (Homebrew's too), by name.
const UNIX_SHELLS: &[(&str, &[&str])] = &[
    ("zsh", &["/bin/zsh", "/usr/bin/zsh", "/opt/homebrew/bin/zsh", "/usr/local/bin/zsh"]),
    ("bash", &["/bin/bash", "/usr/bin/bash", "/opt/homebrew/bin/bash", "/usr/local/bin/bash"]),
    ("fish", &["/opt/homebrew/bin/fish", "/usr/local/bin/fish", "/usr/bin/fish"]),
];

/// The shells the system has: on Unix the user's own first, then zsh, bash, fish (each once,
/// started as login shells); on Windows PowerShell 7, Windows PowerShell, cmd, Git Bash.
pub fn available(probe: &dyn ShellProbe) -> Vec<Shell> {
    match probe.platform() {
        Platform::MacOs | Platform::Linux => unix_shells(probe),
        Platform::Windows => windows_shells(probe),
    }
}

/// The one started unless another is chosen: the first found (the user's own on Unix,
/// PowerShell 7 if installed on Windows, else Windows PowerShell).
pub fn default_shell(shells: &[Shell]) -> Option<&Shell> {
    shells.first()
}

/// The shell chosen by its id, else the default one.
pub fn chosen<'a>(shells: &'a [Shell], id: &str) -> Option<&'a Shell> {
    shells.iter().find(|s| s.id == id).or_else(|| default_shell(shells))
}

fn program_name(path: &str) -> String {
    let name = Path::new(path).file_stem().map_or(path.into(), |n| n.to_string_lossy());
    name.trim_start_matches('-').to_lowercase()
}

fn unix_shells(probe: &dyn ShellProbe) -> Vec<Shell> {
    let login = |id: String, program: String| Shell {
        name: id.clone(),
        id,
        program,
        // A login shell, as a terminal app starts it: the profile files read (`PATH`…).
        args: vec!["-l".into()],
    };
    let mut shells = Vec::new();
    if let Some(own) = probe.login_shell().filter(|s| probe.exists(s)) {
        shells.push(login(program_name(&own), own));
    }
    for (id, paths) in UNIX_SHELLS {
        if shells.iter().any(|s| s.id == *id) {
            continue;
        }
        let found = paths
            .iter()
            .map(|p| (*p).to_owned())
            .find(|p| probe.exists(p))
            .or_else(|| probe.which(id));
        if let Some(program) = found {
            shells.push(login((*id).into(), program));
        }
    }
    shells
}

fn windows_shells(probe: &dyn ShellProbe) -> Vec<Shell> {
    let shell = |id: &str, name: &str, program: String, args: &[&str]| Shell {
        id: id.into(),
        name: name.into(),
        program,
        args: args.iter().map(|a| (*a).to_owned()).collect(),
    };
    let first = |paths: &[&str], program: &str| {
        paths
            .iter()
            .map(|p| (*p).to_owned())
            .find(|p| probe.exists(p))
            .or_else(|| probe.which(program))
    };
    let mut shells = Vec::new();
    if let Some(p) = first(&[r"C:\Program Files\PowerShell\7\pwsh.exe"], "pwsh.exe") {
        shells.push(shell("pwsh", "PowerShell 7", p, &["-NoLogo"]));
    }
    if let Some(p) =
        first(&[r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"], "powershell.exe")
    {
        shells.push(shell("powershell", "Windows PowerShell", p, &["-NoLogo"]));
    }
    if let Some(p) = first(&[r"C:\Windows\System32\cmd.exe"], "cmd.exe") {
        shells.push(shell("cmd", "Command Prompt", p, &[]));
    }
    // Git Bash: next to Git (its `bin\bash.exe`), a login shell, interactive.
    // (By text: a Windows path, whatever system reads it.)
    let next_to_git = probe.which("git.exe").and_then(|git| {
        let parent = |p: &str| p.rfind(['\\', '/']).map(|i| p[..i].to_owned());
        let root = parent(&parent(&git)?)?;
        let bash = format!(r"{root}\bin\bash.exe");
        probe.exists(&bash).then_some(bash)
    });
    let git_bash =
        [r"C:\Program Files\Git\bin\bash.exe", r"C:\Program Files (x86)\Git\bin\bash.exe"]
            .iter()
            .map(|p| (*p).to_owned())
            .find(|p| probe.exists(p))
            .or(next_to_git);
    if let Some(p) = git_bash {
        shells.push(shell("git-bash", "Git Bash", p, &["--login", "-i"]));
    }
    shells
}

#[cfg(test)]
#[path = "../tests/unit/shells.rs"]
mod tests;
