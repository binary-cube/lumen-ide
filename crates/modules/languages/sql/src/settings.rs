//! How SQL is laid out ("Reformat Code", `[sql] …` in `settings.toml`): the keywords'
//! case, a step of indentation, the width lines are kept within, where a broken list's
//! commas go and when lists break. Declared here, shown in the settings window by
//! [`FormatSettings`] (in the editor's section), handed to the formatter by [`apply`] at
//! startup and after every change.

use std::sync::RwLock;

use database::sql::{CommaStyle, FormatOptions, KeywordCase, ListStyle};
use i18n::t;
use settings_model::{Setting, SettingsService};
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsLanguage, SettingsPage, SettingsRow, SettingsScope,
};

/// The keywords' case: `upper`, `lower` or `preserve` (as written).
pub const KEYWORD_CASE: Setting<String> =
    Setting::new("sql", "format_keyword_case", || "upper".into())
        .valid(|v| CASES.iter().any(|(id, _)| id == v));

/// Spaces a step of indentation is.
pub const INDENT: Setting<i64> =
    Setting::new("sql", "format_indent", || 4).valid(|n| (1..=8).contains(n));

/// The width lines are kept within (columns).
pub const WIDTH: Setting<i64> =
    Setting::new("sql", "format_width", || 120).valid(|n| (40..=400).contains(n));

/// Where a broken list's commas go: `trailing` (after each item) or `leading`.
pub const COMMAS: Setting<String> = Setting::new("sql", "format_commas", || "trailing".into())
    .valid(|v| COMMA_STYLES.iter().any(|(id, _)| id == v));

/// When lists break one item a line: `fit` (only when too long) or `always`.
pub const LISTS: Setting<String> = Setting::new("sql", "format_lists", || "fit".into())
    .valid(|v| LIST_STYLES.iter().any(|(id, _)| id == v));

const CASES: [(&str, KeywordCase); 3] = [
    ("upper", KeywordCase::Upper),
    ("lower", KeywordCase::Lower),
    ("preserve", KeywordCase::Preserve),
];
const COMMA_STYLES: [(&str, CommaStyle); 2] =
    [("trailing", CommaStyle::Trailing), ("leading", CommaStyle::Leading)];
const LIST_STYLES: [(&str, ListStyle); 2] =
    [("fit", ListStyle::Fit), ("always", ListStyle::OnePerLine)];

/// The choices offered.
pub const INDENT_CHOICES: [i64; 3] = [2, 4, 8];
pub const WIDTH_CHOICES: [i64; 5] = [80, 100, 120, 160, 200];

/// The options in use (the defaults until configured).
static OPTIONS: RwLock<Option<FormatOptions>> = RwLock::new(None);

/// How SQL is laid out now.
pub fn options() -> FormatOptions {
    OPTIONS.read().ok().and_then(|o| *o).unwrap_or_default()
}

pub fn configure(options: FormatOptions) {
    if let Ok(mut current) = OPTIONS.write() {
        *current = Some(options);
    }
}

fn find<T: Copy>(list: &[(&str, T)], id: &str) -> Option<T> {
    list.iter().find(|(i, _)| *i == id).map(|(_, v)| *v)
}

/// The options the saved settings say.
pub fn read(settings: &SettingsService) -> FormatOptions {
    let defaults = FormatOptions::default();
    FormatOptions {
        keyword_case: find(&CASES, &settings.get(&KEYWORD_CASE)).unwrap_or(defaults.keyword_case),
        indent: settings.get(&INDENT).clamp(1, 8) as usize,
        width: settings.get(&WIDTH).clamp(40, 400) as usize,
        commas: find(&COMMA_STYLES, &settings.get(&COMMAS)).unwrap_or(defaults.commas),
        lists: find(&LIST_STYLES, &settings.get(&LISTS)).unwrap_or(defaults.lists),
    }
}

/// The saved values handed to the formatter (at startup and after a change).
pub fn apply(settings: &SettingsService) {
    configure(read(settings));
}

fn save<T: settings_model::SettingType>(
    settings: &SettingsService,
    setting: &Setting<T>,
    value: T,
) {
    if let Err(e) = settings.set(setting, value) {
        tracing::warn!(error = %e, "can't save a SQL formatting setting");
    }
}

/// A choice's index among `list`'s ids.
fn index_of<T>(list: &[(&str, T)], id: &str) -> i64 {
    list.iter().position(|(i, _)| *i == id).unwrap_or(0) as i64
}

/// The "SQL formatting" block of the settings window (Languages, while SQL is chosen).
#[derive(Debug, Default)]
pub struct FormatSettings;

impl SettingsContribution for FormatSettings {
    fn id(&self) -> &'static str {
        "sql_format"
    }

    fn page(&self) -> &'static str {
        "languages"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::LANGUAGES)
    }

    fn order(&self) -> u16 {
        30
    }

    fn language(&self) -> Option<SettingsLanguage> {
        Some(SettingsLanguage { id: "sql".to_owned(), name: "SQL".to_owned() })
    }

    fn apply(&self, settings: &SettingsService) {
        apply(settings);
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let s = scope.settings;
        let labelled = |keys: &[&str]| -> Vec<(i64, String)> {
            keys.iter().enumerate().map(|(i, k)| (i as i64, t(k))).collect()
        };
        SettingsBlock {
            title: t("settings.sql_format_title"),
            detail: t("settings.sql_format_detail"),
            rows: vec![
                SettingsRow::Segmented {
                    key: "keyword_case",
                    detail: t("settings.sql_format_keyword_case_detail"),
                    default: Some(index_of(&CASES, &KEYWORD_CASE.default_value())),
                    label: t("settings.sql_format_keyword_case"),
                    choices: labelled(&[
                        "settings.sql_format_upper",
                        "settings.sql_format_lower",
                        "settings.sql_format_preserve",
                    ]),
                    current: index_of(&CASES, &s.get(&KEYWORD_CASE)),
                },
                SettingsRow::Choices {
                    key: "indent",
                    detail: t("settings.sql_format_indent_detail"),
                    default: Some(INDENT.default_value()),
                    label: t("settings.sql_format_indent"),
                    choices: INDENT_CHOICES.iter().map(|&n| (n, n.to_string())).collect(),
                    current: s.get(&INDENT),
                },
                SettingsRow::Slider {
                    key: "width",
                    detail: t("settings.sql_format_width_detail"),
                    default: Some(WIDTH.default_value()),
                    label: t("settings.sql_format_width"),
                    stops: WIDTH_CHOICES.iter().map(|&n| (n, n.to_string())).collect(),
                    current: s.get(&WIDTH),
                },
                SettingsRow::Segmented {
                    key: "commas",
                    detail: t("settings.sql_format_commas_detail"),
                    default: Some(index_of(&COMMA_STYLES, &COMMAS.default_value())),
                    label: t("settings.sql_format_commas"),
                    choices: labelled(&[
                        "settings.sql_format_commas_trailing",
                        "settings.sql_format_commas_leading",
                    ]),
                    current: index_of(&COMMA_STYLES, &s.get(&COMMAS)),
                },
                SettingsRow::Toggle {
                    key: "lists",
                    detail: t("settings.sql_format_lists_detail"),
                    default: Some(LISTS.default_value() == "always"),
                    label: t("settings.sql_format_lists"),
                    on: s.get(&LISTS) == "always",
                },
                SettingsRow::Notes(vec![t("settings.sql_format_note")]),
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let s = scope.settings;
        let at = value.max(0) as usize;
        match key {
            "keyword_case" => {
                if let Some((id, _)) = CASES.get(at) {
                    save(s, &KEYWORD_CASE, (*id).to_owned());
                }
            }
            "indent" => save(s, &INDENT, value),
            "width" => save(s, &WIDTH, value),
            "commas" => {
                if let Some((id, _)) = COMMA_STYLES.get(at) {
                    save(s, &COMMAS, (*id).to_owned());
                }
            }
            "lists" => save(s, &LISTS, if value != 0 { "always" } else { "fit" }.to_owned()),
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
