//! What the console's commands do, through the host (the active file's text, its cursor and
//! selection; asking; opening a console's file): run the statement under the cursor, the
//! selection or the whole script; stop it; explain it; commit or roll back; and "Jump to
//! Query Console…" — the source's consoles listed (a new one first), or a new one made
//! straight away when it has none; one opened on a table gets its schema and, empty, a
//! `SELECT * FROM` it with the name selected.

use std::sync::Arc;

use database::DataSourceId;
use database::console::{
    self, ConsoleAccess, ConsoleId, ConsoleSpec, JumpRequest, Overrides, PlanInput,
    PlannedStatement, RunMode, RunPlan, SourceBrief,
};
use workbench_model::{ChoiceItem, ChoiceList, CommandHandler, Notification, WorkbenchHost};

use crate::consoles::{Consoles, consoles};
use crate::runner::{RunRequest, RunSettings};

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

/// What's done once a run's statements have their values.
type WithValues = Box<dyn FnOnce(&mut dyn WorkbenchHost, Vec<PlannedStatement>) -> bool + Send>;

/// `statements` with their parameters' values written in, then `then` done with them: none
/// has parameters, at once; else once the module's modal gave them (the values kept by the
/// console's runner as asked, the names by the console). `false`: nothing runs now.
fn with_values(
    host: &mut dyn WorkbenchHost,
    purpose: crate::params::Purpose,
    target: &Target,
    statements: Vec<PlannedStatement>,
    then: WithValues,
) -> bool {
    let Target { consoles, spec, brief, document, .. } = target;
    let styles = crate::settings::param_styles(brief.flavor);
    let asked = console::parameters(&statements, brief.flavor, &styles);
    if asked.is_empty() {
        return then(host, statements);
    }
    let runner = consoles.runner(spec.id, spec.source);
    let request = params_request(purpose, spec, brief, &statements, &asked, &runner, &styles);
    let (consoles, spec, brief, document) =
        (Arc::clone(consoles), spec.clone(), brief.clone(), Arc::clone(document));
    crate::params::ask(
        host,
        request,
        Box::new(move |host, entries| {
            runner.remember_params(&entries);
            let names: Vec<String> = entries.iter().map(|e| e.label.clone()).collect();
            if spec.params != names {
                consoles.update(spec.id, |s| s.params = names.clone());
            }
            let Some(values) = crate::params::values(&entries) else { return };
            let dialect = runner.dialect().unwrap_or_else(|| console::dialect_for(brief.flavor));
            match console::bind_statements(
                &document,
                &statements,
                brief.flavor,
                &styles,
                &values,
                &*dialect,
            ) {
                Ok(bound) => {
                    then(host, bound);
                }
                Err(e) => host.notify(Notification::error(bind_error(&e))),
            }
        }),
    )
}

/// Why the values couldn't be written in, in words.
fn bind_error(e: &database::sql::BindError) -> String {
    use database::sql::BindError;
    match e {
        BindError::Missing(name) => {
            i18n::tr!("console.params_missing", name = param_label(name).as_str())
        }
        BindError::Refused(name, why) => i18n::tr!(
            "console.params_refused",
            name = param_label(name).as_str(),
            why = crate::runner::message(why)
        ),
    }
}

/// A parameter's name as the user wrote it (near enough: `:name`, `$1`, `?1`).
fn param_label(name: &database::sql::ParamName) -> String {
    use database::sql::ParamName;
    match name {
        ParamName::Positional(n) => format!("?{n}"),
        ParamName::Numbered(n) => format!("${n}"),
        ParamName::Named(s) => format!(":{s}"),
    }
}

/// What the parameters' modal shows: the statements that have some, where each is written,
/// a row per parameter with the value its console kept.
fn params_request(
    purpose: crate::params::Purpose,
    spec: &ConsoleSpec,
    brief: &SourceBrief,
    statements: &[PlannedStatement],
    asked: &[console::AskedParam],
    runner: &crate::runner::Runner,
    styles: &database::sql::ParamStyles,
) -> crate::params::ParamsRequest {
    let mut request =
        crate::params::ParamsRequest::new(purpose, spec.name.clone(), brief.name.clone());
    request.color = brief.color;
    request.production = brief.production;
    for statement in statements {
        let tokens = database::sql::lex(&statement.text, brief.flavor);
        let found = database::sql::params(&statement.text, &tokens, styles);
        if found.is_empty() {
            continue;
        }
        let at = request.statements.len();
        request.statements.push(statement.text.clone());
        for p in found {
            for place in p.places {
                request.places.push((at, place, p.name.clone()));
            }
        }
    }
    let kept = runner.remembered_params();
    request.entries = asked
        .iter()
        .map(|a| crate::params::ParamEntry::new(a, kept.iter().find(|k| k.name == a.name)))
        .collect();
    request
}

/// "Edit Parameters…" (the bar's `(p)`): the values of the active console's parameters
/// (those of the statements Run would run, else of the whole console) edited and kept for
/// its next runs; nothing runs.
pub fn edit_params(host: &mut dyn WorkbenchHost) -> bool {
    let Some((consoles, spec, _access, brief, plan, text)) = planned(host, false) else {
        return false;
    };
    let statements = match plan {
        RunPlan::Run(statements) => statements,
        RunPlan::Choose(all) => all,
        RunPlan::Nothing => Vec::new(),
    };
    let styles = crate::settings::param_styles(brief.flavor);
    let mut asked = console::parameters(&statements, brief.flavor, &styles);
    let mut statements = statements;
    if asked.is_empty() {
        // None in what Run would run: the whole console's.
        let all = console::plan(&PlanInput {
            text: &text,
            selection: None,
            cursor: 0,
            mode: RunMode::Script,
            flavor: brief.flavor,
            options: Default::default(),
        });
        if let RunPlan::Run(all) = all {
            asked = console::parameters(&all, brief.flavor, &styles);
            statements = all;
        }
    }
    if asked.is_empty() {
        host.notify(Notification::info(i18n::t("console.params_none")));
        return false;
    }
    let runner = consoles.runner(spec.id, spec.source);
    let request = params_request(
        crate::params::Purpose::Edit,
        &spec,
        &brief,
        &statements,
        &asked,
        &runner,
        &styles,
    );
    crate::params::ask(
        host,
        request,
        Box::new(move |host, entries| {
            runner.remember_params(&entries);
            let names: Vec<String> = entries.iter().map(|e| e.label.clone()).collect();
            if spec.params != names {
                consoles.update(spec.id, |s| s.params = names.clone());
            }
            host.notify(Notification::success(i18n::tr!(
                "console.params_saved",
                count = entries.iter().filter(|e| e.remember).count()
            )));
        }),
    )
}

/// `statements` (their values written in) run: their dangers and a production write made
/// permanent at once confirmed in the module's modal first.
fn guarded(
    host: &mut dyn WorkbenchHost,
    target: &Target,
    statements: Vec<PlannedStatement>,
    detached: bool,
) -> bool {
    let Target { consoles, spec, access, brief, .. } = target;
    let runner = consoles.runner(spec.id, spec.source);
    let production = access.production(spec.source) || brief.production;
    let safety =
        console::safety(&statements, brief.flavor, production, spec.tx, runner.in_transaction());
    if !safety.is_clear() {
        let request = run_request(spec, brief, &statements, &safety);
        let target = target.clone();
        return crate::confirm::ask(
            host,
            request,
            Box::new(move |host, answer| {
                if answer == crate::confirm::Answer::Go {
                    launch(host, &target, statements, detached);
                }
            }),
        );
    }
    launch(host, target, statements, detached)
}

/// What the confirmation of a run shows: the statements at stake, why.
fn run_request(
    spec: &ConsoleSpec,
    brief: &SourceBrief,
    statements: &[PlannedStatement],
    safety: &console::Safety,
) -> crate::confirm::ConfirmRequest {
    let mut request = crate::confirm::ConfirmRequest::new(
        crate::confirm::ConfirmKind::Run,
        spec.name.clone(),
        brief.name.clone(),
    );
    request.color = brief.color;
    request.production = safety.production;
    request.statements = safety
        .statements()
        .into_iter()
        .filter_map(|i| statements.get(i))
        .map(|s| s.sent().to_owned())
        .collect();
    request.reasons = safety.dangers.iter().map(|(_, d)| crate::confirm::danger_text(d)).collect();
    if !safety.production_writes.is_empty() {
        request.reasons.push(i18n::tr!(
            "console.confirm_production_writes",
            count = safety.production_writes.len()
        ));
    }
    let total = statements.len();
    if total > request.statements.len() {
        request.note =
            Some(i18n::tr!("console.confirm_others", count = total - request.statements.len()));
    }
    request
}

/// Runs `statements` on the console's runner (confirmed, or nothing to confirm).
fn launch(
    host: &mut dyn WorkbenchHost,
    target: &Target,
    statements: Vec<PlannedStatement>,
    detached: bool,
) -> bool {
    let Target { consoles, spec, access, brief, document } = target;
    let connector =
        match access.connector(spec.source, &Overrides { database: spec.database.clone() }) {
            Ok(connector) => connector,
            Err(e) => {
                host.notify(Notification::error(crate::runner::message(&e)));
                return false;
            }
        };
    let runner = consoles.runner(spec.id, spec.source);
    runner.set_idle(access.idle_close());
    let request = RunRequest {
        statements,
        document: Arc::clone(document),
        settings: RunSettings {
            fetch_limit: crate::settings::fetch_limit(),
            stop_on_error: crate::settings::stop_on_error(),
            tx: spec.tx,
            isolation: spec.isolation,
            schema: spec.schema.clone(),
            flavor: brief.flavor,
            result_memory: crate::settings::result_memory(),
        },
        connector,
        source_name: brief.name.clone(),
        detached,
    };
    if !runner.run(request) {
        host.notify(Notification::info(i18n::t("console.busy")));
        return false;
    }
    // Its Output and results are seen in the Services window.
    crate::results_tab::show(host);
    true
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

/// Explains the statement Run would run (`analyze`: as it runs — a write asked first): its
/// parameters' values asked first, as for a run.
pub fn explain(host: &mut dyn WorkbenchHost, analyze: bool) -> bool {
    let Some((consoles, spec, access, brief, plan, text)) = planned(host, false) else {
        return false;
    };
    let statement = match plan {
        RunPlan::Run(statements) => statements.into_iter().next(),
        RunPlan::Choose(_) | RunPlan::Nothing => None,
    };
    let Some(statement) = statement else {
        host.notify(Notification::info(i18n::t("console.nothing_to_run")));
        return false;
    };
    let writes = analyze && statement.writes();
    // What can't run inside the transaction that would undo it isn't profiled as it runs.
    if analyze && let Some(words) = database::profile::actual_refused(&statement.text, brief.flavor)
    {
        let what = console::summary(&statement.text, 40);
        let why =
            i18n::tr!("console.profile_refused", statement = what.as_str(), words = words.as_str());
        consoles.runner(spec.id, spec.source).refused(why.clone());
        host.notify(Notification::warning(why));
        return false;
    }
    if writes && brief.read_only {
        // Run, even if undone: a read-only source never gets it.
        let what = console::summary(&statement.text, 40);
        let why =
            i18n::tr!("console.read_only_refused", name = brief.name.as_str(), statement = what);
        consoles.runner(spec.id, spec.source).refused(why.clone());
        host.notify(Notification::warning(why));
        return false;
    }
    let target = Target { consoles, spec, access, brief, document: text };
    let next = target.clone();
    with_values(
        host,
        crate::params::Purpose::Explain,
        &target,
        vec![statement],
        Box::new(move |host, mut statements| {
            let Some(statement) = statements.pop() else { return false };
            explain_now(host, next, statement, analyze)
        }),
    )
}

/// The key a profile run on production is confirmed once under, per console's session.
pub const PROFILE_ON_PRODUCTION: &str = "profile-production";

/// Explains `statement` (its values written in): Explain Analyze of a write confirmed first
/// (always), a read on production once per console's session (it loads the server).
fn explain_now(
    host: &mut dyn WorkbenchHost,
    target: Target,
    statement: PlannedStatement,
    analyze: bool,
) -> bool {
    let Target { consoles, spec, access, brief, document } = target;
    let writes = analyze && statement.writes();
    let confirm = writes.then(|| {
        let mut request = crate::confirm::ConfirmRequest::new(
            crate::confirm::ConfirmKind::Analyze,
            spec.name.clone(),
            brief.name.clone(),
        );
        request.color = brief.color;
        request.production = brief.production;
        request.statements = vec![statement.sent().to_owned()];
        request.reasons = vec![i18n::t("console.analyze_message")];
        request.note = Some(i18n::t("console.analyze_note"));
        request
    });
    let confirm = confirm.or_else(|| {
        (analyze && brief.production).then(|| {
            let mut request = crate::confirm::ConfirmRequest::new(
                crate::confirm::ConfirmKind::Profile,
                spec.name.clone(),
                brief.name.clone(),
            );
            request.color = brief.color;
            request.production = true;
            request.statements = vec![statement.sent().to_owned()];
            let seconds =
                crate::settings::profile_options(true).timeout.map_or(0, |t| t.as_secs() as i64);
            request.reasons = vec![i18n::tr!("console.confirm_profile_message", seconds = seconds)];
            request.note = Some(i18n::t("console.confirm_profile_note"));
            request.once = Some((spec.id, PROFILE_ON_PRODUCTION));
            request
        })
    });
    let go = move |host: &mut dyn WorkbenchHost| {
        let connector =
            match access.connector(spec.source, &Overrides { database: spec.database.clone() }) {
                Ok(connector) => connector,
                Err(e) => {
                    host.notify(Notification::error(crate::runner::message(&e)));
                    return;
                }
            };
        let runner = consoles.runner(spec.id, spec.source);
        runner.set_idle(access.idle_close());
        // What its analysis knows of the source (39h3).
        runner.set_analysis(crate::profile::Analysis {
            notes: access.notes(spec.source),
            production: brief.production,
        });
        if runner.explain(statement, analyze, connector, brief.name.clone(), brief.flavor, document)
        {
            crate::results_tab::show(host);
        } else {
            host.notify(Notification::info(i18n::t("console.busy")));
        }
    };
    if let Some(request) = confirm {
        return crate::confirm::ask(
            host,
            request,
            Box::new(move |host, answer| {
                if answer == crate::confirm::Answer::Go {
                    go(host);
                }
            }),
        );
    }
    go(host);
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

/// "Jump to Query Console…" (`new`: "New Query Console"): for the source asked (by the
/// "Database" window, a data view), else the active console's, else the window's selection,
/// else the one chosen.
pub fn jump(host: &mut dyn WorkbenchHost, new: bool) -> bool {
    let Some(project) = host.project_id() else { return false };
    let Some(consoles) = consoles(project) else { return false };
    let asked = console::take_jump().filter(|r| r.project == project).or_else(|| {
        let (_, spec) = active(host)?;
        Some(JumpRequest {
            project,
            source: spec.source,
            schema: None,
            table: None,
            new: false,
            list: false,
            database: None,
        })
    });
    let asked = asked.or_else(|| consoles.access()?.selected());
    if let Some(mut request) = asked {
        request.new |= new;
        return open_for(host, &consoles, request);
    }
    // None asked: which source.
    let Some(access) = consoles.access() else {
        host.notify(Notification::warning(i18n::t("console.no_sources")));
        return false;
    };
    let sources: Vec<SourceBrief> = access.sources().into_iter().filter(|s| s.dialect).collect();
    match sources.as_slice() {
        [] => {
            host.notify(Notification::info(i18n::t("console.no_sql_sources")));
            false
        }
        [only] => {
            let request = JumpRequest {
                project,
                source: only.id,
                schema: None,
                table: None,
                new,
                list: false,
                database: None,
            };
            open_for(host, &consoles, request)
        }
        _ => {
            let items = sources
                .iter()
                .map(|s| ChoiceItem {
                    icon: s.icon,
                    label: s.name.clone(),
                    detail: s.driver.clone(),
                })
                .collect();
            let list = ChoiceList {
                title: i18n::t("console.choose_source"),
                subject: String::new(),
                icon: "console",
                action: i18n::t("console.choose_open"),
                items,
            };
            host.choose(
                list,
                Box::new(move |i, host| {
                    if let Some(source) = sources.get(i) {
                        let request = JumpRequest {
                            project,
                            source: source.id,
                            schema: None,
                            table: None,
                            new,
                            list: false,
                            database: None,
                        };
                        open_for(host, &consoles, request);
                    }
                }),
            )
        }
    }
}

/// The console `request` asks for: its source's consoles listed in the module's popover
/// (filtered as typed, a new one first: [`crate::jump`]), or a new one made at once (none
/// yet, or a new one asked; "Database Consoles…" lists them even when there are none).
pub fn open_for(
    host: &mut dyn WorkbenchHost,
    consoles: &Arc<Consoles>,
    request: JumpRequest,
) -> bool {
    let Some((access, brief)) = source_of(host, consoles, request.source) else { return false };
    // Asked on another database of its server: only the consoles opened there.
    let existing: Vec<ConsoleSpec> = consoles
        .consoles_of(request.source)
        .into_iter()
        .filter(|s| request.database.is_none() || s.database == request.database)
        .collect();
    if request.new || (existing.is_empty() && !request.list) {
        return create(host, consoles, &*access, &brief, &request);
    }
    let detail = |s: &ConsoleSpec| match s.last_run_ms {
        Some(ms) => i18n::tr!(
            "console.last_run",
            time = crate::runner::millis(std::time::Duration::from_millis(ms))
        ),
        None => i18n::t("console.never_run"),
    };
    // The popover, its filter having the keyboard.
    let data = crate::jump::JumpData {
        project: consoles.project(),
        request: request.clone(),
        source_name: brief.name.clone(),
        consoles: existing
            .iter()
            .map(|s| {
                let file = s.path(consoles.root());
                crate::jump::JumpConsole {
                    id: s.id,
                    name: s.name.clone(),
                    detail: detail(s),
                    text: host.file_text(&file),
                    file,
                }
            })
            .collect(),
    };
    if host.open_overlay(crate::jump::EXTENSION, &data, true) {
        return true;
    }
    // A host without popovers of modules: its own list.
    let mut items = vec![ChoiceItem {
        icon: "plus",
        label: i18n::t("console.new_console"),
        detail: brief.name.clone(),
    }];
    items.extend(existing.iter().map(|s| ChoiceItem {
        icon: "console",
        label: s.name.clone(),
        detail: detail(s),
    }));
    let list = ChoiceList {
        title: i18n::t("console.jump_title"),
        subject: brief.name.clone(),
        icon: "console",
        action: i18n::t("console.choose_open"),
        items,
    };
    let consoles = Arc::clone(consoles);
    host.choose(
        list,
        Box::new(move |i, host| {
            if i == 0 {
                create(host, &consoles, &*access, &brief, &request);
            } else if let Some(spec) = existing.get(i - 1) {
                open_existing(host, &consoles, &*access, spec, &request);
            }
        }),
    )
}

/// What the row chosen in the consoles' popover asks: a new console, one opened (for the
/// request it was opened for), renamed or deleted.
pub fn jump_picked(
    host: &mut dyn WorkbenchHost,
    request: JumpRequest,
    pick: crate::jump::JumpPick,
) -> bool {
    use crate::jump::JumpPick;
    let Some(consoles) = host.project_id().and_then(consoles) else { return false };
    match pick {
        JumpPick::Rename(id) => crate::rename::ask_name(host, id),
        JumpPick::Delete(id) => crate::rename::confirm_delete(host, id),
        JumpPick::New | JumpPick::Open(_) => {
            let Some((access, brief)) = source_of(host, &consoles, request.source) else {
                return false;
            };
            match pick {
                JumpPick::Open(id) => match consoles.spec(id) {
                    Some(spec) => open_existing(host, &consoles, &*access, &spec, &request),
                    None => {
                        host.notify(Notification::warning(i18n::t("console.gone")));
                        false
                    }
                },
                _ => create(host, &consoles, &*access, &brief, &request),
            }
        }
    }
}

/// A new console of the source asked: its file made (a table's query in it when asked on a
/// table), opened, the table's name selected.
fn create(
    host: &mut dyn WorkbenchHost,
    consoles: &Consoles,
    access: &dyn ConsoleAccess,
    brief: &SourceBrief,
    request: &JumpRequest,
) -> bool {
    let (text, select) = match &request.table {
        Some(table) => {
            let (text, range) = table_query(access, brief, table);
            (text, Some(range))
        }
        None => (String::new(), None),
    };
    let spec = match consoles.create(request.source, &text) {
        Ok(spec) => spec,
        Err(e) => {
            host.notify(Notification::error(i18n::tr!("console.not_created", why = e.to_string())));
            return false;
        }
    };
    let schema =
        request.schema.clone().or_else(|| request.table.as_ref().and_then(|t| t.schema.clone()));
    if schema.is_some() || request.database.is_some() {
        let database = request.database.clone();
        consoles.update(spec.id, |s| {
            s.schema = schema.clone();
            if database.is_some() {
                s.database = database.clone();
            }
        });
    }
    if !host.open_file(&spec.path(consoles.root())) {
        return false;
    }
    if let Some(range) = select {
        host.select_active(range);
    }
    true
}

/// An existing console opened for `request`: the table's schema taken; empty, it gets the
/// table's query (the name selected).
fn open_existing(
    host: &mut dyn WorkbenchHost,
    consoles: &Consoles,
    access: &dyn ConsoleAccess,
    spec: &ConsoleSpec,
    request: &JumpRequest,
) -> bool {
    let schema =
        request.schema.clone().or_else(|| request.table.as_ref().and_then(|t| t.schema.clone()));
    if schema.is_some() {
        consoles.update(spec.id, |s| s.schema = schema.clone());
    }
    if !host.open_file(&spec.path(consoles.root())) {
        return false;
    }
    let Some(table) = &request.table else { return true };
    let empty = host.active_file().is_some_and(|f| f.text.trim().is_empty());
    if !empty {
        return true;
    }
    let Some(brief) = access.source(spec.source) else { return true };
    let (text, range) = table_query(access, &brief, table);
    let len = host.active_file().map_or(0, |f| f.text.len());
    if host.edit_active(0..len, &text) {
        host.select_active(range);
    }
    true
}

/// `SELECT * FROM <table>;` in the source's quoting, and where the name is.
fn table_query(
    _access: &dyn ConsoleAccess,
    brief: &SourceBrief,
    table: &database::TableRef,
) -> (String, std::ops::Range<usize>) {
    let flavor = brief.flavor;
    console::table_query(table, |name| quote(name, flavor))
}

/// A name as written in a statement: bare when it's a plain lowercase word, quoted
/// otherwise (`"Order Items"`, `` `x` `` nowhere: the standard's quotes).
fn quote(name: &str, _flavor: database::sql::Flavor) -> String {
    let plain = name.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if plain { name.to_owned() } else { format!("\"{}\"", name.replace('"', "\"\"")) }
}

/// The active tab's file, when it's a SQL file of the project a source can be attached to
/// (39n), with its project's consoles.
pub fn attachable_active(host: &dyn WorkbenchHost) -> Option<(Arc<Consoles>, std::path::PathBuf)> {
    let file = host.active_file()?;
    let consoles = consoles(host.project_id()?)?;
    database::console::attachable(consoles.root(), &file.path)?;
    Some((consoles, file.path))
}

/// "Attach Data Source…": the active SQL file of the project attached to the source chosen
/// (the one it's attached to marked); `then` done once it is (a run asked for on a file
/// attached to nothing). Its transaction open: refused, said.
pub fn attach(
    host: &mut dyn WorkbenchHost,
    then: Option<fn(&mut dyn WorkbenchHost) -> bool>,
) -> bool {
    let Some((consoles, path)) = attachable_active(host) else { return false };
    let Some(access) = consoles.access() else {
        host.notify(Notification::warning(i18n::t("console.no_sources")));
        return false;
    };
    let current = consoles.attached_of(&path);
    if current.as_ref().and_then(|s| consoles.runner_of(s.id)).is_some_and(|r| r.in_transaction()) {
        host.notify(Notification::warning(i18n::t("console.tx_open_attach")));
        return false;
    }
    let sources: Vec<SourceBrief> = access.sources().into_iter().filter(|s| s.dialect).collect();
    if sources.is_empty() {
        host.notify(Notification::info(i18n::t("console.no_sql_sources")));
        return false;
    }
    let current = current.map(|s| s.source);
    let items = sources
        .iter()
        .map(|s| ChoiceItem {
            icon: s.icon,
            label: s.name.clone(),
            detail: if Some(s.id) == current {
                i18n::t("console.attached_now")
            } else {
                s.driver.clone()
            },
        })
        .collect();
    let list = ChoiceList {
        title: i18n::t("console.attach_title"),
        subject: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        icon: "link",
        action: i18n::t("console.attach_action"),
        items,
    };
    host.choose(
        list,
        Box::new(move |i, host| {
            let Some(source) = sources.get(i) else { return };
            if consoles.attach(&path, source.id).is_some()
                && let Some(then) = then
            {
                then(host);
            }
        }),
    )
}

/// "Detach Data Source": the active SQL file of the project no longer attached (its
/// connection closed); its transaction open: refused, said.
pub fn detach(host: &mut dyn WorkbenchHost) -> bool {
    let Some((consoles, spec)) = active(host) else { return false };
    if !spec.is_attached() {
        return false;
    }
    if consoles.runner_of(spec.id).is_some_and(|r| r.in_transaction()) {
        host.notify(Notification::warning(i18n::t("console.tx_open_attach")));
        return false;
    }
    consoles.detach(&spec.path(consoles.root())).is_some()
}

/// Run on a console or an attached file; on a SQL file of the project attached to nothing,
/// which source is asked first (then it runs).
#[derive(Debug)]
pub struct OnSqlFile(pub fn(&mut dyn WorkbenchHost) -> bool);

impl CommandHandler for OnSqlFile {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        active(host).is_some() || attachable_active(host).is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        if active(host).is_some() {
            return (self.0)(host);
        }
        attach(host, Some(self.0))
    }

    fn when(&self) -> command_model::When {
        command_model::When::EditorLanguage("sql")
    }
}

/// "Attach Data Source…": on a SQL file of the project.
#[derive(Debug)]
pub struct AttachCommand;

impl CommandHandler for AttachCommand {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        attachable_active(host).is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        attach(host, None)
    }

    fn when(&self) -> command_model::When {
        command_model::When::EditorLanguage("sql")
    }
}

/// "Detach Data Source": on an attached SQL file of the project.
#[derive(Debug)]
pub struct DetachCommand;

impl CommandHandler for DetachCommand {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        active(host).is_some_and(|(_, spec)| spec.is_attached())
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        detach(host)
    }

    fn when(&self) -> command_model::When {
        command_model::When::EditorLanguage("sql")
    }
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
#[path = "../tests/unit/commands.rs"]
mod tests;
