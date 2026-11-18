//! SQL laid out in the editor by the domain's formatter (`database::sql::format`), by the
//! document's dialect — a console's source's (told by the consoles' module), else the
//! file's name's — with the options of [`crate::settings`]. "Reformat File" uses it when
//! the project chose no tool for SQL (the selection's statements, else the file); "Reformat
//! Statement" lays out the one at the cursor (or the selection's).

use std::path::Path;
use std::sync::Arc;

use code_quality::builtin::narrowed;
use code_quality::{FormatRequest, FormatScope, Formatted, TextFormatter};
use command_model::CommandId;
use database::sql::{self, Flavor, FormatOptions};
use kernel::ProjectId;
use workbench_model::{CommandHandler, FormatOutcome, Notification, WorkbenchHost};

use crate::Dialect;

/// "Reformat Statement": the statement at the cursor laid out.
pub const FORMAT_STATEMENT: CommandId = CommandId("sql.format_statement");

/// SQL's formatter, for "Reformat File".
#[derive(Debug, Default)]
pub struct SqlFormatter;

impl TextFormatter for SqlFormatter {
    fn languages(&self) -> &'static [&'static str] {
        &["sql"]
    }

    fn format(&self, request: &FormatRequest) -> Formatted {
        lay_out(request, &crate::settings::options())
    }
}

/// The dialect `path` is laid out by: the one a module knows (a console's source's), else
/// its name's.
pub fn flavor(project: Option<ProjectId>, path: &Path) -> Flavor {
    project
        .and_then(|p| database::console::document_flavor(p, path))
        .unwrap_or_else(|| Dialect::of_path(path).flavor())
}

/// `request`'s text laid out with `options`: the change, narrowed to what differs.
pub fn lay_out(request: &FormatRequest, options: &FormatOptions) -> Formatted {
    let flavor = flavor(request.project, &request.path);
    let text = &*request.text;
    let part = |range: std::ops::Range<usize>| sql::format_range(text, range, flavor, options);
    let (range, laid, left) = match &request.scope {
        FormatScope::Whole => {
            let whole = sql::format(text, flavor, options);
            (0..text.len(), whole.text, whole.left)
        }
        FormatScope::Range(range) => match part(range.clone()) {
            Some(p) => (p.range, p.text, p.left),
            None => return Formatted::Nothing,
        },
        FormatScope::At(offset) => match part(*offset..*offset) {
            Some(p) => (p.range, p.text, p.left),
            None => return Formatted::Nothing,
        },
    };
    match narrowed(text, range, &laid) {
        Some(edit) => Formatted::Changed { edit, left },
        None => Formatted::Unchanged { left },
    }
}

/// "Reformat Statement": on a SQL file, the statement at the cursor (or those the selection
/// touches).
#[derive(Debug)]
pub struct FormatStatement;

impl CommandHandler for FormatStatement {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.active_file().is_some_and(|f| host.language_of(&f.path).as_deref() == Some("sql"))
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(file) = host.active_file() else { return false };
        let scope = match file.selection {
            Some(selection) => FormatScope::Range(selection),
            None => FormatScope::At(file.cursor),
        };
        workbench_model::format_active(host, Arc::new(SqlFormatter), scope, report)
    }
}

/// How it went, said when there's something to say.
fn report(host: &mut dyn WorkbenchHost, outcome: FormatOutcome) {
    let message = match outcome {
        FormatOutcome::Changed { left } | FormatOutcome::Unchanged { left } if left > 0 => {
            i18n::tr!("sql.format_left", count = left)
        }
        FormatOutcome::Changed { .. } => return,
        FormatOutcome::Unchanged { .. } => i18n::t("sql.format_unchanged"),
        FormatOutcome::Nothing => i18n::t("sql.format_nothing"),
        FormatOutcome::Moved => i18n::t("sql.format_moved"),
    };
    host.notify(Notification::info(message));
}

#[cfg(test)]
#[path = "../tests/unit/format.rs"]
mod tests;
