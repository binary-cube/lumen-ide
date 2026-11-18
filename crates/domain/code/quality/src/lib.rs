//! The Code quality context: **formatters** (they rewrite code: php-cs-fixer, pint,
//! prettier…) and **checkers** (they report problems: phpcs, phpstan, eslint, stylelint…).
//!
//! A tool is contributed by a language's module ([`QualityTool`]): where it's found, its
//! arguments, how its output reads. What a project uses is its choice ([`Choice`], kept in
//! its own settings): none, a tool found in it, a binary, or a command / script of the
//! project (`./scripts/linux/shell.sh composer cs:fix`). A run is planned here
//! ([`plan::plan`]), run by the infrastructure, its output read here; the runs on a file
//! never overlap ([`runs::Runs`]). A language's
//! module may also bring a formatter of its own ([`TextFormatter`], [`builtin`]), laying
//! the editor's text out itself when no tool is chosen. Pure: no process, no disk — asked
//! through the caller.

pub mod builtin;
pub mod choice;
pub mod plan;
pub mod runs;
pub mod runtime;

use std::path::{Path, PathBuf};

pub use builtin::{FormatRequest, FormatScope, Formatted, TextEdit, TextFormatter};
pub use choice::{Choice, LanguageChoice};
pub use plan::{Invocation, Program, Target, plan};
pub use runs::{Runs, Start};

/// What a tool does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Rewrites the code.
    Formatter,
    /// Reports problems.
    Checker,
}

/// How serious a problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// A problem a checker found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// Relative to the project's root.
    pub path: PathBuf,
    /// 0-based.
    pub line: u32,
    /// 0-based, in characters; `None`: the whole line.
    pub column: Option<u32>,
    pub severity: Severity,
    pub message: String,
    /// The rule broken (`PSR12.Files.EndFileNewline`), if the tool says.
    pub rule: Option<String>,
    /// The tool's name (`PHP_CodeSniffer`).
    pub tool: String,
    /// A formatter can repair it.
    pub fixable: bool,
}

/// What a finished run printed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolOutput {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// A formatter or a checker a language's module knows.
pub trait QualityTool: Send + Sync + std::fmt::Debug {
    /// Stable (saved in a project's settings): `php-cs-fixer`, `phpcs`, `prettier`…
    fn id(&self) -> &'static str;
    /// As shown (`PHP CS Fixer`).
    fn name(&self) -> &'static str;
    fn kind(&self) -> Kind;
    /// The languages it works on (their ids: `php`, `javascript`…).
    fn languages(&self) -> &'static [&'static str];
    /// Where it's looked for, in order: paths in the project (`vendor/bin/pint`), then
    /// program names in `PATH` (`pint`).
    fn candidates(&self) -> &'static [&'static str];
    /// Its arguments for `target` (the file relative to the project, or the whole project),
    /// in the project's folder `root`.
    fn args(&self, target: &Target, root: &Path) -> Vec<String>;
    /// Its program as a Node script in the project (`node_modules/eslint/bin/eslint.js`), in
    /// order: when one is there it's run by Node — the system's, else the app's own (the
    /// language servers') — so it works without Node in `PATH` and without `.bin` shims
    /// (Windows' `.cmd`). None: only [`Self::candidates`].
    fn scripts(&self) -> &'static [&'static str] {
        &[]
    }
    /// A file's run reads the file's text on stdin (its [`Self::args`] name it for the
    /// tool's configuration); a formatter's stdout is then the formatted text, written back
    /// to the file when it differs.
    fn stdin(&self) -> bool {
        false
    }
    /// The problems in its output (a checker's; a formatter's when run to check). The
    /// default: lines `file:line:column: message` (and `file:line: message`).
    fn problems(&self, output: &ToolOutput) -> Vec<Problem> {
        read_lines(output, self.name())
    }
}

/// Problems read from lines `file:line:column: message` or `file:line: message` (the form
/// most tools and scripts print), on stdout then stderr; a message starting with `warning`
/// / `error` (any case) gets that severity, else an error. Lines that don't look so are
/// skipped.
pub fn read_lines(output: &ToolOutput, tool: &str) -> Vec<Problem> {
    output
        .stdout
        .lines()
        .chain(output.stderr.lines())
        .filter_map(|line| read_line(line.trim(), tool))
        .collect()
}

fn read_line(line: &str, tool: &str) -> Option<Problem> {
    // A Windows path's drive (`C:\…`) isn't a field.
    let (drive, line) = match line.as_bytes() {
        [d, b':', b'\\' | b'/', ..] if d.is_ascii_alphabetic() => line.split_at(2),
        _ => ("", line),
    };
    let mut parts = line.splitn(4, ':');
    let path = format!("{drive}{}", parts.next()?.trim());
    let path = path.as_str();
    let line_no: u32 = parts.next()?.trim().parse().ok()?;
    let third = parts.next()?;
    let (column, message) = match third.trim().parse::<u32>() {
        Ok(column) => (Some(column.saturating_sub(1)), parts.next()?.trim().to_owned()),
        // No column: the rest is the message (a `:` in it kept).
        Err(_) => match parts.next() {
            Some(rest) => (None, format!("{third}:{rest}").trim().to_owned()),
            None => (None, third.trim().to_owned()),
        },
    };
    if path.is_empty() || message.is_empty() || line_no == 0 {
        return None;
    }
    let lower = message.to_lowercase();
    let severity = if lower.starts_with("warning") {
        Severity::Warning
    } else if lower.starts_with("info") || lower.starts_with("note") {
        Severity::Info
    } else {
        Severity::Error
    };
    Some(Problem {
        path: PathBuf::from(path),
        line: line_no - 1,
        column,
        severity,
        message,
        rule: None,
        tool: tool.to_owned(),
        fixable: false,
    })
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
