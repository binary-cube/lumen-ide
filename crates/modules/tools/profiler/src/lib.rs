//! Profiling the app (plan 91), as a module: a recording of what the app itself spends —
//! each thread's CPU (and the module it works for), the memory, the GPU's, the frames, who
//! asked for them and why, what each view took on the interface's thread — so a busy app
//! says which module keeps it so.
//!
//! - **Consent first, every time** ([`consent`]): what's kept and what never is, said; the
//!   length chosen; nothing starts without "Start recording".
//! - **Seen while it runs:** its button on the right strip (above the Task Manager's) lit,
//!   a sign in the status bar with the time left; a click on either stops it. It stops by
//!   itself when its time is up.
//! - **One small JSON file** in the app's data folder, opened in its tab when written
//!   ([`view`]: the figures, the charts, who's busy).
//! - **Anonymous by construction** (`.claude/rules/privacy.md`): the probes take only our
//!   code's names; the collector reads no path, no name, no content.
//!
//! It knows no other module: the names it shows come from the probes and the app's
//! resources (each part's owner), the work from its own crates ([`recording`]).

pub mod consent;
pub mod list;
pub mod recording;
pub mod view;

use std::path::Path;
use std::sync::Arc;

use command_model::{CommandId, CommandSpec, MenuAt};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::{Dock, ToolContent, ToolWindowProvider, Viewer, ViewerKind, ViewerProvider};
use workbench_model::{
    CommandContribution, CommandHandler, Indicator, ModuleThreads, Notification, ProjectContext,
    ProjectService, StatusIndicator, WorkbenchHost,
};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("tools.profiler"),
    name: "Profiler",
    description: "Profiling the app with your consent: what it spends, anonymous, in a small file",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "flame", ..extensions::ModuleInfo::BUILT_IN },
};

/// Records (the consent window first), or stops the recording under way.
pub const RECORD: CommandId = CommandId("profiler.record");
/// The recordings kept, in their tab.
pub const RECORDINGS: CommandId = CommandId("profiler.recordings");
/// Its button's id among the tool windows.
pub const TOOL: &str = "profiler";
/// The extension the consent window's view is opened by.
pub const CONSENT: &str = "view-profiler-consent";
/// Its icon (contributed: Lucide's `circle-dot`).
pub const ICON: &str = "circle-dot";
const ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><circle cx="12" cy="12" r="1"/></svg>"##;

#[derive(Debug, Default)]
pub struct ProfilerModule;

impl Module for ProfilerModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(languages::IconSvg { name: ICON, svg: ICON_SVG });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: RECORD,
                label: "Record a Profile…",
                menu: Some(MenuAt::new(workbench_model::menus::sections::HELP_TOOLS, 12)),
                shortcuts: &[],
            },
            icon: Some(ICON),
            handler: Arc::new(Record),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: RECORDINGS,
                label: "Profile Recordings…",
                menu: Some(MenuAt::new(workbench_model::menus::sections::HELP_TOOLS, 13)),
                shortcuts: &[],
            },
            icon: Some("list"),
            handler: Arc::new(Recordings),
        });
        ctx.contribute::<Arc<dyn ToolWindowProvider>>(Arc::new(Button));
        // Its button's right-click menu: record (or stop), the recordings.
        const STRIP: &[workbench_model::MenuPlace] = &[workbench_model::MenuPlace::ToolStrip];
        ctx.contribute(workbench_model::MenuContribution {
            id: TOOL,
            label_key: "profiler.title",
            icon: ICON,
            places: STRIP,
            rows: vec![
                workbench_model::MenuRow::Command(RECORD, STRIP),
                workbench_model::MenuRow::Command(RECORDINGS, STRIP),
            ],
        });
        ctx.contribute::<Arc<dyn StatusIndicator>>(Arc::new(Recording));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(Views));
        ctx.contribute::<Arc<dyn ProjectService>>(Arc::new(Finished));
        ctx.contribute(ModuleThreads(&[profiling_collect::THREAD, view::THREAD]));
        Ok(())
    }
}

/// [`RECORD`]: the consent window, or the recording stopped.
struct Record;

impl CommandHandler for Record {
    fn switched(&self, _host: &dyn WorkbenchHost) -> Option<bool> {
        Some(recording::recording())
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        if recording::recording() {
            recording::stop();
            return true;
        }
        host.open_modal(&i18n::t("profiler.consent_title"), CONSENT, &())
    }
}

/// [`RECORDINGS`]: their tab, brought to the front if it's open.
struct Recordings;

impl CommandHandler for Recordings {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        if host.focus_view(list::EXTENSION) {
            return true;
        }
        host.open_view(&i18n::t("profiler.recordings"), list::EXTENSION, &())
    }
}

/// Its button: on the right strip's bottom group, right above the Task Manager's (order
/// 15), lit while it records; a click records (the consent first) or stops.
#[derive(Debug)]
struct Button;

impl ToolWindowProvider for Button {
    fn id(&self) -> &'static str {
        TOOL
    }

    fn icon(&self) -> &'static str {
        ICON
    }

    fn title(&self) -> String {
        if recording::recording() { i18n::t("profiler.stop") } else { i18n::t("profiler.title") }
    }

    fn dock(&self) -> Dock {
        Dock::Right
    }

    fn order(&self) -> u16 {
        15
    }

    fn strip_end(&self) -> bool {
        true
    }

    fn toggle_command(&self) -> Option<&'static str> {
        Some(RECORD.0)
    }

    fn content(&self) -> ToolContent {
        ToolContent::Command { command: RECORD.0, tab: None }
    }

    fn active(&self) -> bool {
        recording::recording()
    }

    /// Lit in red while it records (seen at a glance, as a recorder's light).
    fn active_tone(&self) -> Option<ui_components::ds::Tone> {
        Some(ui_components::ds::Tone::Danger)
    }
}

/// Its sign in the status bar while it records: the time left; a click stops it.
#[derive(Debug)]
struct Recording;

impl StatusIndicator for Recording {
    fn indicator(&self) -> Option<Indicator> {
        let (elapsed, left) = recording::progress()?;
        let clock =
            |d: std::time::Duration| format!("{}:{:02}", d.as_secs() / 60, d.as_secs() % 60);
        Some(Indicator {
            icon: ICON,
            label: i18n::tr!("profiler.indicator", left = clock(left).as_str()),
            tooltip: i18n::tr!("profiler.indicator_tip", elapsed = clock(elapsed).as_str()),
            command: Some(RECORD),
        })
    }
}

/// The consent window's view, and the recordings' (their files opened as views).
#[derive(Debug)]
struct Views;

impl ViewerProvider for Views {
    fn id(&self) -> &'static str {
        "profiler"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        if path.extension().is_some_and(|e| e == CONSENT || e == list::EXTENSION) {
            return Some(ViewerKind::Replace);
        }
        // A recording in the app's own folder only: a file of the same name in a project is
        // the user's, opened as usual.
        let name = path.file_name()?.to_str()?;
        let ours = recording::folder().is_some_and(|f| path.parent() == Some(f.as_path()));
        (ours && view::is_recording(name)).then_some(ViewerKind::Replace)
    }

    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        if path.extension().is_some_and(|e| e == CONSENT) {
            return Ok(Box::new(consent::ConsentView::new()));
        }
        if path.extension().is_some_and(|e| e == list::EXTENSION) {
            return Ok(Box::new(list::RecordingsView::new()));
        }
        Ok(Box::new(view::ProfileView::new(path)))
    }
}

/// A recording written: said, and its file opened (in the window whose poll comes first).
#[derive(Debug)]
struct Finished;

impl ProjectService for Finished {
    fn attach(&self, _project: &ProjectContext<'_>) -> Vec<kernel::Subscription> {
        Vec::new()
    }

    fn window_opened(&self, wake: &Arc<dyn Fn() + Send + Sync>) -> Vec<kernel::Subscription> {
        vec![recording::add_wake(Arc::clone(wake))]
    }

    fn poll(&self, host: &mut dyn WorkbenchHost) -> bool {
        // While it records its button is lit red (the time left is in the status bar); a
        // recording written: said, and opened.
        match recording::take_finished() {
            Some(Ok(path)) => {
                tracing::info!("a profile recording written");
                host.notify(Notification::success(i18n::t("profiler.done")));
                host.open_file(&path);
                true
            }
            Some(Err(why)) => {
                tracing::warn!(error = %why, "a profile recording couldn't be written");
                host.notify(Notification::warning(i18n::tr!(
                    "profiler.failed",
                    why = why.as_str()
                )));
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
