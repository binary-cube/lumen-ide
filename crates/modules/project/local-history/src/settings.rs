//! The local history's settings: how long it keeps versions and how many per file
//! (`[history] days`, `[history] versions` in `settings.toml`), handed to the history (it
//! reads them each time a version is kept: a change applies at once); its block in the
//! settings' "Editor" section, with what it holds for the open project (counted in the
//! background: a project's history can be many thousands of files) and deleting it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use i18n::{t, tr};
use settings_model::{Setting, SettingsService};
use workbench_model::{
    OnYes, Question, SettingsBlock, SettingsContribution, SettingsPage, SettingsRow, SettingsScope,
};

/// How long the local history keeps versions, in days (`[history] days`).
pub const HISTORY_DAYS: Setting<i64> =
    Setting::new("history", "days", || 30).valid(|d| (1..=3650).contains(d));

/// How many versions the local history keeps per file, the newest (`[history] versions`).
pub const HISTORY_VERSIONS: Setting<i64> =
    Setting::new("history", "versions", || 200).valid(|v| (10..=10_000).contains(v));

/// The choices offered in the settings.
pub const DAYS: [i64; 4] = [7, 30, 90, 365];
pub const VERSIONS: [i64; 4] = [50, 200, 500, 1000];

const CLEAR: &str = "clear";

/// The block in the settings' "Editor" section.
#[derive(Debug)]
pub(crate) struct HistorySettings;

impl SettingsContribution for HistorySettings {
    fn id(&self) -> &'static str {
        "history"
    }

    fn page(&self) -> &'static str {
        "files"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::FILES)
    }

    fn order(&self) -> u16 {
        30
    }

    fn apply(&self, settings: &SettingsService) {
        let days = settings.get(&HISTORY_DAYS).max(1) as u64;
        configure(history::Retention {
            max_age: std::time::Duration::from_secs(days * 24 * 3600),
            max_per_file: settings.get(&HISTORY_VERSIONS).max(1) as usize,
            ..history::Retention::default()
        });
    }

    fn poll(&self) -> bool {
        COUNTED.swap(false, Ordering::AcqRel)
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let settings = scope.settings;
        let used = scope.project_data.map(|data| counted(&data.join(crate::FOLDER), scope.wake));
        let text = match used {
            Some(None) => t("settings.history_counting"),
            Some(Some((bytes, versions))) => {
                tr!("settings.history_used", size = i18n::bytes(bytes), count = versions)
            }
            None => t("settings.history_no_project"),
        };
        let button = used
            .flatten()
            .is_some_and(|(_, versions)| versions > 0)
            .then(|| (CLEAR, t("settings.history_clear")));
        SettingsBlock {
            title: t("settings.history"),
            detail: t("settings.history_detail"),
            rows: vec![
                SettingsRow::Choices {
                    key: "days",
                    detail: t("settings.history_days_detail"),
                    default: Some(HISTORY_DAYS.default_value()),
                    label: t("settings.history_days"),
                    choices: DAYS
                        .iter()
                        .map(|&d| (d, tr!("settings.history_days_value", count = d)))
                        .collect(),
                    current: settings.get(&HISTORY_DAYS),
                },
                SettingsRow::Choices {
                    key: "versions",
                    detail: t("settings.history_versions_detail"),
                    default: Some(HISTORY_VERSIONS.default_value()),
                    label: t("settings.history_versions"),
                    choices: VERSIONS.iter().map(|&v| (v, v.to_string())).collect(),
                    current: settings.get(&HISTORY_VERSIONS),
                },
                SettingsRow::Status { text, button },
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let settings = scope.settings;
        let setting = match key {
            "days" => &HISTORY_DAYS,
            "versions" => &HISTORY_VERSIONS,
            _ => return,
        };
        if let Err(e) = settings.set(setting, value) {
            tracing::warn!(error = %e, "can't save a local history setting");
        }
    }

    /// Deleting the project's history, once confirmed.
    fn press(&self, scope: SettingsScope<'_>, action: &'static str) -> Option<(Question, OnYes)> {
        if action != CLEAR {
            return None;
        }
        let dir = scope.project_data?.join(crate::FOLDER);
        // The figure the block shows (counted in the background): a press reads no disk.
        let versions = known(&dir).map(|(_, versions)| versions);
        let question = Question {
            title: t("dialog.history_clear_title"),
            message: match versions {
                Some(count) => tr!("dialog.history_clear", count = count),
                // Not counted yet (the block's count still runs): said without a figure.
                None => t("dialog.history_clear_all"),
            },
            icon: "trash",
            yes: t("dialog.delete"),
            danger: true,
            list: Vec::new(),
        };
        let then: OnYes = Box::new(move |_| {
            // A long history is many files: deleted off the UI thread.
            kernel::work::run("local-history", move || {
                match std::fs::remove_dir_all(&dir) {
                    Ok(()) => tracing::info!(dir = %dir.display(), "local history deleted"),
                    Err(e) => tracing::warn!(error = %e, "the local history can't be deleted"),
                }
                // Counted again next time.
                if let Ok(mut all) = USAGE.lock() {
                    all.remove(&dir);
                }
            });
        });
        Some((question, then))
    }
}

/// What the projects' histories hold, as last counted (`None` while the first count runs),
/// and when it was counted: the one the settings last asked about (another one's figure
/// would be counted again anyway — it goes).
static USAGE: Mutex<BTreeMap<PathBuf, Counted>> = Mutex::new(BTreeMap::new());

/// Bytes and versions as last counted (`None`: the first count runs), and when.
type Counted = (Option<(u64, usize)>, Instant);
/// A count finished since the settings last looked.
static COUNTED: AtomicBool = AtomicBool::new(false);
/// A count older than this is made again (the history grows as files are saved).
const FRESH: Duration = Duration::from_secs(10);

/// What `dir` holds as last counted, from memory only (`None`: not counted yet).
fn known(dir: &Path) -> Option<(u64, usize)> {
    USAGE.lock().ok()?.get(dir).and_then(|(value, _)| *value)
}

/// What `dir` holds as last counted; counted again in the background when it's old (the app
/// woken when done). `None`: not counted yet.
fn counted(dir: &Path, wake: Option<&Arc<dyn Fn() + Send + Sync>>) -> Option<(u64, usize)> {
    let Ok(mut all) = USAGE.lock() else { return None };
    let known = all.get(dir).copied();
    if known.is_some_and(|(_, at)| at.elapsed() < FRESH) {
        return known.and_then(|(value, _)| value);
    }
    // Being counted from now on (the old figure shown meanwhile); the others' go.
    all.retain(|other, _| other == dir);
    all.insert(dir.to_path_buf(), (known.and_then(|(value, _)| value), Instant::now()));
    drop(all);
    let (dir, wake) = (dir.to_path_buf(), wake.cloned());
    kernel::work::run("history-usage", move || {
        let found = usage(&dir);
        if let Ok(mut all) = USAGE.lock() {
            all.insert(dir, (Some(found), Instant::now()));
        }
        COUNTED.store(true, Ordering::Release);
        if let Some(wake) = wake {
            wake();
        }
    });
    known.and_then(|(value, _)| value)
}

/// What a project's local history holds (its folder `dir`): bytes and versions kept.
pub fn usage(dir: &Path) -> (u64, usize) {
    let Ok(files) = std::fs::read_dir(dir) else { return (0, 0) };
    let mut total = (0, 0);
    for file in files.flatten() {
        let Ok(versions) = std::fs::read_dir(file.path()) else { continue };
        for version in versions.flatten() {
            let Ok(meta) = version.metadata() else { continue };
            total.0 += meta.len();
            if version.file_name().to_string_lossy().ends_with(".gz") {
                total.1 += 1;
            }
        }
    }
    total
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;

/// The retention in use (the user's settings), set at startup and when they change; read
/// each time a version is kept or old ones are dropped.
static CONFIGURED: std::sync::RwLock<Option<history::Retention>> = std::sync::RwLock::new(None);

/// Keep versions this long, this many per file (from now on).
fn configure(retention: history::Retention) {
    if let Ok(mut configured) = CONFIGURED.write() {
        *configured = Some(retention);
    }
}

/// The retention in use (the default until configured).
pub(crate) fn configured() -> history::Retention {
    CONFIGURED.read().ok().and_then(|r| *r).unwrap_or_default()
}
