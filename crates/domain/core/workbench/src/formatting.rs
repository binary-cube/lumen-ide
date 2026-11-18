//! A built-in formatter ([`code_quality::TextFormatter`]) run on the active editor's text:
//! its change made as one undo step. A large text is laid out off the UI thread and changed
//! only if it's still the active one, as it was.

use std::sync::Arc;

use code_quality::{FormatRequest, FormatScope, Formatted, TextFormatter};

use crate::WorkbenchHost;

/// Texts at least this long (bytes) are laid out in the background.
pub const IN_BACKGROUND: usize = 256 * 1024;

/// How laying the active text out went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatOutcome {
    /// Changed; `left` parts left as written (unfinished code).
    Changed { left: usize },
    /// Laid out already.
    Unchanged { left: usize },
    /// Nothing to lay out there.
    Nothing,
    /// The text changed (or another tab came) while it was laid out: nothing done.
    Moved,
}

/// Tells the user how it went (the caller's words).
pub type Report = fn(&mut dyn WorkbenchHost, FormatOutcome);

/// Lays the active file's `scope` out with `formatter`, then `report`s; `false` when
/// there's no active file of a language.
pub fn format_active(
    host: &mut dyn WorkbenchHost,
    formatter: Arc<dyn TextFormatter>,
    scope: FormatScope,
    report: Report,
) -> bool {
    let Some(file) = host.active_file() else { return false };
    let Some(language) = host.language_of(&file.path) else { return false };
    let request = FormatRequest {
        project: host.project_id(),
        path: file.path,
        language,
        text: file.text,
        scope,
    };
    if request.text.len() < IN_BACKGROUND {
        let formatted = formatter.format(&request);
        let outcome = apply(host, &request, formatted);
        report(host, outcome);
        return true;
    }
    host.spawn(Box::new(move || {
        let formatted = formatter.format(&request);
        Box::new(move |host: &mut dyn WorkbenchHost| {
            let outcome = apply(host, &request, formatted);
            report(host, outcome);
        })
    }));
    true
}

fn apply(
    host: &mut dyn WorkbenchHost,
    request: &FormatRequest,
    formatted: Formatted,
) -> FormatOutcome {
    match formatted {
        Formatted::Nothing => FormatOutcome::Nothing,
        Formatted::Unchanged { left } => FormatOutcome::Unchanged { left },
        Formatted::Changed { edit, left } => {
            let same = host.active_file().is_some_and(|f| {
                f.path == request.path
                    && (Arc::ptr_eq(&f.text, &request.text) || *f.text == *request.text)
            });
            if same && host.edit_active(edit.range, &edit.text) {
                FormatOutcome::Changed { left }
            } else {
                FormatOutcome::Moved
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/formatting.rs"]
mod tests;
