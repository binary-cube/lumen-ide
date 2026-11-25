use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use settings_model::{Section, SettingsStore};

use super::*;

/// Settings kept in memory.
#[derive(Debug, Default)]
struct Memory(Mutex<HashMap<String, Section>>);

impl SettingsStore for Memory {
    fn section(&self, name: &str) -> Option<Section> {
        self.0.lock().unwrap().get(name).cloned()
    }
    fn update(&self, name: &str, change: &mut dyn FnMut(&mut Section)) -> std::io::Result<()> {
        change(self.0.lock().unwrap().entry(name.to_owned()).or_default());
        Ok(())
    }
}

fn service() -> SettingsService {
    SettingsService::new(Arc::new(Memory::default()))
}

fn scope(settings: &SettingsService) -> SettingsScope<'_> {
    SettingsScope { settings, project: None, project_data: None, wake: None, language: None }
}

/// Nothing saved: the defaults, as before the settings existed.
#[test]
fn the_defaults_are_the_terminal_as_it_was() {
    let config = read(&service());
    assert_eq!(config, TerminalConfig::default());
}

/// Each choice is saved and read back; a switch is 1 / 0; the sliders their stop.
#[test]
fn the_display_choices_are_saved() {
    let settings = service();
    let display = DisplaySettings;
    display.choose(scope(&settings), "colors", 2);
    display.choose(scope(&settings), "cursor", 1);
    display.choose(scope(&settings), "cursor_blink", 0);
    display.choose(scope(&settings), "bold_bright", 1);
    display.choose(scope(&settings), "font_size", 14);
    display.choose(scope(&settings), "line_height", 150);
    display.choose(scope(&settings), "scrollback", 20_000);
    let config = read(&settings);
    assert_eq!(config.colors, Colors::HighContrast);
    assert_eq!(config.cursor, Cursor::Bar);
    assert!(!config.blink && config.bold_bright);
    assert_eq!(config.font_size, Some(14.0));
    assert_eq!(config.line_height, 1.5);
    assert_eq!(config.scrollback, 20_000);
    // The block shows them.
    let block = display.block(scope(&settings));
    assert!(block.rows.contains(&SettingsRow::Toggle {
        key: "cursor_blink",
        label: i18n::t("settings.terminal_blink"),
        detail: i18n::t("settings.terminal_blink_detail"),
        on: false,
        default: Some(true),
    }));
}

/// The shells found are the choices, then "Other…"; "Other…" shows the command's field,
/// whose text is saved.
#[test]
fn a_shell_is_chosen_by_its_id_and_other_shows_its_command() {
    let settings = service();
    let shell = ShellSettings;
    let block = shell.block(scope(&settings));
    let SettingsRow::Choices { choices, current, .. } = &block.rows[0] else { panic!() };
    assert_eq!(choices.len(), pty::shells().len() + 1);
    assert_eq!(*current, 0, "the default");
    assert_eq!(
        block.rows.len(),
        4,
        "no command field: the shells, the prompt's and the history's switches, the keys"
    );
    assert!(matches!(&block.rows[3], SettingsRow::Notes(notes) if notes.len() == 3));
    assert!(matches!(block.rows[1], SettingsRow::Toggle { key: "themed_prompt", on: false, .. }));
    assert!(matches!(block.rows[2], SettingsRow::Toggle { key: "project_history", on: true, .. }));
    shell.choose(scope(&settings), "themed_prompt", 1);
    assert!(read(&settings).themed_prompt);
    shell.choose(scope(&settings), "project_history", 0);
    assert!(!read(&settings).project_history, "the user's usual history");
    shell.choose(scope(&settings), "shell", 0);
    assert_eq!(settings.get(&SHELL), pty::shells()[0].id);
    shell.choose(scope(&settings), "shell", CUSTOM_CHOICE);
    shell.set_text(scope(&settings), "custom_shell", "/opt/shell -i");
    let block = shell.block(scope(&settings));
    assert!(matches!(&block.rows[1], SettingsRow::Text { value, .. } if value == "/opt/shell -i"));
    let config = read(&settings);
    assert_eq!(config.shell, CUSTOM);
    assert_eq!(crate::config::profile(&config, None, None).program.as_deref(), Some("/opt/shell"));
}

/// What programs' requests to erase the history get: keep it for running programs by
/// default; each choice saved and read back, shown in the block.
#[test]
fn the_history_clear_choice() {
    use terminal_session::history::HistoryClear;
    let settings = service();
    assert_eq!(read(&settings).history_clear, HistoryClear::KeepForRunningPrograms);
    let display = DisplaySettings;
    for (value, policy) in [
        (0, HistoryClear::Standard),
        (2, HistoryClear::KeepAlways),
        (1, HistoryClear::KeepForRunningPrograms),
    ] {
        display.choose(scope(&settings), "history_clear", value);
        assert_eq!(read(&settings).history_clear, policy);
        let block = display.block(scope(&settings));
        assert!(block.rows.iter().any(|row| matches!(
            row,
            SettingsRow::Choices { key: "history_clear", current, default: Some(1), .. }
                if *current == value
        )));
    }
}

/// Sleeping is off by default (nothing stopped unasked); turned on, an hour unless another
/// offered time is chosen (one not offered is refused); its time shown only while it's on.
#[test]
fn sleeping_is_off_until_chosen() {
    let settings = service();
    assert_eq!(read(&settings).sleep_after, None);
    let block = SleepSettings.block(scope(&settings));
    assert_eq!(block.rows.len(), 1, "only the switch while it's off");
    SleepSettings.choose(scope(&settings), "sleep_idle", 1);
    assert_eq!(read(&settings).sleep_after, Some(std::time::Duration::from_secs(3600)));
    SleepSettings.choose(scope(&settings), "sleep_after", 15);
    assert_eq!(read(&settings).sleep_after, Some(std::time::Duration::from_secs(900)));
    SleepSettings.choose(scope(&settings), "sleep_after", 7);
    assert_eq!(read(&settings).sleep_after, Some(std::time::Duration::from_secs(900)));
    let block = SleepSettings.block(scope(&settings));
    assert_eq!(block.rows.len(), 2, "its time too");
    SleepSettings.choose(scope(&settings), "sleep_idle", 0);
    assert_eq!(read(&settings).sleep_after, None);
    enable_sleep(&settings);
    assert!(settings.get(&SLEEP_IDLE), "turned on from elsewhere (a hint)");
    crate::config::configure(TerminalConfig::default());
}
