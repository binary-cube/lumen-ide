//! Explain and Explain Analyze of the statement Run would run, its values asked first and
//! a profile on production confirmed once.

use database::console;
use database::console::{Overrides, PlannedStatement, RunPlan};
use workbench_model::{Notification, WorkbenchHost};

use super::{Target, planned, with_values};

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
