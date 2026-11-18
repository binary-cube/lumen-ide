//! The editor's right-click menu by context: a row only where its command is enabled and its
//! condition holds (the file's language, what its intelligence offers, another command
//! enabled). A query console's own rows first (Go to Declaration among them: its names
//! resolve through its schema), Comment and the two Reformats in SQL, nothing of PHP's refactorings there nor in
//! plain text; PHP's menu as it was (`context_menus.golden`).

use super::*;
use crate::workbench::tests::console;
use ui_workbench::MenuEntry;

/// The menu's rows, one per entry: a command's id, `---`, a submenu's label and `›`.
fn rows(entries: &[MenuEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|e| match e {
            MenuEntry::Item { id, .. } => id.clone(),
            MenuEntry::Separator => "---".to_owned(),
            MenuEntry::Submenu { label, .. } => format!("{label} ›"),
        })
        .collect()
}

fn editor_menu(wb: &Workbench) -> Vec<String> {
    rows(&wb.context_menu(MenuPlace::Editor))
}

/// What only a language with names resolved (PHP's) has: none of it outside one.
const NAME_ROWS: [&str; 6] = [
    "refactor.rename",
    "refactor.change_signature",
    "refactor.move",
    "navigate.definition",
    "refactor.find_usages",
    "diagram.show",
];

#[test]
fn a_console_s_menu_has_its_own_rows_first_and_nothing_of_php_s() {
    let c = console("menu");
    let history = format!("{} ›", i18n::t("history.menu"));
    assert_eq!(
        editor_menu(&c.wb),
        [
            "database.console.run",
            "database.console.run_script",
            "database.console.explain",
            "---",
            // Its names resolve through its source's schema (Find Usages, Rename don't).
            "navigate.definition",
            "database.console.jump",
            "database.console.open_in_services",
            "---",
            "code.comment",
            "quality.format_file",
            "sql.format_statement",
            "---",
            "edit.cut",
            "edit.copy",
            "edit.paste",
            "compare.clipboard",
            "---",
            history.as_str(),
        ]
    );
    // Execute shows its key (⌘↵ / Ctrl+Enter), as the bar's tooltip does.
    let entries = c.wb.context_menu(MenuPlace::Editor);
    let MenuEntry::Item { shortcut, label, icon, .. } = &entries[0] else { panic!() };
    assert_eq!(
        shortcut.as_deref(),
        crate::commands::shortcut_label(database_console::RUN).as_deref()
    );
    assert!(shortcut.is_some());
    assert_eq!(label, &i18n::t("command.database_console_run"));
    assert_eq!(*icon, Some("play"));
}

/// A project's SQL file attached to nothing (39n): Execute (asking for a source first),
/// Execute Script and Attach Data Source…, then its language's rows; none of a console's
/// others (Explain, Go to Declaration, Jump) before it's attached.
#[test]
fn a_plain_sql_file_offers_a_source_then_comment() {
    let mut c = console("menu-sql");
    c.wb.open_file(&c.dir.join("plain.sql")).unwrap();
    let menu = editor_menu(&c.wb);
    assert_eq!(
        menu[..7],
        [
            "database.console.run",
            "database.console.run_script",
            "database.console.attach",
            "---",
            "code.comment",
            "quality.format_file",
            "sql.format_statement"
        ],
        "{menu:?}"
    );
    let console_rows: Vec<&String> =
        menu.iter().filter(|r| r.starts_with("database.console")).collect();
    assert_eq!(console_rows.len(), 3, "{menu:?}");
    assert!(!menu.iter().any(|r| NAME_ROWS.contains(&r.as_str())), "{menu:?}");
}

#[test]
fn plain_text_has_the_clipboard_and_its_history_only() {
    let mut c = console("menu-text");
    std::fs::write(c.dir.join("notes.txt"), "a\n").unwrap();
    c.wb.open_file(&c.dir.join("notes.txt")).unwrap();
    let history = format!("{} ›", i18n::t("history.menu"));
    assert_eq!(
        editor_menu(&c.wb),
        ["edit.cut", "edit.copy", "edit.paste", "compare.clipboard", "---", history.as_str()]
    );
}

#[test]
fn php_keeps_its_refactorings_and_gets_no_sql_rows() {
    let mut c = console("menu-php");
    std::fs::write(c.dir.join("a.php"), "<?php\nclass A {}\n").unwrap();
    c.wb.open_file(&c.dir.join("a.php")).unwrap();
    let menu = editor_menu(&c.wb);
    for row in NAME_ROWS {
        assert!(menu.iter().any(|r| r == row), "{row} in {menu:?}");
    }
    assert!(!menu.iter().any(|r| r.starts_with("database.console") || r == "code.comment"));
    assert_eq!(menu[0], "refactor.rename");
}

/// A file opened from outside the project has no local history (the project's files have).
#[test]
fn local_history_is_only_for_the_project_s_files() {
    let mut c = console("menu-outside");
    let outside = std::env::temp_dir().join(format!("menu-outside-{}.txt", std::process::id()));
    std::fs::write(&outside, "x\n").unwrap();
    c.wb.open_file(&outside).unwrap();
    let menu = editor_menu(&c.wb);
    let _ = std::fs::remove_file(&outside);
    assert_eq!(menu, ["edit.cut", "edit.copy", "edit.paste", "compare.clipboard"]);
}

/// The conditions themselves: a language by its id, a feature its intelligence declares
/// (none without one), another command enabled.
#[test]
fn the_conditions_follow_the_file_s_language() {
    use workbench_model::{LanguageFeature, MenuWhen};
    let mut c = console("menu-when");
    assert!(c.wb.menu_when(MenuWhen::Language("sql"), Some("sql")));
    assert!(!c.wb.menu_when(MenuWhen::Language("sql"), Some("php")));
    assert!(!c.wb.menu_when(MenuWhen::Language("sql"), None));
    let rename = MenuWhen::Provides(LanguageFeature::Rename);
    assert!(c.wb.menu_when(rename, Some("php")));
    assert!(c.wb.menu_when(rename, Some("css")), "CSS renames its classes");
    assert!(!c.wb.menu_when(MenuWhen::Provides(LanguageFeature::ChangeSignature), Some("css")));
    assert!(!c.wb.menu_when(rename, Some("sql")));
    assert!(!c.wb.menu_when(rename, None), "plain text offers nothing");
    // Explain is enabled in the console, not in a SQL file attached to nothing (where
    // Execute asks for a source).
    assert!(c.wb.menu_when(MenuWhen::With(database_console::EXPLAIN), None));
    c.wb.open_file(&c.dir.join("plain.sql")).unwrap();
    assert!(!c.wb.menu_when(MenuWhen::With(database_console::EXPLAIN), None));
    assert!(c.wb.menu_when(MenuWhen::With(database_console::RUN), None));
    assert_eq!(c.wb.menu_language().as_deref(), Some("sql"));
}
