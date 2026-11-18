//! Switching a language's engine from where its files are: the server a download bar
//! offered, picked (`PickOffered`), and "Choose the File's Language Engine…" (the status
//! bar's engine segment, a list: each server the language describes — ready, to download
//! or to install —, the built-in engine, both). A pick applies at once and is saved as the
//! language's engine; a server not there yet is downloaded, or installed once its licence
//! is agreed to (as from the settings), in the background.

use code_intelligence::servers::Engine;
use i18n::{t, tr};
use lsp_client::spec::ServerSpec;
use workbench_model::{
    ChoiceItem, ChoiceList, CommandHandler, Notification, NotificationId, Question, WorkbenchHost,
};

use crate::{catalog, downloads, settings};

/// The active file's language, if it describes a server.
fn active_language(host: &dyn WorkbenchHost) -> Option<String> {
    let file = host.active_file()?;
    let language = host.language_of(&file.path)?;
    catalog::catalog().for_language(&language).next()?;
    Some(language)
}

/// What answers `language`'s files in `project` now, as the status bar names it: the
/// running server, the built-in engine (chosen while a server could answer), both; `None`
/// while none of its servers is there (nothing to switch between).
pub fn engine_label(project: kernel::ProjectId, language: &str) -> Option<String> {
    // The first ready server, worked out again only when the installs, the downloads or
    // the settings changed (asked at every frame by the status bar).
    let generation = (crate::install::generation(), settings::generation());
    let first_ready = {
        let mut cache = READY.lock().ok()?;
        let cache = cache.get_or_insert_with(std::collections::HashMap::new);
        match cache.get(language) {
            Some((seen, name)) if *seen == generation => name.clone(),
            _ => {
                let config = settings::config();
                let name = catalog::catalog()
                    .for_language(language)
                    .find(|s| !config.disabled.contains(&s.name) && launchable(s))
                    .map(|s| s.name.clone());
                cache.insert(language.to_owned(), (generation, name.clone()));
                name
            }
        }
    }?;
    let choice = settings::config().choice(language);
    let running = crate::projects::project(project).and_then(|p| {
        p.servers(|s| {
            s.iter().find(|r| r.languages.contains(language)).map(|r| r.spec.name.clone())
        })
    });
    let name = running.or(choice.server.clone()).unwrap_or(first_ready);
    Some(match choice.engine {
        Engine::BuiltIn => t("lsp.engine_status_built_in"),
        Engine::Server => name,
        Engine::Both => tr!("lsp.engine_status_both", server = name.as_str()),
    })
}

/// Whether server `spec` would start now (its program and runtime there: bundled,
/// downloaded or installed).
fn launchable(spec: &ServerSpec) -> bool {
    let bundle = catalog::bundle();
    let places = catalog::places(bundle.as_deref(), spec, std::path::Path::new("."), 1024);
    spec.launch(&places).is_ok()
}

/// Per language, its first ready server and the generations it was worked out for.
type Ready = std::collections::HashMap<String, ((u64, u64), Option<String>)>;
static READY: std::sync::Mutex<Option<Ready>> = std::sync::Mutex::new(None);

/// Picks `server` (`None`: the built-in engine; `both`: with it) for `language`: saved,
/// and what it needs started — downloads, or an install asked first (its licence).
fn pick(host: &mut dyn WorkbenchHost, language: &str, server: Option<String>, both: bool) {
    let Some(settings_service) = host.user_settings() else { return };
    let engine = match (&server, both) {
        (None, _) => Engine::BuiltIn,
        (Some(_), true) => Engine::Both,
        (Some(_), false) => Engine::Server,
    };
    let spec = server.as_deref().and_then(|n| catalog::catalog().named(n).cloned());
    let choice = settings::Choice { engine, server };
    let installs =
        spec.as_ref().is_some_and(|s| s.install.is_some() && !crate::install::available(s));
    if !installs {
        settings::save_choice(settings_service, language, &choice);
        if let Some(spec) = &spec {
            let needed = downloads::needed_for(spec);
            if !needed.is_empty() {
                downloads::start(needed, None);
            }
        }
        return;
    }
    let Some(spec) = spec else { return };
    let Some(install) = spec.install.clone() else { return };
    let dir = crate::install::installs_dir()
        .map(|d| lsp_client::install::version_dir(&d, &spec.name, &install.version))
        .map_or_else(String::new, |d| d.display().to_string());
    let question = Question {
        title: tr!("settings.lsp_install_title", name = spec.name.as_str()),
        message: tr!(
            "settings.lsp_install_message",
            name = spec.name.as_str(),
            version = install.version.as_str(),
            size = install.size.map_or_else(String::new, i18n::bytes),
            licence = install.licence_url.as_deref().unwrap_or(crate::install::REGISTRY_NOTE),
            path = dir
        ),
        icon: "download",
        yes: tr!("settings.lsp_install", name = spec.name.as_str()),
        danger: false,
        list: Vec::new(),
    };
    let language = language.to_owned();
    host.confirm(
        question,
        Box::new(move |host| {
            if let Some(service) = host.user_settings() {
                settings::save_choice(service, &language, &choice);
            }
            let needed = downloads::needed_for(&spec);
            if !needed.is_empty() {
                downloads::start(needed, None);
            }
            let id = NotificationId::new();
            host.notify(
                Notification::progress(tr!("lsp.installing", name = spec.name.as_str()))
                    .with_id(id),
            );
            host.spawn(Box::new(move || {
                let outcome = crate::install::install(&spec, None);
                Box::new(move |host: &mut dyn WorkbenchHost| {
                    let notification = match outcome {
                        Ok(found) => Notification::success(tr!(
                            "lsp.installed",
                            name = spec.name.as_str(),
                            version = found.version.as_str()
                        )),
                        Err(why) => Notification::error(tr!(
                            "lsp.install_failed",
                            name = spec.name.as_str()
                        ))
                        .with_detail(why),
                    };
                    host.update_notification(notification.with_id(id));
                })
            }));
        }),
    );
}

/// The server a download bar offered, picked.
pub fn pick_offered(host: &mut dyn WorkbenchHost, language: &str, server: String) {
    pick(host, language, Some(server), false);
}

/// Choose the File's Language Engine…: the language's servers (each one's state), the
/// built-in engine, both.
pub struct ChooseEngine;

impl CommandHandler for ChooseEngine {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        active_language(host).is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(language) = active_language(host) else { return false };
        let config = settings::config();
        let choice = config.choice(&language);
        let catalog = catalog::catalog();
        let servers: Vec<ServerSpec> = catalog
            .for_language(&language)
            .filter(|s| !config.disabled.contains(&s.name))
            .cloned()
            .collect();
        // The server answering when it's a server's: the one chosen, else the first ready.
        let ready =
            |s: &ServerSpec| crate::install::available(s) && downloads::needed_for(s).is_empty();
        let answering = choice
            .server
            .clone()
            .or_else(|| servers.iter().find(|s| ready(s)).map(|s| s.name.clone()));
        let current =
            |name: &str| choice.engine == Engine::Server && answering.as_deref() == Some(name);
        let mut items = Vec::new();
        let mut rows: Vec<(Option<String>, bool)> = Vec::new();
        for spec in &servers {
            let needed = downloads::needed_for(spec);
            let installs = spec.install.is_some() && !crate::install::available(spec);
            let detail = if installs {
                let size = spec.install.as_ref().and_then(|i| i.size).unwrap_or(0)
                    + downloads::size_of(&needed);
                tr!("lsp.engine_to_install", size = i18n::bytes(size))
            } else if !needed.is_empty() {
                tr!("lsp.engine_to_download", size = i18n::bytes(downloads::size_of(&needed)))
            } else if !launchable(spec) {
                t("lsp.engine_missing")
            } else if current(&spec.name) {
                t("lsp.engine_current")
            } else {
                t("lsp.engine_ready")
            };
            items.push(ChoiceItem {
                icon: if current(&spec.name) { "check" } else { "server" },
                label: spec.name.clone(),
                detail,
            });
            rows.push((Some(spec.name.clone()), false));
        }
        let built_in = choice.engine == Engine::BuiltIn;
        items.push(ChoiceItem {
            icon: if built_in { "check" } else { "cpu" },
            label: t("lsp.engine_built_in"),
            detail: if built_in { t("lsp.engine_current") } else { String::new() },
        });
        rows.push((None, false));
        if let Some(first) = servers.first() {
            let both = choice.engine == Engine::Both;
            let server = answering.clone().unwrap_or_else(|| first.name.clone());
            items.push(ChoiceItem {
                icon: if both { "check" } else { "layers" },
                label: tr!("lsp.engine_both", server = server.as_str()),
                detail: if both { t("lsp.engine_current") } else { String::new() },
            });
            rows.push((Some(server), true));
        }
        let list = ChoiceList {
            title: tr!(
                "lsp.engine_title",
                language = crate::report::language_name(&language).as_str()
            ),
            subject: host
                .active_file()
                .and_then(|f| f.path.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_default(),
            icon: "server",
            action: t("lsp.engine_use"),
            items,
        };
        host.choose(
            list,
            Box::new(move |i, host| {
                if let Some((server, both)) = rows.get(i).cloned() {
                    pick(host, &language, server, both);
                }
            }),
        )
    }
}
