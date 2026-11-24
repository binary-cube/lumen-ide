//! A run confirmed before it goes: its statements' dangers and a production write made
//! permanent at once asked first, then started on the console's runner.

use std::sync::Arc;

use database::console;
use database::console::{ConsoleSpec, Overrides, PlannedStatement, SourceBrief};
use workbench_model::{Notification, WorkbenchHost};

use crate::runner::{RunRequest, RunSettings};

use super::Target;

/// `statements` (their values written in) run: their dangers and a production write made
/// permanent at once confirmed in the module's modal first.
pub(super) fn guarded(
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
