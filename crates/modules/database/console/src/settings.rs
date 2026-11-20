//! The query consoles' settings (`[database] console_…` in `settings.toml`): how many rows
//! a statement's first page brings, whether a script stops at its first error, how much
//! memory a console's results may hold, which forms of parameter it asks values for, and how
//! its profiles are made and kept (their own block, "Profiling": [`ProfileSettings`]).
//! Declared here, shown in the settings window by [`ConsoleSettings`] (in the editor's
//! section, after the data sources'), handed to the consoles by [`apply`] at startup and
//! after every change; the console bar's ⚙ menu changes them too.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use i18n::t;
use settings_model::{Setting, SettingsService};
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsPage, SettingsRow, SettingsScope,
};

/// Rows of each result a statement brings at first (`[database] console_fetch_limit`).
pub const FETCH_LIMIT: Setting<i64> =
    Setting::new("database", "console_fetch_limit", || i64::from(database::run::FETCH_LIMIT))
        .valid(|n| (1..=100_000).contains(n));

/// The choices offered (the settings, the bar's ⚙ menu).
pub const FETCH_CHOICES: [i64; 5] = [100, 500, 1000, 5000, 10_000];

/// The most memory a console's results hold, in MB (`[database] console_result_memory`):
/// past it, the oldest unpinned results let their rows go.
pub const RESULT_MEMORY: Setting<i64> =
    Setting::new("database", "console_result_memory", || 64).valid(|n| (8..=4096).contains(n));

/// The choices offered (MB).
pub const MEMORY_CHOICES: [i64; 5] = [16, 64, 256, 512, 1024];

/// Which ways of writing a parameter a console asks values for
/// (`[database] console_param_styles`): `engine` — what the source's engine and its usual
/// clients read (PostgreSQL leaves `?` alone, its `jsonb` operator); `all` — every form
/// (`?`, `?1`, `$1`, `:name`, `@name`, `$name`).
pub const PARAM_STYLES: Setting<String> =
    Setting::new("database", "console_param_styles", || "engine".to_owned())
        .valid(|s| matches!(s.as_str(), "engine" | "all"));

/// The choices offered: their saved ids, as the settings window lists them.
pub const PARAM_STYLE_CHOICES: [&str; 2] = ["engine", "all"];

/// A script stops at its first statement that fails (`[database] console_stop_on_error`).
pub const STOP_ON_ERROR: Setting<bool> = Setting::new("database", "console_stop_on_error", || true);

/// The most statements a source's query history keeps (`[database] console_history_size`;
/// the oldest leave).
pub const HISTORY_SIZE: Setting<i64> =
    Setting::new("database", "console_history_size", || database::history::DEFAULT_LIMIT as i64)
        .valid(|n| (10..=100_000).contains(n));

/// The choices offered.
pub const HISTORY_CHOICES: [i64; 4] = [100, 500, 1000, 5000];

/// Up to how many tables a source's schema is read for completion with every column at
/// once (`[database] console_eager_tables`); past it, the tables' names at once and their
/// columns when a statement names them.
pub const EAGER_TABLES: Setting<i64> =
    Setting::new("database", "console_eager_tables", || 1000).valid(|n| (10..=100_000).contains(n));

/// The choices offered.
pub const EAGER_CHOICES: [i64; 4] = [200, 1000, 5000, 20_000];

/// The most memory a source's schema index for completion holds, in MB
/// (`[database] console_index_memory`): one bigger reads its columns when they're named.
pub const INDEX_MEMORY: Setting<i64> =
    Setting::new("database", "console_index_memory", || 64).valid(|n| (8..=2048).contains(n));

/// The choices offered (MB).
pub const INDEX_MEMORY_CHOICES: [i64; 4] = [16, 64, 256, 512];

/// The SQL grammar's errors shown in a console, faint, as hints
/// (`[database] console_grammar_hints`; off: a permissive grammar for several dialects
/// often doubts valid SQL — the real errors come from the server).
pub const GRAMMAR_HINTS: Setting<bool> =
    Setting::new("database", "console_grammar_hints", || false);

// ─── Profiles (plan 39h2; their block in the settings window, 39h4) ───

/// How long a profile run may take, in seconds (`[database] console_profile_timeout`): the
/// engine's own timeout (PostgreSQL's local `statement_timeout`, SQLite's deadline).
pub const PROFILE_TIMEOUT: Setting<i64> =
    Setting::new("database", "console_profile_timeout", || 300).valid(|n| (1..=86_400).contains(n));

/// A profile run counts each step's blocks (`[database] console_profile_buffers`).
pub const PROFILE_BUFFERS: Setting<bool> =
    Setting::new("database", "console_profile_buffers", || true);

/// A profile run times each step (`[database] console_profile_timing`; off: the real rows
/// only, cheaper on queries of many rows).
pub const PROFILE_TIMING: Setting<bool> =
    Setting::new("database", "console_profile_timing", || true);

/// A profile says each step's columns (`[database] console_profile_verbose`).
pub const PROFILE_VERBOSE: Setting<bool> =
    Setting::new("database", "console_profile_verbose", || false);

/// A profile reads the server's history of the same query when it has one
/// (`[database] console_profile_statement_stats`).
pub const PROFILE_STATEMENT_STATS: Setting<bool> =
    Setting::new("database", "console_profile_statement_stats", || true);

/// How many profiles a console keeps (`[database] console_profile_keep`; the oldest unpinned
/// go first).
pub const PROFILE_KEEP: Setting<i64> =
    Setting::new("database", "console_profile_keep", || 20).valid(|n| (1..=200).contains(n));

/// The most memory a console's profiles hold, in MB (`[database] console_profile_memory`).
pub const PROFILE_MEMORY: Setting<i64> =
    Setting::new("database", "console_profile_memory", || 32).valid(|n| (4..=1024).contains(n));

static PROFILE_SECONDS: AtomicU64 = AtomicU64::new(u64::MAX);
static PROFILE_FLAGS: AtomicU64 = AtomicU64::new(u64::MAX);
static PROFILE_KEPT: AtomicU64 = AtomicU64::new(u64::MAX);
static PROFILE_MB: AtomicU64 = AtomicU64::new(u64::MAX);

const BUFFERS_BIT: u64 = 1;
const TIMING_BIT: u64 = 2;
const VERBOSE_BIT: u64 = 4;
const STATS_BIT: u64 = 8;

/// The profiles' settings in use.
pub fn configure_profiles(
    timeout_s: i64,
    buffers: bool,
    timing: bool,
    verbose: bool,
    statement_stats: bool,
    keep: i64,
    memory_mb: i64,
) {
    PROFILE_SECONDS.store(timeout_s.clamp(1, 86_400) as u64, Ordering::Relaxed);
    PROFILE_FLAGS.store(bits(buffers, timing, verbose, statement_stats), Ordering::Relaxed);
    PROFILE_KEPT.store(keep.clamp(1, 200) as u64, Ordering::Relaxed);
    PROFILE_MB.store(memory_mb.clamp(4, 1024) as u64, Ordering::Relaxed);
}

/// The switches as one number.
fn bits(buffers: bool, timing: bool, verbose: bool, statement_stats: bool) -> u64 {
    [
        (buffers, BUFFERS_BIT),
        (timing, TIMING_BIT),
        (verbose, VERBOSE_BIT),
        (statement_stats, STATS_BIT),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .fold(0, |all, (_, bit)| all | bit)
}

fn profile_flags() -> u64 {
    match PROFILE_FLAGS.load(Ordering::Relaxed) {
        u64::MAX => bits(
            PROFILE_BUFFERS.default_value(),
            PROFILE_TIMING.default_value(),
            PROFILE_VERBOSE.default_value(),
            PROFILE_STATEMENT_STATS.default_value(),
        ),
        flags => flags,
    }
}

/// A profile's option the bar's ⚙ menu and the settings switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileSwitch {
    Buffers,
    Timing,
    Verbose,
    StatementStats,
}

impl ProfileSwitch {
    pub const ALL: [ProfileSwitch; 4] = [
        ProfileSwitch::Buffers,
        ProfileSwitch::Timing,
        ProfileSwitch::Verbose,
        ProfileSwitch::StatementStats,
    ];

    fn bit(self) -> u64 {
        match self {
            ProfileSwitch::Buffers => BUFFERS_BIT,
            ProfileSwitch::Timing => TIMING_BIT,
            ProfileSwitch::Verbose => VERBOSE_BIT,
            ProfileSwitch::StatementStats => STATS_BIT,
        }
    }

    /// Stable (the menu's rows): `buffers`…
    pub fn id(self) -> &'static str {
        match self {
            ProfileSwitch::Buffers => "buffers",
            ProfileSwitch::Timing => "timing",
            ProfileSwitch::Verbose => "verbose",
            ProfileSwitch::StatementStats => "statement_stats",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    /// Its setting.
    pub fn setting(self) -> &'static Setting<bool> {
        match self {
            ProfileSwitch::Buffers => &PROFILE_BUFFERS,
            ProfileSwitch::Timing => &PROFILE_TIMING,
            ProfileSwitch::Verbose => &PROFILE_VERBOSE,
            ProfileSwitch::StatementStats => &PROFILE_STATEMENT_STATS,
        }
    }
}

/// `switch` is on for the next profiles.
pub fn profile_switch(switch: ProfileSwitch) -> bool {
    profile_flags() & switch.bit() != 0
}

/// `switch` turned on or off for the next profiles (in use at once; saved by the caller).
pub fn set_profile_switch(switch: ProfileSwitch, on: bool) {
    let flags = profile_flags();
    let flags = if on { flags | switch.bit() } else { flags & !switch.bit() };
    PROFILE_FLAGS.store(flags, Ordering::Relaxed);
}

/// What a profile asks of its driver: `analyze` — the statement run (timed per step unless
/// the setting says not), within its time limit; else its plan.
pub fn profile_options(analyze: bool) -> database::profile::ProfileOptions {
    let seconds = match PROFILE_SECONDS.load(Ordering::Relaxed) {
        u64::MAX => PROFILE_TIMEOUT.default_value() as u64,
        s => s,
    };
    options_with(profile_flags(), seconds, analyze)
}

/// [`profile_options`] of the settings' `flags` and time limit (`seconds`).
fn options_with(flags: u64, seconds: u64, analyze: bool) -> database::profile::ProfileOptions {
    use database::profile::{ProfileDepth, ProfileOptions};
    let depth = if !analyze {
        ProfileDepth::Estimated
    } else {
        ProfileDepth::Actual { timing: flags & TIMING_BIT != 0 }
    };
    ProfileOptions {
        depth,
        buffers: flags & BUFFERS_BIT != 0,
        verbose: flags & VERBOSE_BIT != 0,
        statement_history: flags & STATS_BIT != 0,
        timeout: analyze.then(|| std::time::Duration::from_secs(seconds)),
        ..ProfileOptions::ESTIMATED
    }
}

/// How many profiles a console keeps, and their most bytes.
pub fn profile_limits() -> (usize, usize) {
    let keep = match PROFILE_KEPT.load(Ordering::Relaxed) {
        u64::MAX => PROFILE_KEEP.default_value() as u64,
        n => n,
    };
    let mb = match PROFILE_MB.load(Ordering::Relaxed) {
        u64::MAX => PROFILE_MEMORY.default_value() as u64,
        n => n,
    };
    (keep as usize, (mb as usize) << 20)
}

/// The values in use; `u64::MAX` until configured (the defaults then).
static FETCH: AtomicU64 = AtomicU64::new(u64::MAX);
static STOP: AtomicBool = AtomicBool::new(true);
static MEMORY: AtomicU64 = AtomicU64::new(u64::MAX);
static HISTORY_KEPT: AtomicU64 = AtomicU64::new(u64::MAX);
static EAGER: AtomicU64 = AtomicU64::new(u64::MAX);
static INDEX_MB: AtomicU64 = AtomicU64::new(u64::MAX);

pub fn configure_index(eager_tables: i64, memory_mb: i64) {
    EAGER.store(eager_tables.clamp(10, 100_000) as u64, Ordering::Relaxed);
    INDEX_MB.store(memory_mb.clamp(8, 2048) as u64, Ordering::Relaxed);
}

/// Up to how many tables every column is read at once.
pub fn eager_tables() -> usize {
    match EAGER.load(Ordering::Relaxed) {
        u64::MAX => EAGER_TABLES.default_value() as usize,
        n => n as usize,
    }
}

/// The most bytes a source's schema index holds.
pub fn index_memory() -> usize {
    let mb = match INDEX_MB.load(Ordering::Relaxed) {
        u64::MAX => INDEX_MEMORY.default_value() as u64,
        n => n,
    };
    (mb as usize) << 20
}
/// Every form of parameter read (`all`), or the engine's (`engine`).
static ALL_STYLES: AtomicBool = AtomicBool::new(false);

pub fn configure_param_styles(id: &str) {
    ALL_STYLES.store(id == "all", Ordering::Relaxed);
}

/// The ways of writing a parameter read in `flavor`'s consoles.
pub fn param_styles(flavor: database::sql::Flavor) -> database::sql::ParamStyles {
    if ALL_STYLES.load(Ordering::Relaxed) {
        database::sql::ParamStyles::all()
    } else {
        database::sql::ParamStyles::of(flavor)
    }
}

pub fn configure_history(size: i64) {
    HISTORY_KEPT.store(size.clamp(10, 100_000) as u64, Ordering::Relaxed);
}

/// The most entries a source's history keeps.
pub fn history_size() -> usize {
    match HISTORY_KEPT.load(Ordering::Relaxed) {
        u64::MAX => HISTORY_SIZE.default_value() as usize,
        n => n as usize,
    }
}

pub fn configure_memory(mb: i64) {
    MEMORY.store(mb.clamp(8, 4096) as u64, Ordering::Relaxed);
}

/// The most bytes a console's results hold together.
pub fn result_memory() -> usize {
    let mb = match MEMORY.load(Ordering::Relaxed) {
        u64::MAX => RESULT_MEMORY.default_value() as u64,
        mb => mb,
    };
    (mb as usize) << 20
}

pub fn configure_fetch(rows: i64) {
    FETCH.store(rows.clamp(1, 100_000) as u64, Ordering::Relaxed);
}

/// Rows of each result a statement brings at first.
pub fn fetch_limit() -> u32 {
    match FETCH.load(Ordering::Relaxed) {
        u64::MAX => FETCH_LIMIT.default_value() as u32,
        rows => rows as u32,
    }
}

/// The grammar's errors are shown as hints.
static HINTS: AtomicBool = AtomicBool::new(false);

pub fn configure_grammar_hints(on: bool) {
    HINTS.store(on, Ordering::Relaxed);
}

/// A console shows its grammar's errors, faint.
pub fn grammar_hints() -> bool {
    HINTS.load(Ordering::Relaxed)
}

pub fn configure_stop(on: bool) {
    STOP.store(on, Ordering::Relaxed);
}

/// A script stops at its first error.
pub fn stop_on_error() -> bool {
    STOP.load(Ordering::Relaxed)
}

/// Every value from the saved settings (at startup and after a change).
pub fn apply(settings: &SettingsService) {
    configure_fetch(settings.get(&FETCH_LIMIT));
    configure_stop(settings.get(&STOP_ON_ERROR));
    configure_memory(settings.get(&RESULT_MEMORY));
    configure_param_styles(&settings.get(&PARAM_STYLES));
    configure_history(settings.get(&HISTORY_SIZE));
    configure_index(settings.get(&EAGER_TABLES), settings.get(&INDEX_MEMORY));
    configure_grammar_hints(settings.get(&GRAMMAR_HINTS));
    configure_profiles(
        settings.get(&PROFILE_TIMEOUT),
        settings.get(&PROFILE_BUFFERS),
        settings.get(&PROFILE_TIMING),
        settings.get(&PROFILE_VERBOSE),
        settings.get(&PROFILE_STATEMENT_STATS),
        settings.get(&PROFILE_KEEP),
        settings.get(&PROFILE_MEMORY),
    );
}

/// Saves `value` for `setting` in `settings` (the bar's ⚙ menu, the settings window).
pub fn save<T: settings_model::SettingType>(
    settings: &SettingsService,
    setting: &Setting<T>,
    value: T,
) {
    if let Err(e) = settings.set(setting, value) {
        tracing::warn!(error = %e, "can't save a console setting");
    }
}

/// The "Query consoles" block of the settings window.
#[derive(Debug, Default)]
pub struct ConsoleSettings;

impl SettingsContribution for ConsoleSettings {
    fn id(&self) -> &'static str {
        "database-console"
    }

    fn page(&self) -> &'static str {
        "database"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::DATABASE)
    }

    fn order(&self) -> u16 {
        20
    }

    fn apply(&self, settings: &SettingsService) {
        apply(settings);
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let s = scope.settings;
        SettingsBlock {
            title: t("settings.console_title"),
            detail: t("settings.console_detail"),
            rows: vec![
                SettingsRow::Choices {
                    key: "console_fetch_limit",
                    detail: t("settings.console_fetch_limit_detail"),
                    default: Some(FETCH_LIMIT.default_value()),
                    label: t("settings.console_fetch_limit"),
                    choices: FETCH_CHOICES.iter().map(|&n| (n, n.to_string())).collect(),
                    current: s.get(&FETCH_LIMIT),
                },
                SettingsRow::Toggle {
                    key: "console_stop_on_error",
                    detail: t("settings.console_stop_on_error_detail"),
                    default: Some(STOP_ON_ERROR.default_value()),
                    label: t("settings.console_stop_on_error"),
                    on: s.get(&STOP_ON_ERROR),
                },
                SettingsRow::Choices {
                    key: "console_result_memory",
                    detail: t("settings.console_result_memory_detail"),
                    default: Some(RESULT_MEMORY.default_value()),
                    label: t("settings.console_result_memory"),
                    choices: MEMORY_CHOICES.iter().map(|&n| (n, format!("{n} MB"))).collect(),
                    current: s.get(&RESULT_MEMORY),
                },
                SettingsRow::Choices {
                    key: "console_param_styles",
                    detail: t("settings.console_param_styles_detail"),
                    default: PARAM_STYLE_CHOICES
                        .iter()
                        .position(|id| *id == PARAM_STYLES.default_value())
                        .map(|i| i as i64),
                    label: t("settings.console_param_styles"),
                    choices: PARAM_STYLE_CHOICES
                        .iter()
                        .enumerate()
                        .map(|(i, id)| {
                            (i as i64, t(&format!("settings.console_param_styles_{id}")))
                        })
                        .collect(),
                    current: PARAM_STYLE_CHOICES
                        .iter()
                        .position(|id| *id == s.get(&PARAM_STYLES))
                        .unwrap_or(0) as i64,
                },
                SettingsRow::Choices {
                    key: "console_history_size",
                    detail: t("settings.console_history_size_detail"),
                    default: Some(HISTORY_SIZE.default_value()),
                    label: t("settings.console_history_size"),
                    choices: HISTORY_CHOICES.iter().map(|&n| (n, n.to_string())).collect(),
                    current: s.get(&HISTORY_SIZE),
                },
                SettingsRow::Choices {
                    key: "console_eager_tables",
                    detail: t("settings.console_eager_tables_detail"),
                    default: Some(EAGER_TABLES.default_value()),
                    label: t("settings.console_eager_tables"),
                    choices: EAGER_CHOICES.iter().map(|&n| (n, n.to_string())).collect(),
                    current: s.get(&EAGER_TABLES),
                },
                SettingsRow::Choices {
                    key: "console_index_memory",
                    detail: t("settings.console_index_memory_detail"),
                    default: Some(INDEX_MEMORY.default_value()),
                    label: t("settings.console_index_memory"),
                    choices: INDEX_MEMORY_CHOICES.iter().map(|&n| (n, format!("{n} MB"))).collect(),
                    current: s.get(&INDEX_MEMORY),
                },
                SettingsRow::Toggle {
                    key: "console_grammar_hints",
                    detail: t("settings.console_grammar_hints_detail"),
                    default: Some(GRAMMAR_HINTS.default_value()),
                    label: t("settings.console_grammar_hints"),
                    on: s.get(&GRAMMAR_HINTS),
                },
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let s = scope.settings;
        match key {
            "console_fetch_limit" => save(s, &FETCH_LIMIT, value),
            "console_stop_on_error" => save(s, &STOP_ON_ERROR, value != 0),
            "console_result_memory" => save(s, &RESULT_MEMORY, value),
            "console_history_size" => save(s, &HISTORY_SIZE, value),
            "console_eager_tables" => save(s, &EAGER_TABLES, value),
            "console_index_memory" => save(s, &INDEX_MEMORY, value),
            "console_grammar_hints" => save(s, &GRAMMAR_HINTS, value != 0),
            "console_param_styles" => {
                let id = PARAM_STYLE_CHOICES.get(value as usize).copied().unwrap_or("engine");
                save(s, &PARAM_STYLES, id.to_owned());
            }
            _ => {}
        }
    }
}

/// The time limits offered for a profile run (seconds).
pub const PROFILE_TIMEOUT_CHOICES: [i64; 5] = [30, 60, 300, 900, 3600];
/// How many profiles a console may keep (the choices).
pub const PROFILE_KEEP_CHOICES: [i64; 4] = [5, 20, 50, 100];
/// The memory a console's profiles may hold (MB, the choices).
pub const PROFILE_MEMORY_CHOICES: [i64; 4] = [8, 32, 128, 512];

/// The "Profiling" block of the settings window, after the consoles' (plan 39h4): what a
/// profile asks of the engine and how many a console keeps.
#[derive(Debug, Default)]
pub struct ProfileSettings;

impl SettingsContribution for ProfileSettings {
    fn id(&self) -> &'static str {
        "database-console-profile"
    }

    fn page(&self) -> &'static str {
        "database"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::DATABASE)
    }

    fn order(&self) -> u16 {
        40
    }

    fn advanced(&self) -> bool {
        true
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let s = scope.settings;
        let seconds = |n: i64| {
            if n >= 60 { format!("{} min", n / 60) } else { format!("{n} s") }
        };
        SettingsBlock {
            title: t("settings.profile_title"),
            detail: t("settings.profile_detail"),
            rows: vec![
                SettingsRow::Toggle {
                    key: "console_profile_buffers",
                    detail: t("settings.profile_buffers_detail"),
                    default: Some(PROFILE_BUFFERS.default_value()),
                    label: t("settings.profile_buffers"),
                    on: s.get(&PROFILE_BUFFERS),
                },
                SettingsRow::Toggle {
                    key: "console_profile_timing",
                    detail: t("settings.profile_timing_detail"),
                    default: Some(PROFILE_TIMING.default_value()),
                    label: t("settings.profile_timing"),
                    on: s.get(&PROFILE_TIMING),
                },
                SettingsRow::Toggle {
                    key: "console_profile_verbose",
                    detail: t("settings.profile_verbose_detail"),
                    default: Some(PROFILE_VERBOSE.default_value()),
                    label: t("settings.profile_verbose"),
                    on: s.get(&PROFILE_VERBOSE),
                },
                SettingsRow::Toggle {
                    key: "console_profile_statement_stats",
                    detail: t("settings.profile_statement_stats_detail"),
                    default: Some(PROFILE_STATEMENT_STATS.default_value()),
                    label: t("settings.profile_statement_stats"),
                    on: s.get(&PROFILE_STATEMENT_STATS),
                },
                SettingsRow::Choices {
                    key: "console_profile_timeout",
                    detail: t("settings.profile_timeout_detail"),
                    default: Some(PROFILE_TIMEOUT.default_value()),
                    label: t("settings.profile_timeout"),
                    choices: PROFILE_TIMEOUT_CHOICES.iter().map(|&n| (n, seconds(n))).collect(),
                    current: s.get(&PROFILE_TIMEOUT),
                },
                SettingsRow::Choices {
                    key: "console_profile_keep",
                    detail: t("settings.profile_keep_detail"),
                    default: Some(PROFILE_KEEP.default_value()),
                    label: t("settings.profile_keep"),
                    choices: PROFILE_KEEP_CHOICES.iter().map(|&n| (n, n.to_string())).collect(),
                    current: s.get(&PROFILE_KEEP),
                },
                SettingsRow::Choices {
                    key: "console_profile_memory",
                    detail: t("settings.profile_memory_detail"),
                    default: Some(PROFILE_MEMORY.default_value()),
                    label: t("settings.profile_memory"),
                    choices: PROFILE_MEMORY_CHOICES
                        .iter()
                        .map(|&n| (n, format!("{n} MB")))
                        .collect(),
                    current: s.get(&PROFILE_MEMORY),
                },
                SettingsRow::Notes(vec![t("settings.profile_note")]),
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let s = scope.settings;
        match key {
            "console_profile_buffers" => save(s, &PROFILE_BUFFERS, value != 0),
            "console_profile_timing" => save(s, &PROFILE_TIMING, value != 0),
            "console_profile_verbose" => save(s, &PROFILE_VERBOSE, value != 0),
            "console_profile_statement_stats" => save(s, &PROFILE_STATEMENT_STATS, value != 0),
            "console_profile_timeout" => save(s, &PROFILE_TIMEOUT, value),
            "console_profile_keep" => save(s, &PROFILE_KEEP, value),
            "console_profile_memory" => save(s, &PROFILE_MEMORY, value),
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
