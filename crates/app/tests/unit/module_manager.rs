use super::*;
use extensions::ModuleSelection;

/// Runs the test `name` of this binary alone, in a process of its own (`EDITOR_TEST_ALONE`
/// set there): what a module keeps in memory is the process's (PHP's stubs are one static),
/// and the tests running beside it in this one load it meanwhile. `true` in that process:
/// the caller does the test's work.
fn alone(name: &str) -> bool {
    if std::env::var_os("EDITOR_TEST_ALONE").is_some() {
        return true;
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--test-threads=1", "--nocapture"])
        .env("EDITOR_TEST_ALONE", "1")
        .status()
        .unwrap();
    assert!(status.success(), "{name}, alone: {status}");
    false
}

#[test]
fn nothing_is_in_memory_before_it_is_needed() {
    if !alone("module_manager::tests::nothing_is_in_memory_before_it_is_needed") {
        return;
    }
    let (support, _) = crate::modules::load(&ModuleSelection::default());
    let manager = &support.manager;
    // Activated: only described, nothing read.
    assert!(!manager.loaded("lang.yaml"));
    assert!(!manager.loaded("lang.php"));
    // A YAML file opened: its grammar is compiled — the module is in memory.
    support.syntax.grammars().warm_up(&LanguageId::new("yaml"));
    assert!(manager.loaded("lang.yaml"));
    // PHP read ahead (its stubs, its grammar), then let go once unused.
    manager.preload(&["lang.php".to_owned()]);
    assert!(manager.loaded("lang.php"));
    assert!(manager.registry.release_idle(Duration::ZERO, |_| false) > 0);
    assert!(!manager.registry.loaded("lang.php"), "the stubs are let go");
    // "Clear and Rebuild Caches": every module's data goes now, and the compiled grammars
    // no open file holds.
    support.syntax.grammars().warm_up(&LanguageId::new("yaml"));
    manager.preload(&["lang.php".to_owned()]);
    assert!(manager.loaded("lang.php") && manager.loaded("lang.yaml"));
    let (modules, grammars) = manager.purge();
    assert!(modules > 0 && grammars > 0);
    assert!(!manager.loaded("lang.php") && !manager.loaded("lang.yaml"));
}

/// A module's visibility saved and read back (a new service on the same file, as at the
/// next start): the diagram editor hidden by default, shown again and remembered shown (the
/// default never hides it again), hidden again; another module shown by default, hidden by
/// the user. The selection the app starts with says the same.
#[test]
fn a_module_hidden_or_shown_is_saved_and_its_default_kept() {
    use crate::settings::{MODULES_HIDDEN, MODULES_SHOWN, module_hidden};
    let file = std::env::temp_dir().join(format!("modules-{}.toml", kernel::id::new_uuid()));
    let service = |file: &std::path::Path| {
        let store = storage::TomlSettingsStore::new(Some(file.to_path_buf()));
        settings_model::SettingsService::new(Arc::new(store))
    };
    let editor = "tool.diagram-editor";
    let first = service(&file);
    assert!(module_hidden(&first, editor), "hidden by default");
    assert!(!module_hidden(&first, "lang.php"));
    set_hidden(&first, editor, false);
    set_hidden(&first, "lang.php", true);
    let again = service(&file);
    assert!(!module_hidden(&again, editor), "shown again, read back");
    assert_eq!(again.get(&MODULES_SHOWN), vec![editor.to_owned()]);
    assert!(module_hidden(&again, "lang.php"));
    assert_eq!(again.get(&MODULES_HIDDEN), vec!["lang.php".to_owned()]);
    set_hidden(&again, editor, true);
    set_hidden(&again, "lang.php", false);
    let last = service(&file);
    assert!(module_hidden(&last, editor) && !module_hidden(&last, "lang.php"));
    assert!(last.get(&MODULES_SHOWN).is_empty(), "hidden: no longer „shown”");
    let _ = std::fs::remove_file(&file);
}

/// What the app starts with: the diagram editor hidden by default, then as chosen.
#[test]
fn the_selection_hides_the_modules_hidden_by_default_until_shown() {
    let settings = crate::config::settings();
    let hides = || crate::modules::selection_from_settings().hidden;
    assert_eq!(hides(), vec!["tool.diagram-editor".to_owned()]);
    set_hidden(settings, "tool.diagram-editor", false);
    assert!(hides().is_empty());
    set_hidden(settings, "lang.php", true);
    assert_eq!(hides(), vec!["lang.php".to_owned()]);
    set_hidden(settings, "lang.php", false);
    set_hidden(settings, "tool.diagram-editor", true);
    assert_eq!(hides(), vec!["tool.diagram-editor".to_owned()]);
}
