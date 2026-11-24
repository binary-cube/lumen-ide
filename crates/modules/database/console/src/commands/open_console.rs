//! "Jump to Query Console…" and "New Query Console": the source's consoles listed in the
//! background and offered, one picked, made or opened (on a table, its schema taken and an
//! empty one given `SELECT * FROM` it).

use std::sync::Arc;

use database::console;
use database::console::{ConsoleAccess, ConsoleSpec, JumpRequest, SourceBrief};
use workbench_model::{ChoiceItem, ChoiceList, Notification, WorkbenchHost};

use crate::consoles::{Consoles, consoles};

use super::{active, source_of};

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
    if source_of(host, consoles, request.source).is_none() {
        return false;
    }
    // The source's folder looked at in the background (the disk never on the UI thread).
    let listing = Arc::clone(consoles);
    host.spawn(Box::new(move || {
        let listed = listing.consoles_of(request.source);
        Box::new(move |host: &mut dyn WorkbenchHost| {
            offer_listed(host, &listing, request, listed);
        }) as workbench_model::Completion
    }));
    true
}

/// [`open_for`] once the source's consoles are listed (`listed`, as on disk).
fn offer_listed(
    host: &mut dyn WorkbenchHost,
    consoles: &Arc<Consoles>,
    request: JumpRequest,
    listed: Vec<ConsoleSpec>,
) -> bool {
    let Some((access, brief)) = source_of(host, consoles, request.source) else { return false };
    // Asked on another database of its server: only the consoles opened there.
    let existing: Vec<ConsoleSpec> = listed
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
                    // An open tab's (memory); a closed one's is read by the popover's job.
                    text: match host.file_text_later(&file) {
                        workbench_model::FileText::Open(text) => Some(text),
                        _ => None,
                    },
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

/// A new console of the source asked: its file made in the background (a table's query in
/// it when asked on a table), then opened, the table's name selected.
pub(super) fn create(
    host: &mut dyn WorkbenchHost,
    consoles: &Arc<Consoles>,
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
    let (making, request) = (Arc::clone(consoles), request.clone());
    host.spawn(Box::new(move || {
        let made = making.create(request.source, &text);
        Box::new(move |host: &mut dyn WorkbenchHost| {
            created(host, &making, (&request, select), made);
        }) as workbench_model::Completion
    }));
    true
}

/// [`create`] once its file is made (or not): the request's schema and database noted,
/// the console opened, `select` selected.
fn created(
    host: &mut dyn WorkbenchHost,
    consoles: &Consoles,
    (request, select): (&JumpRequest, Option<std::ops::Range<usize>>),
    made: std::io::Result<ConsoleSpec>,
) -> bool {
    let spec = match made {
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
pub(super) fn quote(name: &str, _flavor: database::sql::Flavor) -> String {
    let plain = name.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if plain { name.to_owned() } else { format!("\"{}\"", name.replace('"', "\"\"")) }
}
