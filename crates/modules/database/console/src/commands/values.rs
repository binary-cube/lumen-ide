//! A run's parameters: their values written into the statements (asked in the module's
//! modal when some have none), why they couldn't be, "Edit Parameters…".

use std::sync::Arc;

use database::console;
use database::console::{ConsoleSpec, PlanInput, PlannedStatement, RunMode, RunPlan, SourceBrief};
use workbench_model::{Notification, WorkbenchHost};

use super::{Target, planned};

/// What's done once a run's statements have their values.
type WithValues = Box<dyn FnOnce(&mut dyn WorkbenchHost, Vec<PlannedStatement>) -> bool + Send>;

/// `statements` with their parameters' values written in, then `then` done with them: none
/// has parameters, at once; else once the module's modal gave them (the values kept by the
/// console's runner as asked, the names by the console). `false`: nothing runs now.
pub(super) fn with_values(
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
