//! PHP's formatters and checkers, for the Code quality context: PHP CS Fixer (it formats,
//! and in `--dry-run` it checks), Pint, PHP_CodeSniffer (phpcs checks, phpcbf repairs),
//! PHPStan. Found in the project (`vendor/bin`) first, then in `PATH`; their own config
//! (`.php-cs-fixer.php`, `pint.json`, `phpcs.xml`, `phpstan.neon`) is theirs to find in the
//! project's folder. Their JSON reports read as problems.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use code_quality::{Kind, Problem, QualityTool, Severity, Target, ToolOutput};
use serde_json::Value;

/// Every PHP tool (contributed by the module).
pub fn tools() -> Vec<Arc<dyn QualityTool>> {
    vec![
        Arc::new(CsFixer),
        Arc::new(CsFixerCheck),
        Arc::new(Pint),
        Arc::new(Phpcbf),
        Arc::new(Phpcs),
        Arc::new(Phpstan),
    ]
}

const LANGUAGES: &[&str] = &["php"];

fn file_arg(target: &Target) -> Vec<String> {
    match target {
        Target::File(file) => vec![file.to_string_lossy().into_owned()],
        Target::Project => Vec::new(),
    }
}

/// PHP CS Fixer, fixing.
#[derive(Debug)]
struct CsFixer;

impl QualityTool for CsFixer {
    fn id(&self) -> &'static str {
        "php-cs-fixer"
    }
    fn name(&self) -> &'static str {
        "PHP CS Fixer"
    }
    fn kind(&self) -> Kind {
        Kind::Formatter
    }
    fn languages(&self) -> &'static [&'static str] {
        LANGUAGES
    }
    fn candidates(&self) -> &'static [&'static str] {
        &["vendor/bin/php-cs-fixer", "tools/php-cs-fixer/vendor/bin/php-cs-fixer", "php-cs-fixer"]
    }
    fn args(&self, target: &Target, _root: &Path) -> Vec<String> {
        let mut args = vec!["fix".to_owned(), "--quiet".to_owned()];
        args.extend(file_arg(target));
        args
    }
}

/// PHP CS Fixer, checking (`--dry-run`): the files it would change, each as a problem a
/// formatter repairs.
#[derive(Debug)]
struct CsFixerCheck;

impl QualityTool for CsFixerCheck {
    fn id(&self) -> &'static str {
        "php-cs-fixer"
    }
    fn name(&self) -> &'static str {
        "PHP CS Fixer"
    }
    fn kind(&self) -> Kind {
        Kind::Checker
    }
    fn languages(&self) -> &'static [&'static str] {
        LANGUAGES
    }
    fn candidates(&self) -> &'static [&'static str] {
        CsFixer.candidates()
    }
    fn args(&self, target: &Target, _root: &Path) -> Vec<String> {
        let mut args: Vec<String> = ["fix", "--dry-run", "--format=json"].map(str::to_owned).into();
        args.extend(file_arg(target));
        args
    }
    fn problems(&self, output: &ToolOutput) -> Vec<Problem> {
        let Ok(report) = serde_json::from_str::<Value>(&output.stdout) else { return Vec::new() };
        let files = report["files"].as_array().into_iter().flatten();
        files
            .filter_map(|file| {
                let name = file["name"].as_str()?;
                let rules: Vec<&str> = file["appliedFixers"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                Some(Problem {
                    path: PathBuf::from(name),
                    line: 0,
                    column: None,
                    severity: Severity::Warning,
                    message: i18n::tr!("quality.not_formatted", rules = rules.join(", ")),
                    rule: rules.first().map(|r| (*r).to_owned()),
                    tool: self.name().to_owned(),
                    fixable: true,
                })
            })
            .collect()
    }
}

/// Laravel Pint.
#[derive(Debug)]
struct Pint;

impl QualityTool for Pint {
    fn id(&self) -> &'static str {
        "pint"
    }
    fn name(&self) -> &'static str {
        "Pint"
    }
    fn kind(&self) -> Kind {
        Kind::Formatter
    }
    fn languages(&self) -> &'static [&'static str] {
        LANGUAGES
    }
    fn candidates(&self) -> &'static [&'static str] {
        &["vendor/bin/pint", "pint"]
    }
    fn args(&self, target: &Target, _root: &Path) -> Vec<String> {
        let mut args = vec!["--quiet".to_owned()];
        args.extend(file_arg(target));
        args
    }
}

/// PHP_CodeSniffer's fixer.
#[derive(Debug)]
struct Phpcbf;

impl QualityTool for Phpcbf {
    fn id(&self) -> &'static str {
        "phpcbf"
    }
    fn name(&self) -> &'static str {
        "PHP Code Beautifier"
    }
    fn kind(&self) -> Kind {
        Kind::Formatter
    }
    fn languages(&self) -> &'static [&'static str] {
        LANGUAGES
    }
    fn candidates(&self) -> &'static [&'static str] {
        &["vendor/bin/phpcbf", "phpcbf"]
    }
    fn args(&self, target: &Target, _root: &Path) -> Vec<String> {
        let mut args = vec!["-q".to_owned()];
        args.extend(file_arg(target));
        args
    }
}

/// PHP_CodeSniffer.
#[derive(Debug)]
struct Phpcs;

impl QualityTool for Phpcs {
    fn id(&self) -> &'static str {
        "phpcs"
    }
    fn name(&self) -> &'static str {
        "PHP_CodeSniffer"
    }
    fn kind(&self) -> Kind {
        Kind::Checker
    }
    fn languages(&self) -> &'static [&'static str] {
        LANGUAGES
    }
    fn candidates(&self) -> &'static [&'static str] {
        &["vendor/bin/phpcs", "phpcs"]
    }
    fn args(&self, target: &Target, _root: &Path) -> Vec<String> {
        let mut args: Vec<String> = ["--report=json", "-q"].map(str::to_owned).into();
        args.extend(file_arg(target));
        args
    }
    /// `{"files": {"path": {"messages": [{message, source, severity, type, line, column,
    /// fixable}]}}}`.
    fn problems(&self, output: &ToolOutput) -> Vec<Problem> {
        let Ok(report) = serde_json::from_str::<Value>(&output.stdout) else { return Vec::new() };
        let Some(files) = report["files"].as_object() else { return Vec::new() };
        let mut out = Vec::new();
        for (path, file) in files {
            for message in file["messages"].as_array().into_iter().flatten() {
                let severity = match message["type"].as_str() {
                    Some("WARNING") => Severity::Warning,
                    _ => Severity::Error,
                };
                out.push(Problem {
                    path: PathBuf::from(path),
                    line: line_of(&message["line"]),
                    column: column_of(&message["column"]),
                    severity,
                    message: message["message"].as_str().unwrap_or_default().to_owned(),
                    rule: message["source"].as_str().map(str::to_owned),
                    tool: self.name().to_owned(),
                    fixable: message["fixable"].as_bool().unwrap_or(false),
                });
            }
        }
        out
    }
}

/// PHPStan.
#[derive(Debug)]
struct Phpstan;

impl QualityTool for Phpstan {
    fn id(&self) -> &'static str {
        "phpstan"
    }
    fn name(&self) -> &'static str {
        "PHPStan"
    }
    fn kind(&self) -> Kind {
        Kind::Checker
    }
    fn languages(&self) -> &'static [&'static str] {
        LANGUAGES
    }
    fn candidates(&self) -> &'static [&'static str] {
        &["vendor/bin/phpstan", "phpstan"]
    }
    fn args(&self, target: &Target, _root: &Path) -> Vec<String> {
        let mut args: Vec<String> =
            ["analyse", "--error-format=json", "--no-progress", "--no-interaction"]
                .map(str::to_owned)
                .into();
        args.extend(file_arg(target));
        args
    }
    /// `{"files": {"path": {"messages": [{message, line, identifier}]}}}`.
    fn problems(&self, output: &ToolOutput) -> Vec<Problem> {
        let Ok(report) = serde_json::from_str::<Value>(&output.stdout) else { return Vec::new() };
        let Some(files) = report["files"].as_object() else { return Vec::new() };
        let mut out = Vec::new();
        for (path, file) in files {
            for message in file["messages"].as_array().into_iter().flatten() {
                out.push(Problem {
                    path: PathBuf::from(path),
                    line: line_of(&message["line"]),
                    column: None,
                    severity: Severity::Error,
                    message: message["message"].as_str().unwrap_or_default().to_owned(),
                    rule: message["identifier"].as_str().map(str::to_owned),
                    tool: self.name().to_owned(),
                    fixable: false,
                });
            }
        }
        out
    }
}

/// A 1-based line from a report, 0-based (missing: the first).
fn line_of(value: &Value) -> u32 {
    value.as_u64().map_or(0, |l| l.saturating_sub(1) as u32)
}

/// A 1-based column from a report, 0-based.
fn column_of(value: &Value) -> Option<u32> {
    value.as_u64().map(|c| c.saturating_sub(1) as u32)
}

#[cfg(test)]
#[path = "../tests/unit/quality.rs"]
mod tests;
