use super::*;

/// Every preset builds a keymap on every platform, and gives the app's commands exactly
/// the shortcuts they had before the layers (`keymaps.golden`: preset, command, shortcuts).
#[test]
fn every_preset_builds_a_keymap_and_keeps_its_shortcuts() {
    let golden = include_str!("keymaps.golden");
    for preset in PRESETS {
        assert!(select(preset.id));
        for mac in [true, false] {
            assert!(keymap(mac).is_ok(), "{}", preset.id);
        }
        for spec in SPECS {
            let shortcuts = shortcuts_on(spec.id, true).join(" | ");
            let line = format!("{} {} = {}", preset.id, spec.id.0, shortcuts);
            assert!(golden.lines().any(|l| l == line), "changed: {line}");
        }
    }
    assert_eq!(golden.lines().count(), PRESETS.len() * SPECS.len(), "a command is missing");
    assert!(select("vscode"));
    assert_eq!(shortcuts(c::GOTO_DEFINITION), ["F12"]);
    // Not overridden: the default ones.
    assert_eq!(shortcuts(c::SAVE), ["CmdOrCtrl+S"]);
    assert!(select("default"));
    // The shortcuts shown are the system's (`⌘X` / `Ctrl+X`): the golden is macOS's.
    if cfg!(target_os = "macos") {
        the_context_menus_keep_their_rows();
    }
    the_menu_bar_keeps_its_tree();
    assert!(select("lumen"), "the id saved before the rename still works");
    // In the same test: the active preset is shared, tests run in parallel.
    the_presets_have_only_the_known_conflicts();
}

/// The bindings carry where their shortcuts come from; no preset gives one chord to two
/// commands of the menu bar on the same system (every module loaded, Windows and Linux as
/// their menus are), but for the pairs that never meet (`EXCLUSIVE`): plan 55a.
fn the_presets_have_only_the_known_conflicts() {
    use command_model::bindings::Origin;
    fn commands(items: &[platform_api::MenuItem], out: &mut Vec<String>) {
        for item in items {
            match item {
                platform_api::MenuItem::Command { id, .. } => out.push(id.clone()),
                platform_api::MenuItem::Submenu { items, .. } => commands(items, out),
                _ => {}
            }
        }
    }
    let _ = crate::modules::load(&extensions::ModuleSelection::default());
    let mut found = Vec::new();
    for preset in PRESETS {
        assert!(select(preset.id));
        for system in crate::commands::System::ALL {
            let mut in_menus = Vec::new();
            for menu in crate::commands::menus_on(system) {
                commands(&menu.items, &mut in_menus);
            }
            let all = bindings_on(system.mac());
            for conflict in command_model::bindings::conflicts(&all, system.mac()) {
                let ids: Vec<&str> = conflict
                    .commands
                    .iter()
                    .map(|c| c.0)
                    .filter(|id| in_menus.iter().any(|m| m == id))
                    .collect();
                if ids.len() < 2 {
                    continue;
                }
                let line = format!("{} {}", conflict.shortcut, ids.join(" "));
                if !EXCLUSIVE.contains(&line.as_str()) {
                    found.push(format!("{} {system:?}: {line}", preset.id));
                }
            }
        }
        if preset.id == "vscode" {
            let all = with_layers(true, |l| command_model::bindings::resolve(SPECS, l, when));
            let line = all.iter().find(|b| b.command == c::GOTO_LINE).unwrap();
            assert_eq!(line.origin, Origin::Preset);
        }
    }
    assert!(select("default"));
    assert!(found.is_empty(), "one chord, two menu commands: {found:#?}");
    // The originals' keys (plan 55a).
    let on = |preset: &str, id, mac| {
        assert!(select(preset));
        let found = shortcuts_on(id, mac);
        assert!(select("default"));
        found
    };
    assert_eq!(on("jetbrains", c::SEARCH_FILES, true), ["CmdOrCtrl+Shift+O"]);
    assert_eq!(on("jetbrains", c::SEARCH_FILES, false), ["CmdOrCtrl+Shift+N"]);
    assert_eq!(on("jetbrains", c::NEW_WINDOW, false), Vec::<String>::new());
    assert_eq!(on("jetbrains", c::GOTO_LINE, false), ["Ctrl+G"]);
    assert_eq!(on("vscode", c::FIND_NEXT, true), ["CmdOrCtrl+G", "F3"]);
    assert_eq!(on("vscode", c::FIND_NEXT, false), ["F3"]);
    assert_eq!(on("vscode", c::NAVIGATE_FORWARD, false), ["Alt+Right"]);
    assert_eq!(on("default", c::FIND_NEXT, true), ["CmdOrCtrl+G", "F3"]);
    assert_eq!(on("default", c::GOTO_LINE, true), ["CmdOrCtrl+L"]);
}

/// Menu commands sharing a chord by design: they work in places that never meet (a view's
/// own keys, the editor's), as `"<shortcut> <first> <second>"`.
const EXCLUSIVE: &[&str] = &[];

/// The views' keys, as they were before they became commands: in each view, the same key
/// (as a key press makes it: `+` is Shift and `=`) gives the same action — and only there.
#[test]
fn the_views_keys_give_the_same_actions() {
    use command_model::{ChordKey, KeyChord};
    let _ = crate::modules::load(&extensions::ModuleSelection::default());
    let keymap = keymap(cfg!(target_os = "macos")).unwrap();
    let press = |c: char| {
        let (key, shift) = match c {
            '+' => ('=', true),
            '_' => ('-', true),
            'R' => ('r', true),
            c => (c, false),
        };
        KeyChord { primary: false, shift, alt: false, control: false, key: ChordKey::Char(key) }
    };
    let views: &[(&str, &[(char, &str)])] = &[
        (
            "canvas",
            &[
                ('+', "canvas.zoom_in"),
                ('=', "canvas.zoom_in"),
                ('-', "canvas.zoom_out"),
                ('0', "canvas.fit"),
                ('1', "canvas.functions"),
                ('2', "canvas.files"),
                ('3', "canvas.code"),
            ],
        ),
        (
            "compare",
            &[
                ('n', "compare.next"),
                ('j', "compare.next"),
                ('p', "compare.previous"),
                ('k', "compare.previous"),
                ('[', "compare.take_left"),
                (']', "compare.take_right"),
                ('m', "compare.merge"),
            ],
        ),
        ("diagram", &[('0', "diagram.fit")]),
        ("history.local", &[('r', "history.reload")]),
        ("git.log", &[('r', "git.reload_log")]),
        (
            "image",
            &[
                ('+', "image.zoom_in"),
                ('=', "image.zoom_in"),
                ('-', "image.zoom_out"),
                ('_', "image.zoom_out"),
                ('0', "image.fit"),
                ('1', "image.actual"),
                ('r', "image.rotate_right"),
                ('R', "image.rotate_left"),
                ('h', "image.flip_horizontal"),
                ('v', "image.flip_vertical"),
                ('b', "image.background"),
            ],
        ),
    ];
    for (view, keys) in views {
        let place = [command_model::When::View(view)];
        for (key, command) in *keys {
            let found = keymap.lookup_in(&press(*key), &place);
            assert_eq!(found.map(|c| c.0), Some(*command), "{view}: {key}");
            let in_editor = keymap.lookup_in(&press(*key), &[command_model::When::Editor]);
            assert_eq!(in_editor, None, "{key} types in the editor");
        }
    }
    let back = KeyChord::parse("Backspace").unwrap();
    let place = [command_model::When::View("diagram")];
    assert_eq!(keymap.lookup_in(&back, &place).map(|c| c.0), Some("diagram.back"));
    assert_eq!(keymap.lookup_in(&back, &[command_model::When::Editor]), None);
}

/// The context menus' rows (editor, tree file / folder), one per line: what they run, their
/// label, shown shortcut and icon, submenus indented — kept as they were
/// (`context_menus.golden`) whoever contributes them. In the keymaps' test: the shortcuts
/// shown are the active preset's.
fn the_context_menus_keep_their_rows() {
    fn outline(entries: &[ui_workbench::MenuEntry], depth: usize, out: &mut String) {
        for entry in entries {
            let indent = "  ".repeat(depth);
            match entry {
                ui_workbench::MenuEntry::Item { id, label, shortcut, icon, .. } => {
                    let shortcut = shortcut.as_deref().unwrap_or("");
                    let icon = icon.unwrap_or("");
                    out.push_str(&format!("{indent}{id} | {label} | {shortcut} | {icon}\n"));
                }
                ui_workbench::MenuEntry::Submenu { label, icon, entries } => {
                    out.push_str(&format!("{indent}{label} › | {}\n", icon.unwrap_or("")));
                    outline(entries, depth + 1, out);
                }
                ui_workbench::MenuEntry::Separator => out.push_str(&format!("{indent}---\n")),
            }
        }
    }
    let menus = crate::workbench::tests::context_menus();
    let mut found = String::new();
    for (name, entries) in &menus {
        found.push_str(&format!("# {name}\n"));
        outline(entries, 0, &mut found);
    }
    let golden = include_str!("context_menus.golden");
    if found != golden {
        let out = std::env::temp_dir().join("context_menus.found");
        let _ = std::fs::write(&out, &found);
        panic!("the context menus changed (now in {})", out.display());
    }
}

/// The menu bar on each system, one row per line: its menus (title, icon, the window list
/// / Help), their rows (command, label, shortcut as written), submenus indented, the
/// system's own rows, separators — kept as they are (`menus.golden`), with every module. In
/// the keymaps' test: the shortcuts are the active preset's.
fn the_menu_bar_keeps_its_tree() {
    use platform_api::MenuItem;
    fn outline(items: &[MenuItem], depth: usize, out: &mut String) {
        for item in items {
            let indent = "  ".repeat(depth + 1);
            match item {
                MenuItem::Command { id, label, shortcut, .. } => {
                    let shortcut = shortcut.as_deref().unwrap_or("");
                    out.push_str(&format!("{indent}{id} | {label} | {shortcut}\n"));
                }
                MenuItem::Submenu { title, icon, items } => {
                    out.push_str(&format!("{indent}{title} › | {}\n", icon.unwrap_or("")));
                    outline(items, depth + 1, out);
                }
                MenuItem::Native { item, label } => {
                    out.push_str(&format!("{indent}[{item:?}] {label}\n"));
                }
                MenuItem::Separator => out.push_str(&format!("{indent}---\n")),
            }
        }
    }
    let _ = crate::modules::load(&extensions::ModuleSelection::default());
    let mut found = String::new();
    for system in crate::commands::System::ALL {
        found.push_str(&format!("= {system:?} =\n"));
        for menu in crate::commands::menus_on(system) {
            let mut flags = String::new();
            if menu.windows {
                flags.push_str(" | windows");
            }
            if menu.help {
                flags.push_str(" | help");
            }
            found.push_str(&format!("# {} | {}{flags}\n", menu.title, menu.icon.unwrap_or("")));
            outline(&menu.items, 0, &mut found);
        }
    }
    let golden = include_str!("menus.golden");
    if found != golden {
        let out = std::env::temp_dir().join("menus.found");
        let _ = std::fs::write(&out, &found);
        panic!("the menu bar changed (now in {})", out.display());
    }
}

/// The modules' places, specs and shortcuts only grow: modules loaded again without some
/// (another window's selection, a test's) never take away what the others registered — a
/// console's ⌘↵ stays the console's (the race the console's key tests once lost).
#[test]
fn modules_loaded_again_without_some_keep_the_others_places() {
    let _ = crate::modules::load(&extensions::ModuleSelection::default());
    let sql = editor_language("sql");
    assert!(sql.is_some(), "the console's place");
    let run = crate::commands::spec_of(database_console::RUN).expect("its spec");
    let without = extensions::ModuleSelection {
        disabled: vec![database_console::MODULE_ID.into(), "lang.html".into()],
        ..Default::default()
    };
    let _ = crate::modules::load(&without);
    assert_eq!(editor_language("sql"), sql, "kept");
    assert_eq!(crate::commands::spec_of(database_console::RUN), Some(run));
}

#[test]
fn a_merge_replaces_by_key_and_takes_nothing_away() {
    let mut all = vec![(1, "a"), (2, "b")];
    crate::commands::merge_by(&mut all, vec![(2, "B"), (3, "c")], |(k, _)| *k);
    assert_eq!(all, [(1, "a"), (2, "B"), (3, "c")]);
    crate::commands::merge_by(&mut all, Vec::new(), |(k, _)| *k);
    assert_eq!(all.len(), 3);
}
