//! The language servers' blocks in the settings: on the Languages page (the language chosen
//! at its top), what answers it — its server (one of those described), the built-in
//! engine, or both — applied at once, and what its server is doing in the open project
//! (with Restart); on the Language servers page's "Advanced", the memory limit, when unused
//! servers stop, whether their traffic is logged, and "Add a server": a program speaking
//! LSP, for some languages, written to the user's `language-servers.toml` and offered at
//! once.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use code_intelligence::servers::Engine;
use i18n::{t, tr};
use settings_model::SettingsService;
use workbench_model::{
    OnYes, Question, SettingsBlock, SettingsContribution, SettingsLanguage, SettingsPage,
    SettingsRow, SettingsScope,
};

use crate::catalog;
use crate::projects::{self, ProjectServers};
use crate::report::{language_name, state_text};
use crate::settings::{self, Choice};

/// The engine choices' values besides the servers' (theirs: their place, from 1).
const BUILT_IN: i64 = -1;
const BOTH: i64 = -2;

/// "Engine" (Settings → Languages): what answers the language chosen at the page's top —
/// a server, the built-in engine, both — and a server the user installs (Install, its
/// licence key).
#[derive(Debug, Default)]
pub struct ServerSettings {
    /// The installs' generation last drawn (a change draws the block again).
    drawn: std::sync::atomic::AtomicU64,
}

/// The open project's servers, found by its settings.
fn project_of(scope: &SettingsScope<'_>) -> Option<Arc<ProjectServers>> {
    let settings = scope.project?;
    projects::all().into_iter().find(|p| p.uses(settings))
}

/// The page's language, when the servers know it.
fn language_of(scope: &SettingsScope<'_>) -> Option<String> {
    let language = scope.language?;
    catalog::catalog().languages().iter().any(|l| l == language).then(|| language.to_owned())
}

/// Every language the servers know, for the page's chooser.
fn known_languages() -> Vec<SettingsLanguage> {
    catalog::catalog()
        .languages()
        .iter()
        .map(|l| SettingsLanguage { id: l.clone(), name: language_name(l) })
        .collect()
}

/// What a language's server is doing in the project, as a status line.
fn status_line(project: Option<&ProjectServers>, language: &str) -> String {
    let Some(project) = project else { return t("settings.lsp_no_project") };
    let now = Instant::now();
    let running = project.servers(|servers| {
        servers
            .iter()
            .filter(|r| r.languages.contains(language))
            .map(|r| {
                let snapshot = r.client.status().snapshot(now);
                let name = match &r.version {
                    Some(v) => format!("{} {v}", r.spec.name),
                    None => r.spec.name.clone(),
                };
                tr!(
                    "settings.lsp_running",
                    name = name,
                    state = state_text(&snapshot),
                    count = snapshot.documents as i64
                )
            })
            .collect::<Vec<_>>()
    });
    if !running.is_empty() {
        return running.join(" · ");
    }
    match project.missing(language) {
        Some(why) => tr!("settings.lsp_missing", reason = why),
        None => t("settings.lsp_not_started"),
    }
}

/// The engine choices' place of the server `name` (the first: 1).
fn server_place(servers: &[String], name: Option<&String>) -> i64 {
    name.and_then(|n| servers.iter().position(|s| s == n)).map_or(1, |i| i as i64 + 1)
}

/// The engine choice's value for `choice`.
fn engine_value(servers: &[String], choice: &Choice) -> i64 {
    match choice.engine {
        Engine::Server => server_place(servers, choice.server.as_ref()),
        Engine::BuiltIn => BUILT_IN,
        Engine::Both => BOTH,
    }
}

impl SettingsContribution for ServerSettings {
    fn id(&self) -> &'static str {
        "language-servers"
    }

    fn page(&self) -> &'static str {
        SettingsPage::LANGUAGES.id
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::LANGUAGES)
    }

    fn order(&self) -> u16 {
        10
    }

    fn languages(&self, _scope: SettingsScope<'_>) -> Vec<SettingsLanguage> {
        known_languages()
    }

    fn apply(&self, settings: &SettingsService) {
        settings::apply(settings);
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let mut block = SettingsBlock {
            title: t("settings.lsp_engine_title"),
            detail: t("settings.lsp_detail"),
            rows: Vec::new(),
        };
        self.drawn.store(crate::install::generation(), std::sync::atomic::Ordering::Relaxed);
        // Another language (SQL's, only formatted): nothing to say.
        let Some(language) = language_of(&scope) else { return block };
        // Those the user installs, once installed.
        let servers = settings::offered(&language);
        let mut choices: Vec<(i64, String)> = servers
            .iter()
            .enumerate()
            .map(|(i, n)| (i as i64 + 1, tr!("settings.lsp_engine_server", name = n.as_str())))
            .collect();
        choices.push((BUILT_IN, t("settings.lsp_engine_built_in")));
        choices.push((BOTH, t("settings.lsp_engine_both")));
        let choice = settings::config().choice(&language);
        block.rows.push(SettingsRow::Choices {
            key: "engine",
            label: t("settings.lsp_engine"),
            detail: t("settings.lsp_engine_detail"),
            choices,
            current: engine_value(&servers, &choice),
            default: Some(engine_value(&servers, &settings::default_choice(&language))),
        });
        if let Some(spec) = installable(&language) {
            install_rows(&mut block.rows, &spec, &scope);
        }
        block
    }

    fn poll(&self) -> bool {
        crate::install::generation() != self.drawn.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn set_text(&self, scope: SettingsScope<'_>, key: &'static str, value: &str) {
        let (Some(language), "licence") = (language_of(&scope), key) else { return };
        let Some(id) = installable(&language).and_then(|spec| licence_id(&spec)) else { return };
        // The keychain may wait (or ask the user): never on this thread.
        let mut value = value.trim().to_owned();
        kernel::work::run("lsp-secrets", move || {
            let _ = crate::licence::save(&id, &value);
            kernel::secret::wipe(&mut value);
            crate::install::bump();
        });
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        if key != "engine" {
            return;
        }
        let Some(language) = language_of(&scope) else { return };
        let servers = settings::offered(&language);
        let first = catalog::catalog().for_language(&language).next().map(|s| s.name.clone());
        // The first server described is "the first that can start" (saved without its
        // name); another one by its name.
        let named = |name: Option<String>| name.filter(|n| Some(n) != first.as_ref());
        let choice = match value {
            BUILT_IN => Choice { engine: Engine::BuiltIn, server: None },
            // Both keeps the server chosen.
            BOTH => Choice {
                engine: Engine::Both,
                server: named(settings::config().choice(&language).server),
            },
            n => {
                let server = usize::try_from(n - 1).ok().and_then(|i| servers.get(i).cloned());
                Choice { engine: Engine::Server, server: named(server) }
            }
        };
        settings::save_choice(scope.settings, &language, &choice);
    }

    fn press(&self, scope: SettingsScope<'_>, action: &'static str) -> Option<(Question, OnYes)> {
        let spec = language_of(&scope).and_then(|l| installable(&l))?;
        matches!(action, "install" | "uninstall" | "stop-install" | "import-licence")
            .then(|| press_install(&spec, action, &scope))
            .flatten()
    }
}

/// "Server" (Settings → Languages, last): what the chosen language's server is doing in the
/// open project, with Restart while one runs.
#[derive(Debug, Default)]
pub struct ServerStatus;

impl SettingsContribution for ServerStatus {
    fn id(&self) -> &'static str {
        "language-server-status"
    }

    fn page(&self) -> &'static str {
        SettingsPage::LANGUAGES.id
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::LANGUAGES)
    }

    fn order(&self) -> u16 {
        40
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let mut block = SettingsBlock {
            title: t("settings.lsp_status_title"),
            detail: t("settings.lsp_status_detail"),
            rows: Vec::new(),
        };
        let Some(language) = language_of(&scope) else { return block };
        let servers = settings::offered(&language);
        let choice = settings::config().choice(&language);
        let project = project_of(&scope);
        // A server its description leaves off by default on this OS (phpactor on Windows):
        // said, still offered.
        let off_here = settings::default_choice(&language).engine == Engine::BuiltIn;
        let line = if choice.engine.uses_server() {
            status_line(project.as_deref(), &language)
        } else if off_here && let Some(name) = servers.first() {
            tr!("settings.lsp_built_in_by_default", name = name.as_str())
        } else {
            t("settings.lsp_built_in_only")
        };
        // Restart: only a server running for the language (a status card's button reads as
        // destructive: never offered for nothing).
        let running = project
            .as_ref()
            .is_some_and(|p| p.servers(|s| s.iter().any(|r| r.languages.contains(&language))));
        let restart = running && choice.engine.uses_server();
        block.rows.push(SettingsRow::Status {
            text: line,
            button: restart.then(|| ("restart", t("settings.lsp_restart"))),
        });
        block
    }

    fn choose(&self, _scope: SettingsScope<'_>, _key: &'static str, _value: i64) {}

    fn press(&self, scope: SettingsScope<'_>, action: &'static str) -> Option<(Question, OnYes)> {
        if action == "restart"
            && let (Some(project), Some(language)) = (project_of(&scope), language_of(&scope))
        {
            let ids: Vec<u128> = project.servers(|s| {
                s.iter().filter(|r| r.languages.contains(&language)).map(|r| r.id).collect()
            });
            for id in ids {
                project.restart(id);
            }
            project.refresh();
        }
        None
    }
}

/// "Limits and log" (Settings → Language servers → Advanced): the memory limit, when unused
/// servers stop, whether their traffic is logged, where the bundled ones are.
#[derive(Debug, Default)]
pub struct ServerLimits;

impl SettingsContribution for ServerLimits {
    fn id(&self) -> &'static str {
        "language-servers-limits"
    }

    fn page(&self) -> &'static str {
        SettingsPage::LANGUAGE_SERVERS.id
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::LANGUAGE_SERVERS)
    }

    fn order(&self) -> u16 {
        20
    }

    fn advanced(&self) -> bool {
        true
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let s = scope.settings;
        let mut rows = vec![
            SettingsRow::Slider {
                key: "memory",
                label: t("settings.lsp_memory"),
                detail: t("settings.lsp_memory_detail"),
                stops: settings::MAX_MEMORY_CHOICES
                    .iter()
                    .map(|mb| (*mb, i18n::bytes(*mb as u64 * 1024 * 1024)))
                    .collect(),
                current: s.get(&settings::MAX_MEMORY),
                default: Some(settings::MAX_MEMORY.default_value()),
            },
            SettingsRow::Segmented {
                key: "idle",
                label: t("settings.lsp_idle"),
                detail: t("settings.lsp_idle_detail"),
                choices: settings::IDLE_MINUTES_CHOICES
                    .iter()
                    .map(|m| {
                        (
                            *m,
                            if *m == 0 {
                                t("settings.lsp_idle_never")
                            } else {
                                tr!("settings.lsp_idle_minutes", count = *m)
                            },
                        )
                    })
                    .collect(),
                current: s.get(&settings::IDLE_MINUTES),
                default: Some(settings::IDLE_MINUTES.default_value()),
            },
            SettingsRow::Toggle {
                key: "trace",
                label: t("settings.lsp_trace"),
                detail: t("settings.lsp_trace_detail"),
                on: s.get(&settings::TRACE),
                default: Some(settings::TRACE.default_value()),
            },
        ];
        let bundle = match catalog::bundle() {
            Some(b) => tr!("settings.lsp_bundled", path = b.root.display().to_string()),
            None => t("settings.lsp_not_bundled"),
        };
        rows.push(SettingsRow::Notes(vec![t("settings.lsp_embedded"), bundle]));
        SettingsBlock {
            title: t("settings.lsp_limits_title"),
            detail: t("settings.lsp_limits_detail"),
            rows,
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let s = scope.settings;
        match key {
            "memory" => save(s, &settings::MAX_MEMORY, value),
            "idle" => save(s, &settings::IDLE_MINUTES, value),
            "trace" => {
                if let Err(e) = s.set(&settings::TRACE, value != 0) {
                    tracing::warn!("can't save a language server setting: {e}");
                }
                settings::apply(s);
            }
            _ => {}
        }
    }
}

/// The first server of `language` the user installs, if it has one.
fn installable(language: &str) -> Option<lsp_client::spec::ServerSpec> {
    catalog::catalog().for_language(language).find(|s| s.install.is_some()).cloned()
}

/// The id of server `spec`'s licence key among the secrets, if it takes one.
fn licence_id(spec: &lsp_client::spec::ServerSpec) -> Option<String> {
    spec.secret_options.values().next().cloned()
}

/// A size as said (`4.8 MB`).
fn size_text(bytes: u64) -> String {
    i18n::bytes(bytes)
}

/// The rows of a server the user installs: where it is (Install / Uninstall / its progress),
/// its licence key (a secret: set or not, never shown), the licence file offered.
fn install_rows(
    rows: &mut Vec<SettingsRow>,
    spec: &lsp_client::spec::ServerSpec,
    scope: &SettingsScope<'_>,
) {
    let Some(install) = &spec.install else { return };
    let name = spec.name.as_str();
    let wake = scope.wake.cloned();
    let state = crate::install::state(spec);
    let error = state.error.as_deref().map(|e| tr!("settings.lsp_install_failed", reason = e));
    match (&state.found, state.installing) {
        (_, Some((received, total))) => rows.push(SettingsRow::Status {
            text: match total {
                Some(total) => tr!(
                    "settings.lsp_installing",
                    name = name,
                    version = install.version.as_str(),
                    received = size_text(received),
                    total = size_text(total)
                ),
                None => tr!(
                    "settings.lsp_installing_unknown",
                    name = name,
                    version = install.version.as_str(),
                    received = size_text(received)
                ),
            },
            button: Some(("stop-install", t("settings.lsp_install_stop"))),
        }),
        (crate::install::Found::Unknown | crate::install::Found::Looking, None) => {
            rows.push(SettingsRow::Status {
                text: tr!("settings.lsp_looking", name = name),
                button: None,
            });
        }
        (crate::install::Found::Missing, None) => {
            // npm's global folder looked in too (once: the system's npm, in the background).
            crate::install::look(spec, true, wake.clone());
            let text = error.unwrap_or_else(|| match install.size {
                Some(size) => {
                    tr!("settings.lsp_not_installed", name = name, size = size_text(size))
                }
                None => tr!("settings.lsp_not_installed_unknown", name = name),
            });
            rows.push(SettingsRow::Actions {
                text,
                buttons: vec![("install", tr!("settings.lsp_install", name = name))],
            });
        }
        (crate::install::Found::Installed(found), None) => {
            let text = match (found.ours, error) {
                (_, Some(error)) => error,
                (true, None) => {
                    tr!("settings.lsp_installed", name = name, version = found.version.as_str())
                }
                (false, None) => {
                    tr!("settings.lsp_found", name = name, version = found.version.as_str())
                }
            };
            rows.push(SettingsRow::Status {
                text,
                button: found.ours.then(|| ("uninstall", t("settings.lsp_uninstall"))),
            });
            if let Some(id) = licence_id(spec) {
                let set = crate::licence::is_set(&id, wake.clone());
                rows.push(SettingsRow::Secret {
                    key: "licence",
                    detail: t("settings.lsp_licence_detail"),
                    label: tr!("settings.lsp_licence", name = name),
                    set: set == Some(true),
                    placeholder: match set {
                        Some(true) => t("settings.lsp_licence_set"),
                        Some(false) => t("settings.lsp_licence_hint"),
                        None => t("settings.lsp_licence_reading"),
                    },
                });
                if set == Some(false)
                    && let Some(file) = crate::licence::licence_file()
                {
                    rows.push(SettingsRow::Actions {
                        text: tr!("settings.lsp_licence_found", path = file.display().to_string()),
                        buttons: vec![("import-licence", t("settings.lsp_licence_use"))],
                    });
                }
            }
        }
    }
    let licence = install.licence_url.as_deref().unwrap_or(crate::install::REGISTRY_NOTE);
    let mut notes = vec![tr!("settings.lsp_install_note", name = name, licence = licence)];
    if let crate::install::Found::Installed(found) = &state.found {
        notes.push(tr!("settings.lsp_install_place", path = found.dir.display().to_string()));
    }
    rows.push(SettingsRow::Notes(notes));
}

/// A button of a server the user installs: Install (asked first: its licence, what's
/// downloaded and where), Uninstall (asked first), Stop, the licence file taken in.
fn press_install(
    spec: &lsp_client::spec::ServerSpec,
    action: &'static str,
    scope: &SettingsScope<'_>,
) -> Option<(Question, OnYes)> {
    let install = spec.install.clone()?;
    let name = spec.name.clone();
    let wake = scope.wake.cloned();
    match action {
        "stop-install" => {
            crate::install::stop(&name);
            None
        }
        "import-licence" => {
            let id = licence_id(spec)?;
            let file = crate::licence::licence_file()?;
            kernel::work::run("lsp-secrets", move || {
                if let Err(e) = crate::licence::import(&id, &file) {
                    tracing::warn!("a licence file can't be used: {e}");
                }
                crate::install::bump();
                if let Some(wake) = wake {
                    wake();
                }
            });
            None
        }
        "install" => {
            let dir = crate::install::installs_dir()
                .map(|d| lsp_client::install::version_dir(&d, &name, &install.version))
                .map_or_else(String::new, |d| d.display().to_string());
            let question = Question {
                title: tr!("settings.lsp_install_title", name = name.as_str()),
                message: tr!(
                    "settings.lsp_install_message",
                    name = name.as_str(),
                    version = install.version.as_str(),
                    size = install.size.map_or_else(String::new, size_text),
                    licence =
                        install.licence_url.as_deref().unwrap_or(crate::install::REGISTRY_NOTE),
                    path = dir
                ),
                icon: "download",
                yes: tr!("settings.lsp_install", name = name.as_str()),
                danger: false,
                list: Vec::new(),
            };
            let spec = spec.clone();
            let then: OnYes = Box::new(move |host| {
                let id = workbench_model::NotificationId::new();
                host.notify(
                    workbench_model::Notification::progress(tr!(
                        "lsp.installing",
                        name = spec.name.as_str()
                    ))
                    .with_id(id),
                );
                host.spawn(Box::new(move || {
                    let outcome = crate::install::install(&spec, wake.as_deref());
                    Box::new(move |host| {
                        let notification = match outcome {
                            Ok(found) => workbench_model::Notification::success(tr!(
                                "lsp.installed",
                                name = spec.name.as_str(),
                                version = found.version.as_str()
                            )),
                            Err(why) => workbench_model::Notification::error(tr!(
                                "lsp.install_failed",
                                name = spec.name.as_str()
                            ))
                            .with_detail(why),
                        };
                        host.update_notification(notification.with_id(id));
                    })
                }));
            });
            Some((question, then))
        }
        "uninstall" => {
            let question = Question {
                title: tr!("settings.lsp_uninstall_title", name = name.as_str()),
                message: tr!("settings.lsp_uninstall_message", name = name.as_str()),
                icon: "trash",
                yes: t("settings.lsp_uninstall"),
                danger: true,
                list: Vec::new(),
            };
            let spec = spec.clone();
            let then: OnYes = Box::new(move |host| {
                host.spawn(Box::new(move || {
                    let outcome = crate::install::uninstall(&spec);
                    if let Some(wake) = wake {
                        wake();
                    }
                    Box::new(move |host| {
                        let notification = match outcome {
                            Ok(()) => workbench_model::Notification::success(tr!(
                                "lsp.uninstalled",
                                name = spec.name.as_str()
                            )),
                            Err(why) => workbench_model::Notification::error(tr!(
                                "lsp.uninstall_failed",
                                name = spec.name.as_str()
                            ))
                            .with_detail(why),
                        };
                        host.notify(notification);
                    })
                }));
            });
            Some((question, then))
        }
        _ => None,
    }
}

/// `value` saved and applied (said in the log when it can't be).
fn save(settings: &SettingsService, setting: &settings_model::Setting<i64>, value: i64) {
    if !setting.accepts(&value) {
        return;
    }
    if let Err(e) = settings.set(setting, value) {
        tracing::warn!("can't save a language server setting: {e}");
    }
    settings::apply(settings);
}

/// The server being added (what's written in the fields so far).
#[derive(Debug, Default, Clone)]
struct Draft {
    name: String,
    command: String,
    languages: String,
    args: String,
    /// How the last addition went.
    outcome: Option<String>,
}

/// "Add a server".
#[derive(Debug, Default)]
pub struct AddServer {
    draft: Mutex<Draft>,
}

/// A list typed with commas or spaces.
fn words(text: &str) -> Vec<String> {
    text.split([',', ' ']).map(str::trim).filter(|w| !w.is_empty()).map(str::to_owned).collect()
}

impl SettingsContribution for AddServer {
    fn id(&self) -> &'static str {
        "language-servers-add"
    }

    fn page(&self) -> &'static str {
        "language-servers"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::LANGUAGE_SERVERS)
    }

    fn order(&self) -> u16 {
        30
    }

    fn advanced(&self) -> bool {
        true
    }

    fn block(&self, _scope: SettingsScope<'_>) -> SettingsBlock {
        let draft = self.draft.lock().map(|d| d.clone()).unwrap_or_default();
        let text = |key, label: &str, value: &str, placeholder: &str| SettingsRow::Text {
            key,
            label: t(label),
            detail: t(&format!("{label}_detail")),
            value: value.to_owned(),
            placeholder: t(placeholder),
        };
        let user = catalog::user_file().map_or_else(String::new, |p| p.display().to_string());
        SettingsBlock {
            title: t("settings.lsp_add_title"),
            detail: t("settings.lsp_add_detail"),
            rows: vec![
                text("name", "settings.lsp_add_name", &draft.name, "settings.lsp_add_name_hint"),
                text(
                    "command",
                    "settings.lsp_add_command",
                    &draft.command,
                    "settings.lsp_add_command_hint",
                ),
                text(
                    "languages",
                    "settings.lsp_add_languages",
                    &draft.languages,
                    "settings.lsp_add_languages_hint",
                ),
                text("args", "settings.lsp_add_args", &draft.args, "settings.lsp_add_args_hint"),
                SettingsRow::Status {
                    text: draft.outcome.unwrap_or_else(|| t("settings.lsp_add_ready")),
                    button: Some(("add", t("settings.lsp_add_button"))),
                },
                SettingsRow::Notes(vec![tr!("settings.lsp_add_file", path = user)]),
            ],
        }
    }

    fn choose(&self, _scope: SettingsScope<'_>, _key: &'static str, _value: i64) {}

    fn set_text(&self, _scope: SettingsScope<'_>, key: &'static str, value: &str) {
        let Ok(mut draft) = self.draft.lock() else { return };
        let value = value.trim().to_owned();
        match key {
            "name" => draft.name = value,
            "command" => draft.command = value,
            "languages" => draft.languages = value,
            "args" => draft.args = value,
            _ => {}
        }
    }

    fn press(&self, _scope: SettingsScope<'_>, action: &'static str) -> Option<(Question, OnYes)> {
        if action != "add" {
            return None;
        }
        let Ok(mut draft) = self.draft.lock() else { return None };
        let languages = words(&draft.languages);
        let outcome = if draft.name.is_empty() || draft.command.is_empty() || languages.is_empty() {
            t("settings.lsp_add_incomplete")
        } else {
            match catalog::add_user_server(
                &draft.name,
                &draft.command,
                &draft.args.split_whitespace().map(str::to_owned).collect::<Vec<_>>(),
                &languages,
            ) {
                Ok(_) => {
                    projects::refresh_all();
                    let added = tr!("settings.lsp_add_done", name = draft.name.as_str());
                    *draft = Draft::default();
                    added
                }
                Err(why) => tr!("settings.lsp_add_failed", reason = why),
            }
        };
        draft.outcome = Some(outcome);
        None
    }
}

#[cfg(test)]
#[path = "../tests/unit/page.rs"]
mod tests;
