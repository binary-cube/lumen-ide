//! Switching between the projects open in the app's windows (one each). Every open project
//! has its number while it's open (the lowest free when its window got it: the app's
//! [`workbench_model::ProjectWindow`]) and its shortcut — ⌥⌘1…9 (Ctrl+Alt+1…9 on Windows
//! and Linux; chosen in the keymap) — shown in the top bar after the git branch
//! ([`Badge`]); the shortcut brings that project's window to the front. ⌥⌘P opens their
//! list ([`view`]): filtered as typed, a card each, ↵ or a click brings one to the front —
//! also its button on the right strip (above Keep awake's). A project's number is the
//! user's to choose ([`NUMBER`]: kept with the project; it swaps with the one that had
//! it), from the top bar's badge (a click) or the menu its badge and button share (a right
//! click).

pub mod view;

use std::sync::Arc;

use command_model::{CommandId, CommandSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::{Dock, ToolContent, ToolWindow, ToolWindowProvider, ViewerProvider};
use workbench_model::{
    ChoiceItem, ChoiceList, CommandContribution, CommandHandler, MenuContribution, MenuPlace,
    MenuRow, ProjectWindow, TopBadge, TopBarBadge, WorkbenchHost,
};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("project.switcher"),
    name: "Project switcher",
    description: "Each open project its number and shortcut (⌥⌘1…9), shown in the top bar",
    depends_on: &[],
};

/// Opens the open projects' list.
pub const SWITCHER: CommandId = CommandId("projects.switcher");
/// Chooses this project's number.
pub const NUMBER: CommandId = CommandId("projects.number");
/// Its button on the right strip, and the menu it shares with the top bar's badge.
pub const TOOL: &str = "open-projects";
/// Its button's icon.
pub const ICON: &str = "layers";

/// Brings project N's window to the front (N: 1–9, the numbers the app gives).
pub const GO: [CommandId; 9] = [
    CommandId("projects.go_1"),
    CommandId("projects.go_2"),
    CommandId("projects.go_3"),
    CommandId("projects.go_4"),
    CommandId("projects.go_5"),
    CommandId("projects.go_6"),
    CommandId("projects.go_7"),
    CommandId("projects.go_8"),
    CommandId("projects.go_9"),
];

/// Their shortcuts (by default): ⌥⌘N on macOS, Ctrl+Alt+N elsewhere.
const KEYS: [&[&str]; 9] = [
    &["CmdOrCtrl+Alt+1"],
    &["CmdOrCtrl+Alt+2"],
    &["CmdOrCtrl+Alt+3"],
    &["CmdOrCtrl+Alt+4"],
    &["CmdOrCtrl+Alt+5"],
    &["CmdOrCtrl+Alt+6"],
    &["CmdOrCtrl+Alt+7"],
    &["CmdOrCtrl+Alt+8"],
    &["CmdOrCtrl+Alt+9"],
];

/// Their labels (the keymap's, the palette's).
const LABELS: [&str; 9] = [
    "Go to Project 1",
    "Go to Project 2",
    "Go to Project 3",
    "Go to Project 4",
    "Go to Project 5",
    "Go to Project 6",
    "Go to Project 7",
    "Go to Project 8",
    "Go to Project 9",
];

#[derive(Debug, Default)]
pub struct SwitcherModule;

impl Module for SwitcherModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        for (n, ((id, keys), label)) in GO.iter().zip(KEYS).zip(LABELS).enumerate() {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id: *id, label, menu: None, shortcuts: keys },
                icon: Some("folder"),
                handler: Arc::new(GoTo(n as u8 + 1)),
            });
        }
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: SWITCHER,
                label: "Open Projects…",
                menu: None,
                shortcuts: &["CmdOrCtrl+Alt+P"],
            },
            icon: Some("folder"),
            handler: Arc::new(OpenSwitcher),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: NUMBER,
                label: "Change Project Number…",
                menu: None,
                shortcuts: &[],
            },
            icon: Some("hash"),
            handler: Arc::new(ChooseNumber),
        });
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(view::Provider));
        // Its button on the right strip's end, right above Keep awake's (order 3).
        ctx.contribute::<Arc<dyn ToolWindowProvider>>(Arc::new(ToolWindow {
            id: TOOL,
            icon: ICON,
            title_key: "projects.switcher_title",
            dock: Dock::Right,
            order: 2,
            strip_end: true,
            group: "",
            toggle: Some(SWITCHER.0),
            content: ToolContent::Command { command: SWITCHER.0, tab: None },
        }));
        // Its right-click menu: the button's, and the top bar badge's.
        const STRIP: &[MenuPlace] = &[MenuPlace::ToolStrip];
        ctx.contribute(MenuContribution {
            id: TOOL,
            label_key: "projects.switcher_title",
            icon: ICON,
            places: STRIP,
            rows: vec![MenuRow::Command(SWITCHER, STRIP), MenuRow::Command(NUMBER, STRIP)],
        });
        // This window's project's shortcut, after the git branch.
        ctx.contribute::<Arc<dyn TopBarBadge>>(Arc::new(Badge));
        Ok(())
    }
}

/// The open project numbered `number` (`None`: none has it).
pub fn numbered(projects: &[ProjectWindow], number: u8) -> Option<&ProjectWindow> {
    projects.iter().find(|p| p.number == Some(number))
}

/// The shortcut of project `number` (as the keymap has it now): `⌥⌘1`.
pub fn shortcut_of(number: u8) -> Option<String> {
    let id = GO.get(usize::from(number).checked_sub(1)?)?;
    ui_components::shortcuts::label(id.0)
}

/// [`SWITCHER`]: the list of the windows' projects, as a modal.
struct OpenSwitcher;

impl CommandHandler for OpenSwitcher {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        // This window's picture as it is now (the others' were taken as they lost the
        // focus): on its card once read.
        host.capture_window();
        let data = view::SwitcherData { projects: host.open_projects() };
        host.open_modal(&i18n::t("projects.switcher_title"), view::EXTENSION, &data)
    }
}

/// [`NUMBER`]: the numbers to choose from (which project has each, or free), this
/// project's chosen; the one picked is this project's (they swap).
struct ChooseNumber;

impl CommandHandler for ChooseNumber {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.open_projects().iter().any(|p| p.current)
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let projects = host.open_projects();
        let Some(own) = projects.iter().find(|p| p.current) else { return false };
        let items = (1..=workbench_model::windows::NUMBERED)
            .map(|n| {
                let holder = numbered(&projects, n);
                ChoiceItem {
                    icon: if holder.is_some_and(|p| p.current) { "check" } else { "hash" },
                    label: shortcut_of(n).map_or_else(|| n.to_string(), |k| format!("{n} · {k}")),
                    detail: holder
                        .map_or_else(|| i18n::t("projects.number_free"), |p| p.name.clone()),
                }
            })
            .collect();
        let list = ChoiceList {
            title: i18n::t("projects.number_title"),
            subject: own.name.clone(),
            icon: "hash",
            action: i18n::t("projects.number_action"),
            items,
        };
        host.choose(
            list,
            Box::new(|i, host: &mut dyn WorkbenchHost| {
                if let Ok(number) = u8::try_from(i + 1) {
                    host.set_project_number(number);
                }
            }),
        )
    }
}

/// [`GO`]'s handler: project N's window to the front (enabled while another window has it).
struct GoTo(u8);

impl CommandHandler for GoTo {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        numbered(&host.open_projects(), self.0).is_some_and(|p| !p.current)
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let projects = host.open_projects();
        let Some(project) = numbered(&projects, self.0) else { return false };
        !project.current && host.focus_project(project.window)
    }
}

/// This window's project's shortcut (as the keymap has it now), in the top bar: a click
/// chooses its number, a right click shows its menu.
#[derive(Debug)]
pub struct Badge;

impl TopBarBadge for Badge {
    fn badge(&self, project: &ProjectWindow) -> Option<TopBadge> {
        let number = project.number?;
        Some(TopBadge {
            text: shortcut_of(number)?,
            tooltip: i18n::tr!("projects.badge_tooltip", number = number),
            command: Some(NUMBER),
            menu: Some(TOOL),
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
