//! The app's log read back (plan 89d): a log file (`app-<day>[-<run>][.<part>].jsonl`)
//! opens in its editor tab as a grid of its lines instead of its text — the newest first,
//! filtered by the grid's WHERE, followed live (Pause stops only what's shown), the file
//! before and after a button away. Help → "Show Log" opens today's. The file is read by a
//! thread of the view's own, in pieces, never whole in memory (`reader`); the view keeps
//! only the blocks of rows shown (`source`).

pub mod entry;
pub mod reader;
pub mod source;
pub mod view;

use std::path::Path;
use std::sync::Arc;

use command_model::{CommandId, CommandSpec, MenuAt};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::{Viewer, ViewerKind, ViewerProvider};
use workbench_model::{
    CommandContribution, CommandHandler, ModuleThreads, Notification, WorkbenchHost,
};

/// Help → "Show Log": today's log file in a tab.
pub const SHOW_LOG: CommandId = CommandId("log.show_log");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("log-viewer"),
    name: "Log viewer",
    description: "The app's log read back: its lines in a grid, filtered, followed live",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "file-lines", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct LogViewerModule;

impl Module for LogViewerModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        let help = workbench_model::menus::sections::HELP_TOOLS;
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: SHOW_LOG,
                label: "Show Log",
                menu: Some(MenuAt::new(help, 15)),
                shortcuts: &[],
            },
            icon: Some("file-text"),
            handler: Arc::new(ShowLog),
        });
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(LogFiles));
        ctx.contribute(ModuleThreads(&[source::THREAD]));
        Ok(())
    }
}

/// A log file's name read: its day and its part (0: the day's first); `None`: not one.
pub fn log_name(name: &str) -> Option<(&str, u32)> {
    let rest = name.strip_prefix(logging::files::PREFIX)?.strip_suffix(".jsonl")?;
    let day = rest.get(..10)?;
    let b = day.as_bytes();
    let is_day = b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    if !is_day {
        return None;
    }
    let tail = &rest[10..];
    // `-<run>` (another instance's own) and `.<part>`, both optional.
    let part = match tail.rsplit_once('.') {
        Some((_, n)) => n.parse().ok()?,
        None => 0,
    };
    let own = tail.split('.').next().unwrap_or("");
    if !own.is_empty() && !(own.starts_with('-') && own[1..].bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return None;
    }
    Some((day, part))
}

/// The log's files shown as their lines.
#[derive(Debug)]
struct LogFiles;

impl ViewerProvider for LogFiles {
    fn id(&self) -> &'static str {
        "log-viewer"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        let name = path.file_name()?.to_str()?;
        log_name(name).map(|_| ViewerKind::Replace)
    }

    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(view::LogFileView::new(path)))
    }
}

/// [`SHOW_LOG`]: today's file (its name known from the day: nothing read on a click).
struct ShowLog;

impl CommandHandler for ShowLog {
    fn enabled(&self, _host: &dyn WorkbenchHost) -> bool {
        logging::folder().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(dir) = logging::folder() else {
            host.notify(Notification::info(i18n::t("log_viewer.off")));
            return false;
        };
        let today = logging::line::day_name(logging::layer::now_ms());
        let path = dir.join(logging::files::name(&today, 0, None));
        host.open_file(&path)
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
