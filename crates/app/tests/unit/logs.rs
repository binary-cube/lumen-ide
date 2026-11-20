use super::*;

/// The level's filter, the dependencies kept quiet; `off` nothing at all; a level not known
/// is `info`; the modules' own levels added, one that can't be read left out.
#[test]
fn the_filter_is_the_level_and_the_modules_own() {
    assert_eq!(filter("info", ""), format!("info,{QUIET}"));
    assert_eq!(filter("debug", ""), format!("debug,{QUIET}"));
    assert_eq!(filter("off", "database_explorer=debug"), "off");
    assert_eq!(filter("loud", ""), format!("info,{QUIET}"));
    assert_eq!(
        filter("warn", " database_explorer=debug , vcs_git=trace "),
        format!("warn,{QUIET},database_explorer=debug,vcs_git=trace")
    );
    assert_eq!(filter("info", "database=loud, git=warn"), format!("info,{QUIET},git=warn"));
    assert_eq!(unreadable("database=loud, git=warn,,"), ["database=loud"]);
    // Each filter made reads as one.
    for (level, _) in LEVELS {
        assert!(logging::check_filter(&filter(level, "a=debug")).is_ok(), "{level}");
    }
}

/// Every level has its label, in both languages' order.
#[test]
fn every_level_is_one_the_setting_accepts() {
    for (level, label) in LEVELS {
        assert!(crate::settings::LOG_LEVEL.accepts(&level.to_owned()), "{level}");
        assert!(!i18n::t(label).is_empty());
    }
    assert!(!crate::settings::LOG_LEVEL.accepts(&"verbose".to_owned()));
    assert_eq!(crate::settings::LOG_LEVEL.default_value(), "info");
}

/// A module's level set, changed and taken away (as the others), the others' kept; what a
/// row shows for it; every module's list offers what the filter reads.
#[test]
fn a_modules_level_is_set_and_taken_away() {
    let one = with_level("", "terminal_panel", Some("debug"));
    assert_eq!(one, "terminal_panel=debug");
    let two = with_level(&one, "vcs_git", Some("warn"));
    assert_eq!(two, "terminal_panel=debug,vcs_git=warn");
    assert_eq!(
        with_level(&two, "terminal_panel", Some("trace")),
        "vcs_git=warn,terminal_panel=trace"
    );
    assert_eq!(with_level(&two, "terminal_panel", None), "vcs_git=warn");
    let chosen = pairs(" vcs_git = WARN ,, broken");
    assert_eq!(chosen, [("vcs_git".to_owned(), "warn".to_owned())]);
    assert_eq!(module_choice(&chosen, "vcs_git"), 3);
    assert_eq!(module_choice(&chosen, "terminal_panel"), 0);
    for level in MODULE_LEVELS {
        assert!(logging::check_filter(&format!("a={level}")).is_ok(), "{level}");
    }
    assert_eq!(module_choices().len(), MODULE_LEVELS.len() + 1);
}

/// The modules listed: only the active ones, one row per target, by name, each with its
/// icon.
#[test]
fn only_the_active_modules_are_listed_once_each() {
    note_module("tests.b", "Beta", "terminal", "test_b");
    note_module("tests.a", "Alpha", "database", "test_a");
    note_module("tests.a2", "Alpha Two", "modules", "test_a");
    note_module("tests.off", "Off", "modules", "test_off");
    set_active(["tests.a", "tests.a2", "tests.b"]);
    let rows: Vec<_> = listed().into_iter().filter(|(t, ..)| t.starts_with("test_")).collect();
    assert_eq!(
        rows,
        [("test_a", "Alpha".to_owned(), "database"), ("test_b", "Beta".to_owned(), "terminal")]
    );
}
