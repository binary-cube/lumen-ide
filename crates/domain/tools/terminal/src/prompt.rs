//! The prompt in the theme's colors (an option, off by default): user@host, the directory,
//! the git branch and the prompt's sign, each in an ANSI color — so the theme's palette.
//! Given to the shell when it starts, never by touching the user's files: their own
//! configuration is read first, then only the prompt is set.
//!
//! - zsh: `ZDOTDIR` points to our startup files, each reading the user's own, the last one
//!   puts `ZDOTDIR` back;
//! - bash (Git Bash too): `--rcfile`, which reads the login files first (`PATH`…);
//! - fish: `-C`, run after its configuration;
//! - PowerShell: `-NoExit -EncodedCommand`, after the profile;
//! - cmd: the `PROMPT` variable (the directory and the sign: it can't show variables).
//!
//! Other shells (or a custom command) are left as they are.

use crate::ShellProfile;

/// A shell started with the themed prompt: its profile, and the files to write first
/// (names in the prompt's folder, their text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemedPrompt {
    pub profile: ShellProfile,
    pub files: Vec<(&'static str, String)>,
}

/// The shell's kind, by its program's name.
fn kind(program: &str) -> Option<&'static str> {
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program).to_lowercase();
    let name = name.trim_end_matches(".exe");
    Some(match name {
        "zsh" => "zsh",
        "bash" => "bash",
        "fish" => "fish",
        "pwsh" | "powershell" => "powershell",
        "cmd" => "cmd",
        _ => return None,
    })
}

/// `profile` with the themed prompt, its files to be written in `dir` (a folder of the
/// app's, given as the shell will read it); the user's `ZDOTDIR` when set. `None`: a shell
/// it doesn't know (left as it is).
pub fn themed(
    profile: &ShellProfile,
    dir: &str,
    user_zdotdir: Option<&str>,
) -> Option<ThemedPrompt> {
    let program = profile.program.as_deref()?;
    let mut themed = profile.clone();
    let files = match kind(program)? {
        "zsh" => {
            themed.env.insert("ZDOTDIR".into(), dir.into());
            if let Some(own) = user_zdotdir {
                themed.env.insert("EDITOR_USER_ZDOTDIR".into(), own.into());
            }
            vec![
                (".zshenv", zsh_reads(".zshenv")),
                (".zprofile", zsh_reads(".zprofile")),
                (".zshrc", format!("{}{ZSH_PROMPT}{ZSH_RESTORE}", zsh_reads(".zshrc"))),
            ]
        }
        "bash" => {
            // Not a login shell (that would skip `--rcfile`): the rcfile reads the login
            // files itself.
            themed.args = vec!["--rcfile".into(), format!("{dir}/bashrc"), "-i".into()];
            vec![("bashrc", BASH_RC.into())]
        }
        "fish" => {
            themed.args.retain(|a| a != "-C");
            themed.args.extend(["-C".into(), FISH_PROMPT.into()]);
            Vec::new()
        }
        "powershell" => {
            // Encoded (UTF-16LE, base64): no quoting for the command line to get wrong
            // (Windows PowerShell 5.1 parses quotes in `-Command` its own way).
            themed.args = vec![
                "-NoLogo".into(),
                "-NoExit".into(),
                "-EncodedCommand".into(),
                // The project's history kept too (its own command replaced by this one).
                encoded(&format!(
                    "{}; {POWERSHELL_PROMPT}",
                    crate::shell_history::POWERSHELL_HISTORY
                )),
            ];
            Vec::new()
        }
        "cmd" => {
            themed.env.insert("PROMPT".into(), CMD_PROMPT.into());
            Vec::new()
        }
        _ => return None,
    };
    Some(ThemedPrompt { profile: themed, files })
}

/// zsh with the project's history only (no prompt of ours): started with `ZDOTDIR` at `dir`,
/// whose files read the user's own, then name the history file last — macOS's
/// `/etc/zshrc` sets `HISTFILE` itself, after the environment and before `.zshrc`. `None`:
/// not zsh.
pub fn zsh_history_only(
    profile: &ShellProfile,
    dir: &str,
    user_zdotdir: Option<&str>,
) -> Option<ThemedPrompt> {
    if kind(profile.program.as_deref()?)? != "zsh" {
        return None;
    }
    let mut wrapped = profile.clone();
    wrapped.env.insert("ZDOTDIR".into(), dir.into());
    if let Some(own) = user_zdotdir {
        wrapped.env.insert("EDITOR_USER_ZDOTDIR".into(), own.into());
    }
    let files = vec![
        (".zshenv", zsh_reads(".zshenv")),
        (".zprofile", zsh_reads(".zprofile")),
        (".zshrc", format!("{}{ZSH_RESTORE}", zsh_reads(".zshrc"))),
    ];
    Some(ThemedPrompt { profile: wrapped, files })
}

/// A PowerShell command as `-EncodedCommand` takes it: UTF-16LE, in base64.
pub fn encoded(command: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = command.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// One of our zsh startup files: reads the user's own `name` from their `ZDOTDIR` (or
/// home), keeping a `ZDOTDIR` it may set, then points back to ours.
fn zsh_reads(name: &str) -> String {
    format!(
        r#"EDITOR_ZDOTDIR="$ZDOTDIR"
ZDOTDIR="${{EDITOR_USER_ZDOTDIR:-$HOME}}"
[ -f "$ZDOTDIR/{name}" ] && . "$ZDOTDIR/{name}"
EDITOR_USER_ZDOTDIR="$ZDOTDIR"
ZDOTDIR="$EDITOR_ZDOTDIR"
"#
    )
}

/// zsh's prompt: user@host green, the directory blue, the branch magenta (`vcs_info`).
const ZSH_PROMPT: &str = r#"autoload -Uz vcs_info
zstyle ':vcs_info:git:*' formats ' %F{magenta}(%b)%f'
zstyle ':vcs_info:*' enable git
__editor_prompt_vcs() { vcs_info }
precmd_functions+=(__editor_prompt_vcs)
setopt prompt_subst
PROMPT='%F{green}%n@%m%f %F{blue}%~%f${vcs_info_msg_0_} %F{yellow}%#%f '
"#;

/// Back to the user's `ZDOTDIR` (their `.zlogin`, subshells): unset if it was home.
/// The project's history last (after the user's files, which may set their own).
const ZSH_RESTORE: &str = r#"if [ "$EDITOR_USER_ZDOTDIR" = "$HOME" ]; then unset ZDOTDIR; else ZDOTDIR="$EDITOR_USER_ZDOTDIR"; fi
unset EDITOR_ZDOTDIR EDITOR_USER_ZDOTDIR
[ -n "$EDITOR_SHELL_HISTORY" ] && HISTFILE="$EDITOR_SHELL_HISTORY"
"#;

/// bash: the login files as a login shell reads them (`.bashrc` through them, as usual),
/// then the prompt, and the project's history last.
const BASH_RC: &str = r#"[ -f /etc/profile ] && . /etc/profile
if [ -f "$HOME/.bash_profile" ]; then . "$HOME/.bash_profile"
elif [ -f "$HOME/.bash_login" ]; then . "$HOME/.bash_login"
elif [ -f "$HOME/.profile" ]; then . "$HOME/.profile"
fi
__editor_prompt_git() { local b; b=$(git symbolic-ref --short HEAD 2>/dev/null) && printf ' (%s)' "$b"; }
PS1='\[\e[32m\]\u@\h\[\e[0m\] \[\e[34m\]\w\[\e[35m\]$(__editor_prompt_git)\[\e[0m\] \[\e[33m\]\$\[\e[0m\] '
[ -n "$EDITOR_SHELL_HISTORY" ] && HISTFILE="$EDITOR_SHELL_HISTORY"
"#;

/// fish: after its configuration.
const FISH_PROMPT: &str = "function fish_prompt; set_color green; printf '%s@%s' $USER (prompt_hostname); set_color normal; printf ' '; set_color blue; printf '%s' (prompt_pwd); set_color magenta; printf '%s' (fish_vcs_prompt); set_color yellow; printf ' > '; set_color normal; end";

/// PowerShell (5 and 7): after the profile.
const POWERSHELL_PROMPT: &str = "function global:prompt { $e = [char]27; $b = (git symbolic-ref --short HEAD 2>$null); $g = if ($b) { \" $e[35m($b)\" } else { '' }; \"$e[32m$env:USERNAME@$env:COMPUTERNAME$e[0m $e[34m$($executionContext.SessionState.Path.CurrentLocation)$g$e[0m $e[33m>$e[0m \" }";

/// cmd: its `PROMPT` codes (`$E` escape, `$P` the directory, `$G` `>`; no variables there).
const CMD_PROMPT: &str = "$E[34m$P$E[0m $E[33m$G$E[0m ";

#[cfg(test)]
#[path = "../tests/unit/prompt.rs"]
mod tests;
