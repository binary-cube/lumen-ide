use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use code_quality::Choice;
use settings_model::{Section, SettingsStore};

use super::*;

/// Settings kept in memory.
#[derive(Debug, Default)]
pub(crate) struct Memory(Mutex<HashMap<String, Section>>);

impl SettingsStore for Memory {
    fn section(&self, name: &str) -> Option<Section> {
        self.0.lock().unwrap().get(name).cloned()
    }
    fn update(&self, name: &str, change: &mut dyn FnMut(&mut Section)) -> std::io::Result<()> {
        change(self.0.lock().unwrap().entry(name.to_owned()).or_default());
        Ok(())
    }
}

/// Nothing chosen: none; each choice saved and read back, per language, the on-save flags
/// apart.
#[test]
fn a_language_s_choices_are_saved_per_project() {
    let settings = SettingsService::new(Arc::new(Memory::default()));
    assert_eq!(choice(&settings, "php"), LanguageChoice::default());
    let script =
        Choice::Command { line: "./scripts/linux/shell.sh composer cs:fix".into(), reads_as: None };
    save_choice(&settings, "php", Kind::Formatter, &script).unwrap();
    save_choice(&settings, "php", Kind::Checker, &Choice::Tool("phpcs".into())).unwrap();
    save_on_save(&settings, "php", Kind::Checker, true).unwrap();
    let php = choice(&settings, "php");
    assert_eq!(php.formatter, script);
    assert_eq!(php.checker, Choice::Tool("phpcs".into()));
    assert!(php.check_on_save && !php.format_on_save);
    assert_eq!(choice(&settings, "javascript"), LanguageChoice::default(), "another language");
    let binary = Choice::Binary { path: "tools/cs".into(), reads_as: Some("phpcs".into()) };
    save_choice(&settings, "php", Kind::Checker, &binary).unwrap();
    assert_eq!(choice(&settings, "php").checker, binary);
    save_choice(&settings, "php", Kind::Checker, &Choice::None).unwrap();
    assert_eq!(choice(&settings, "php").checker, Choice::None);
}
