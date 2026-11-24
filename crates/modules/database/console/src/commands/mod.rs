//! What the console's commands do, through the host (the active file's text, its cursor and
//! selection; asking; opening a console's file): run the statement under the cursor, the
//! selection or the whole script; stop it; explain it; commit or roll back; and "Jump to
//! Query Console…" — the source's consoles listed (a new one first), or a new one made
//! straight away when it has none; one opened on a table gets its schema and, empty, a
//! `SELECT * FROM` it with the name selected.

use std::sync::Arc;

use database::DataSourceId;
use database::console::{
    self, ConsoleAccess, ConsoleId, ConsoleSpec, PlanInput, PlannedStatement, RunMode, RunPlan,
    SourceBrief,
};
use workbench_model::{ChoiceItem, ChoiceList, CommandHandler, Notification, WorkbenchHost};

use crate::consoles::{Consoles, consoles};

mod attach;
mod explain;
mod guarded;
mod open_console;
mod values;

pub use attach::{AttachCommand, DetachCommand, OnSqlFile, attach, attachable_active, detach};
pub use explain::{PROFILE_ON_PRODUCTION, explain};
pub use open_console::{jump, jump_picked, open_for};
pub use values::edit_params;

use guarded::guarded;
use values::with_values;

/// The console of the active tab: its project's consoles and its spec.
pub fn active(host: &dyn WorkbenchHost) -> Option<(Arc<Consoles>, ConsoleSpec)> {
    let file = host.active_file()?;
    let consoles = consoles(host.project_id()?)?;
    let spec = consoles.spec_of_path(&file.path)?;
    Some((consoles, spec))
}

/// The source's port and its brief; why not, told.
fn source_of(
    host: &mut dyn WorkbenchHost,
    consoles: &Consoles,
    id: DataSourceId,
) -> Option<(Arc<dyn ConsoleAccess>, SourceBrief)> {
    let Some(access) = consoles.access() else {
        host.notify(Notification::warning(i18n::t("console.no_sources")));
        return None;
    };
    let Some(brief) = access.source(id) else {
        host.notify(Notification::warning(i18n::t("console.source_gone")));
        return None;
    };
    if !brief.dialect {
        host.notify(Notification::info(i18n::tr!("console.no_sql", name = brief.name.as_str())));
        return None;
    }
    Some((access, brief))
}

/// The active console read for a run: its consoles, its spec, its source's port and brief,
/// the plan, the text it was made from.
type Planned = (Arc<Consoles>, ConsoleSpec, Arc<dyn ConsoleAccess>, SourceBrief, RunPlan, Arc<str>);

/// How "Run" reads the active console: its plan, or why nothing runs.
fn planned(host: &mut dyn WorkbenchHost, script: bool) -> Option<Planned> {
    let file = host.active_file()?;
    let (consoles, spec) = active(host)?;
    let (access, brief) = source_of(host, &consoles, spec.source)?;
    let mode = if script { RunMode::Script } else { spec.mode };
    let plan = console::plan(&PlanInput {
        text: &file.text,
        selection: file.selection.clone(),
        cursor: file.cursor,
        mode,
        flavor: brief.flavor,
        options: Default::default(),
    });
    Some((consoles, spec, access, brief, plan, Arc::clone(&file.text)))
}

/// Runs the active console's statements (`script`: all of them); none under the cursor:
/// which one, asked.
pub fn run(host: &mut dyn WorkbenchHost, script: bool) -> bool {
    let Some((consoles, spec, access, brief, plan, text)) = planned(host, script) else {
        return false;
    };
    let subject = spec.name.clone();
    let target = Target { consoles, spec, access, brief, document: Arc::clone(&text) };
    match plan {
        RunPlan::Nothing => {
            host.notify(Notification::info(i18n::t("console.nothing_to_run")));
            false
        }
        RunPlan::Run(statements) => start(host, &target, statements, false),
        RunPlan::Choose(statements) => {
            let items = statements
                .iter()
                .map(|s| ChoiceItem {
                    icon: if s.writes() { "file-pen" } else { "play" },
                    label: console::summary(&s.text, 80),
                    detail: i18n::tr!(
                        "console.statement_line",
                        line = text[..s.range.start].lines().count().max(1)
                    ),
                })
                .collect();
            let list = ChoiceList {
                title: i18n::t("console.choose_statement"),
                subject,
                icon: "play",
                action: i18n::t("console.choose_run"),
                items,
            };
            host.choose(
                list,
                Box::new(move |i, host| {
                    if let Some(statement) = statements.get(i).cloned() {
                        start(host, &target, vec![statement], false);
                    }
                }),
            )
        }
    }
}

/// What a run needs of its console, whatever it sends.
#[derive(Clone)]
struct Target {
    consoles: Arc<Consoles>,
    spec: ConsoleSpec,
    access: Arc<dyn ConsoleAccess>,
    brief: SourceBrief,
    document: Arc<str>,
}

/// Starts `statements` on the console's runner: a read-only source's write refused first
/// (said in the Output and a toast), then their parameters' values asked (the module's
/// modal), then their dangers and a production write made permanent at once confirmed.
/// `detached`: they aren't the console's text (a statement of the history run again).
fn start(
    host: &mut dyn WorkbenchHost,
    target: &Target,
    statements: Vec<PlannedStatement>,
    detached: bool,
) -> bool {
    let Target { consoles, spec, brief, .. } = target;
    if let Some(i) = console::refused(&statements, brief.read_only) {
        let what = console::summary(&statements[i].text, 40);
        let why =
            i18n::tr!("console.read_only_refused", name = brief.name.as_str(), statement = what);
        consoles.runner(spec.id, spec.source).refused(why.clone());
        host.notify(Notification::warning(why));
        return false;
    }
    let next = target.clone();
    with_values(
        host,
        crate::params::Purpose::Run,
        target,
        statements,
        Box::new(move |host, statements| guarded(host, &next, statements, detached)),
    )
}

/// "Query History…" (the bar's 🕘, ⌥⌘E): the window of the statements run on the active
/// console's source (read in the background the first time).
pub fn history(host: &mut dyn WorkbenchHost) -> bool {
    let Some((consoles, spec)) = active(host) else { return false };
    let Some(project) = host.project_id() else { return false };
    let brief = consoles.access().and_then(|a| a.source(spec.source));
    let request = crate::history_view::HistoryRequest {
        project,
        console: spec.id,
        console_name: spec.name.clone(),
        source: spec.source,
        source_name: brief.as_ref().map(|b| b.name.clone()).unwrap_or_default(),
        color: brief.and_then(|b| b.color),
        file: spec.path(consoles.root()),
    };
    // A broken file kept aside is said once.
    for aside in consoles.history().take_broken() {
        host.notify(Notification::warning(i18n::tr!(
            "console.history_broken",
            file = aside.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        )));
    }
    host.open_modal(&i18n::t("console.history_title"), crate::history_view::EXTENSION, &request)
}

/// `text` (a statement of the history: perhaps several) run as it is on console `id`'s
/// connection — not the console's text: nothing marked or underlined in it —, by the same
/// rules as Run (a read-only source's write refused, its parameters asked, its dangers and a
/// production write confirmed).
pub fn run_text(host: &mut dyn WorkbenchHost, id: ConsoleId, text: &str) -> bool {
    let Some(consoles) = host.project_id().and_then(consoles) else { return false };
    let Some(spec) = consoles.spec(id) else {
        host.notify(Notification::warning(i18n::t("console.gone")));
        return false;
    };
    let Some((access, brief)) = source_of(host, &consoles, spec.source) else { return false };
    let document: Arc<str> = Arc::from(text);
    let plan = console::plan(&PlanInput {
        text: &document,
        selection: None,
        cursor: 0,
        mode: RunMode::Script,
        flavor: brief.flavor,
        options: Default::default(),
    });
    match plan {
        RunPlan::Run(statements) if !statements.is_empty() => {
            let target = Target { consoles, spec, access, brief, document };
            start(host, &target, statements, true)
        }
        _ => {
            host.notify(Notification::info(i18n::t("console.nothing_to_run")));
            false
        }
    }
}

/// "Open in Services": the console's session shown in the Services window, its Output
/// first (the session made if it has none yet; nothing connects).
pub fn open_in_services(host: &mut dyn WorkbenchHost) -> bool {
    let Some((consoles, spec)) = active(host) else { return false };
    consoles.runner(spec.id, spec.source).show_output();
    crate::results_tab::show(host);
    true
}

/// The active console's runner, if it has one.
fn active_runner(host: &dyn WorkbenchHost) -> Option<crate::runner::Runner> {
    let (consoles, spec) = active(host)?;
    consoles.runner_of(spec.id)
}

/// Stops what the active console runs.
pub fn stop(host: &mut dyn WorkbenchHost) -> bool {
    active_runner(host).is_some_and(|r| r.stop())
}

/// Commits (`true`) or rolls back the active console's transaction.
pub fn finish(host: &mut dyn WorkbenchHost, commit: bool) -> bool {
    active_runner(host).is_some_and(|r| r.finish_transaction(commit))
}

/// Asks what becomes of the open transaction before the tab closes: Commit, Roll Back,
/// or keep it (the tab stays).
pub fn close(host: &mut dyn WorkbenchHost, id: ConsoleId) -> bool {
    let Some(consoles) = host.project_id().and_then(consoles) else { return false };
    let Some(runner) = consoles.runner_of(id) else { return false };
    let Some(spec) = consoles.spec(id) else { return false };
    let brief = consoles.access().and_then(|a| a.source(spec.source));
    let mut request = crate::confirm::ConfirmRequest::new(
        crate::confirm::ConfirmKind::CloseTransaction,
        spec.name.clone(),
        brief.as_ref().map(|b| b.name.clone()).unwrap_or_default(),
    );
    request.color = brief.as_ref().and_then(|b| b.color);
    request.production = brief.as_ref().is_some_and(|b| b.production);
    request.reasons = vec![i18n::tr!(
        "console.close_tx_message",
        name = spec.name.as_str(),
        count = runner.tx().pending()
    )];
    request.statements = runner.transaction_statements();
    request.note = Some(i18n::t("console.close_tx_note"));
    crate::confirm::ask(
        host,
        request,
        Box::new(move |host, answer| {
            match answer {
                crate::confirm::Answer::Commit => {
                    // Committed in the background; the connection closes once it's done.
                    runner.finish_transaction(true);
                }
                _ => runner.abandon(),
            }
            if let Some(consoles) = host.project_id().and_then(crate::consoles::consoles) {
                consoles.allow_close(id);
            }
            host.request_command(workbench_model::CLOSE_TAB);
        }),
    )
}

/// A setting changed from the bar's ⚙ menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingChange {
    FetchLimit(i64),
    StopOnError(bool),
    /// A profile's option (the bar's ⚙ › Profiling).
    Profile(crate::settings::ProfileSwitch, bool),
}

/// Saves the setting the bar asked for (the user's settings: every console follows).
pub fn save_setting(host: &mut dyn WorkbenchHost, change: SettingChange) -> bool {
    match change {
        SettingChange::FetchLimit(n) => crate::settings::configure_fetch(n),
        SettingChange::StopOnError(on) => crate::settings::configure_stop(on),
        SettingChange::Profile(switch, on) => crate::settings::set_profile_switch(switch, on),
    }
    if let Some(settings) = host.user_settings() {
        match change {
            SettingChange::FetchLimit(n) => {
                crate::settings::save(settings, &crate::settings::FETCH_LIMIT, n);
            }
            SettingChange::StopOnError(on) => {
                crate::settings::save(settings, &crate::settings::STOP_ON_ERROR, on);
            }
            SettingChange::Profile(switch, on) => {
                crate::settings::save(settings, switch.setting(), on);
            }
        }
    }
    true
}

/// A command doing `f` with the host.
#[derive(Debug)]
pub struct Run(pub fn(&mut dyn WorkbenchHost) -> bool, pub command_model::When);

impl CommandHandler for Run {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        (self.0)(host)
    }

    fn when(&self) -> command_model::When {
        self.1
    }
}

/// A console's command: only on a console's tab.
#[derive(Debug)]
pub struct OnConsole(pub fn(&mut dyn WorkbenchHost) -> bool);

impl CommandHandler for OnConsole {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        active(host).is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        active(host).is_some() && (self.0)(host)
    }

    fn when(&self) -> command_model::When {
        command_model::When::EditorLanguage("sql")
    }
}

#[cfg(test)]
#[path = "../../tests/unit/commands.rs"]
mod tests;
