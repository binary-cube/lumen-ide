use super::*;
use command_model::Keymap;

#[test]
fn every_shortcut_parses_and_ids_are_unique() {
    Keymap::from_specs(SPECS, true).unwrap();
    Keymap::from_specs(SPECS, false).unwrap();
    let mut ids: Vec<_> = SPECS.iter().map(|s| s.id.0).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), SPECS.len());
}

/// The commands' ids in `entries`, submenus included.
fn ids(entries: &[ui_workbench::MenuEntry]) -> Vec<String> {
    use ui_workbench::MenuEntry;
    entries
        .iter()
        .flat_map(|e| match e {
            MenuEntry::Item { id, .. } => vec![id.clone()],
            MenuEntry::Submenu { entries, .. } => ids(entries),
            MenuEntry::Separator => Vec::new(),
        })
        .collect()
}

fn compare_items() -> Vec<MenuItem> {
    vec![MenuItem::command("compare.tabs:2:3", "b.php", None)]
}

#[test]
fn every_context_menu_item_has_an_existing_icon() {
    // The modules' commands (the tree's operations, Move…) bring their icons: loaded here,
    // whatever ran before.
    let _ = crate::modules::load(&extensions::ModuleSelection::default());
    let icons = icons::Icons::builtin();
    let state = TabMenuState {
        count: 5,
        has_path: true,
        in_project: true,
        pinned: false,
        can_reopen: true,
        locked: Some(false),
        renamable: false,
    };
    let entries = tab_context_menu((2, 2), state, "Reveal in Finder", compare_items());
    for id in ids(&entries) {
        let icon = menu_icon(&id).unwrap_or_else(|| panic!("no icon: {id}"));
        assert!(icons.id(icon).is_some(), "no such icon: {icon}");
    }
}

#[test]
fn tab_menu_items_carry_the_document_and_are_grouped() {
    use ui_workbench::MenuEntry;
    let state = TabMenuState {
        count: 5,
        has_path: true,
        in_project: true,
        pinned: false,
        can_reopen: false,
        locked: None,
        renamable: false,
    };
    let entries = tab_context_menu((2, 2), state, "Reveal in Finder", compare_items());
    let all = ids(&entries);
    let parsed: Vec<_> = all.iter().filter_map(|id| parse_with_arg(id)).collect();
    assert!(parsed.contains(&(TAB_CLOSE_LEFT, 2)));
    assert!(parsed.contains(&(REVEAL_IN_PROJECT, 2)));
    assert!(parsed.contains(&(TAB_PIN, 2)) && parsed.contains(&(TAB_RENAME, 2)));
    assert_eq!(parsed.len() + 1, all.len(), "all but the comparison carry the document");
    // Close ›, Split › and Compare with ›: submenus.
    let submenus: Vec<&str> = entries
        .iter()
        .filter_map(|e| match e {
            MenuEntry::Submenu { label, .. } => Some(label.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(submenus.len(), 2, "Close and Split; one comparison is inline: {submenus:?}");
    // A single tab, without a file: "Close" and "Pin Tab".
    let state = TabMenuState {
        count: 1,
        has_path: false,
        in_project: false,
        pinned: false,
        can_reopen: false,
        locked: None,
        renamable: false,
    };
    let single = tab_context_menu((0, 0), state, "x", Vec::new());
    assert_eq!(ids(&single).len(), 2);
}

/// Lock (Read-only) / Unlock only on a tab that can be locked (a text's), for that tab.
#[test]
fn the_lock_row_is_only_on_lockable_tabs() {
    let state = |locked| TabMenuState {
        count: 2,
        has_path: true,
        in_project: true,
        pinned: false,
        can_reopen: false,
        locked,
        renamable: false,
    };
    let row = |locked| {
        let entries = tab_context_menu((1, 7), state(locked), "x", Vec::new());
        fn find(entries: &[ui_workbench::MenuEntry]) -> Option<(String, String)> {
            entries.iter().find_map(|e| match e {
                ui_workbench::MenuEntry::Item { id, label, .. }
                    if id.starts_with(TOGGLE_READ_ONLY.0) =>
                {
                    Some((id.clone(), label.clone()))
                }
                ui_workbench::MenuEntry::Submenu { entries, .. } => find(entries),
                _ => None,
            })
        }
        find(&entries)
    };
    assert_eq!(row(None), None, "an image's tab: no lock");
    let (id, lock) = row(Some(false)).expect("a text's tab can be locked");
    assert_eq!(parse_with_arg(&id), Some((TOGGLE_READ_ONLY, 7)), "for its document");
    let (_, unlock) = row(Some(true)).unwrap();
    assert_ne!(lock, unlock, "Lock (Read-only), then Unlock");
}

#[test]
fn tool_tab_menu_round_trips() {
    let tab = |index, count, pinned| ToolTab { index, count, pinned, closable: true };
    let items = tool_tab_menu(tab(2, 3, true), false);
    let parsed: Vec<_> = items
        .iter()
        .filter_map(|i| match i {
            MenuItem::Command { id, .. } => parse_tool_tab(id),
            _ => None,
        })
        .collect();
    // Rename, pin, close, the others, to the left (none to the right), all; moved into a
    // split (no other view made: the window doesn't make them).
    assert_eq!(parsed.len(), 8);
    assert!(parsed.contains(&(ToolTabAction::TogglePin, 2)));
    assert!(parsed.contains(&(ToolTabAction::CloseLeft, 2)));
    assert!(!parsed.contains(&(ToolTabAction::CloseRight, 2)));
    assert!(parsed.contains(&(ToolTabAction::SplitMoveDown, 2)));
    assert!(!parsed.contains(&(ToolTabAction::SplitRight, 2)));
    assert!(items.iter().any(|i| matches!(i, MenuItem::Command { label, .. }
            if *label == i18n::t("command.unpin_tab"))));
    assert_eq!(tool_tab_menu(tab(0, 1, false), false).len(), 4);
    // A fixed tab: no Close of its own (the others' closes stay).
    let fixed = tool_tab_menu(ToolTab { closable: false, ..tab(0, 2, false) }, false);
    let fixed: Vec<_> = fixed
        .iter()
        .filter_map(|i| match i {
            MenuItem::Command { id, .. } => parse_tool_tab(id),
            _ => None,
        })
        .collect();
    assert!(!fixed.contains(&(ToolTabAction::Close, 0)));
    assert!(fixed.contains(&(ToolTabAction::CloseOthers, 0)));
    // A window making views (the terminal): another one split off, even alone.
    let alone = tool_tab_menu(tab(0, 1, false), true);
    let parsed: Vec<_> = alone
        .iter()
        .filter_map(|i| match i {
            MenuItem::Command { id, .. } => parse_tool_tab(id),
            _ => None,
        })
        .collect();
    assert!(parsed.contains(&(ToolTabAction::SplitRight, 0)));
    assert!(parsed.contains(&(ToolTabAction::SplitDown, 0)));
    assert!(!parsed.contains(&(ToolTabAction::SplitMoveRight, 0)));
}

/// Every row of `items`, submenus' included (each with the titles above it).
fn rows(items: &[MenuItem], depth: usize, out: &mut Vec<(usize, MenuItem)>) {
    for item in items {
        out.push((depth, item.clone()));
        if let MenuItem::Submenu { items, .. } = item {
            rows(items, depth + 1, out);
        }
    }
}

/// The commands of `items`, submenus' included.
fn commands_in(items: &[MenuItem]) -> Vec<String> {
    let mut all = Vec::new();
    rows(items, 0, &mut all);
    all.into_iter()
        .filter_map(|(_, i)| match i {
            MenuItem::Command { id, .. } => Some(id),
            _ => None,
        })
        .collect()
}

/// No separator leading, trailing or doubled, at any level; no empty submenu.
fn tidy(items: &[MenuItem]) -> bool {
    let separator = |i: Option<&MenuItem>| i == Some(&MenuItem::Separator);
    !separator(items.first())
        && !separator(items.last())
        && !items.windows(2).any(|w| w[0] == MenuItem::Separator && w[1] == MenuItem::Separator)
        && items.iter().all(|i| match i {
            MenuItem::Submenu { items, .. } => !items.is_empty() && tidy(items),
            _ => true,
        })
}

#[test]
fn the_menus_are_tidy_translated_and_show_each_command_once() {
    let _ = crate::modules::load(&extensions::ModuleSelection::default());
    for system in System::ALL {
        let mac = system.mac();
        let menus = menus_on(system);
        // Every top menu shown is a declared one, in their order; a module's (Git) only with
        // rows.
        let order: Vec<String> =
            MENUS.iter().filter(|m| m.platforms.includes(mac)).map(menu_title).collect();
        let shown: Vec<String> = menus.iter().map(|m| m.title.clone()).collect();
        assert!(shown.iter().all(|t| order.contains(t)), "{shown:?}");
        assert!(shown.windows(2).all(|w| {
            order.iter().position(|t| *t == w[0]) < order.iter().position(|t| *t == w[1])
        }));
        // Translated (no raw keys), named after the app where it's the app's.
        assert!(menus.iter().all(|m| !m.title.starts_with("menu.")));
        let mut all = Vec::new();
        for menu in &menus {
            rows(&menu.items, 0, &mut all);
            assert!(tidy(&menu.items), "{}", menu.title);
            // Short enough to read: a top menu's own rows.
            let visible = menu.items.iter().filter(|i| **i != MenuItem::Separator).count();
            assert!(visible <= 15, "{}: {visible} rows", menu.title);
        }
        assert!(all.iter().all(|(_, i)| match i {
            MenuItem::Command { label, .. } => !label.starts_with("command."),
            MenuItem::Submenu { title, .. } => !title.starts_with("menu"),
            _ => true,
        }));
        let mut ids: Vec<String> = menus.iter().flat_map(|m| commands_in(&m.items)).collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count, "a command shown twice");
        // One window list (macOS adds the open windows to it and to the Dock's menu).
        assert_eq!(menus.iter().filter(|m| m.windows).count(), 1);
        assert!(menus.iter().filter(|m| m.help).count() <= 1);
    }
}

/// Every place named is a section declared on every system; no section twice on one.
#[test]
fn every_command_names_a_known_section() {
    let _ = crate::modules::load(&extensions::ModuleSelection::default());
    for system in System::ALL {
        let tree = MenuTree::new(system);
        let mut ids: Vec<&str> = tree.sections.iter().map(|s| s.id).collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count, "a section declared twice");
        let menus: Vec<&str> =
            MENUS.iter().map(|m| m.id).chain(tree.submenus.iter().map(|s| s.id)).collect();
        for section in &tree.sections {
            assert!(menus.contains(&section.menu), "{}: no menu {}", section.id, section.menu);
        }
        let places = tree
            .commands
            .iter()
            .filter_map(|c| c.menu.map(|at| (c.id.0, at.section)))
            .chain(tree.submenus.iter().map(|s| (s.id, s.at.section)))
            // The system's own rows: macOS' only.
            .chain(NATIVE.iter().filter(|_| system.mac()).map(|n| ("native", n.0.section)));
        for (id, section) in places {
            assert!(ids.contains(&section), "{id}: no section {section}");
        }
    }
}

#[test]
fn the_drawer_shows_the_native_menus_each_with_an_existing_icon() {
    let _ = crate::modules::load(&extensions::ModuleSelection::default());
    let icons = icons::Icons::builtin();
    let native = menus();
    let drawer = drawer_menus(&|_| false, &|_| None);
    // The same menus, in the same order, with the same commands.
    let titles = |m: &[Menu]| m.iter().map(|m| m.title.clone()).collect::<Vec<_>>();
    assert_eq!(drawer.iter().map(|m| m.title.clone()).collect::<Vec<_>>(), titles(&native));
    for (menu, shown) in native.iter().zip(&drawer) {
        let icon = menu.icon.unwrap_or_else(|| panic!("no icon for {}", menu.title));
        assert!(icons.id(icon).is_some(), "no such icon: {icon}");
        assert_eq!(shown.icon, menu.icon);
        assert_eq!(ids(&shown.entries), commands_in(&menu.items), "{}", menu.title);
    }
    // Every top menu has an icon in the set (one shown only with its module on too).
    assert!(MENUS.iter().all(|m| icons.id(m.icon).is_some()));
}

/// A row the drawer leaves out (another language's) is gone from it, with a menu or a
/// submenu left without rows; the others stay where they were.
#[test]
fn the_drawer_leaves_out_what_makes_sense_only_elsewhere() {
    let _ = crate::modules::load(&extensions::ModuleSelection::default());
    let all = drawer_menus(&|_| false, &|_| None);
    let every: Vec<String> = all.iter().flat_map(|m| ids(&m.entries)).collect();
    let (gone, kept) = (every[0].clone(), every[1].clone());
    let shown = drawer_menus(&|id| id == gone, &|_| None);
    let left: Vec<String> = shown.iter().flat_map(|m| ids(&m.entries)).collect();
    assert!(!left.contains(&gone) && left.contains(&kept));
    assert_eq!(left.len(), every.len() - 1);
    let first = &all[0];
    let mine: Vec<String> = ids(&first.entries);
    let none = drawer_menus(&|id| mine.iter().any(|m| m == id), &|_| None);
    assert!(none.iter().all(|m| m.title != first.title), "a menu left empty goes");
}

/// A command's place in the menus, as the palette and the shortcuts' editor show it.
#[test]
fn a_command_knows_its_path_in_the_menus() {
    assert_eq!(menu_place(SAVE), Some(i18n::t("menu.file")));
    assert_eq!(menu_place(MENU), None);
    // About: in the app's menu on macOS, in Help elsewhere.
    let about = if cfg!(target_os = "macos") {
        kernel::brand::NAME.to_owned()
    } else {
        i18n::t("menu.help")
    };
    assert_eq!(menu_path(ABOUT), [about]);
}

/// Every command, the app's and every module's, has its label in the locales: none shows in
/// English in another language.
#[test]
fn every_command_has_its_label_in_the_locales() {
    let _ = crate::modules::load(&extensions::ModuleSelection::default());
    let words = |name: &str| {
        let path =
            format!("{}/../domain/core/i18n/locales/{name}.toml", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).unwrap();
        let start = text.find("\n[command]\n").unwrap();
        let section = &text[start + 1..];
        let end = section[1..].find("\n[").map_or(section.len(), |e| e + 1);
        section[..end]
            .lines()
            .filter_map(|l| l.split_once('=').map(|(k, _)| k.trim().to_owned()))
            .collect::<std::collections::HashSet<_>>()
    };
    assert!(contributed().iter().any(|(s, _)| s.id.0 == "quality.format_file"), "modules' too");
    let ids: Vec<&str> =
        SPECS.iter().map(|s| s.id.0).chain(contributed().iter().map(|(s, _)| s.id.0)).collect();
    for language in ["ro", "en"] {
        let keys = words(language);
        let missing: Vec<_> =
            ids.iter().filter(|id| !keys.contains(&id.replace('.', "_"))).collect();
        assert!(missing.is_empty(), "{language}: no label for {missing:?}");
    }
}

#[test]
fn arrows_show_as_arrows_on_every_system() {
    let shown = display_shortcut("CmdOrCtrl+Shift+Up");
    if cfg!(target_os = "macos") {
        assert_eq!(shown, "⌘⇧↑");
    } else {
        assert_eq!(shown, "Ctrl+Shift+↑");
    }
    assert!(display_shortcut("Alt+Down").ends_with('↓'));
    assert!(display_shortcut("Ctrl+Alt+Left").ends_with('←'));
    assert!(display_shortcut("Ctrl+Alt+Right").ends_with('→'));
}

/// Every row of the menu bar (every module loaded, on each system) has an icon — the menu
/// drawer draws them (plan 55b).
#[test]
fn every_menu_row_has_an_icon() {
    fn walk(items: &[platform_api::MenuItem], out: &mut Vec<String>) {
        for item in items {
            match item {
                platform_api::MenuItem::Command { id, .. } if menu_icon(id).is_none() => {
                    out.push(id.clone());
                }
                platform_api::MenuItem::Submenu { items, .. } => walk(items, out),
                _ => {}
            }
        }
    }
    let _ = crate::modules::load(&extensions::ModuleSelection::default());
    let mut missing = Vec::new();
    for system in System::ALL {
        for menu in menus_on(system) {
            walk(&menu.items, &mut missing);
        }
    }
    missing.dedup();
    assert!(missing.is_empty(), "rows without an icon: {missing:?}");
}

/// A renamable view's tab (a session's terminal): its own rename, none of a file's rows.
#[test]
fn a_renamable_view_s_tab_offers_its_rename_not_a_file_s() {
    let state = TabMenuState {
        count: 2,
        has_path: true,
        in_project: true,
        pinned: false,
        can_reopen: false,
        locked: None,
        renamable: true,
    };
    let entries = tab_context_menu((0, 9), state, "Finder", Vec::new());
    let ids: Vec<String> = flatten(&entries);
    assert!(ids.iter().any(|id| id == &format!("{}@9", TAB_RENAME.0)), "{ids:?}");
    assert!(!ids.iter().any(|id| id.starts_with(COPY_PATH.0)), "no file's rows: {ids:?}");
    assert!(!ids.iter().any(|id| id.starts_with(REVEAL_IN_FILE_MANAGER.0)), "{ids:?}");
}

fn flatten(entries: &[ui_workbench::MenuEntry]) -> Vec<String> {
    entries
        .iter()
        .flat_map(|e| match e {
            ui_workbench::MenuEntry::Item { id, .. } => vec![id.clone()],
            ui_workbench::MenuEntry::Submenu { entries, .. } => flatten(entries),
            ui_workbench::MenuEntry::Separator => Vec::new(),
        })
        .collect()
}

/// A row that turns something on or off shows how it stands in the drawer (the window kept
/// on top, in the Window menu); the others stay plain actions.
#[test]
fn the_drawer_shows_how_a_switch_stands() {
    fn found(entries: &[ui_workbench::MenuEntry], wanted: &str) -> Option<Option<bool>> {
        entries.iter().find_map(|e| match e {
            ui_workbench::MenuEntry::Item { id, on, .. } if id == wanted => Some(*on),
            ui_workbench::MenuEntry::Submenu { entries, .. } => found(entries, wanted),
            _ => None,
        })
    }
    let on_top = KEEP_ON_TOP.0;
    let drawer = drawer_menus(&|_| false, &|id| (id == on_top).then_some(true));
    let rows: Vec<Option<bool>> = drawer.iter().filter_map(|m| found(&m.entries, on_top)).collect();
    assert_eq!(rows, [Some(true)], "once, switched on");
    let save: Vec<Option<bool>> = drawer.iter().filter_map(|m| found(&m.entries, SAVE.0)).collect();
    assert_eq!(save, [None], "a plain action");
}
