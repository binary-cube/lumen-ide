use super::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use settings_model::{Section, SettingsStore};

#[derive(Debug, Default)]
struct Memory {
    sections: Mutex<HashMap<String, Section>>,
}

impl SettingsStore for Memory {
    fn section(&self, name: &str) -> Option<Section> {
        self.sections.lock().unwrap().get(name).cloned()
    }
    fn update(&self, name: &str, change: &mut dyn FnMut(&mut Section)) -> std::io::Result<()> {
        change(self.sections.lock().unwrap().entry(name.to_owned()).or_default());
        Ok(())
    }
}

fn settings() -> SettingsService {
    SettingsService::new(Arc::new(Memory::default()))
}

#[test]
fn only_the_values_offered_are_taken() {
    assert!(HISTORY.accepts(&300) && !HISTORY.accepts(&120));
    assert!(INTERVAL.accepts(&5) && !INTERVAL.accepts(&0));
    assert!(DETAIL_RATIO.accepts(&15) && DETAIL_RATIO.accepts(&60) && !DETAIL_RATIO.accepts(&61));
    assert!(COLUMNS.accepts(&String::new()));
    assert!(COLUMNS.accepts(&"cpu, +gpu,-threads".to_owned()));
    assert!(!COLUMNS.accepts(&"cpu,cpu".to_owned()), "once each");
    assert!(!COLUMNS.accepts(&"+colour".to_owned()), "known ids only");
    assert_eq!(
        columns_of("memory,+gpu,-threads"),
        Some(vec![
            ("memory", ColumnChoice::Auto),
            ("gpu", ColumnChoice::Pinned),
            ("threads", ColumnChoice::Hidden),
        ])
    );
    let s = settings();
    assert_eq!((s.get(&HISTORY), s.get(&INTERVAL), s.get(&DETAIL_RATIO)), (60, 1, 30));
}

#[test]
fn the_block_chooses_the_history_and_the_interval() {
    let _turn =
        crate::view::PREFERENCES_TESTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let s = settings();
    let scope = SettingsScope {
        settings: &s,
        project: None,
        project_data: None,
        wake: None,
        language: None,
    };
    let block = TaskSettings.block(scope);
    assert!(
        matches!(&block.rows[0], SettingsRow::Segmented { key: "history", current: 60, choices, .. } if choices.len() == 3)
    );
    assert!(matches!(&block.rows[1], SettingsRow::Segmented { key: "interval", current: 1, .. }));
    TaskSettings.choose(scope, "history", 900);
    TaskSettings.choose(scope, "interval", 2);
    TaskSettings.choose(scope, "interval", 3);
    assert_eq!((s.get(&HISTORY), s.get(&INTERVAL)), (900, 2), "3 s isn't offered");
    TaskSettings.apply(&s);
    assert_eq!(crate::collector::hub::interval(), Duration::from_secs(2));
    assert_eq!(crate::collector::hub::history_window(), Duration::from_secs(900));
    // Back to the defaults (other tests read them).
    TaskSettings.choose(scope, "history", 60);
    TaskSettings.choose(scope, "interval", 1);
    TaskSettings.apply(&s);
    assert_eq!(crate::collector::hub::interval(), Duration::from_secs(1));
}

/// What a view chose (its columns, its detail's share, its timing) written to the settings
/// and read back by the views.
#[test]
fn a_view_s_preferences_are_saved_and_read_back() {
    let _turn =
        crate::view::PREFERENCES_TESTS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let s = settings();
    let mut chosen = crate::Preferences::default();
    chosen.choices[crate::layout::ColumnId::Gpu.index()] = Choice::Pinned;
    chosen.choices[crate::layout::ColumnId::Threads.index()] = Choice::Hidden;
    chosen.widths[crate::layout::ColumnId::Name.index()] = 260.0;
    chosen.ratio = 0.42;
    chosen.interval = 2;
    chosen.history = 300;
    save_preferences(&s, &chosen);
    assert_eq!(s.get(&COLUMNS), "+gpu,-threads");
    assert_eq!(s.get(&WIDTHS), "name:260");
    assert_eq!((s.get(&DETAIL_RATIO), s.get(&INTERVAL), s.get(&HISTORY)), (42, 2, 300));
    apply(&s);
    let read = crate::preferences();
    assert_eq!(read.choices, chosen.choices);
    assert_eq!(read.widths, chosen.widths);
    assert!((read.ratio - 0.42).abs() < 1e-6);
    // A share past its limits saved within them.
    save_preferences(&s, &crate::Preferences { ratio: 0.9, ..chosen });
    assert_eq!(s.get(&DETAIL_RATIO), 60);
    // Back to the defaults (other tests read them).
    save_preferences(&s, &crate::Preferences::default());
    apply(&s);
    assert_eq!(crate::preferences(), crate::Preferences::default());
}

/// The columns' widths written as `[tasks] widths` says them and read back; what can't be
/// read is refused whole.
#[test]
fn column_widths_round_trip() {
    let mut widths = [0.0; crate::layout::COLUMNS.len()];
    widths[crate::layout::ColumnId::Name.index()] = 240.0;
    widths[crate::layout::ColumnId::Memory.index()] = 91.6;
    let text = widths_text(&widths);
    assert_eq!(text, "name:240,memory:92");
    widths[crate::layout::ColumnId::Memory.index()] = 92.0;
    assert_eq!(widths_of(&text), Some(widths));
    assert_eq!(widths_of(""), Some([0.0; crate::layout::COLUMNS.len()]));
    for bad in ["name:abc", "nope:100", "name:100,name:120", "name:5", "name:NaN", "name"] {
        assert_eq!(widths_of(bad), None, "{bad}");
        assert!(!WIDTHS.accepts(&bad.to_owned()), "{bad}");
    }
}
