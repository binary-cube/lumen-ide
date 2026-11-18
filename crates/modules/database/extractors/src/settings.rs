//! The extractors' settings: `[extractors] skip_computed`, `skip_generated` and
//! `csv_formats` (every grid's), and `[extractors] chosen` in a project's own settings
//! (`<project dir>/project.toml`: the extractor its grids copy and export with). Declared
//! here, handed to the catalog by [`apply`] at startup and after every change, shown in the
//! "Data extractors" block ([`ExtractorSettings`], in the editor's section).

use extractors::{CsvFormat, Options, catalog};
use i18n::t;
use settings_model::{Setting, SettingsService};
use workbench_model::{
    SettingsBlock, SettingsContribution, SettingsPage, SettingsRow, SettingsScope,
};

/// The SQL extractors leave out the columns the engine computes (`[extractors]
/// skip_computed`).
pub const SKIP_COMPUTED: Setting<bool> = Setting::new("extractors", "skip_computed", || false);

/// … and those it makes (auto-increments, identities: `[extractors] skip_generated`).
pub const SKIP_GENERATED: Setting<bool> = Setting::new("extractors", "skip_generated", || false);

/// The separated formats offered, one [`CsvFormat::encode`] line each (`[extractors]
/// csv_formats`; empty: the defaults).
pub const CSV_FORMATS: Setting<Vec<String>> =
    Setting::new("extractors", "csv_formats", Vec::new).valid(|lines| lines.len() <= 100);

/// The extractor a project's grids use (`[extractors] chosen` of its `project.toml`; empty:
/// the default).
pub const CHOSEN: Setting<String> =
    Setting::new("extractors", "chosen", String::new).valid(|id| id.len() <= 400);

/// The formats `lines` keep (those that can't be read, or repeat an id, dropped; none:
/// `None` — the defaults).
pub fn formats_of(lines: &[String]) -> Option<Vec<CsvFormat>> {
    let mut formats: Vec<CsvFormat> = Vec::new();
    for format in lines.iter().filter_map(|l| CsvFormat::decode(l)) {
        if !formats.iter().any(|f| f.id == format.id) {
            formats.push(format);
        }
    }
    (!formats.is_empty()).then_some(formats)
}

/// The settings' values handed to the catalog.
pub fn apply(settings: &SettingsService) {
    let catalog = catalog();
    catalog.set_options(Options {
        skip_computed: settings.get(&SKIP_COMPUTED),
        skip_generated: settings.get(&SKIP_GENERATED),
    });
    catalog.set_csv_formats(formats_of(&settings.get(&CSV_FORMATS)));
}

/// `value` saved (said in the log when it can't be).
pub fn save<T: settings_model::SettingType + PartialEq>(
    settings: &SettingsService,
    setting: &Setting<T>,
    value: T,
) {
    if settings.get(setting) == value && settings.is_set(setting) {
        return;
    }
    if let Err(e) = settings.set(setting, value) {
        tracing::warn!("can't save an extractors' setting: {e}");
    }
}

/// The formats saved (the defaults: nothing kept, they stay the defaults as they change).
pub fn save_formats(settings: &SettingsService, formats: &[CsvFormat]) {
    let lines: Vec<String> = if formats == CsvFormat::defaults().as_slice() {
        Vec::new()
    } else {
        formats.iter().map(CsvFormat::encode).collect()
    };
    save(settings, &CSV_FORMATS, lines);
}

/// The "Data extractors" block of the settings window.
#[derive(Debug, Default)]
pub struct ExtractorSettings;

impl SettingsContribution for ExtractorSettings {
    fn id(&self) -> &'static str {
        "extractors"
    }

    fn page(&self) -> &'static str {
        "database"
    }

    fn page_info(&self) -> Option<SettingsPage> {
        Some(SettingsPage::DATABASE)
    }

    fn order(&self) -> u16 {
        30
    }

    fn apply(&self, settings: &SettingsService) {
        apply(settings);
    }

    fn block(&self, scope: SettingsScope<'_>) -> SettingsBlock {
        let s = scope.settings;
        let formats = formats_of(&s.get(&CSV_FORMATS)).unwrap_or_else(CsvFormat::defaults);
        let names: Vec<String> = formats.iter().map(|f| f.name.clone()).collect();
        SettingsBlock {
            title: t("settings.extractors_title"),
            detail: t("settings.extractors_detail"),
            rows: vec![
                SettingsRow::Toggle {
                    key: "skip_computed",
                    detail: t("settings.extractors_skip_computed_detail"),
                    default: Some(SKIP_COMPUTED.default_value()),
                    label: t("extract.skip_computed"),
                    on: s.get(&SKIP_COMPUTED),
                },
                SettingsRow::Toggle {
                    key: "skip_generated",
                    detail: t("settings.extractors_skip_generated_detail"),
                    default: Some(SKIP_GENERATED.default_value()),
                    label: t("extract.skip_generated"),
                    on: s.get(&SKIP_GENERATED),
                },
                SettingsRow::Notes(vec![
                    i18n::tr!("settings.extractors_formats", names = names.join(", ")),
                    t("settings.extractors_chosen"),
                    t("settings.extractors_templates"),
                ]),
            ],
        }
    }

    fn choose(&self, scope: SettingsScope<'_>, key: &'static str, value: i64) {
        let s = scope.settings;
        match key {
            "skip_computed" => save(s, &SKIP_COMPUTED, value != 0),
            "skip_generated" => save(s, &SKIP_GENERATED, value != 0),
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;
