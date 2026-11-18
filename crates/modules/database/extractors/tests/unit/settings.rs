use super::*;

use crate::memory;

#[test]
fn formats_are_read_back_without_the_broken_or_repeated() {
    let mine = CsvFormat::new("csv-1", "Mine", "#", false);
    let lines = vec![mine.encode(), "garbage".to_owned(), mine.encode()];
    assert_eq!(formats_of(&lines), Some(vec![mine]));
    assert_eq!(formats_of(&[]), None);
    assert_eq!(formats_of(&["id=;name=x".to_owned()]), None);
}

#[test]
fn the_defaults_are_not_kept_and_others_are() {
    let s = memory::settings();
    save_formats(&s, &CsvFormat::defaults());
    assert!(s.get(&CSV_FORMATS).is_empty());
    let mut formats = CsvFormat::defaults();
    formats[0].null_text = "NULL".into();
    save_formats(&s, &formats);
    assert_eq!(formats_of(&s.get(&CSV_FORMATS)), Some(formats));
}

#[test]
fn the_settings_block_switches_the_skips() {
    let s = memory::settings();
    let scope = SettingsScope {
        settings: &s,
        project: None,
        project_data: None,
        wake: None,
        language: None,
    };
    let block = ExtractorSettings.block(scope);
    assert!(matches!(block.rows[0], SettingsRow::Toggle { key: "skip_computed", on: false, .. }));
    ExtractorSettings.choose(scope, "skip_computed", 1);
    ExtractorSettings.choose(scope, "skip_generated", 1);
    assert!(s.get(&SKIP_COMPUTED) && s.get(&SKIP_GENERATED));
    let block = ExtractorSettings.block(scope);
    assert!(matches!(block.rows[1], SettingsRow::Toggle { key: "skip_generated", on: true, .. }));
}
