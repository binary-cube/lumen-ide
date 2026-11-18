use super::*;

#[test]
fn the_settings_reach_the_consoles() {
    configure_fetch(1000);
    assert_eq!(fetch_limit(), 1000);
    configure_fetch(0);
    assert_eq!(fetch_limit(), 1, "at least a row");
    configure_fetch(i64::from(database::run::FETCH_LIMIT));
    configure_stop(false);
    assert!(!stop_on_error());
    configure_stop(true);
    assert!(FETCH_LIMIT.default_value() == 500 && STOP_ON_ERROR.default_value());
}

#[test]
fn the_profiles_settings_make_their_options() {
    use database::profile::ProfileDepth;
    // Pure: the shared values aren't changed (the other tests read them).
    let run = options_with(VERBOSE_BIT, 60, true);
    assert_eq!(run.depth, ProfileDepth::Actual { timing: false });
    assert_eq!(run.timeout, Some(std::time::Duration::from_secs(60)));
    assert!(!run.buffers && run.verbose && !run.statement_history);
    let plan = options_with(BUFFERS_BIT | TIMING_BIT | STATS_BIT, 60, false);
    assert_eq!((plan.depth, plan.timeout), (ProfileDepth::Estimated, None), "a plan runs nothing");
    assert!(plan.buffers && plan.statement_history);
    // The defaults: timed, with buffers, five minutes, twenty profiles in 32 MB.
    let defaults = profile_options(true);
    assert_eq!(defaults.depth, ProfileDepth::Actual { timing: true });
    assert_eq!(defaults.timeout, Some(std::time::Duration::from_secs(300)));
    assert!(defaults.buffers && !defaults.verbose && defaults.statement_history);
    assert_eq!(profile_limits(), (20, 32 << 20));
}

/// The "Profiling" block: the seven settings, each saved by its own key.
#[test]
fn the_profiling_block_has_its_settings() {
    let settings = SettingsService::new(std::sync::Arc::new(Memory::default()));
    let scope = SettingsScope {
        settings: &settings,
        project: None,
        project_data: None,
        wake: None,
        language: None,
    };
    let block = ProfileSettings.block(scope);
    assert_eq!(block.title, t("settings.profile_title"));
    let keys: Vec<&str> = block
        .rows
        .iter()
        .filter_map(|r| match r {
            SettingsRow::Toggle { key, .. } | SettingsRow::Choices { key, .. } => Some(*key),
            _ => None,
        })
        .collect();
    assert_eq!(
        keys,
        [
            "console_profile_buffers",
            "console_profile_timing",
            "console_profile_verbose",
            "console_profile_statement_stats",
            "console_profile_timeout",
            "console_profile_keep",
            "console_profile_memory"
        ]
    );
    ProfileSettings.choose(scope, "console_profile_keep", 50);
    ProfileSettings.choose(scope, "console_profile_verbose", 1);
    assert_eq!(settings.get(&PROFILE_KEEP), 50);
    assert!(settings.get(&PROFILE_VERBOSE));
}

/// The settings in memory (no file).
#[derive(Debug, Default)]
struct Memory(std::sync::Mutex<std::collections::HashMap<String, settings_model::Section>>);

impl settings_model::SettingsStore for Memory {
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

#[test]
fn a_profile_switch_names_its_setting() {
    for switch in ProfileSwitch::ALL {
        assert_eq!(ProfileSwitch::from_id(switch.id()), Some(switch));
        assert!(switch.setting().key.starts_with("console_profile_"));
    }
}
