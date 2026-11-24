use code_intelligence::servers::Engine;
use lsp_client::fake::Behaviour;
use settings_model::SettingsService;
use workbench_model::{SettingsContribution, SettingsRow, SettingsScope};

use super::*;
use crate::harness::Harness;

/// Settings kept in memory.
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

/// What the blocks see, the Languages page showing `language`.
fn scope<'a>(settings: &'a SettingsService, language: &'a str) -> SettingsScope<'a> {
    SettingsScope {
        settings,
        project: None,
        project_data: None,
        wake: None,
        language: Some(language),
    }
}

#[test]
fn the_engine_is_chosen_per_language_and_applied() {
    let _h = Harness::new(Behaviour::default());
    let settings = SettingsService::new(std::sync::Arc::new(Memory::default()));
    let page = ServerSettings::default();
    let block = page.block(scope(&settings, "typescript"));
    let engine = block.rows.iter().find_map(|r| match r {
        SettingsRow::Choices { key: "engine", choices, current, .. } => {
            Some((choices.clone(), *current))
        }
        _ => None,
    });
    let (choices, current) = engine.unwrap();
    assert_eq!(choices.len(), 4, "{choices:?}");
    assert_eq!(current, 1);
    page.choose(scope(&settings, "typescript"), "engine", BUILT_IN);
    assert_eq!(settings.get(&crate::settings::ENGINES), ["typescript=built-in"]);
    assert_eq!(crate::settings::config().choice("typescript").engine, Engine::BuiltIn);
    page.choose(scope(&settings, "typescript"), "engine", 2);
    assert_eq!(settings.get(&crate::settings::ENGINES), ["typescript=server:other"]);
    // The first server is the default: nothing saved.
    page.choose(scope(&settings, "typescript"), "engine", 1);
    assert!(settings.get(&crate::settings::ENGINES).is_empty());
    // The page's language is the block's: another one, its own engine.
    page.choose(scope(&settings, "tsx"), "engine", BUILT_IN);
    assert_eq!(settings.get(&crate::settings::ENGINES), ["tsx=built-in"]);
    page.choose(scope(&settings, "tsx"), "engine", 1);
    // A language the servers don't know (SQL's formatting only): nothing shown.
    assert!(page.block(scope(&settings, "sql")).rows.is_empty());
    assert!(page.languages(scope(&settings, "sql")).iter().any(|l| l.id == "tsx"));
    // The limits, on the Language servers page's "Advanced".
    let limits = ServerLimits;
    assert!(limits.advanced() && limits.page() == "language-servers");
    limits.choose(scope(&settings, "typescript"), "memory", 2048);
    assert_eq!(crate::settings::config().max_memory, 2048);
    limits.choose(scope(&settings, "typescript"), "idle", 0);
    assert_eq!(crate::settings::config().idle, None);
    limits.choose(scope(&settings, "typescript"), "trace", 1);
    assert!(crate::settings::tracing_flag().load(std::sync::atomic::Ordering::Relaxed));
    limits.choose(scope(&settings, "typescript"), "trace", 0);
}

#[test]
fn a_server_is_added_from_its_fields() {
    let _h = Harness::new(Behaviour::default());
    let settings = SettingsService::new(std::sync::Arc::new(Memory::default()));
    let user = crate::catalog::user_file().unwrap();
    let before = std::fs::read_to_string(&user).unwrap();
    let add = AddServer::default();
    add.press(scope(&settings, "typescript"), "add");
    let status = |add: &AddServer| {
        add.block(scope(&settings, "typescript")).rows.iter().find_map(|r| match r {
            SettingsRow::Status { text, .. } => Some(text.clone()),
            _ => None,
        })
    };
    assert_eq!(status(&add), Some(i18n::t("settings.lsp_add_incomplete")));
    let exe = std::env::current_exe().unwrap().to_string_lossy().into_owned();
    add.set_text(scope(&settings, "typescript"), "name", " lua-ls ");
    add.set_text(scope(&settings, "typescript"), "command", &exe);
    add.set_text(scope(&settings, "typescript"), "languages", "lua, luau");
    add.set_text(scope(&settings, "typescript"), "args", "--stdio --log=off");
    add.press(scope(&settings, "typescript"), "add");
    // Written on a worker (a press never writes the disk): said when it's done.
    let done = Some(i18n::tr!("settings.lsp_add_done", name = "lua-ls"));
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while status(&add) != done {
        assert!(std::time::Instant::now() < until, "added: {:?}", status(&add));
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let catalog = crate::catalog::catalog();
    let added = catalog.named("lua-ls").unwrap();
    assert_eq!(added.languages, ["lua", "luau"]);
    assert_eq!(added.args, ["--stdio", "--log=off"]);
    // Offered at once.
    assert!(catalog.languages().contains(&"lua".to_owned()));
    std::fs::write(&user, before).unwrap();
    crate::catalog::reload();
}

/// A language whose server is off by default on this OS (phpactor on Windows): the built-in
/// engine shown chosen, the reason said, the server still offered — and choosing it saves
/// it (the default saves nothing).
#[test]
fn a_server_off_by_default_here_is_said_and_still_offered() {
    let _h = Harness::new(Behaviour::default());
    let settings = SettingsService::new(std::sync::Arc::new(Memory::default()));
    let page = ServerSettings::default();
    let block = page.block(scope(&settings, "toml"));
    let current = block.rows.iter().find_map(|r| match r {
        SettingsRow::Choices { key: "engine", current, .. } => Some(*current),
        _ => None,
    });
    assert_eq!(current, Some(BUILT_IN));
    let said = ServerStatus
        .block(scope(&settings, "toml"))
        .rows
        .iter()
        .any(|r| matches!(r, SettingsRow::Status { text, .. } if text.contains("opt-in")));
    assert!(said, "{:?}", block.rows);
    page.choose(scope(&settings, "toml"), "engine", 1);
    assert_eq!(settings.get(&crate::settings::ENGINES), ["toml=server"]);
    page.choose(scope(&settings, "toml"), "engine", BUILT_IN);
    assert!(settings.get(&crate::settings::ENGINES).is_empty());
}

/// The block's rows, for PHP.
fn php_rows(page: &ServerSettings, settings: &SettingsService) -> Vec<SettingsRow> {
    page.block(scope(settings, "php")).rows
}

#[test]
fn a_server_to_install_is_offered_with_its_licence_and_installed_on_yes() {
    let _h = Harness::new(Behaviour::default());
    crate::harness::fresh_installs("page");
    let settings = SettingsService::new(std::sync::Arc::new(Memory::default()));
    let page = ServerSettings::default();
    let spec = crate::catalog::catalog().named("installable").unwrap().clone();
    crate::harness::wait("looked for", || {
        matches!(crate::install::state(&spec).found, crate::install::Found::Missing)
    });
    let rows = php_rows(&page, &settings);
    // Not installed: not among the engines, an Install button (nothing destroyed).
    let engines = rows.iter().find_map(|r| match r {
        SettingsRow::Choices { key: "engine", choices, .. } => Some(choices.clone()),
        _ => None,
    });
    assert_eq!(engines.unwrap().len(), 3, "php-first, built-in, both");
    let install = rows.iter().find_map(|r| match r {
        SettingsRow::Actions { buttons, .. } => Some(buttons.clone()),
        _ => None,
    });
    assert_eq!(install.unwrap()[0].0, "install");
    assert!(!rows.iter().any(|r| matches!(r, SettingsRow::Secret { .. })));
    // Install asks first (its licence, what's downloaded, where), and isn't "dangerous".
    let (question, _then) = page.press(scope(&settings, "php"), "install").unwrap();
    assert!(!question.danger);
    assert!(question.message.contains("registry.npmjs.org"), "{}", question.message);
    assert!(harness_registry_untouched());
    // Installed (as "yes" does, on a background thread): offered, its licence field shown.
    crate::install::install(&spec, None).unwrap();
    let rows = php_rows(&page, &settings);
    let engines = rows.iter().find_map(|r| match r {
        SettingsRow::Choices { key: "engine", choices, .. } => Some(choices.clone()),
        _ => None,
    });
    assert_eq!(engines.unwrap().len(), 4, "php-first, installable, built-in, both");
    assert!(
        rows.iter()
            .any(|r| matches!(r, SettingsRow::Status { button: Some(("uninstall", _)), .. }))
    );
    crate::harness::wait("the keychain read", || {
        php_rows(&page, &settings)
            .iter()
            .any(|r| matches!(r, SettingsRow::Secret { key: "licence", set: false, .. }))
    });
    // A licence file found: offered, and used on a click (only read).
    let file = crate::harness::temp().join("page-licence.txt");
    std::fs::write(&file, "PAGE-KEY\n").unwrap();
    crate::licence::use_licence_files(vec![file]);
    // `set: false` above is also shown while it's read: the offer comes once it's known.
    crate::harness::wait("the licence file offered", || {
        php_rows(&page, &settings).iter().any(|r| {
            matches!(r, SettingsRow::Actions { buttons, .. } if buttons[0].0 == "import-licence")
        })
    });
    assert!(page.press(scope(&settings, "php"), "import-licence").is_none());
    crate::harness::wait("the key kept", || {
        crate::harness::secrets().0.lock().unwrap().get("installable-licence").is_some()
    });
    assert!(page.poll(), "drawn again");
    let rows = php_rows(&page, &settings);
    assert!(
        rows.iter().any(|r| matches!(r, SettingsRow::Secret { key: "licence", set: true, .. }))
    );
    // Written in the field: kept in the secrets, never in the settings.
    page.set_text(scope(&settings, "php"), "licence", "TYPED-KEY");
    crate::harness::wait("the typed key kept", || {
        crate::harness::secrets().0.lock().unwrap().get("installable-licence").map(String::as_str)
            == Some("TYPED-KEY")
    });
    assert!(settings.get(&crate::settings::ENGINES).is_empty());
    page.choose(scope(&settings, "php"), "engine", 2);
    // Installed, it's the default (php-first is the built-in engine's here): nothing saved.
    assert!(settings.get(&crate::settings::ENGINES).is_empty());
    page.choose(scope(&settings, "php"), "engine", 1);
    assert_eq!(settings.get(&crate::settings::ENGINES), ["php=server"]);
    page.choose(scope(&settings, "php"), "engine", 2);
    assert!(settings.get(&crate::settings::ENGINES).is_empty());
    let (question, _) = page.press(scope(&settings, "php"), "uninstall").unwrap();
    assert!(question.danger);
    crate::install::uninstall(&spec).unwrap();
    crate::licence::use_licence_files(vec![crate::harness::temp().join("no-licence.txt")]);
}

fn harness_registry_untouched() -> bool {
    crate::harness::registry().asked.lock().unwrap().is_empty()
}
