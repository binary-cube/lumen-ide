//! A SQL file of the project attached to a source, or detached, and the commands that do
//! it.

use std::sync::Arc;

use database::console::SourceBrief;
use workbench_model::{ChoiceItem, ChoiceList, CommandHandler, Notification, WorkbenchHost};

use crate::consoles::{Consoles, consoles};

use super::active;

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
