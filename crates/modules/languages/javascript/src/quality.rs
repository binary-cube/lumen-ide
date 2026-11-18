//! JavaScript's formatter and checker, for the Code quality context: Prettier (it formats
//! JavaScript and what's written beside it — TypeScript, CSS / SCSS, JSON, YAML, HTML,
//! Vue) and ESLint — never bundled nor downloaded: the project's own
//! (plan 46c). Their package in the project's `node_modules` is run by a Node (the
//! system's, else the app's own), else their `.bin` shim, else one in `PATH`; a file is
//! given on stdin (Prettier prints it formatted, ESLint its problems as JSON); their config
//! (`.prettierrc`, `eslint.config.js`) is theirs to find.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use code_quality::{Kind, Problem, QualityTool, Severity, Target, ToolOutput};
use serde_json::Value;

/// Every JavaScript tool (contributed by the module).
pub fn tools() -> Vec<Arc<dyn QualityTool>> {
    vec![Arc::new(Prettier), Arc::new(Eslint)]
}

/// Prettier.
#[derive(Debug)]
struct Prettier;

impl QualityTool for Prettier {
    fn id(&self) -> &'static str {
        "prettier"
    }
    fn name(&self) -> &'static str {
        "Prettier"
    }
    fn kind(&self) -> Kind {
        Kind::Formatter
    }
    fn languages(&self) -> &'static [&'static str] {
        &["javascript", "typescript", "tsx", "css", "scss", "json", "yaml", "html", "vue"]
    }
    fn candidates(&self) -> &'static [&'static str] {
        &["node_modules/.bin/prettier", "prettier"]
    }
    /// Prettier 3's entry, then 2's.
    fn scripts(&self) -> &'static [&'static str] {
        &["node_modules/prettier/bin/prettier.cjs", "node_modules/prettier/bin-prettier.js"]
    }
    fn stdin(&self) -> bool {
        true
    }
    /// A file: its text on stdin, named for its parser and its config, printed formatted;
    /// the project: every file rewritten in place.
    fn args(&self, target: &Target, _root: &Path) -> Vec<String> {
        match target {
            Target::File(file) => {
                vec!["--stdin-filepath".to_owned(), file.to_string_lossy().into_owned()]
            }
            Target::Project => {
                vec!["--write".to_owned(), "--log-level=warn".to_owned(), ".".to_owned()]
            }
        }
    }
}

/// ESLint.
#[derive(Debug)]
struct Eslint;

impl QualityTool for Eslint {
    fn id(&self) -> &'static str {
        "eslint"
    }
    fn name(&self) -> &'static str {
        "ESLint"
    }
    fn kind(&self) -> Kind {
        Kind::Checker
    }
    fn languages(&self) -> &'static [&'static str] {
        &["javascript", "typescript", "tsx", "vue"]
    }
    fn candidates(&self) -> &'static [&'static str] {
        &["node_modules/.bin/eslint", "eslint"]
    }
    fn scripts(&self) -> &'static [&'static str] {
        &["node_modules/eslint/bin/eslint.js"]
    }
    fn stdin(&self) -> bool {
        true
    }
    /// A file: its text on stdin, named for the config's rules; the project: its folder.
    fn args(&self, target: &Target, _root: &Path) -> Vec<String> {
        match target {
            Target::File(file) => vec![
                "--format=json".to_owned(),
                "--stdin".to_owned(),
                "--stdin-filename".to_owned(),
                file.to_string_lossy().into_owned(),
            ],
            Target::Project => vec!["--format=json".to_owned(), ".".to_owned()],
        }
    }
    /// `[{filePath, messages: [{ruleId, severity (1 warning, 2 error), message, line,
    /// column, fix?}]}]`.
    fn problems(&self, output: &ToolOutput) -> Vec<Problem> {
        let Ok(Value::Array(files)) = serde_json::from_str::<Value>(&output.stdout) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for file in &files {
            let Some(path) = file["filePath"].as_str() else { continue };
            for message in file["messages"].as_array().into_iter().flatten() {
                out.push(Problem {
                    path: PathBuf::from(path),
                    line: message["line"].as_u64().map_or(0, |l| l.saturating_sub(1) as u32),
                    column: message["column"].as_u64().map(|c| c.saturating_sub(1) as u32),
                    severity: if message["severity"].as_u64() == Some(1) {
                        Severity::Warning
                    } else {
                        Severity::Error
                    },
                    message: message["message"].as_str().unwrap_or_default().to_owned(),
                    rule: message["ruleId"].as_str().map(str::to_owned),
                    tool: self.name().to_owned(),
                    fixable: !message["fix"].is_null(),
                });
            }
        }
        out
    }
}

#[cfg(test)]
#[path = "../tests/unit/quality.rs"]
mod tests;
