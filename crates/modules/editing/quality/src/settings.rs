//! A project's choices for a language, in its own settings (`<project dir>/project.toml`, the
//! section `[quality-<language>]`): the formatter and the checker (each `none`, `tool`,
//! `binary` or `command`, its value and the format its output is read in), and what runs on
//! save.

use code_quality::{Choice, Kind, LanguageChoice};
use settings_model::{Section, SettingValue, SettingsService};

/// The section of `language`'s choices.
pub fn section(language: &str) -> String {
    format!("quality-{language}")
}

fn prefix(kind: Kind) -> &'static str {
    match kind {
        Kind::Formatter => "formatter",
        Kind::Checker => "checker",
    }
}

fn text(section: &Section, key: &str) -> Option<String> {
    section.get(key).and_then(SettingValue::as_str).map(str::to_owned).filter(|s| !s.is_empty())
}

fn read(section: &Section, kind: Kind) -> Choice {
    let p = prefix(kind);
    let value = text(section, &format!("{p}_value"));
    let reads_as = text(section, &format!("{p}_reads"));
    match (text(section, p).as_deref(), value) {
        (Some("tool"), Some(id)) => Choice::Tool(id),
        (Some("binary"), Some(path)) => Choice::Binary { path, reads_as },
        (Some("command"), Some(line)) => Choice::Command { line, reads_as },
        _ => Choice::None,
    }
}

/// What the project uses for `language` (nothing chosen: none).
pub fn choice(settings: &SettingsService, language: &str) -> LanguageChoice {
    let Some(section) = settings.section(&section(language)) else {
        return LanguageChoice::default();
    };
    let on = |key: &str| section.get(key).and_then(SettingValue::as_bool).unwrap_or(false);
    LanguageChoice {
        formatter: read(&section, Kind::Formatter),
        checker: read(&section, Kind::Checker),
        format_on_save: on("format_on_save"),
        check_on_save: on("check_on_save"),
    }
}

/// What the settings hold for a `kind` tool of `language`, as written: what (`none`,
/// `tool`, `binary`, `command`), its value and the tool it reads as — a binary or a command
/// kept chosen while its value is still to be written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Saved {
    pub what: String,
    pub value: String,
    pub reads: String,
}

/// The saved form of a `kind` tool for `language`.
pub fn saved(settings: &SettingsService, language: &str, kind: Kind) -> Saved {
    let Some(section) = settings.section(&section(language)) else { return Saved::default() };
    let p = prefix(kind);
    let get = |key: &str| text(&section, key).unwrap_or_default();
    Saved { what: get(p), value: get(&format!("{p}_value")), reads: get(&format!("{p}_reads")) }
}

/// Saves a `kind` tool for `language` as written (empty values removed).
pub fn save(
    settings: &SettingsService,
    language: &str,
    kind: Kind,
    saved: &Saved,
) -> std::io::Result<()> {
    let p = prefix(kind);
    settings.update(&section(language), &mut |section| {
        let fields = [(p.to_owned(), &saved.what), (format!("{p}_value"), &saved.value)];
        for (key, value) in fields.into_iter().chain([(format!("{p}_reads"), &saved.reads)]) {
            if value.is_empty() {
                section.remove(&key);
            } else {
                section.insert(key, SettingValue::Text(value.clone()));
            }
        }
    })
}

/// Saves the project's choice of a `kind` tool for `language`.
pub fn save_choice(
    settings: &SettingsService,
    language: &str,
    kind: Kind,
    choice: &Choice,
) -> std::io::Result<()> {
    let (what, value, reads) = match choice {
        Choice::None => ("none", None, None),
        Choice::Tool(id) => ("tool", Some(id), None),
        Choice::Binary { path, reads_as } => ("binary", Some(path), reads_as.as_ref()),
        Choice::Command { line, reads_as } => ("command", Some(line), reads_as.as_ref()),
    };
    let owned = |v: Option<&String>| v.cloned().unwrap_or_default();
    save(
        settings,
        language,
        kind,
        &Saved { what: what.into(), value: owned(value), reads: owned(reads) },
    )
}

/// Whether a `kind` tool runs on save for `language`.
pub fn on_save(settings: &SettingsService, language: &str, kind: Kind) -> bool {
    let choice = choice(settings, language);
    match kind {
        Kind::Formatter => choice.format_on_save,
        Kind::Checker => choice.check_on_save,
    }
}

/// Saves whether a `kind` tool runs on save for `language`.
pub fn save_on_save(
    settings: &SettingsService,
    language: &str,
    kind: Kind,
    on: bool,
) -> std::io::Result<()> {
    let key = match kind {
        Kind::Formatter => "format_on_save",
        Kind::Checker => "check_on_save",
    };
    settings.update(&section(language), &mut |section| {
        section.insert(key.to_owned(), SettingValue::Bool(on));
    })
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
pub(crate) mod tests;
