//! A console's result edited and submitted (plan 39l), as the data view's grid is: its
//! changes ([`crate::result_source::ResultSource`]) become the domain's statements
//! ([`database::transaction::apply_script`]: each update and delete changing exactly one row,
//! a row changed since it was read being a conflict), shown first in the console's modal
//! ([`crate::confirm`]: the statements exactly as they'll run, the rows deleted, the source's
//! "production" label), then run by the console's runner on its own connection
//! ([`crate::runner::Runner::submit`]) — in `Tx: Auto` committed at once (on production,
//! "I understand" ticked first), in `Manual` inside the console's transaction, waiting for its
//! Commit. A read-only source refuses them before anything is asked.
//!
//! The result's view can't ask the host itself: it hands [`submit`] over through its action
//! (`ViewerAction::WithHost`).

use std::sync::Arc;

use command_model::CommandId;
use database::console::{SourceBrief, TxMode};
use database::edits::EditableResult;
use database::sql::{StatementKind, WriteKind};
use database::{Change, ChangeSet, transaction};
use ui_components::ServiceId;
use workbench_model::{Notification, WorkbenchHost};

use crate::confirm::{Answer, ConfirmKind, ConfirmRequest};
use crate::runner::{Runner, SubmitJob};

/// The kind a result's view gives its actions' keys (`When::View`): Submit, Add Row…
pub const VIEW: &str = "view-console-result";

/// The result view's actions (remappable): the command, its label, its keys, what the view
/// is sent.
pub const ACTIONS: [(CommandId, &str, &[&str], &str); 5] = [
    (
        CommandId("database.console.submit_result"),
        "Submit Result Changes",
        &["CmdOrCtrl+Enter"],
        "submit",
    ),
    (CommandId("database.console.add_row"), "Add Result Row", &[], "grid:add-row"),
    (CommandId("database.console.clone_row"), "Clone Result Row", &[], "grid:clone-row"),
    (
        CommandId("database.console.delete_rows"),
        "Delete Result Rows",
        &["CmdOrCtrl+Backspace"],
        "grid:delete-rows",
    ),
    (CommandId("database.console.revert"), "Revert Result Changes", &[], "grid:revert-all"),
];

/// A result's changes, to submit.
#[derive(Clone)]
pub struct SubmitAsk {
    pub runner: Runner,
    pub result: ServiceId,
    pub set: ChangeSet,
    pub editable: Arc<EditableResult>,
}

impl std::fmt::Debug for SubmitAsk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubmitAsk").field("set", &self.set).finish_non_exhaustive()
    }
}

/// What each of `set`'s changes is, in the order they run.
pub fn kinds(set: &ChangeSet) -> Vec<StatementKind> {
    transaction::ordered(set)
        .changes
        .iter()
        .map(|c| {
            StatementKind::Write(match c {
                Change::Insert { .. } => WriteKind::Insert,
                Change::Update { .. } => WriteKind::Update,
                Change::Delete { .. } => WriteKind::Delete,
            })
        })
        .collect()
}

/// The confirmation of `script`'s statements (`set`'s changes) on `brief`'s source:
/// `joins` — they go into the console's transaction (nothing permanent before its Commit).
pub fn request(
    console: &str,
    brief: &SourceBrief,
    production: bool,
    statements: Vec<String>,
    set: &ChangeSet,
    joins: bool,
) -> ConfirmRequest {
    let mut request = ConfirmRequest::new(ConfirmKind::Submit, console, brief.name.clone());
    request.color = brief.color;
    request.production = production;
    request.pending = joins;
    let deletes = set.deletes();
    if deletes > 0 {
        request.reasons.push(i18n::tr!("console.submit_deletes", count = deletes));
    }
    if production && !joins {
        request
            .reasons
            .push(i18n::tr!("console.confirm_production_writes", count = statements.len()));
    }
    request.note = Some(i18n::t(if joins {
        "console.submit_note_manual"
    } else {
        "console.submit_note_auto"
    }));
    request.statements = statements;
    request
}

/// `asked`'s changes shown first and, confirmed, run on its console's runner (see the
/// module's doc). `false`: nothing was asked.
pub fn submit(host: &mut dyn WorkbenchHost, asked: SubmitAsk) -> bool {
    let Some(consoles) = host.project_id().and_then(crate::consoles::consoles) else {
        return false;
    };
    let runner = asked.runner.clone();
    let Some(spec) = consoles.spec(runner.console()) else {
        host.notify(Notification::warning(i18n::t("console.gone")));
        return false;
    };
    let Some(access) = consoles.access() else {
        host.notify(Notification::warning(i18n::t("console.no_sources")));
        return false;
    };
    let Some(brief) = access.source(spec.source) else {
        host.notify(Notification::warning(i18n::t("console.source_gone")));
        return false;
    };
    if brief.read_only || !brief.capabilities.edit_rows {
        let why = if brief.read_only {
            i18n::tr!("console.edit_ro_refused", name = brief.name.as_str())
        } else {
            i18n::t("database.ro_driver")
        };
        runner.refused(why.clone());
        host.notify(Notification::warning(why));
        return false;
    }
    if runner.tx().is_failed() {
        host.notify(Notification::warning(i18n::t("console.edit_tx_failed")));
        return false;
    }
    let dialect = runner.dialect().unwrap_or_else(|| database::console::dialect_for(brief.flavor));
    let script = match transaction::apply_script(
        dialect.as_ref(),
        &asked.set,
        Some(&asked.editable.structure),
    ) {
        Ok(script) => script,
        Err(e) => {
            host.notify(Notification::error(i18n::tr!(
                "database.submit_failed",
                why = crate::runner::message(&e)
            )));
            return false;
        }
    };
    let joins = runner.in_transaction() || spec.tx == TxMode::Manual;
    let production = brief.production || access.production(spec.source);
    let statements = script.steps.iter().map(|s| s.sql.clone()).collect();
    let request = request(&spec.name, &brief, production, statements, &asked.set, joins);
    let job = SubmitJob {
        result: asked.result,
        kinds: kinds(&asked.set),
        script,
        table: asked.editable.table.clone(),
        tx: spec.tx,
        isolation: spec.isolation,
    };
    crate::confirm::ask(
        host,
        request,
        Box::new(move |host, answer| {
            if answer == Answer::Go && !runner.submit(job) {
                host.notify(Notification::info(i18n::t("console.busy")));
            }
        }),
    )
}

#[cfg(test)]
#[path = "../tests/unit/edit.rs"]
mod tests;
