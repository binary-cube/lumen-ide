//! The AI agents' sessions, as a module: a window on the left, under the project, listing the
//! project's sessions of every agent on the machine (each agent's module offers it through
//! `agents::AgentProvider`; this one names none) — taken up again or started in a terminal tab
//! of the editor, asked of whoever runs programs in a terminal (`RunInTerminal`: it never
//! names the terminal). Its button shows only while some agent is offered.

mod age;
mod favorites;
mod folders;
mod follow;
mod panel;

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;

use contracts::agents::RenameSession;
use contracts::sleep::SleepStateOf;
use contracts::terminal::{ResumeLaunch, TerminalRunning};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::{Dock, ToolContent, ToolPanel, ToolWindowProvider};

pub use panel::SessionsPanel;

/// Its window on the left.
pub const TOOL: &str = "agent-sessions";
/// Its icon (contributed: a little robot of our own drawing) — its button's and its
/// sessions' tabs'.
pub const ICON: &str = "agent-sessions";
const ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="4" y="8" width="16" height="12" rx="3"/><path d="M12 8V5"/><circle cx="12" cy="4" r="1"/><path d="M9 13v1"/><path d="M15 13v1"/><path d="M10 17h4"/><path d="M2 13v2"/><path d="M22 13v2"/></svg>"##;

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("agents.sessions"),
    name: "Agent Sessions",
    description: "The AI agents' sessions of the project, on the left: taken up again or started in a terminal tab",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "comment", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct SessionsModule;

impl Module for SessionsModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(languages::IconSvg { name: ICON, svg: ICON_SVG });
        ctx.contribute(languages::IconSvg { name: FAVORITE_ICON, svg: FAVORITE_SVG });
        ctx.contribute(languages::IconSvg { name: FAVORITE_ON_ICON, svg: FAVORITE_ON_SVG });
        ctx.contribute(languages::IconSvg { name: UNREAD_ICON, svg: UNREAD_SVG });
        ctx.contribute(workbench_model::CommandContribution {
            spec: command_model::CommandSpec {
                id: FORGET_MISSING,
                label: "Remove Missing Favorite Sessions",
                menu: None,
                shortcuts: &[],
            },
            icon: Some(FAVORITE_ICON),
            handler: Arc::new(ForgetMissing),
        });
        ctx.contribute::<Arc<dyn ToolWindowProvider>>(Arc::new(SessionsTool));
        ctx.contribute::<Arc<dyn workbench_model::ProjectService>>(Arc::new(Renamer));
        // A session's tab renamed by the user: the session too, in its agent's files once the
        // agent ends (nothing typed into it: what the user was writing there stays theirs).
        ctx.contribute(workbench_model::Answers::of(|asked: RenameSession, _host| {
            let title = one_line(&asked.title);
            !title.is_empty() && follow::renamed(&asked.key, &title).is_some()
        }));
        // A session's terminal put to sleep (untouched, idle): how it resumes, its agent's way
        // — only for one whose file has been still as long (no turn running).
        ctx.contribute(workbench_model::Answers::of(|asked: ResumeLaunch, _host| {
            follow::resume_launch(&asked.key, asked.idle)
        }));
        Ok(())
    }
}

/// The window: on the left, right under the project's; its panel made when first shown.
#[derive(Debug)]
struct SessionsTool;

impl ToolWindowProvider for SessionsTool {
    fn id(&self) -> &'static str {
        TOOL
    }

    fn icon(&self) -> &'static str {
        ICON
    }

    fn title(&self) -> String {
        i18n::t("tooltip.agent_sessions")
    }

    fn dock(&self) -> Dock {
        Dock::Left
    }

    /// Right after the project's (the default order).
    fn order(&self) -> u16 {
        110
    }

    fn content(&self) -> ToolContent {
        ToolContent::Panel
    }

    /// Only while some agent offered is installed (none: nothing to list or start).
    fn available(&self) -> bool {
        agents::any(|a| a.installed())
    }

    fn create(&self) -> Option<Box<dyn ToolPanel>> {
        Some(Box::new(SessionsPanel::default()))
    }
}

/// The sessions' tabs renamed as their sessions are (followed: [`follow`]), at a window's
/// poll — asked of the terminal, never named.
#[derive(Debug)]
struct Renamer;

impl workbench_model::ProjectService for Renamer {
    fn attach(&self, project: &workbench_model::ProjectContext<'_>) -> Vec<kernel::Subscription> {
        follow::set_wake(project.id, Arc::clone(&project.wake));
        favorites::set_storage(project.id, project.storage);
        // How its sessions' terminals stand, as they tell it (asleep, on their way, back):
        // the rows and menus follow at once, its window woken to draw them.
        let (wake, id) = (Arc::clone(&project.wake), project.id);
        let states = project.events.subscribe(move |e: &contracts::sleep::SleepStateChanged| {
            follow::told(id, &e.key, e.state);
            wake();
        });
        // The project closed: its window not woken, its favorites let go.
        vec![
            states,
            kernel::Subscription::on_end(move || {
                follow::unset_wake(id);
                follow::take_told(Some(id));
                favorites::release(id);
            }),
        ]
    }

    fn poll(&self, host: &mut dyn workbench_model::WorkbenchHost) -> bool {
        // Names waiting for their agents to end: looked at now and then (a few lookups).
        if follow::any_pending()
            && LOOKED.lock().is_ok_and(|mut at| {
                let due = at.is_none_or(|t: Instant| t.elapsed() >= LOOK_EVERY);
                if due {
                    *at = Some(Instant::now());
                }
                due
            })
        {
            let mut runs =
                |key: &str| host.ask(TerminalRunning { key: key.to_owned() }).unwrap_or(false);
            for (agent, folder, id, title) in follow::due(&mut runs) {
                kernel::work::run("agent-sessions", move || {
                    let renamed = agents::provider(agent)
                        .map_or(Err("no such agent".into()), |a| {
                            a.rename_session(&folder, &id, &title)
                        });
                    if let Err(e) = renamed {
                        tracing::warn!(error = %e, "a session wasn't renamed in its agent");
                    }
                });
            }
        }
        // What each session's terminal holds (its memory, asleep), for the rows: asked now
        // and then while the panel shows (read from the terminal's memory, measured on its
        // thread only while asked).
        // Told by their events since: drawn again.
        let mut states_changed = follow::take_told(host.project_id());
        if follow::panel_shown()
            && STATES.lock().is_ok_and(|mut at| {
                let due = at.is_none_or(|t: Instant| t.elapsed() >= STATE_EVERY);
                if due {
                    *at = Some(Instant::now());
                }
                due
            })
        {
            for key in follow::keys() {
                let state = host.ask(SleepStateOf { key: key.clone() }).flatten();
                states_changed |= follow::set_state(&key, state);
            }
        }
        let renames = follow::take_renames();
        let any = !renames.is_empty() || states_changed;
        for (key, title) in renames {
            let asked = contracts::terminal::RenameTerminal { key: key.clone(), title };
            // No such tab any more (closed): let go.
            if host.ask(asked) != Some(true) {
                follow::forget(&key);
            }
        }
        any
    }
}

/// When the sessions' terminals were last asked what they hold, and how often.
static STATES: Mutex<Option<Instant>> = Mutex::new(None);
const STATE_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// When the names waiting for their agents were last looked at, and how often.
static LOOKED: Mutex<Option<Instant>> = Mutex::new(None);
const LOOK_EVERY: std::time::Duration = std::time::Duration::from_millis(500);

/// A title as given to an agent: one line, no control characters, trimmed, at most 100
/// characters (written into its files: nothing in it may break a line).
pub(crate) fn one_line(title: &str) -> String {
    let line: String = title.chars().filter(|c| !c.is_control()).take(100).collect();
    line.trim().to_owned()
}

/// A favorite's star (contributed: of our own drawing).
pub const FAVORITE_ICON: &str = "agent-favorite";
/// The same star, filled: the favorites' filter on.
pub const FAVORITE_ON_ICON: &str = "agent-favorite-on";
const FAVORITE_ON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000" stroke="#000" stroke-width="2" stroke-linejoin="round"><path d="M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z"/></svg>"##;
const FAVORITE_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linejoin="round"><path d="M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z"/></svg>"##;

/// A session marked unread (its menu's row): a ring, as the dot its row shows.
pub const UNREAD_ICON: &str = "agent-unread";
const UNREAD_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2"><circle cx="12" cy="12" r="5"/></svg>"##;

/// The project's favorites whose sessions are gone, taken off the list (the notification
/// that told of them offers it).
pub const FORGET_MISSING: command_model::CommandId =
    command_model::CommandId("agents.forget_missing_favorites");

struct ForgetMissing;

impl workbench_model::CommandHandler for ForgetMissing {
    fn run(&self, host: &mut dyn workbench_model::WorkbenchHost) -> bool {
        let Some(project) = host.project_id() else { return false };
        favorites::forget_missing(project);
        true
    }
}

/// How many new sessions of each agent were started (their tabs' names apart).
static STARTED: LazyLock<Mutex<HashMap<String, u32>>> = LazyLock::new(Default::default);

/// A new session's tab name: the agent's, then "(2)", "(3)"… — a tab asked for with a name
/// already open is brought to the front, not started again.
pub(crate) fn next_title(agent: &str) -> String {
    let mut started = panel::lock(&STARTED);
    let n = started.entry(agent.to_owned()).or_default();
    *n += 1;
    match *n {
        1 => agent.to_owned(),
        n => format!("{agent} ({n})"),
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
