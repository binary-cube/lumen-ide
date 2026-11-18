//! CSS's checker, for the Code quality context: Stylelint (formatting is Prettier's, from
//! the JavaScript module). Found in the project (`node_modules/.bin`) first, then in `PATH`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use code_quality::{Kind, Problem, QualityTool, Severity, Target, ToolOutput};
use serde_json::Value;

/// Every CSS tool (contributed by the module).
pub fn tools() -> Vec<Arc<dyn QualityTool>> {
    vec![Arc::new(Stylelint)]
}

/// Stylelint.
#[derive(Debug)]
struct Stylelint;

impl QualityTool for Stylelint {
    fn id(&self) -> &'static str {
        "stylelint"
    }
    fn name(&self) -> &'static str {
        "Stylelint"
    }
    fn kind(&self) -> Kind {
        Kind::Checker
    }
    fn languages(&self) -> &'static [&'static str] {
        &["css", "scss"]
    }
    fn candidates(&self) -> &'static [&'static str] {
        &["node_modules/.bin/stylelint", "stylelint"]
    }
    fn args(&self, target: &Target, _root: &Path) -> Vec<String> {
        let files = match target {
            Target::File(file) => file.to_string_lossy().into_owned(),
            Target::Project => "**/*.{css,scss}".to_owned(),
        };
        vec!["--formatter=json".to_owned(), files]
    }
    /// `[{source, warnings: [{line, column, rule, severity ("error" / "warning"), text}]}]`
    /// (on stdout, or stderr in newer versions).
    fn problems(&self, output: &ToolOutput) -> Vec<Problem> {
        let report = [&output.stdout, &output.stderr]
            .into_iter()
            .find_map(|text| match serde_json::from_str::<Value>(text) {
                Ok(Value::Array(files)) => Some(files),
                _ => None,
            })
            .unwrap_or_default();
        let mut out = Vec::new();
        for file in &report {
            let Some(path) = file["source"].as_str() else { continue };
            for warning in file["warnings"].as_array().into_iter().flatten() {
                out.push(Problem {
                    path: PathBuf::from(path),
                    line: warning["line"].as_u64().map_or(0, |l| l.saturating_sub(1) as u32),
                    column: warning["column"].as_u64().map(|c| c.saturating_sub(1) as u32),
                    severity: if warning["severity"].as_str() == Some("warning") {
                        Severity::Warning
                    } else {
                        Severity::Error
                    },
                    message: warning["text"].as_str().unwrap_or_default().to_owned(),
                    rule: warning["rule"].as_str().map(str::to_owned),
                    tool: self.name().to_owned(),
                    fixable: false,
                });
            }
        }
        out
    }
}

#[cfg(test)]
#[path = "../tests/unit/quality.rs"]
mod tests;
