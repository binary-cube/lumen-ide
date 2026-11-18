//! A profile exported: as text, the way `EXPLAIN` writes a plan with our figures
//! ([`database::profile::export::to_text`]); the engine's own answer as it gave it (its JSON
//! formatted, to take to the usual tools — nothing is ever sent anywhere); our versioned
//! JSON ([`database::profile::export::to_json`], saved through the save dialog); and as
//! Markdown — the summary, the findings and their suggestions, for a pull request or a
//! ticket ([`markdown`]).

use std::fmt::Write as _;

use database::profile::derive::ProfileReport;
use database::profile::rules::Finding;
use database::profile::{RawFormat, export};

use crate::profile::{analysis, figures, summary};

/// What "Export ▾" offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Text,
    EngineJson,
    Json,
    Markdown,
}

impl ExportKind {
    pub const ALL: [ExportKind; 4] =
        [ExportKind::Text, ExportKind::EngineJson, ExportKind::Json, ExportKind::Markdown];

    pub fn id(self) -> &'static str {
        match self {
            ExportKind::Text => "text",
            ExportKind::EngineJson => "engine_json",
            ExportKind::Json => "json",
            ExportKind::Markdown => "markdown",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }

    pub fn label(self) -> String {
        i18n::t(&format!("profile.view.export_{}", self.id()))
    }

    /// It needs something the profile may not have (the engine's answer).
    pub fn available(self, report: &ProfileReport) -> bool {
        self != ExportKind::EngineJson || report.profile.raw.is_some()
    }
}

/// The profile as `EXPLAIN`-like text.
pub fn text(report: &ProfileReport) -> String {
    export::to_text(&report.profile)
}

/// Our JSON (`format` first).
pub fn json(report: &ProfileReport) -> String {
    export::to_json(&report.profile)
}

/// The engine's answer as it came, its JSON formatted (`None`: none kept).
pub fn engine_json(report: &ProfileReport) -> Option<String> {
    let raw = report.profile.raw.as_ref()?;
    if raw.format == RawFormat::Json
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw.text)
        && let Ok(pretty) = serde_json::to_string_pretty(&value)
    {
        return Some(pretty);
    }
    Some(raw.text.to_string())
}

/// The summary, the findings (each with why and what to do) and their SQL, as Markdown.
pub fn markdown(report: &ProfileReport, statement: &str, findings: &[&Finding]) -> String {
    let mut out = String::new();
    let p = &report.profile;
    let mut engine = p.engine.name.clone();
    if let Some(v) = &p.engine.version {
        engine = format!("{engine} {v}");
    }
    let _ = writeln!(out, "## {}", i18n::tr!("profile.view.md_title", engine = engine.trim()));
    let _ = writeln!(out);
    let _ = writeln!(out, "```sql\n{}\n```", statement.trim());
    let _ = writeln!(out);
    for pill in summary::pills(report) {
        let _ = writeln!(out, "- **{}**: {}", pill.label, pill.value);
    }
    let problems: Vec<&&Finding> = findings.iter().filter(|f| f.is_problem()).collect();
    let _ = writeln!(out);
    let _ = writeln!(out, "### {}", i18n::t("profile.view.findings"));
    let _ = writeln!(out);
    if problems.is_empty() {
        let _ = writeln!(out, "{}", i18n::t("profile.view.nothing"));
    }
    for f in findings {
        let text = analysis::texts(f);
        let _ = writeln!(
            out,
            "- **{} · {}** ({}) — {}",
            analysis::severity(f.severity),
            text.title,
            figures::share(f.impact),
            text.why
        );
        if !text.fix.is_empty() {
            let _ = writeln!(out, "  {}", text.fix);
        }
        if let Some(sql) = f.suggestion.as_ref().and_then(|s| s.sql()) {
            let _ = writeln!(out, "\n  ```sql\n  {}\n  ```", sql.replace('\n', "\n  "));
        }
    }
    out
}

/// A file name for our JSON (`profile-orders.json`).
pub fn file_name(title: &str) -> String {
    let slug: String = title
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let slug = slug.split('-').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("-");
    format!("{}.json", if slug.is_empty() { "profile".to_owned() } else { slug })
}

#[cfg(test)]
#[path = "../../tests/unit/profile/export.rs"]
mod tests;
