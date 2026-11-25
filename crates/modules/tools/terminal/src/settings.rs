//! The terminal's settings (`[terminal] …` in `settings.toml`): the shell new tabs start,
//! the colors, the cursor, the text, the history, how it behaves — its page in the settings
//! window ("Shell", "Display" and "Behavior"), handed to the terminal ([`crate::config`]) at
//! startup and after each change. A few are also switched from its window's menu.

use i18n::t;
use settings_model::{Setting, SettingsService};
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsGroup, SettingsPage, SettingsRow, SettingsScope,
};

/// Tools › Terminal: the terminal's page of the settings (its two blocks).
const PAGE: SettingsPage = SettingsPage {
    id: "terminal",
    label: "settings.terminal_section",
    icon: "terminal",
    group: SettingsGroup::Tools,
    order: 10,
};

use crate::config::{CUSTOM, Colors, Cursor, TabTitle, TerminalConfig};
use terminal_session::history::HistoryClear;

/// The shell new tabs start: one found on the system, by its id (`zsh`, `pwsh`,
/// `git-bash`…; `""`: the default), or `custom`.
pub const SHELL: Setting<String> = Setting::new("terminal", "shell", String::new);
/// "Other…"'s command: the program and its arguments.
pub const CUSTOM_SHELL: Setting<String> = Setting::new("terminal", "custom_shell", String::new);
/// The prompt in the theme's colors (off: the shell's own).
pub const THEMED_PROMPT: Setting<bool> = Setting::new("terminal", "themed_prompt", || false);
/// Each project's shells with their own command history (off: the user's usual one).
pub const PROJECT_HISTORY: Setting<bool> = Setting::new("terminal", "project_history", || true);
/// `theme`, `classic` or `contrast`.
pub const COLORS: Setting<String> = Setting::new("terminal", "colors", || "theme".into())
    .valid(|v| matches!(v.as_str(), "theme" | "classic" | "contrast"));
/// `block`, `bar` or `underline`.
pub const CURSOR: Setting<String> = Setting::new("terminal", "cursor", || "block".into())
    .valid(|v| matches!(v.as_str(), "block" | "bar" | "underline"));
pub const BLINK: Setting<bool> = Setting::new("terminal", "cursor_blink", || true);
/// The font size in points; 0: the editor's.
pub const FONT_SIZE: Setting<i64> =
    Setting::new("terminal", "font_size", || 0).valid(|n| *n == 0 || (8..=32).contains(n));
/// The line height, in percent of the font size.
pub const LINE_HEIGHT: Setting<i64> =
    Setting::new("terminal", "line_height", || 135).valid(|n| (100..=200).contains(n));
pub const BOLD_BRIGHT: Setting<bool> = Setting::new("terminal", "bold_bright", || false);
/// The lines kept above the screen (each ~10 KB at usual widths, so bounded).
pub const SCROLLBACK: Setting<i64> =
    Setting::new("terminal", "scrollback", || 5_000).valid(|n| (100..=100_000).contains(n));

/// What a program's request to erase the history gets: `standard` (honored),
/// `keep-running` (ignored while a program runs in the foreground for a while) or
/// `keep-always` (never honored).
pub const HISTORY_CLEAR: Setting<String> =
    Setting::new("terminal", "history_clear", || "keep-running".into())
        .valid(|v| HISTORY_CLEAR_IDS.contains(&v.as_str()));

/// A tab named after what its program says it is (OSC 0 / 2): `never`, `running` (while a
/// command runs, its own name otherwise) or `always`.
pub const TAB_TITLE: Setting<String> = Setting::new("terminal", "tab_title", || "running".into())
    .valid(|v| TAB_TITLE_IDS.contains(&v.as_str()));
/// A tab closes when its shell ends (off: it stays, saying so, until closed).
pub const CLOSE_ON_EXIT: Setting<bool> = Setting::new("terminal", "close_on_exit", || true);
/// Programs that ask for the mouse get its wheel (off: it always scrolls the history).
pub const MOUSE_REPORTING: Setting<bool> = Setting::new("terminal", "mouse_reporting", || true);
/// Text selected with the mouse copied at once.
pub const COPY_ON_SELECT: Setting<bool> = Setting::new("terminal", "copy_on_select", || false);
/// Links and file paths underlined under the pointer, opened with a click.
pub const HIGHLIGHT_LINKS: Setting<bool> = Setting::new("terminal", "highlight_links", || true);

/// Programs other modules run (an agents' sessions) put to sleep when unseen and idle: stopped,
/// their tabs kept, resumed when shown (off by default: nothing is stopped unasked).
pub const SLEEP_IDLE: Setting<bool> = Setting::new("terminal", "sleep_idle", || false);
/// After how many minutes unseen and idle.
pub const SLEEP_AFTER: Setting<i64> =
    Setting::new("terminal", "sleep_after", || DEFAULT_SLEEP_AFTER)
        .valid(|n| SLEEP_AFTERS.contains(n));
/// The idle time it starts at (minutes): an hour.
const DEFAULT_SLEEP_AFTER: i64 = 60;
/// The idle times offered (minutes).
pub const SLEEP_AFTERS: [i64; 5] = [15, 30, 60, 120, 240];

const TAB_TITLE_IDS: [&str; 3] = ["never", "running", "always"];

/// The stops offered.
pub const FONT_SIZES: [i64; 7] = [0, 11, 12, 13, 14, 16, 18];
pub const LINE_HEIGHTS: [i64; 6] = [100, 115, 125, 135, 150, 170];
pub const SCROLLBACKS: [i64; 5] = [1_000, 5_000, 10_000, 20_000, 50_000];

const COLOR_IDS: [&str; 3] = ["theme", "classic", "contrast"];
const CURSOR_IDS: [&str; 3] = ["block", "bar", "underline"];
const HISTORY_CLEAR_IDS: [&str; 3] = ["standard", "keep-running", "keep-always"];
/// The value of "Other…" among the shells' choices (the others: their place in the list).
const CUSTOM_CHOICE: i64 = 1_000;

/// What the saved settings say.
pub fn read(settings: &SettingsService) -> TerminalConfig {
    TerminalConfig {
        shell: settings.get(&SHELL),
        custom: settings.get(&CUSTOM_SHELL),
        themed_prompt: settings.get(&THEMED_PROMPT),
        project_history: settings.get(&PROJECT_HISTORY),
        colors: match settings.get(&COLORS).as_str() {
            "classic" => Colors::Classic,
            "contrast" => Colors::HighContrast,
            _ => Colors::Theme,
        },
        cursor: match settings.get(&CURSOR).as_str() {
            "bar" => Cursor::Bar,
            "underline" => Cursor::Underline,
            _ => Cursor::Block,
        },
        blink: settings.get(&BLINK),
        font_size: Some(settings.get(&FONT_SIZE)).filter(|n| *n > 0).map(|n| n as f32),
        line_height: settings.get(&LINE_HEIGHT) as f32 / 100.0,
        bold_bright: settings.get(&BOLD_BRIGHT),
        scrollback: settings.get(&SCROLLBACK).max(100) as usize,
        history_clear: match settings.get(&HISTORY_CLEAR).as_str() {
            "standard" => HistoryClear::Standard,
            "keep-always" => HistoryClear::KeepAlways,
            _ => HistoryClear::KeepForRunningPrograms,
        },
        tab_title: match settings.get(&TAB_TITLE).as_str() {
            "never" => TabTitle::Never,
            "always" => TabTitle::Always,
            _ => TabTitle::WhileRunning,
        },
        close_on_exit: settings.get(&CLOSE_ON_EXIT),
        mouse_reporting: settings.get(&MOUSE_REPORTING),
        copy_on_select: settings.get(&COPY_ON_SELECT),
        highlight_links: settings.get(&HIGHLIGHT_LINKS),
        sleep_after: settings.get(&SLEEP_IDLE).then(|| sleep_minutes(settings.get(&SLEEP_AFTER))),
    }
}

/// `minutes` as a duration (never below the shortest offered).
fn sleep_minutes(minutes: i64) -> std::time::Duration {
    let minutes = u64::try_from(minutes.max(SLEEP_AFTERS[0])).unwrap_or(DEFAULT_SLEEP_AFTER as u64);
    std::time::Duration::from_secs(minutes.saturating_mul(60))
}

/// Sleeping turned on (the hint's "Turn on", a command) and handed to the terminal at once.
pub fn enable_sleep(settings: &SettingsService) {
    save(settings, &SLEEP_IDLE, true);
    crate::config::configure(read(settings));
}

/// `setting` turned the other way and handed to the terminal at once (from its window's
/// menu): its new value.
pub fn flip(settings: &SettingsService, setting: &Setting<bool>) -> bool {
    let on = !settings.get(setting);
    save(settings, setting, on);
    crate::config::configure(read(settings));
    on
}

fn save<T: settings_model::SettingType>(
    settings: &SettingsService,
    setting: &Setting<T>,
    value: T,
) {
    if let Err(e) = settings.set(setting, value) {
        tracing::warn!(error = %e, "can't save a terminal setting");
    }
}

/// The "Shell" block, on the terminal's own page: which shell new tabs start.
#[derive(Debug)]
pub(crate) struct ShellSettings;

impl SettingsContribution for ShellSettings {
    fn id(&self) -> &'static str {
        "terminal-shell"
    }

    fn page(&self) -> &'static str {
        "terminal"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(PAGE)
    }

    fn order(&self) -> u16 {
        10
    }

    fn apply(&self, settings: &SettingsService) {
        crate::config::configure(read(settings));
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let saved = scope.settings.get(&SHELL);
        let shells = pty::shells();
        let mut choices: Vec<(i64, String)> =
            shells.iter().enumerate().map(|(i, s)| (i as i64, s.name.clone())).collect();
        choices.push((CUSTOM_CHOICE, t("settings.terminal_shell_custom")));
        let current = if saved == CUSTOM {
            CUSTOM_CHOICE
        } else {
            // One gone (or none chosen): the default, the first.
            shells.iter().position(|s| s.id == saved).unwrap_or(0) as i64
        };
        let mut rows = vec![SettingsRow::Choices {
            key: "shell",
            detail: t("settings.terminal_shell_choice_detail"),
            default: Some(0),
            label: t("settings.terminal_shell"),
            choices,
            current,
        }];
        if saved == CUSTOM {
            rows.push(SettingsRow::Text {
                key: "custom_shell",
                detail: t("settings.terminal_shell_command_detail"),
                label: t("settings.terminal_shell_command"),
                value: scope.settings.get(&CUSTOM_SHELL),
                placeholder: t(if cfg!(windows) {
                    "settings.terminal_shell_placeholder_windows"
                } else {
                    "settings.terminal_shell_placeholder"
                }),
            });
        }
        rows.push(SettingsRow::Toggle {
            key: "themed_prompt",
            detail: t("settings.terminal_themed_prompt_detail"),
            default: Some(THEMED_PROMPT.default_value()),
            label: t("settings.terminal_themed_prompt"),
            on: scope.settings.get(&THEMED_PROMPT),
        });
        rows.push(SettingsRow::Toggle {
            key: "project_history",
            detail: t("settings.terminal_project_history_detail"),
            default: Some(PROJECT_HISTORY.default_value()),
            label: t("settings.terminal_project_history"),
            on: scope.settings.get(&PROJECT_HISTORY),
        });
        // The keys that work differently in a terminal (each system's own).
        let keys = if cfg!(target_os = "macos") { "mac" } else { "other" };
        rows.push(SettingsRow::Notes(vec![
            t(&format!("settings.terminal_keys_newline_{keys}")),
            t(&format!("settings.terminal_keys_paste_{keys}")),
            t(&format!("settings.terminal_keys_copy_{keys}")),
        ]));
        SettingsBlock {
            title: t("settings.terminal_shell_title"),
            detail: t("settings.terminal_shell_detail"),
            rows,
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let settings = scope.settings;
        if key == "themed_prompt" {
            save(settings, &THEMED_PROMPT, value != 0);
            return;
        }
        if key == "project_history" {
            save(settings, &PROJECT_HISTORY, value != 0);
            return;
        }
        if key != "shell" {
            return;
        }
        let id = match value {
            CUSTOM_CHOICE => CUSTOM.to_owned(),
            i => pty::shells().get(i as usize).map(|s| s.id.clone()).unwrap_or_default(),
        };
        save(settings, &SHELL, id);
    }

    fn set_text(&self, scope: SettingsScope<'_>, key: &'static str, value: &str) {
        let settings = scope.settings;
        if key == "custom_shell" {
            save(settings, &CUSTOM_SHELL, value.to_owned());
        }
    }
}

/// The "Display" block: colors, cursor, text, history.
#[derive(Debug)]
pub(crate) struct DisplaySettings;

impl SettingsContribution for DisplaySettings {
    fn id(&self) -> &'static str {
        "terminal-display"
    }

    fn page(&self) -> &'static str {
        "terminal"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(PAGE)
    }

    fn order(&self) -> u16 {
        20
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let settings = scope.settings;
        let index = |ids: &[&str], value: String| {
            ids.iter().position(|id| *id == value).unwrap_or(0) as i64
        };
        let named = |keys: &[&str]| -> Vec<(i64, String)> {
            keys.iter().enumerate().map(|(i, k)| (i as i64, t(k))).collect()
        };
        let stops = |values: &[i64], label: &dyn Fn(i64) -> String| -> Vec<(i64, String)> {
            values.iter().map(|&v| (v, label(v))).collect()
        };
        SettingsBlock {
            title: t("settings.terminal_display"),
            detail: t("settings.terminal_display_detail"),
            rows: vec![
                SettingsRow::Choices {
                    key: "colors",
                    detail: t("settings.terminal_colors_detail"),
                    default: Some(index(&COLOR_IDS, COLORS.default_value())),
                    label: t("settings.terminal_colors"),
                    choices: named(&[
                        "settings.terminal_colors_theme",
                        "settings.terminal_colors_classic",
                        "settings.terminal_colors_contrast",
                    ]),
                    current: index(&COLOR_IDS, settings.get(&COLORS)),
                },
                SettingsRow::Choices {
                    key: "cursor",
                    detail: t("settings.terminal_cursor_detail"),
                    default: Some(index(&CURSOR_IDS, CURSOR.default_value())),
                    label: t("settings.terminal_cursor"),
                    choices: named(&[
                        "settings.terminal_cursor_block",
                        "settings.terminal_cursor_bar",
                        "settings.terminal_cursor_underline",
                    ]),
                    current: index(&CURSOR_IDS, settings.get(&CURSOR)),
                },
                SettingsRow::Toggle {
                    key: "cursor_blink",
                    detail: t("settings.terminal_blink_detail"),
                    default: Some(BLINK.default_value()),
                    label: t("settings.terminal_blink"),
                    on: settings.get(&BLINK),
                },
                SettingsRow::Slider {
                    key: "font_size",
                    detail: t("settings.terminal_font_size_detail"),
                    default: Some(FONT_SIZE.default_value()),
                    label: t("settings.terminal_font_size"),
                    stops: stops(&FONT_SIZES, &|n| {
                        if n == 0 { t("settings.terminal_font_editor") } else { n.to_string() }
                    }),
                    current: settings.get(&FONT_SIZE),
                },
                SettingsRow::Slider {
                    key: "line_height",
                    detail: t("settings.terminal_line_height_detail"),
                    default: Some(LINE_HEIGHT.default_value()),
                    label: t("settings.terminal_line_height"),
                    stops: stops(&LINE_HEIGHTS, &|n| format!("{:.2}", n as f32 / 100.0)),
                    current: settings.get(&LINE_HEIGHT),
                },
                SettingsRow::Toggle {
                    key: "bold_bright",
                    detail: t("settings.terminal_bold_bright_detail"),
                    default: Some(BOLD_BRIGHT.default_value()),
                    label: t("settings.terminal_bold_bright"),
                    on: settings.get(&BOLD_BRIGHT),
                },
                SettingsRow::Slider {
                    key: "scrollback",
                    detail: t("settings.terminal_scrollback_detail"),
                    default: Some(SCROLLBACK.default_value()),
                    label: t("settings.terminal_scrollback"),
                    stops: stops(&SCROLLBACKS, &|n| format!("{}k", n / 1_000)),
                    current: settings.get(&SCROLLBACK),
                },
                SettingsRow::Choices {
                    key: "history_clear",
                    detail: t("settings.terminal_history_clear_detail"),
                    default: Some(index(&HISTORY_CLEAR_IDS, HISTORY_CLEAR.default_value())),
                    label: t("settings.terminal_history_clear"),
                    choices: named(&[
                        "settings.terminal_history_clear_standard",
                        "settings.terminal_history_clear_running",
                        "settings.terminal_history_clear_always",
                    ]),
                    current: index(&HISTORY_CLEAR_IDS, settings.get(&HISTORY_CLEAR)),
                },
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let settings = scope.settings;
        let id = |ids: &[&str]| ids.get(value as usize).map(|s| (*s).to_owned());
        match key {
            "colors" => {
                if let Some(colors) = id(&COLOR_IDS) {
                    save(settings, &COLORS, colors);
                }
            }
            "cursor" => {
                if let Some(cursor) = id(&CURSOR_IDS) {
                    save(settings, &CURSOR, cursor);
                }
            }
            "cursor_blink" => save(settings, &BLINK, value != 0),
            "bold_bright" => save(settings, &BOLD_BRIGHT, value != 0),
            "font_size" => save(settings, &FONT_SIZE, value),
            "line_height" => save(settings, &LINE_HEIGHT, value),
            "scrollback" => save(settings, &SCROLLBACK, value),
            "history_clear" => {
                if let Some(policy) = id(&HISTORY_CLEAR_IDS) {
                    save(settings, &HISTORY_CLEAR, policy);
                }
            }
            _ => {}
        }
    }
}

/// The "Behavior" block: the tabs' names, a session that ends, the mouse, selections, links.
#[derive(Debug)]
pub(crate) struct BehaviorSettings;

impl SettingsContribution for BehaviorSettings {
    fn id(&self) -> &'static str {
        "terminal-behavior"
    }

    fn page(&self) -> &'static str {
        "terminal"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(PAGE)
    }

    fn order(&self) -> u16 {
        30
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let settings = scope.settings;
        let index = TAB_TITLE_IDS.iter().position(|id| *id == settings.get(&TAB_TITLE));
        let default = TAB_TITLE_IDS.iter().position(|id| *id == TAB_TITLE.default_value());
        let toggle = |key, setting: &Setting<bool>| SettingsRow::Toggle {
            key,
            detail: t(&format!("settings.terminal_{key}_detail")),
            default: Some(setting.default_value()),
            label: t(&format!("settings.terminal_{key}")),
            on: settings.get(setting),
        };
        SettingsBlock {
            title: t("settings.terminal_behavior"),
            detail: t("settings.terminal_behavior_detail"),
            rows: vec![
                SettingsRow::Choices {
                    key: "tab_title",
                    detail: t("settings.terminal_tab_title_detail"),
                    default: default.map(|i| i as i64),
                    label: t("settings.terminal_tab_title"),
                    choices: ["never", "running", "always"]
                        .iter()
                        .enumerate()
                        .map(|(i, k)| (i as i64, t(&format!("settings.terminal_tab_title_{k}"))))
                        .collect(),
                    current: index.unwrap_or(1) as i64,
                },
                toggle("close_on_exit", &CLOSE_ON_EXIT),
                toggle("mouse_reporting", &MOUSE_REPORTING),
                toggle("copy_on_select", &COPY_ON_SELECT),
                toggle("highlight_links", &HIGHLIGHT_LINKS),
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let settings = scope.settings;
        match key {
            "tab_title" => {
                if let Some(id) = TAB_TITLE_IDS.get(value as usize) {
                    save(settings, &TAB_TITLE, (*id).to_owned());
                }
            }
            "close_on_exit" => save(settings, &CLOSE_ON_EXIT, value != 0),
            "mouse_reporting" => save(settings, &MOUSE_REPORTING, value != 0),
            "copy_on_select" => save(settings, &COPY_ON_SELECT, value != 0),
            "highlight_links" => save(settings, &HIGHLIGHT_LINKS, value != 0),
            _ => {}
        }
    }
}

/// The "Sleeping sessions" block: programs other modules run, stopped when unseen and idle.
#[derive(Debug)]
pub(crate) struct SleepSettings;

impl SettingsContribution for SleepSettings {
    fn id(&self) -> &'static str {
        "terminal-sleep"
    }

    fn page(&self) -> &'static str {
        "terminal"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(PAGE)
    }

    fn order(&self) -> u16 {
        40
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let settings = scope.settings;
        let mut rows = vec![SettingsRow::Toggle {
            key: "sleep_idle",
            detail: t("settings.terminal_sleep_idle_detail"),
            default: Some(SLEEP_IDLE.default_value()),
            label: t("settings.terminal_sleep_idle"),
            on: settings.get(&SLEEP_IDLE),
        }];
        if settings.get(&SLEEP_IDLE) {
            rows.push(SettingsRow::Dropdown {
                key: "sleep_after",
                label: t("settings.terminal_sleep_after"),
                detail: t("settings.terminal_sleep_after_detail"),
                icon: None,
                choices: SLEEP_AFTERS.iter().map(|&m| (m, sleep_label(m))).collect(),
                current: settings.get(&SLEEP_AFTER),
                default: Some(SLEEP_AFTER.default_value()),
            });
        }
        SettingsBlock {
            title: t("settings.terminal_sleep"),
            detail: t("settings.terminal_sleep_detail"),
            rows,
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let settings = scope.settings;
        match key {
            "sleep_idle" => save(settings, &SLEEP_IDLE, value != 0),
            "sleep_after" if SLEEP_AFTERS.contains(&value) => save(settings, &SLEEP_AFTER, value),
            _ => {}
        }
    }
}

/// An idle time offered, as shown: `15 min`, `1 h`.
fn sleep_label(minutes: i64) -> String {
    if minutes % 60 == 0 {
        i18n::tr!("settings.terminal_sleep_hours", n = minutes / 60)
    } else {
        i18n::tr!("settings.terminal_sleep_minutes", n = minutes)
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
