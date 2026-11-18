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

fn scope(settings: &SettingsService) -> SettingsScope<'_> {
    SettingsScope { settings, project: None, project_data: None, wake: None, language: None }
}

#[test]
fn the_defaults_are_the_formatter_s() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    assert_eq!(read(&settings), FormatOptions::default());
    assert!(INDENT_CHOICES.contains(&INDENT.default_value()));
    assert!(WIDTH_CHOICES.contains(&WIDTH.default_value()));
}

/// Each row of the block saves its setting, read back as the formatter's options; the
/// configured options follow `apply`.
#[test]
fn each_choice_is_saved_and_reaches_the_formatter() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let block = FormatSettings;
    block.choose(scope(&settings), "keyword_case", 1);
    block.choose(scope(&settings), "indent", 2);
    block.choose(scope(&settings), "width", 80);
    block.choose(scope(&settings), "commas", 1);
    block.choose(scope(&settings), "lists", 1);
    let options = read(&settings);
    assert_eq!(
        options,
        FormatOptions {
            keyword_case: KeywordCase::Lower,
            indent: 2,
            width: 80,
            commas: CommaStyle::Leading,
            lists: ListStyle::OnePerLine,
        }
    );
    // The only test touching the options in use (they're the module's, for all).
    block.apply(&settings);
    assert_eq!(super::options(), options);
    block.choose(scope(&settings), "keyword_case", 2);
    assert_eq!(read(&settings).keyword_case, KeywordCase::Preserve);
    // A value out of its range reads as the default.
    block.choose(scope(&settings), "width", 5);
    assert_eq!(read(&settings).width, 120);
    configure(FormatOptions::default());
}

#[test]
fn the_block_shows_the_current_values() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    let block = FormatSettings.block(scope(&settings));
    let current: Vec<i64> = block
        .rows
        .iter()
        .filter_map(|r| match r {
            SettingsRow::Segmented { current, .. }
            | SettingsRow::Choices { current, .. }
            | SettingsRow::Slider { current, .. } => Some(*current),
            SettingsRow::Toggle { on, .. } => Some(i64::from(*on)),
            _ => None,
        })
        .collect();
    assert_eq!(current, [0, 4, 120, 0, 0]);
    assert_eq!((FormatSettings.id(), FormatSettings.page()), ("sql_format", "languages"));
}
