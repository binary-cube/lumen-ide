//! The servers' commands (their code actions are the workbench's "Show Context Actions",
//! beside the built-in engine's): "Format with Language Server", "Restart Language Servers"
//! and "Show Language Server Log"; "Download the File's Language Server" and "Use the
//! Built-in Engine for the File's Language" (also the bar's over a file). What a server takes long for is waited for on a
//! background job, never on the UI thread.

use std::sync::Arc;
use std::time::Duration;

use code_intelligence::servers::WorkspaceChange;
use command_model::CommandId;
use i18n::{t, tr};
use lsp_client::client::{Client, FormatOptions};
use lsp_client::status::Feature;
use workbench_model::{CommandHandler, Notification, WorkbenchHost};

use crate::{edits, log_view, projects};

pub const FORMAT: CommandId = CommandId("lsp.format");
pub const RESTART: CommandId = CommandId("lsp.restart");
pub const SHOW_LOG: CommandId = CommandId("lsp.show_log");
pub const DOWNLOAD: CommandId = CommandId("lsp.download");
pub const USE_BUILT_IN: CommandId = CommandId(crate::bar::USE_BUILT_IN);

/// How long a command waits for its server.
const WAIT: Duration = Duration::from_secs(10);

/// The active file's server, if it answers `feature`: its client, the document, the file.
fn server_of(
    host: &dyn WorkbenchHost,
    feature: Feature,
) -> Option<(Arc<Client>, kernel::DocumentId, workbench_model::ActiveFile)> {
    let file = host.active_file()?;
    let document = host.active_document()?;
    let client = projects::project(host.project_id()?)?.client_of(document)?;
    client.status().answers(feature).then_some((client, document, file))
}

/// Applies `change` as a refactoring (one undo for all of it), saying how it went.
fn apply(host: &mut dyn WorkbenchHost, change: &WorkspaceChange) {
    if change.is_empty() {
        host.notify(Notification::info(t("lsp.nothing_changed")));
        return;
    }
    if let Err(why) = host.apply_refactoring(&edits::plan(change)) {
        host.notify(Notification::error(tr!("lsp.not_applied", reason = why)));
    }
}

/// Format with Language Server: the selection, else the file.
pub struct Format;

impl CommandHandler for Format {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        server_of(host, Feature::Formatting).is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some((client, document, file)) = server_of(host, Feature::Formatting) else {
            host.notify(Notification::info(t("lsp.no_server")));
            return true;
        };
        // The file's own indentation (its first indented line), else four spaces.
        let indent = file.text.lines().find_map(|l| {
            let lead: String = l.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            (!lead.is_empty()).then_some(lead)
        });
        let options = match indent.as_deref() {
            Some(lead) if lead.starts_with('\t') => {
                FormatOptions { tab_size: 4, insert_spaces: false }
            }
            Some(lead) => {
                FormatOptions { tab_size: lead.len().clamp(2, 8) as u32, insert_spaces: true }
            }
            None => FormatOptions { tab_size: 4, insert_spaces: true },
        };
        let reply = client.format(document, file.selection, options);
        host.spawn(Box::new(move || {
            let change = reply.wait(WAIT);
            Box::new(move |host: &mut dyn WorkbenchHost| match change {
                Some(Ok(change)) if change.is_empty() => {
                    host.notify(Notification::info(t("lsp.formatted_already")))
                }
                Some(Ok(change)) => apply(host, &change),
                Some(Err(why)) => {
                    host.notify(Notification::error(tr!("lsp.not_applied", reason = why)))
                }
                None => host.notify(Notification::warning(t("lsp.no_answer"))),
            })
        }));
        true
    }
}

/// Restart Language Servers (the project's).
pub struct Restart;

impl CommandHandler for Restart {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.project_id().and_then(projects::project).is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(project) = host.project_id().and_then(projects::project) else { return false };
        project.restart_all();
        host.notify(Notification::success(t("lsp.restarted")));
        true
    }
}

/// Show Language Server Log: the server chosen in the Task Manager, else the project's
/// first.
pub struct ShowLog;

impl CommandHandler for ShowLog {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.project_id().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(project) = host.project_id() else { return false };
        let Some(data) = log_view::chosen(project) else {
            host.notify(Notification::info(t("lsp.no_servers_running")));
            return true;
        };
        let name = projects::project(project)
            .and_then(|p| {
                p.servers(|s| s.iter().find(|r| r.id == data.server).map(|r| r.spec.name.clone()))
            })
            .unwrap_or_default();
        host.open_view_below(&tr!("lsp.log_tab", name = name), log_view::EXTENSION, &data)
    }
}

/// The active file's language, if it has one.
fn active_language(host: &dyn WorkbenchHost) -> Option<String> {
    let file = host.active_file()?;
    host.language_of(&file.path)
}

/// Download the File's Language Server: what the active file's language's server needs, in
/// the background (as the bar's Download).
pub struct Download;

impl CommandHandler for Download {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        active_language(host).is_some_and(|l| crate::bar::offer(&l).is_some())
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let offer = active_language(host).and_then(|l| crate::bar::offer(&l));
        match offer {
            Some(offer) => crate::downloads::start(offer.components, None),
            None => host.notify(Notification::info(t("lsp.nothing_to_download"))),
        }
        true
    }
}

/// Use the Built-in Engine for the File's Language: saved as the language's engine in the
/// user's settings (as from the settings' page), said.
pub struct UseBuiltIn;

impl CommandHandler for UseBuiltIn {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        active_language(host)
            .is_some_and(|l| crate::catalog::catalog().for_language(&l).next().is_some())
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let (Some(language), Some(settings)) = (active_language(host), host.user_settings()) else {
            return false;
        };
        let choice = crate::settings::Choice {
            engine: code_intelligence::servers::Engine::BuiltIn,
            server: None,
        };
        crate::settings::save_choice(settings, &language, &choice);
        let name = crate::report::language_name(&language);
        host.notify(Notification::info(tr!("lsp.built_in_chosen", language = name.as_str())));
        true
    }
}
