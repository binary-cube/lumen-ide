//! Commands found by id, their keys (the keymap, a shortcut as shown), and the menus
//! turned into rows (the drawer's, a menu's entries).

use super::*;

/// The command and its argument (the tab index) from an `id@arg` identifier.
pub fn parse_with_arg(id: &str) -> Option<(CommandId, u64)> {
    let (id, arg) = id.split_once('@')?;
    Some((by_id(id)?, arg.parse().ok()?))
}

/// Finds the command by the identifier received from the native menu.
pub fn by_id(id: &str) -> Option<CommandId> {
    SPECS
        .iter()
        .find(|s| s.id.0 == id)
        .map(|s| s.id)
        .or_else(|| contributed().into_iter().map(|(s, _)| s.id).find(|c| c.0 == id))
}

/// Every command with its keys, for Settings → Shortcuts: its label, its menu (or where it
/// works), its shortcuts (as written and as shown), whether the user changed them; in menu
/// order, then the views' actions.
pub fn keymap_bindings() -> std::sync::Arc<Vec<ui_workbench::BindingRow>> {
    use command_model::When;
    let rows = crate::keymaps::bindings().into_iter().map(|b| {
        let (place, place_name) = match b.when {
            When::Everywhere => (String::new(), None),
            When::Editor => ("editor".to_owned(), Some(i18n::t("shortcuts.place_editor"))),
            When::EditorLanguage(language) => (
                format!("editor-{language}"),
                Some(i18n::tr!(
                    "shortcuts.place_editor_language",
                    language = language.to_uppercase()
                )),
            ),
            When::View(view) => {
                let key = format!("shortcuts.view_{}", view.replace('.', "_"));
                let name = i18n::t(&key);
                (view.to_owned(), Some(if name == key { view.to_owned() } else { name }))
            }
        };
        ui_workbench::BindingRow {
            command: b.command.0.to_owned(),
            label: label(b.command).trim_end_matches('…').to_owned(),
            group: place_name.or_else(|| menu_place(b.command)).unwrap_or_default(),
            place,
            // Text is typed everywhere and in an editor; a view's own keys are its.
            typing: !matches!(b.when, When::View(_)),
            shown: b.shortcuts.iter().map(|s| display_shortcut(s)).collect(),
            shortcuts: b.shortcuts,
            changed: b.origin == command_model::bindings::Origin::User,
        }
    });
    // In the menus' order (a module's command where its menu puts it: Theme Studio before
    // Quit), then the rest as they were.
    fn commands(items: Vec<MenuItem>, out: &mut Vec<String>) {
        for item in items {
            match item {
                MenuItem::Command { id, .. } => out.push(id),
                MenuItem::Submenu { items, .. } => commands(items, out),
                MenuItem::Separator | MenuItem::Native { .. } => {}
            }
        }
    }
    let mut order = Vec::new();
    for menu in menus() {
        commands(menu.items, &mut order);
    }
    let mut rows: Vec<ui_workbench::BindingRow> = rows.collect();
    rows.sort_by_key(|r| order.iter().position(|id| *id == r.command).unwrap_or(usize::MAX));
    std::sync::Arc::new(rows)
}

/// The menus as the menu drawer shows them (the native menu bar's, in its order): each
/// with its icon, its rows with theirs and their shortcuts as the system shows them.
/// `hidden`: the rows left out (what makes sense only elsewhere — another language's file);
/// the separators and submenus left empty go with them.
pub fn drawer_menus(hidden: &dyn Fn(&str) -> bool) -> Vec<ui_workbench::DrawerMenu> {
    menus()
        .into_iter()
        .map(|menu| ui_workbench::DrawerMenu {
            title: menu.title,
            icon: menu.icon,
            entries: ui_workbench::tidy_menu(without(
                one_level_of_submenus(menu_entries(menu.items)),
                hidden,
            )),
        })
        .filter(|menu| !menu.entries.is_empty())
        .collect()
}

/// `entries` without the rows `hidden` says (in submenus too).
fn without(
    entries: Vec<ui_workbench::MenuEntry>,
    hidden: &dyn Fn(&str) -> bool,
) -> Vec<ui_workbench::MenuEntry> {
    use ui_workbench::MenuEntry;
    entries
        .into_iter()
        .filter_map(|entry| match entry {
            MenuEntry::Item { ref id, .. } if hidden(id) => None,
            MenuEntry::Submenu { label, icon, entries } => {
                Some(MenuEntry::Submenu { label, icon, entries: without(entries, hidden) })
            }
            entry => Some(entry),
        })
        .collect()
}

/// The menus' submenus as the drawer shows them (its third panel): their own submenus'
/// rows in their place.
fn one_level_of_submenus(entries: Vec<ui_workbench::MenuEntry>) -> Vec<ui_workbench::MenuEntry> {
    use ui_workbench::MenuEntry;
    entries
        .into_iter()
        .map(|entry| match entry {
            MenuEntry::Submenu { label, icon, entries } => {
                MenuEntry::Submenu { label, icon, entries: inline_submenus(entries) }
            }
            entry => entry,
        })
        .collect()
}

/// The submenus' rows in their place, between separators.
fn inline_submenus(entries: Vec<ui_workbench::MenuEntry>) -> Vec<ui_workbench::MenuEntry> {
    use ui_workbench::MenuEntry;
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        match entry {
            MenuEntry::Submenu { entries, .. } => {
                out.push(MenuEntry::Separator);
                out.extend(inline_submenus(entries));
                out.push(MenuEntry::Separator);
            }
            entry => out.push(entry),
        }
    }
    out
}

/// Menu items as our context menu's rows (icons and shortcuts as the system shows them);
/// the system's own rows are left out.
pub fn menu_entries(items: Vec<MenuItem>) -> Vec<ui_workbench::MenuEntry> {
    items
        .into_iter()
        .filter_map(|item| match item {
            MenuItem::Command { id, label, shortcut, .. } => {
                let icon = menu_icon(&id);
                let shortcut = shortcut.as_deref().map(display_shortcut);
                Some(ui_workbench::MenuEntry::Item {
                    id,
                    label,
                    shortcut,
                    icon,
                    on: None,
                    look: Default::default(),
                })
            }
            MenuItem::Separator => Some(ui_workbench::MenuEntry::Separator),
            MenuItem::Submenu { title, icon, items } => Some(ui_workbench::MenuEntry::Submenu {
                label: title,
                icon,
                entries: menu_entries(items),
            }),
            MenuItem::Native { .. } => None,
        })
        .collect()
}

/// The command's first shortcut, as the system shows it: `⌘⇧F` on macOS, `Ctrl+Shift+F`
/// elsewhere (for tooltips).
pub fn shortcut_label(id: CommandId) -> Option<String> {
    crate::keymaps::shortcuts(id).first().map(|s| display_shortcut(s))
}

/// A shortcut as written in the specs (`CmdOrCtrl+Shift+C`), as the system shows it.
pub fn display_shortcut(shortcut: &str) -> String {
    let mac = cfg!(target_os = "macos");
    let parts: Vec<String> = shortcut
        .split('+')
        .map(|part| match (part, mac) {
            ("CmdOrCtrl", true) | ("Cmd", true) => "⌘".into(),
            ("CmdOrCtrl", false) | ("Ctrl", false) => "Ctrl".into(),
            ("Ctrl", true) => "⌃".into(),
            ("Shift", true) => "⇧".into(),
            ("Alt", true) => "⌥".into(),
            ("Left", _) => "←".into(),
            ("Right", _) => "→".into(),
            ("Up", _) => "↑".into(),
            ("Down", _) => "↓".into(),
            (key, _) if key.eq_ignore_ascii_case("NumpadAdd") => "NumPad+".into(),
            (key, _) if key.eq_ignore_ascii_case("NumpadSubtract") => "NumPad-".into(),
            (other, _) => other.to_owned(),
        })
        .collect();
    if mac { parts.concat() } else { parts.join("+") }
}
