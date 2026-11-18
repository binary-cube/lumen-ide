//! The MCP server's settings (`[mcp] …`): on or off (off at first), its port (`0` at first:
//! the one the system picks is then kept, so a configured agent still reaches it after a
//! restart; taken by then, another, and the page says to configure again). Its token isn't
//! a setting: it's in the keychain ([`crate::secret`]); one an older version kept here is
//! moved there and taken out of the file. — its page in the settings window (Tools › MCP server): the switch, where it
//! listens, the tools it offers, and the agents configured with it in one click.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use i18n::t;
use settings_model::{Setting, SettingsService};
use workbench_model::{
    Question, SettingsBlock, SettingsContribution, SettingsGroup, SettingsPage, SettingsRow,
    SettingsScope,
};

const PAGE: SettingsPage = SettingsPage {
    id: "mcp",
    label: "settings.mcp_section",
    icon: "server",
    group: SettingsGroup::Tools,
    order: 15,
};

pub const ENABLED: Setting<bool> = Setting::new("mcp", "enabled", || false);
pub const PORT: Setting<i64> =
    Setting::new("mcp", "port", || 0).valid(|p| (0..=65_535).contains(p));
/// Where an older version kept the token (moved to the keychain, then emptied).
pub const TOKEN: Setting<String> = Setting::new("mcp", "token", String::new);

/// The status card's button: the agents configured.
const CONFIGURE: &str = "configure";

fn save<T: settings_model::SettingType>(
    settings: &SettingsService,
    setting: &Setting<T>,
    value: T,
) {
    if let Err(e) = settings.set(setting, value) {
        tracing::warn!("can't save an MCP setting: {e}");
    }
}

/// Whether the settings want it running (the last [`apply`]): a start that comes after a
/// "stop" stops again.
static WANTED: AtomicBool = AtomicBool::new(false);
/// The port it listens on, to be saved (by a window's poll: [`take_saves`]); `0`: none.
static LISTENED: AtomicU32 = AtomicU32::new(0);
/// The settings' token was moved to the keychain: to be emptied there.
static MOVED_TOKEN: AtomicBool = AtomicBool::new(false);

/// The server as the settings say: started on a worker (its token read from the keychain, or
/// made and kept there, the first time) or stopped. A failure to start is logged and said on
/// the page. What the start learned (the port, the token moved) is saved at a window's poll.
pub fn apply(settings: &SettingsService) {
    let wanted = settings.get(&ENABLED);
    WANTED.store(wanted, Ordering::Release);
    if !wanted {
        crate::server::stop();
        return;
    }
    let older = Some(settings.get(&TOKEN)).filter(|t| !t.is_empty());
    let port = u16::try_from(settings.get(&PORT)).unwrap_or(0);
    kernel::work::run("mcp-start", move || {
        let moving = older.is_some();
        let (token, kept) = crate::secret::token(older);
        match crate::server::start(port, &token) {
            Ok(listening) if listening != port => {
                LISTENED.store(u32::from(listening), Ordering::Release)
            }
            Ok(_) => {}
            Err(why) => tracing::warn!("the MCP server didn't start: {why}"),
        }
        if moving && kept {
            MOVED_TOKEN.store(true, Ordering::Release);
        }
        // Turned off while it started: off.
        if !WANTED.load(Ordering::Acquire) {
            crate::server::stop();
        }
        crate::wake_windows();
    });
}

/// What a start learned, saved in `settings` (a window's poll, on its UI thread): the port
/// it listens on; the token moved out of the file. `true` if anything was.
pub(crate) fn take_saves(settings: &SettingsService) -> bool {
    let mut any = false;
    let port = LISTENED.swap(0, Ordering::AcqRel);
    if port != 0 && settings.get(&PORT) != i64::from(port) {
        save(settings, &PORT, i64::from(port));
        any = true;
    }
    if MOVED_TOKEN.swap(false, Ordering::AcqRel) {
        save(settings, &TOKEN, String::new());
        any = true;
    }
    any
}

/// A port as written in its field: empty, the system's pick (`0`); else 1024–65535 (the
/// ones below need the administrator). `None`: not a port.
pub(crate) fn port_of(text: &str) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() {
        return Some(0);
    }
    let port: i64 = text.parse().ok()?;
    (1024..=65_535).contains(&port).then_some(port)
}

#[derive(Debug)]
pub(crate) struct McpSettings;

impl SettingsContribution for McpSettings {
    fn id(&self) -> &'static str {
        "mcp-server"
    }

    fn page(&self) -> &'static str {
        "mcp"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(PAGE)
    }

    fn apply(&self, settings: &SettingsService) {
        apply(settings);
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let settings = scope.settings;
        let mut rows = vec![SettingsRow::Toggle {
            key: "enabled",
            detail: t("settings.mcp_enabled_detail"),
            default: Some(false),
            label: t("settings.mcp_enabled"),
            on: settings.get(&ENABLED),
        }];
        match crate::server::running() {
            Some((url, _)) => {
                let configurable = agents::any(|a| a.installed());
                let mut text = i18n::tr!("settings.mcp_listening", url = url.as_str());
                if crate::server::moved() {
                    text = format!("{text} — {}", t("settings.mcp_moved"));
                }
                rows.push(SettingsRow::Status {
                    text,
                    button: configurable.then(|| (CONFIGURE, t("settings.mcp_configure"))),
                });
            }
            None if settings.get(&ENABLED) => {
                rows.push(SettingsRow::Status { text: t("settings.mcp_failed"), button: None })
            }
            None => {}
        }
        let port = settings.get(&PORT);
        rows.push(SettingsRow::Text {
            key: "port",
            label: t("settings.mcp_port"),
            detail: t("settings.mcp_port_detail"),
            value: if port > 0 { port.to_string() } else { String::new() },
            placeholder: t("settings.mcp_port_auto"),
        });
        let mut names: Vec<&str> = mcp::tools().iter().map(|t| t.name()).collect();
        names.sort_unstable();
        rows.push(SettingsRow::Notes(vec![
            i18n::tr!("settings.mcp_tools", tools = names.join(", ").as_str()),
            t("settings.mcp_safety"),
        ]));
        SettingsBlock { title: t("settings.mcp_title"), detail: t("settings.mcp_detail"), rows }
    }

    /// Its port written: a number from 1024 to 65535, or nothing (one the system picks);
    /// anything else isn't taken. A new one moves the server there (the agents configured with
    /// the old one are to be configured again: the page says so).
    fn set_text(&self, scope: SettingsScope<'_>, key: &'static str, value: &str) {
        if key != "port" {
            return;
        }
        let Some(port) = port_of(value) else { return };
        if scope.settings.get(&PORT) == port {
            return;
        }
        save(scope.settings, &PORT, port);
        if crate::server::running().is_some() {
            crate::server::mark_moved();
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        if key == "enabled" {
            save(scope.settings, &ENABLED, value != 0);
        }
    }

    /// The agents installed configured with the server (asked first): on a worker, their
    /// commands never shown (the token is in them), the outcome said.
    fn press(
        &self,
        _scope: SettingsScope<'_>,
        action: &'static str,
    ) -> Option<(Question, workbench_model::OnYes)> {
        if action != CONFIGURE {
            return None;
        }
        let (url, _) = crate::server::running()?;
        let question = Question {
            title: t("settings.mcp_configure_title"),
            message: t("settings.mcp_configure_message"),
            icon: "server",
            yes: t("settings.mcp_configure"),
            danger: false,
            list: Vec::new(),
        };
        Some((
            question,
            Box::new(move |_host| {
                kernel::work::run("mcp-configure", move || crate::configure(&url));
            }),
        ))
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
