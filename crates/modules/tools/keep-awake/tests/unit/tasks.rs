use super::*;
use std::time::Duration;

/// Off: no row. On: a row saying for how long it holds — and never as running work (it would
/// keep itself awake while working).
#[test]
fn a_row_while_on_that_is_never_running_work() {
    assert!(report_at(Instant::now()).is_empty(), "off: no row");
    let settings = settings_model::SettingsService::new(std::sync::Arc::new(Store::default()));
    crate::settings::save_on(&settings, true);
    let _ = crate::settings::apply(&settings);
    let later = Instant::now() + Duration::from_secs(180);
    let rows = report_at(later);
    assert_eq!(rows.len(), 1);
    assert!(rows[0].detail.contains('3'), "{}", rows[0].detail);
    assert!(Reporter.running().is_empty());
    crate::settings::save_on(&settings, false);
    let _ = crate::settings::apply(&settings);
}

#[derive(Debug, Default)]
struct Store(std::sync::Mutex<std::collections::HashMap<String, settings_model::Section>>);

impl settings_model::SettingsStore for Store {
    fn section(&self, name: &str) -> Option<settings_model::Section> {
        self.0.lock().unwrap().get(name).cloned()
    }
    fn update(
        &self,
        name: &str,
        change: &mut dyn FnMut(&mut settings_model::Section),
    ) -> std::io::Result<()> {
        change(self.0.lock().unwrap().entry(name.to_owned()).or_default());
        Ok(())
    }
}
