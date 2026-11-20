//! The terminal, as a module: its window at the bottom, registered like
//! any tool window, each shell a view there (a tab: "Local", "Local (2)"…, "+" for another);
//! the shells start in the project's directory (or the active file's, without a project)
//! and keep running while the window is hidden; reopened at the next start. Its window's
//! menu (its button's right click, its header's "⋮") switches how it behaves and opens its
//! settings.

pub mod colors;
pub mod config;
pub mod settings;
mod view;

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use command_model::{CommandId, CommandSpec};
use contracts::terminal::{RenameTerminal, RunInTerminal, TerminalPlace, TerminalRunning};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use settings_model::Setting;
use ui_components::{
    Dock, ToolContent, ToolWindow, ToolWindowProvider, Viewer, ViewerKind, ViewerProvider,
};
use workbench_model::{
    CommandContribution, CommandHandler, MenuContribution, MenuPlace, MenuRow, ToggleTool,
    WorkbenchHost,
};

pub use view::{TerminalLink, TerminalStart, TerminalView};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("terminal"),
    name: "Terminal",
    description: "The terminal at the bottom: shells as tabs, links to files, the theme's colors",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "terminal", ..extensions::ModuleInfo::BUILT_IN },
};

/// Its window at the bottom.
pub const TOOL: &str = "terminal";
/// A shell's view (its tab's virtual extension).
pub const EXTENSION: &str = "view-terminal";
/// Opens another shell (and shows the window).
pub const NEW: CommandId = CommandId("terminal.new");
/// Empties the history (and the screen above the prompt), whatever programs may do: ⌘K on
/// macOS, Ctrl+Shift+K elsewhere (Ctrl+K is the shell's: it cuts to the end of the line).
pub const CLEAR: CommandId = CommandId("terminal.clear");
/// The view's action for [`CLEAR`].
pub(crate) const CLEAR_ACTION: &str = "clear-history";
/// The shells' viewer (their views' kind).
pub const VIEWER: &str = "terminal.shell";
/// Shows / hides its window (⌥F12, Ctrl+`).
pub const TOGGLE: CommandId = CommandId("view.toggle_terminal");
/// Its window's switches (its menu), each a setting turned the other way.
pub const COPY_ON_SELECT: CommandId = CommandId("terminal.copy_on_select");
pub const HIGHLIGHT_LINKS: CommandId = CommandId("terminal.highlight_links");
pub const MOUSE_REPORTING: CommandId = CommandId("terminal.mouse_reporting");
pub const CLOSE_ON_EXIT: CommandId = CommandId("terminal.close_on_exit");
/// Its page of the settings.
pub const SETTINGS: CommandId = CommandId("terminal.settings");

/// The number of the last shell opened in each project ("Local (n)"), from 1 again once
/// all of its shells are closed.
static NUMBER: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<Option<kernel::ProjectId>, u32>>,
> = std::sync::LazyLock::new(Default::default);

#[derive(Debug, Default)]
pub struct TerminalModule;

impl Module for TerminalModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn ToolWindowProvider>>(Arc::new(ToolWindow {
            id: TOOL,
            icon: "terminal",
            title_key: "tooltip.terminal",
            dock: Dock::Bottom,
            // Above Git, Remote and the local history (in the strip's corner), under the
            // services.
            order: 10,
            strip_end: false,
            group: "",
            toggle: Some(TOGGLE.0),
            content: ToolContent::Views { command: NEW.0, new: Some(NEW.0) },
        }));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(Shells));
        // Its page in the settings: the shell, then how it looks.
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            settings::ShellSettings,
        ));
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            settings::DisplaySettings,
        ));
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            settings::BehaviorSettings,
        ));
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: TOGGLE,
                label: "Terminal",
                // In View › Tool Windows: its tool window's row.
                menu: None,
                shortcuts: &["Alt+F12", "Ctrl+`"],
            },
            icon: Some("terminal"),
            handler: Arc::new(ToggleTool(TOOL)),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: NEW,
                label: "New Terminal Tab",
                menu: Some(command_model::MenuAt::new(
                    workbench_model::menus::sections::TOOLS_DATA,
                    20,
                )),
                shortcuts: &[],
            },
            icon: Some("terminal"),
            handler: Arc::new(NewShell),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: CLEAR,
                label: "Clear Terminal",
                menu: None,
                shortcuts: &["mac:CmdOrCtrl+K", "pc:CmdOrCtrl+Shift+K"],
            },
            icon: Some("terminal"),
            handler: Arc::new(workbench_model::ViewAction { view: VIEWER, action: CLEAR_ACTION }),
        });
        ctx.contribute::<Arc<dyn workbench_model::ResourceReporter>>(Arc::new(Shells));
        // A program another module runs in a terminal (an agent's session): a tab of its
        // own, in the editor or at the bottom; asked again, that tab comes to the front.
        // Such a terminal renamed by its key (its session renamed elsewhere).
        ctx.contribute(workbench_model::Answers::of(|rename: RenameTerminal, _host| {
            view::rename(&rename.key, &rename.title)
        }));
        ctx.contribute(workbench_model::Answers::of(|asked: TerminalRunning, _host| {
            view::running(&asked.key)
        }));
        ctx.contribute(workbench_model::Answers::of(|run: RunInTerminal, host| {
            let start = TerminalStart {
                cwd: run.launch.cwd.clone().or_else(|| host.project_root()),
                project: host.project_id(),
                launch: Some(run.launch),
                icon: run.icon,
                title: Some(run.title),
                key: Some(run.key.clone()),
            };
            // Known by its key (safe as a name), shown by its title.
            let key = contracts::terminal::tab_key(&run.key);
            match run.place {
                TerminalPlace::Editor => host.open_view(&key, EXTENSION, &start),
                TerminalPlace::Bottom => host.show_in_panel(TOOL, &key, EXTENSION, &start),
            }
        }));
        // Its window's switches and its settings: in its menu.
        let switches: [(CommandId, &str, &'static Setting<bool>, &'static str); 4] = [
            (COPY_ON_SELECT, "Copy to Clipboard on Selection", &settings::COPY_ON_SELECT, "copy"),
            (HIGHLIGHT_LINKS, "Highlight Links", &settings::HIGHLIGHT_LINKS, "link"),
            (MOUSE_REPORTING, "Mouse Reporting", &settings::MOUSE_REPORTING, "text-cursor"),
            (CLOSE_ON_EXIT, "Close the Session When It Ends", &settings::CLOSE_ON_EXIT, "log-out"),
        ];
        for (id, label, setting, icon) in switches {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu: None, shortcuts: &[] },
                icon: Some(icon),
                handler: Arc::new(Switch(setting)),
            });
        }
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: SETTINGS,
                label: "Terminal Settings…",
                menu: None,
                shortcuts: &[],
            },
            icon: Some("settings"),
            handler: Arc::new(OpenSettings),
        });
        const MENU: &[MenuPlace] = &[MenuPlace::ToolStrip];
        ctx.contribute(MenuContribution {
            id: TOOL,
            label_key: "tooltip.terminal",
            icon: "terminal",
            places: MENU,
            rows: vec![
                MenuRow::Command(NEW, MENU),
                MenuRow::Command(CLEAR, MENU),
                MenuRow::Separator,
                MenuRow::Command(COPY_ON_SELECT, MENU),
                MenuRow::Command(HIGHLIGHT_LINKS, MENU),
                MenuRow::Command(MOUSE_REPORTING, MENU),
                MenuRow::Command(CLOSE_ON_EXIT, MENU),
                MenuRow::Separator,
                MenuRow::Command(SETTINGS, MENU),
            ],
        });
        // Its keys in the editors' keymaps (as they show their terminal).
        for (preset, shortcuts) in [
            ("jetbrains", &["Alt+F12"][..]),
            ("vscode", &["Ctrl+`"]),
            ("zed", &["Ctrl+`"]),
            ("sublime", &["Ctrl+`"]),
        ] {
            ctx.contribute(command_model::bindings::PresetShortcuts {
                preset,
                command: TOGGLE,
                shortcuts,
            });
        }
        Ok(())
    }
}

/// The Task Manager's action closing a session.
const CLOSE_SESSION: &str = "close-session";

/// The shells' views, and what they cost.
#[derive(Debug)]
struct Shells;

impl ViewerProvider for Shells {
    fn id(&self) -> &'static str {
        VIEWER
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(TerminalView::new()))
    }
}

impl workbench_model::ResourceReporter for Shells {
    fn report(&self) -> Vec<workbench_model::ResourceUse> {
        let count = view::LIVE.load(Ordering::Relaxed);
        let shells = view::shell_pids();
        // No session: nothing to show (the row would say only that).
        if count == 0 && shells.is_empty() {
            return Vec::new();
        }
        let mut part =
            workbench_model::ResourceUse::new(i18n::t("tasks.terminal"), MANIFEST.id.as_str());
        part.threads = vec!["PTY reader", "pty-"];
        part.group = workbench_model::resources::TERMINALS;
        part.detail = i18n::tr!("tasks.terminal_detail", count = count);
        // The shells, and what runs in them.
        part.processes = shells;
        // Each one's project, said on its row (the part is every project's).
        part.process_projects = view::shell_projects();
        // Each session can be closed (asked first: what runs in it ends).
        part.actions = part
            .processes
            .iter()
            .map(|pid| {
                workbench_model::PartAction::new(
                    CLOSE_SESSION,
                    workbench_model::ActionKind::Close,
                    workbench_model::ActionTarget::Process(*pid),
                )
                .confirmed()
            })
            .collect();
        vec![part]
    }

    /// The shells with a program running in them (not those at their prompt): what closes
    /// with them is lost, so asked first; stopped by closing the session.
    fn running(&self) -> Vec<workbench_model::RunningWork> {
        let shells = view::SHELLS.lock().map(|s| s.clone()).unwrap_or_default();
        running_programs(&shells)
    }

    /// A session closed: its view ends, its tab and its shell with it.
    fn act(
        &self,
        _part: &workbench_model::PartRef,
        action: &str,
        target: workbench_model::ActionTarget,
    ) -> workbench_model::ActOutcome {
        match (action, target) {
            (CLOSE_SESSION, workbench_model::ActionTarget::Process(pid))
                if view::close_shell(pid) =>
            {
                workbench_model::ActOutcome::Done
            }
            _ => workbench_model::ActOutcome::Unsupported,
        }
    }
}

/// The programs running in `shells` (each one's foreground, asked now), as work a closing
/// would stop.
fn running_programs(shells: &[view::Shell]) -> Vec<workbench_model::RunningWork> {
    use terminal_session::history::Foreground;
    use workbench_model::running::{StopVia, WorkKind};
    let part = workbench_model::PartRef {
        owner: MANIFEST.id.as_str().to_owned(),
        name: i18n::t("tasks.terminal"),
        project: None,
    };
    shells
        .iter()
        .filter_map(|shell| {
            let watch = shell.watch.as_ref()?;
            let Foreground::Program(age) = watch.foreground() else { return None };
            let program = watch.program();
            let title = program
                .clone()
                .or_else(|| watch.title())
                .unwrap_or_else(|| i18n::t("running.terminal_program"));
            let mut work = workbench_model::RunningWork::new(
                WorkKind::Program,
                title,
                MANIFEST.id.as_str(),
                shell.project,
                workbench_model::StopImpact::LosesWork,
            );
            // The shell's title says more (the command line), when the program is named.
            work.detail = watch
                .title()
                .filter(|t| program.as_ref().is_some_and(|p| t != p))
                .unwrap_or_else(|| i18n::t("running.terminal_detail"));
            work.since = std::time::Instant::now().checked_sub(age);
            work.stop = Some(StopVia {
                part: part.clone(),
                action: CLOSE_SESSION,
                target: workbench_model::ActionTarget::Process(shell.pid),
            });
            Some(work)
        })
        .collect()
}

/// The next shell's tab title in `project`: "Local", "Local (2)"… — past every shell open
/// in it (reopened ones included), from 1 again once all are closed; another project's
/// shells don't count.
fn next_title(project: Option<kernel::ProjectId>) -> String {
    let live = view::live_in(project) as u32;
    let n = NUMBER.lock().map_or(live + 1, |mut all| {
        // A project without shells starts from 1 again: its number isn't kept.
        all.retain(|p, _| *p == project || view::live_in(*p) > 0);
        let last = all.entry(project).or_default();
        if live == 0 {
            *last = 0;
        }
        *last = (*last).max(live) + 1;
        *last
    });
    match n {
        1 => i18n::t("terminal.local"),
        n => i18n::tr!("terminal.local_n", n = n),
    }
}

/// One of its window's switches: the setting as it stands, turned the other way.
struct Switch(&'static Setting<bool>);

impl CommandHandler for Switch {
    fn switched(&self, host: &dyn WorkbenchHost) -> Option<bool> {
        Some(host.user_settings().map_or_else(|| self.0.default_value(), |s| s.get(self.0)))
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(user) = host.user_settings() else { return false };
        settings::flip(user, self.0);
        true
    }
}

/// [`SETTINGS`]: the settings, at its page.
struct OpenSettings;

impl CommandHandler for OpenSettings {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        host.open_settings_page("terminal")
    }
}

/// Another shell, in the project's directory (or the active file's), its window shown.
struct NewShell;

impl CommandHandler for NewShell {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let cwd = host
            .project_root()
            .or_else(|| host.active_file().and_then(|f| f.path.parent().map(Path::to_path_buf)));
        let project = host.project_id();
        let title = next_title(project);
        let start = TerminalStart { cwd, project, ..TerminalStart::default() };
        host.show_in_panel(TOOL, &title, EXTENSION, &start)
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
