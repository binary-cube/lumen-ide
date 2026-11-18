//! The settings' blocks as the window gets them from every module: their pages, every
//! setting still reachable, the Languages page's one language, a module off taking its page.

use std::sync::Arc;

use super::*;
use crate::workbench::Ports;

/// A workbench with every module (or all but `disabled`), its settings' model made.
fn workbench(disabled: &[&str]) -> Workbench {
    let selection = extensions::ModuleSelection {
        disabled: disabled.iter().map(|d| (*d).to_owned()).collect(),
        ..Default::default()
    };
    let (support, _) = crate::modules::load(&selection);
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.set_settings_model(ui_workbench::SettingsModel::default());
    wb
}

/// A block's row keys.
fn keys(wb: &Workbench, block: &str) -> Vec<&'static str> {
    let found = wb.settings_model.blocks.iter().find(|b| b.id == block);
    let found = found.unwrap_or_else(|| panic!("no block {block}"));
    found.block.rows.iter().filter_map(|r| r.searchable().map(|(k, ..)| k)).collect()
}

/// Every setting the window had before it was reorganized is still on a page: each block,
/// each of its rows, and each block's page is one the navigation lists (an app page or one
/// a block describes).
#[test]
fn every_setting_is_still_reachable() {
    let mut wb = workbench(&[]);
    let expected: &[(&str, &str, &[&str])] = &[
        ("decorations", "appearance", &["mode", "mine", "fps", "idle", "follow_system"]),
        ("notifications", "notifications", &["position"]),
        ("caret", "editor", &["own"]),
        ("code_insight", "editor", &["inlay_hints", "code_lens", "action_bulb", "document_links"]),
        ("autosave", "files", &["autosave"]),
        ("final_newline", "files", &["final_newline"]),
        ("history", "files", &["days", "versions"]),
        ("language-servers", "languages", &["engine"]),
        ("language-server-status", "languages", &[]),
        ("quality", "languages", &[]),
        ("language-servers-parts", "language-servers", &[]),
        ("language-servers-limits", "language-servers", &["memory", "idle", "trace"]),
        ("language-servers-add", "language-servers", &["name", "command", "languages", "args"]),
        ("terminal-shell", "terminal", &["shell", "themed_prompt"]),
        (
            "terminal-display",
            "terminal",
            &[
                "colors",
                "cursor",
                "cursor_blink",
                "font_size",
                "line_height",
                "bold_bright",
                "scrollback",
            ],
        ),
        ("database", "database", &["page_size", "idle_close", "csv_sample", "confirm_delete"]),
        (
            "database-console",
            "database",
            &[
                "console_fetch_limit",
                "console_stop_on_error",
                "console_result_memory",
                "console_param_styles",
                "console_history_size",
                "console_eager_tables",
                "console_index_memory",
                "console_grammar_hints",
            ],
        ),
        ("extractors", "database", &["skip_computed", "skip_generated"]),
        (
            "database-console-profile",
            "database",
            &[
                "console_profile_buffers",
                "console_profile_timing",
                "console_profile_verbose",
                "console_profile_statement_stats",
                "console_profile_timeout",
                "console_profile_keep",
                "console_profile_memory",
            ],
        ),
        ("sql_format", "languages", &["keyword_case", "indent", "width", "commas", "lists"]),
        ("tasks", "tasks", &["history", "interval"]),
        ("remote", "remote", &["uploads", "downloads", "retries"]),
        ("semantic", "search", &["on"]),
    ];
    // PHP: the engine's rows are the language's (the first one listed).
    wb.select_settings_language("php");
    let blocks = &wb.settings_model.blocks;
    for (id, page, rows) in expected {
        let block = blocks.iter().find(|b| b.id == *id);
        let block = block.unwrap_or_else(|| panic!("the block {id} is gone"));
        assert_eq!(block.page, *page, "{id}'s page");
        let described = ["appearance", "icons", "keymap", "modules", "language"].contains(page)
            || blocks.iter().any(|b| b.page_info.is_some_and(|p| p.id == *page));
        assert!(described, "{page} is in the navigation");
        let keys = keys(&wb, id);
        for row in *rows {
            assert!(keys.contains(row), "{id} › {row}: {keys:?}");
        }
    }
    // The advanced ones: under their page's "Advanced".
    let advanced: Vec<&str> = blocks.iter().filter(|b| b.advanced).map(|b| b.id).collect();
    assert!(
        advanced.contains(&"language-servers-limits") && advanced.contains(&"language-servers-add")
    );
    assert!(advanced.contains(&"database-console-profile"));
}

/// One language for the whole Languages page: chosen once, every block shows it — the
/// engine's for a language with servers, SQL's formatting only with SQL.
#[test]
fn the_languages_page_shares_one_language() {
    let mut wb = workbench(&[]);
    let ids: Vec<&str> =
        wb.settings_model.code_languages.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids.first(), Some(&"php"), "the usual ones first: {ids:?}");
    assert!(ids.contains(&"sql") && ids.contains(&"typescript"), "{ids:?}");
    assert_eq!(wb.settings_model.code_language.as_deref(), Some("php"), "the first, at first");
    let sql = wb.settings_model.blocks.iter().find(|b| b.id == "sql_format").unwrap();
    assert_eq!(sql.language.as_deref(), Some("sql"), "SQL's own block");
    assert!(wb.settings_model.blocks.iter().any(|b| b.id == "language-servers"));
    wb.select_settings_language("sql");
    assert_eq!(wb.settings_model.code_language.as_deref(), Some("sql"));
    // No server knows SQL: no engine, no server to show; its formatting is there.
    assert!(!wb.settings_model.blocks.iter().any(|b| b.id == "language-servers"));
    assert!(!wb.settings_model.blocks.iter().any(|b| b.id == "language-server-status"));
    assert!(wb.settings_model.blocks.iter().any(|b| b.id == "sql_format"));
    // A language that isn't listed: the first again.
    wb.select_settings_language("no-such-language");
    assert_eq!(wb.settings_model.code_language.as_deref(), Some("php"));
}

/// A module turned off takes its blocks along: the Databases page is gone.
#[test]
fn a_module_off_takes_its_page() {
    let wb = workbench(&[]);
    assert!(wb.settings_model.blocks.iter().any(|b| b.page == "database"));
    let wb = workbench(&["database.explorer", "database.console", "database.extractors"]);
    assert!(!wb.settings_model.blocks.iter().any(|b| b.page == "database"));
}

/// A route added (its file written) is suggested at once: only Symfony's own generation
/// moves (a file of its own), not for a file it doesn't watch — nothing else is redone.
#[test]
fn a_route_added_is_known_without_reindexing() {
    let root = std::env::temp_dir().join(format!("symfony-routes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src/Controller")).unwrap();
    // As the project's events name it (the watcher's paths: `/private/var/…` on macOS).
    let root = crate::workbench::canonical(&root);
    std::fs::write(
        root.join("composer.json"),
        r#"{"require": {"symfony/framework-bundle": "^7"}}"#,
    )
    .unwrap();
    let route = |name: &str| {
        format!(
            "<?php\nclass C\n{{\n    #[Route('/{name}', name: '{name}')]\n    public function f() {{}}\n}}\n"
        )
    };
    std::fs::write(root.join("src/Controller/A.php"), route("first")).unwrap();
    let mut wb = workbench(&[]);
    wb.open_project(&root).unwrap();
    assert!(wb.data.frameworks.is_active("symfony"));
    wb.open_file(&root.join("src/Controller/A.php")).unwrap();
    let source = "<?php $this->redirectToRoute('');";
    let offset = source.find("''").unwrap() + 1;
    let suggested = |wb: &Workbench| -> Vec<String> {
        let found = wb.framework_completion(source, offset);
        found.map(|c| c.items.into_iter().map(|i| i.label).collect()).unwrap_or_default()
    };
    assert_eq!(suggested(&wb), ["first"]);
    let own = |wb: &Workbench| wb.data.frameworks.active()[0].own();
    let before = own(&wb);
    let written = |path: &str| workspace::events::FilesWritten { paths: vec![root.join(path)] };
    std::fs::create_dir_all(root.join("public")).unwrap();
    wb.data.events.publish(&written("public/app.css"));
    assert_eq!(own(&wb), before, "not its file");
    std::fs::write(root.join("src/Controller/B.php"), route("second")).unwrap();
    wb.data.events.publish(&workspace::events::EntriesChanged {
        paths: vec![root.join("src/Controller/B.php")],
    });
    assert_eq!(own(&wb), before + 1);
    // Made in the background: the UI thread answers from the model made last (never
    // making it itself), the new route there once it's made.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let active = || wb.data.frameworks.active()[0].clone();
    while active().answering() != active().own() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(suggested(&wb), ["first", "second"]);
    let _ = std::fs::remove_dir_all(&root);
}

/// A choice in one of a module's blocks reaches its module even when another of its blocks
/// is the one saying how (the terminal's "Behavior" block, handed over by its "Shell"
/// block's `apply`): every block of the page is applied after a choice.
#[test]
fn a_choice_reaches_its_module_whichever_block_it_s_in() {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    // The terminal's configuration is the process's (each test thread has its own
    // settings): no other test's window applies its defaults over it meanwhile.
    let _alone = crate::modules::hold_process();
    let copying = || terminal_panel::config::current().copy_on_select;
    assert!(!copying(), "off by default");
    wb.settings_choose("terminal-behavior", "copy_on_select", 1);
    assert!(copying(), "on as soon as it's chosen");
    wb.settings_choose("terminal-behavior", "copy_on_select", 0);
    assert!(!copying());
}
