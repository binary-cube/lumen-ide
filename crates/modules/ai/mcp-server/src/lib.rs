//! The editor's MCP server, as a module: agents on the machine (Claude Code…) use the editor
//! through tools — a file opened at a line, the selection, the open tabs, an open file's
//! text; more from the modules that offer them (`mcp::McpTool`, found in a registry the app
//! fills: this module names none). Off until turned on in its settings; then only on the
//! loopback, with a token, reading freely while writing and running ask first (not offered
//! yet). The agents installed are configured with it in one click, through their own
//! modules (`agents::AgentProvider::mcp_setup`).
//!
//! A call reaches the editor through a queue: the connection's thread queues it and wakes
//! the windows; the window it's for runs it on its UI thread at its next poll (this module's
//! project service) and answers ([`server`]).

pub mod secret;
mod server;
pub mod settings;
mod tools;

use std::sync::{Arc, Mutex, PoisonError};

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use workbench_model::{Notification, Notifier, ProjectContext, ProjectService, WorkbenchHost};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("agents.mcp"),
    name: "MCP Server",
    description: "The editor's MCP server: agents on the machine use the editor through the tools the modules offer",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "server", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct McpModule;

impl Module for McpModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn ProjectService>>(Arc::new(Windows));
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            settings::McpSettings,
        ));
        for tool in [
            Arc::new(tools::OpenFile) as Arc<dyn mcp::McpTool>,
            Arc::new(tools::Selection),
            Arc::new(tools::OpenFiles),
            Arc::new(tools::FileText),
        ] {
            ctx.contribute(tool);
        }
        Ok(())
    }
}

/// Where the server listens and its token, while it does (a test, the settings' page).
pub fn listening() -> Option<(String, String)> {
    server::running()
}

/// Where the outcome of a configuration is said: each open window's notifier (the newest
/// used; a closed window's let go).
static NOTIFIERS: Mutex<Vec<(kernel::ProjectId, Arc<dyn Notifier>)>> = Mutex::new(Vec::new());

/// Each window that may run calls, followed while its project is open.
#[derive(Debug)]
struct Windows;

impl ProjectService for Windows {
    fn attach(&self, project: &ProjectContext<'_>) -> Vec<kernel::Subscription> {
        let id = project.id;
        {
            let mut notifiers = NOTIFIERS.lock().unwrap_or_else(PoisonError::into_inner);
            notifiers.retain(|(p, _)| *p != id);
            notifiers.push((id, Arc::clone(&project.notifier)));
        }
        let forget = kernel::Subscription::on_end(move || {
            NOTIFIERS.lock().unwrap_or_else(PoisonError::into_inner).retain(|(p, _)| *p != id);
        });
        vec![server::attach(project.id, project.root, Arc::clone(&project.wake)), forget]
    }

    /// The calls for this window, run here (its UI thread) — what writes or runs once the
    /// user allows it.
    fn poll(&self, host: &mut dyn WorkbenchHost) -> bool {
        // What a start learned (its port), saved here: the settings are the UI thread's.
        let saved = host.user_settings().is_some_and(settings::take_saves);
        let Some(id) = host.project_id() else { return saved };
        let focused = host.window_focused();
        let taken = server::take(id, || host.project_root(), focused);
        let any = !taken.is_empty();
        for call in taken {
            let (tool, args, reply) = call.into_parts();
            run(tool, &args, host, reply);
        }
        any || saved
    }
}

/// The tools the user allowed for the rest of this run ("allow in this session").
static ALLOWED: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

/// `tool` called: at once when it only reads (or the user allowed it for this run), else
/// after asking — allow once, allow for this session, refuse; the list dismissed is a
/// refusal (the reply dropped says so).
fn run(
    tool: Arc<dyn mcp::McpTool>,
    args: &serde_json::Value,
    host: &mut dyn WorkbenchHost,
    reply: mcp::Reply,
) {
    let name = tool.name();
    let allowed = ALLOWED.lock().unwrap_or_else(PoisonError::into_inner).contains(&name);
    if tool.kind() == mcp::ToolKind::Read || allowed {
        tool.call(args, host, reply);
        return;
    }
    let mut what = args.to_string();
    if what.chars().count() > 200 {
        what = what.chars().take(199).chain(['…']).collect();
    }
    let row = |icon, key: &str, detail: String| workbench_model::ChoiceItem {
        icon,
        label: i18n::t(key),
        detail,
    };
    let list = workbench_model::ChoiceList {
        title: i18n::tr!("settings.mcp_ask", tool = name),
        subject: what,
        icon: "server",
        action: i18n::t("settings.mcp_allow"),
        items: vec![
            row("check", "settings.mcp_allow_once", String::new()),
            row(
                "check",
                "settings.mcp_allow_session",
                i18n::t("settings.mcp_allow_session_detail"),
            ),
            row("x", "settings.mcp_refuse", String::new()),
        ],
    };
    let args = args.clone();
    host.choose(
        list,
        Box::new(move |chosen, host| match chosen {
            // Answered after the agent gave up waiting: not run (it was told it failed).
            _ if reply.given_up() => {}
            0 | 1 => {
                if chosen == 1 {
                    ALLOWED.lock().unwrap_or_else(PoisonError::into_inner).push(name);
                }
                tool.call(&args, host, reply);
            }
            _ => reply.send(mcp::ToolOutcome::Failed("refused by the user".into())),
        }),
    );
}

/// Each open window woken (a start ended: its poll saves what it learned).
pub(crate) fn wake_windows() {
    server::wake_all();
}

/// The agents installed configured with the server at `url` (on a worker): each one's
/// commands run, unseen; how it went said. They get the token through the helper (this
/// program, [`secret::HELPER_ARG`]): it's in none of the commands.
pub(crate) fn configure(url: &str) {
    let mut done = Vec::new();
    let mut failed = Vec::new();
    let Some(helper) = secret::helper_command() else {
        tracing::warn!("the MCP helper's path isn't known: no agent configured");
        return;
    };
    for agent in agents::providers().into_iter().filter(|a| a.installed()) {
        let steps = agent.mcp_setup(url, &helper);
        if steps.is_empty() {
            continue;
        }
        // The last step is the one that matters (the first removes an old entry).
        let mut ok = false;
        for launch in &steps {
            let mut run = process::Run::new(&launch.program, &launch.args).owner("mcp-configure");
            if let Some(dir) = &launch.cwd {
                run = run.dir(dir);
            }
            for (key, value) in &launch.env {
                run = run.env(key, value);
            }
            ok = process::run(&run).is_ok_and(|out| out.success());
        }
        if ok { done.push(agent.name()) } else { failed.push(agent.name()) }
    }
    if failed.is_empty() && !done.is_empty() {
        server::configured();
    }
    let notifier =
        NOTIFIERS.lock().unwrap_or_else(PoisonError::into_inner).last().map(|(_, n)| Arc::clone(n));
    let Some(notifier) = notifier else { return };
    if !done.is_empty() {
        let text = i18n::tr!("settings.mcp_configured", agents = done.join(", ").as_str());
        notifier.show(Notification::info(text));
    }
    if !failed.is_empty() {
        let text = i18n::tr!("settings.mcp_configure_failed", agents = failed.join(", ").as_str());
        notifier.show(Notification::warning(text));
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
