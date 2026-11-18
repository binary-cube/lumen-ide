//! Each project's own command history: a shell started in a project keeps what's typed in
//! a file of that project's (in the app's data folder, never in the project), so ↑ and
//! Ctrl+R bring back its commands, not every terminal's. How each shell is told:
//!
//! - zsh, bash: `HISTFILE` (Oh My Zsh and the usual configurations keep it when it's set);
//!   Git Bash / MSYS2 read it as `/c/Users/…`;
//! - fish: `fish_history`, a session name (fish keeps the file itself, beside its own);
//! - PowerShell (5 and 7): PSReadLine's `-HistorySavePath`, set by a command after the
//!   user's profile;
//! - WSL: `HISTFILE` passed into Linux by `WSLENV` (`/p`: its path translated);
//! - cmd keeps no history between sessions; another shell stays as it is.
//!
//! [`HISTORY_VAR`] names the file too: the themed prompt's startup files set it again after
//! the user's own (a `.zshrc` setting `HISTFILE` itself doesn't win then).

use crate::ShellProfile;

/// The history file, as the shell reads it (the themed prompt's files use it last).
pub const HISTORY_VAR: &str = "EDITOR_SHELL_HISTORY";

/// PowerShell: the history kept in that file, when PSReadLine is there (after the profile).
pub const POWERSHELL_HISTORY: &str = "if ($env:EDITOR_SHELL_HISTORY -and (Get-Command Set-PSReadLineOption -ErrorAction SilentlyContinue)) { Set-PSReadLineOption -HistorySavePath $env:EDITOR_SHELL_HISTORY }";

/// How a shell keeps its history, by its program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Zsh,
    Bash,
    Fish,
    PowerShell,
    Wsl,
    /// cmd (none between sessions), or a shell it doesn't know.
    Other,
}

/// The shell's kind, by its program's name (a path, `.exe` or not).
pub fn kind(program: &str) -> Kind {
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program).to_lowercase();
    match name.trim_end_matches(".exe") {
        "zsh" => Kind::Zsh,
        "bash" => Kind::Bash,
        "fish" => Kind::Fish,
        "pwsh" | "powershell" => Kind::PowerShell,
        "wsl" => Kind::Wsl,
        _ => Kind::Other,
    }
}

/// The file a kind's history is kept in, in the project's folder (`None`: fish keeps its
/// own; the others keep none).
pub fn file_name(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::Zsh => Some("zsh_history"),
        Kind::Bash | Kind::Wsl => Some("bash_history"),
        Kind::PowerShell => Some("powershell_history.txt"),
        Kind::Fish | Kind::Other => None,
    }
}

/// The project's key (its folder's name in the app's data; fish's session name): its root's
/// path hashed (FNV-1a, 64 bits) — the same every start, and no path in a file name.
pub fn project_key(root: &str) -> String {
    let hash = root.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

/// `profile` keeping its history in `file` (the path as the app writes it; empty for fish),
/// `key` the project's (fish's session), `wslenv` the app's own `WSLENV`. `None`: a shell
/// that keeps no history of its own to move (left as it is).
pub fn per_project(
    profile: &ShellProfile,
    file: &str,
    key: &str,
    wslenv: Option<&str>,
) -> Option<ShellProfile> {
    let program = profile.program.as_deref()?;
    let mut moved = profile.clone();
    let mut set = |name: &str, value: String| {
        moved.env.insert(name.to_owned(), value);
    };
    match kind(program) {
        Kind::Zsh => {
            set("HISTFILE", file.to_owned());
            set(HISTORY_VAR, file.to_owned());
        }
        Kind::Bash => {
            // Git Bash / MSYS2 on Windows: a drive's path in their form.
            let file = msys_path(file).unwrap_or_else(|| file.to_owned());
            set("HISTFILE", file.clone());
            set(HISTORY_VAR, file);
        }
        Kind::Fish => set("fish_history", format!("project_{key}")),
        Kind::PowerShell => {
            set(HISTORY_VAR, file.to_owned());
            // After the user's profile (never skipped), the shell staying open.
            moved.args.retain(|a| !a.eq_ignore_ascii_case("-NoExit"));
            moved.args.extend([
                "-NoExit".into(),
                "-EncodedCommand".into(),
                crate::prompt::encoded(POWERSHELL_HISTORY),
            ]);
        }
        Kind::Wsl => {
            set("HISTFILE", file.to_owned());
            let passed = match wslenv.filter(|w| !w.is_empty()) {
                Some(own) if own.split(':').any(|v| v.starts_with("HISTFILE")) => own.to_owned(),
                Some(own) => format!("{own}:HISTFILE/p"),
                None => "HISTFILE/p".to_owned(),
            };
            set("WSLENV", passed);
        }
        Kind::Other => return None,
    }
    Some(moved)
}

/// `C:\Users\a b\x` as MSYS writes it (`/c/Users/a b/x`); `None`: not a drive's path.
pub fn msys_path(path: &str) -> Option<String> {
    let mut chars = path.chars();
    let drive = chars.next().filter(char::is_ascii_alphabetic)?;
    let rest = chars.as_str().strip_prefix(':')?;
    if !rest.starts_with(['\\', '/']) {
        return None;
    }
    Some(format!("/{}{}", drive.to_ascii_lowercase(), rest.replace('\\', "/")))
}

#[cfg(test)]
#[path = "../tests/unit/shell_history.rs"]
mod tests;
