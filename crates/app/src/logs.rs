//! The log's settings (plan 89, Settings → Application → "Log"): how much is written (off,
//! errors, warnings, info, debug), where (a folder of the user's, by default `<config>/logs/`),
//! and, to look into one part, another level for some modules: each active module listed
//! with a list of its own (its default, or a level), saved as `target=level` pairs (the
//! module's crate, as the log writes it).
//! Declared in `settings.rs`; applied at once, without starting again (the filter reloaded,
//! the writer told its folder). `RUST_LOG` / `EDITOR_LOG`, when set, win over them all.

use std::path::PathBuf;
use std::sync::{LazyLock, RwLock};

use i18n::t;
use settings_model::SettingsService;
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsGroup, SettingsPage, SettingsRow, SettingsScope,
};

use crate::settings::{LOG_DIR, LOG_LEVEL, LOG_MODULES};

/// The levels to choose, by the id saved, with their labels.
pub const LEVELS: [(&str, &str); 5] = [
    ("off", "settings.log_off"),
    ("error", "settings.log_error"),
    ("warn", "settings.log_warn"),
    ("info", "settings.log_info"),
    ("debug", "settings.log_debug"),
];

/// The levels a module may have of its own, after "as the others" (0): by the id written.
pub const MODULE_LEVELS: [&str; 6] = ["off", "error", "warn", "info", "debug", "trace"];

/// A module registered: its id, its name (English), the target its lines are written under,
/// and whether it's active now.
#[derive(Debug, Clone)]
struct Noted {
    id: String,
    name: &'static str,
    target: &'static str,
    active: bool,
}

static MODULES: LazyLock<RwLock<Vec<Noted>>> = LazyLock::new(|| RwLock::new(Vec::new()));

/// Module `id` (named `name`) writes its lines under `target`: listed in the log's settings.
pub fn note_module(id: &str, name: &'static str, target: &'static str) {
    let Ok(mut modules) = MODULES.write() else { return };
    if !modules.iter().any(|m| m.id == id) {
        modules.push(Noted { id: id.to_owned(), name, target, active: false });
    }
}

/// The modules active now (the others aren't listed: they write nothing).
pub fn set_active<'a>(active: impl IntoIterator<Item = &'a str>) {
    let active: Vec<&str> = active.into_iter().collect();
    let Ok(mut modules) = MODULES.write() else { return };
    for m in modules.iter_mut() {
        m.active = active.contains(&m.id.as_str());
    }
}

/// The active modules' rows: their target and name (one per target: several modules of one
/// crate write as one), by name.
fn listed() -> Vec<(&'static str, String)> {
    let Ok(modules) = MODULES.read() else { return Vec::new() };
    let mut rows: Vec<(&'static str, String)> = Vec::new();
    for m in modules.iter().filter(|m| m.active) {
        if rows.iter().any(|(t, _)| *t == m.target) {
            continue;
        }
        rows.push((m.target, crate::workbench::frameworks::localized(&m.id, "_name", m.name)));
    }
    rows.sort_by_key(|(_, name)| name.to_lowercase());
    rows
}

/// `modules` (`a=debug, b=warn`) as its pairs, in order.
pub fn pairs(modules: &str) -> Vec<(String, String)> {
    modules
        .split(',')
        .filter_map(|p| {
            let (target, level) = p.trim().split_once('=')?;
            Some((target.trim().to_owned(), level.trim().to_lowercase()))
        })
        .filter(|(t, l)| !t.is_empty() && !l.is_empty())
        .collect()
}

/// `modules` with `target`'s level `level` (`None`: as the others: left out), the rest kept.
pub fn with_level(modules: &str, target: &str, level: Option<&str>) -> String {
    let mut pairs: Vec<(String, String)> =
        pairs(modules).into_iter().filter(|(t, _)| t != target).collect();
    if let Some(level) = level {
        pairs.push((target.to_owned(), level.to_owned()));
    }
    pairs.iter().map(|(t, l)| format!("{t}={l}")).collect::<Vec<_>>().join(",")
}

/// A module's choice as its list shows it: 0 as the others, else its level's place + 1.
fn module_choice(modules: &[(String, String)], target: &str) -> i64 {
    modules
        .iter()
        .find(|(t, _)| t == target)
        .and_then(|(_, l)| MODULE_LEVELS.iter().position(|m| m == l))
        .map_or(0, |i| i as i64 + 1)
}

/// What a module's list offers: as the others, then each level.
fn module_choices() -> Vec<(i64, String)> {
    let mut choices = vec![(0, t("settings.log_as_others"))];
    choices.extend(
        MODULE_LEVELS
            .iter()
            .enumerate()
            .map(|(i, l)| (i as i64 + 1, t(&format!("settings.log_{l}")))),
    );
    choices
}

/// What the dependencies say at most, whatever the level (their chatter isn't ours).
const QUIET: &str = "wgpu_core=warn,wgpu_hal=warn,naga=warn";

/// The filter the environment asks for (`EDITOR_LOG`, else `RUST_LOG`): it wins over the
/// settings.
pub fn from_environment() -> Option<String> {
    ["EDITOR_LOG", "RUST_LOG"]
        .iter()
        .find_map(|name| std::env::var(name).ok().filter(|v| !v.trim().is_empty()))
}

/// The filter for `level` and the modules' own levels (`modules`: `a=debug, b=warn`), the
/// dependencies kept quiet; `off`: nothing at all. A module's level that can't be read is
/// left out (said by the setting's row).
pub fn filter(level: &str, modules: &str) -> String {
    if level == "off" {
        return "off".to_owned();
    }
    let level = if LEVELS.iter().any(|(id, _)| *id == level) { level } else { "info" };
    let mut spec = format!("{level},{QUIET}");
    for part in modules.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        if logging::check_filter(part).is_ok() {
            spec.push(',');
            spec.push_str(part);
        }
    }
    spec
}

/// The parts of `modules` that can't be read, as written.
pub fn unreadable(modules: &str) -> Vec<String> {
    modules
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty() && logging::check_filter(p).is_err())
        .map(str::to_owned)
        .collect()
}

/// The folder the log goes to by default.
pub fn default_dir() -> Option<PathBuf> {
    crate::config::config_dir().map(|c| c.join("logs"))
}

/// The folder chosen (else the default one).
pub fn folder(settings: &SettingsService) -> Option<PathBuf> {
    let chosen = settings.get(&LOG_DIR);
    let chosen = chosen.trim();
    if chosen.is_empty() { default_dir() } else { Some(PathBuf::from(chosen)) }
}

/// Where the lines go now: the folder, unless the log is off (`None`: none on disk).
pub fn dir(settings: &SettingsService) -> Option<PathBuf> {
    (settings.get(&LOG_LEVEL) != "off" || from_environment().is_some())
        .then(|| folder(settings))
        .flatten()
}

/// The filter now: the environment's, else the settings'.
pub fn current_filter(settings: &SettingsService) -> String {
    from_environment()
        .unwrap_or_else(|| filter(&settings.get(&LOG_LEVEL), &settings.get(&LOG_MODULES)))
}

/// The settings handed to the log: its filter and its folder, from now.
pub fn apply(settings: &SettingsService) {
    if let Err(why) = logging::set_filter(&current_filter(settings)) {
        tracing::warn!(%why, "the log's level wasn't changed");
    }
    logging::set_dir(dir(settings));
}

/// The row setting every module at once (no module's target: a crate's name has no `*`).
const ALL: &str = "*";
/// That row's value while the modules differ.
const ALL_MIXED: i64 = -1;

/// The "Log" block, on a page of its own (Application).
#[derive(Debug)]
pub struct LogSettings;

impl SettingsContribution for LogSettings {
    fn id(&self) -> &'static str {
        "log"
    }

    fn page(&self) -> &'static str {
        "log"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage {
            id: "log",
            label: "settings.log_page",
            icon: "file-text",
            group: SettingsGroup::Application,
            order: 90,
        })
    }

    fn apply(&self, settings: &SettingsService) {
        apply(settings);
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let settings = scope.settings;
        let level = settings.get(&LOG_LEVEL);
        let place = |id: &str| LEVELS.iter().position(|(l, _)| *l == id).unwrap_or(3) as i64;
        let modules = settings.get(&LOG_MODULES);
        let mut rows = vec![
            SettingsRow::Choices {
                key: "level",
                label: t("settings.log_level"),
                detail: t("settings.log_level_detail"),
                choices: LEVELS
                    .iter()
                    .enumerate()
                    .map(|(i, (_, key))| (i as i64, t(key)))
                    .collect(),
                current: place(&level),
                default: Some(place(&LOG_LEVEL.default_value())),
            },
            SettingsRow::Text {
                key: "dir",
                label: t("settings.log_dir"),
                detail: t("settings.log_dir_detail"),
                value: settings.get(&LOG_DIR),
                placeholder: default_dir().map(|d| d.display().to_string()).unwrap_or_default(),
            },
            SettingsRow::Actions {
                text: t("settings.log_folder_actions"),
                buttons: vec![
                    ("choose", t("settings.log_choose")),
                    ("reveal", t("settings.log_reveal")),
                ],
            },
        ];
        let chosen = pairs(&modules);
        let listed = listed();
        // Every module at once: a level for all of them, or all back as the others.
        let same = listed
            .first()
            .map(|(t, _)| module_choice(&chosen, t))
            .filter(|first| listed.iter().all(|(t, _)| module_choice(&chosen, t) == *first));
        let mut all = vec![(ALL_MIXED, t("settings.log_all_mixed"))];
        all.extend(module_choices());
        rows.push(SettingsRow::Dropdown {
            key: ALL,
            label: t("settings.log_modules"),
            detail: t("settings.log_modules_detail"),
            choices: if same.is_some() { module_choices() } else { all },
            current: same.unwrap_or(ALL_MIXED),
            default: Some(0),
        });
        for (target, name) in &listed {
            rows.push(SettingsRow::Dropdown {
                key: target,
                label: name.clone(),
                detail: (*target).to_owned(),
                choices: module_choices(),
                current: module_choice(&chosen, target),
                default: Some(0),
            });
        }
        let wrong = unreadable(&modules);
        if !wrong.is_empty() {
            rows.push(SettingsRow::Notes(vec![i18n::tr!(
                "settings.log_modules_wrong",
                parts = wrong.join(", ")
            )]));
        }
        if let Some(env) = from_environment() {
            rows.push(SettingsRow::Notes(vec![i18n::tr!(
                "settings.log_environment",
                filter = env
            )]));
        }
        SettingsBlock { title: t("settings.log"), detail: t("settings.log_detail"), rows }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        if key != "level" {
            let level = usize::try_from(value - 1).ok().and_then(|i| MODULE_LEVELS.get(i)).copied();
            let modules = scope.settings.get(&LOG_MODULES);
            let modules = if key == ALL {
                if value == ALL_MIXED {
                    return;
                }
                listed().iter().fold(modules, |m, (target, _)| with_level(&m, target, level))
            } else {
                with_level(&modules, key, level)
            };
            if let Err(e) = scope.settings.set(&LOG_MODULES, modules) {
                tracing::warn!(error = %e, "can't save the modules' log levels");
            }
            return;
        }
        let Some((id, _)) = usize::try_from(value).ok().and_then(|i| LEVELS.get(i)) else { return };
        if let Err(e) = scope.settings.set(&LOG_LEVEL, (*id).to_owned()) {
            tracing::warn!(error = %e, "can't save the log's level");
        }
    }

    fn set_text(&self, scope: SettingsScope<'_>, key: &'static str, value: &str) {
        let saved = match key {
            "dir" => scope.settings.set(&LOG_DIR, value.trim().to_owned()),
            _ => return,
        };
        if let Err(e) = saved {
            tracing::warn!(error = %e, "can't save the log's setting");
        }
    }

    fn command(&self, action: &'static str) -> Option<command_model::CommandId> {
        match action {
            "choose" => Some(crate::commands::LOG_CHOOSE_FOLDER),
            "reveal" => Some(crate::commands::LOG_REVEAL_FOLDER),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/logs.rs"]
mod tests;
