//! The keymap service, one for every window: a command's shortcuts through the layers
//! (`command_model::bindings`) — its own (`commands::SPECS`, a module's spec), the chosen
//! preset's (`[general] keymap = "jetbrains"`: the app's presets below and the modules'
//! contributed `PresetShortcuts`), the user's (`[shortcuts]` in `settings.toml`). A preset
//! only lists what differs from the commands' own shortcuts; everything else stays the same.
//!
//! Shortcut notation as in `SPECS`: `CmdOrCtrl` is ⌘ on macOS and Ctrl elsewhere.

use std::sync::RwLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use command_model::bindings::{Binding, Layers, PresetShortcuts, UserShortcuts};
use command_model::{CommandId, CommandSpec, When};

use crate::commands::{self as c, SPECS};

/// A set of shortcuts.
#[derive(Debug)]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    /// The commands whose shortcuts differ from the commands' own (an empty list removes them).
    overrides: &'static [(CommandId, &'static [&'static str])],
}

pub const PRESETS: &[Preset] = &[
    Preset { id: "default", name: "{app}", overrides: &[] },
    Preset {
        id: "jetbrains",
        name: "PhpStorm / JetBrains",
        overrides: &[
            // Go to File is ⌘⇧O on macOS (the preset's ⌘⇧N, New Scratch File, isn't ours:
            // New Window keeps it there), Ctrl+Shift+N on Windows / Linux (no New Window key).
            (c::SEARCH_FILES, &["mac:CmdOrCtrl+Shift+O", "pc:CmdOrCtrl+Shift+N"]),
            (c::NEW_WINDOW, &["mac:CmdOrCtrl+Shift+N"]),
            (c::OPEN_PROJECT, &[]),
            // Go to Line ⌘L / Ctrl+G; Find Next ⌘G / F3 (Ctrl+G is Go to Line).
            (c::GOTO_LINE, &["mac:CmdOrCtrl+L", "pc:Ctrl+G"]),
            (c::FIND_NEXT, &["mac:CmdOrCtrl+G", "F3"]),
            (c::FIND_PREV, &["mac:CmdOrCtrl+Shift+G", "Shift+F3"]),
            (c::GOTO_DEFINITION, &["CmdOrCtrl+B", "F4"]),
            (c::DUPLICATE, &["CmdOrCtrl+D"]),
            (c::DELETE_LINE, &["CmdOrCtrl+Backspace", "Ctrl+Y"]),
            (c::REDO, &["CmdOrCtrl+Shift+Z"]),
            (c::REPLACE, &["CmdOrCtrl+R"]),
            (c::TAB_RENAME, &["Shift+F6"]),
        ],
    },
    Preset {
        id: "vscode",
        name: "VS Code",
        overrides: &[
            (c::SEARCH_FILES, &["CmdOrCtrl+P"]),
            (c::GOTO_DEFINITION, &["F12"]),
            (c::GOTO_LINE, &["Ctrl+G"]),
            // ⌃G is Go to Line: Find Next is ⌘G on macOS, F3 elsewhere (Ctrl+G there).
            (c::FIND_NEXT, &["mac:CmdOrCtrl+G", "F3"]),
            (c::FIND_PREV, &["mac:CmdOrCtrl+Shift+G", "Shift+F3"]),
            (c::DUPLICATE, &["Alt+Shift+Down"]),
            (c::DELETE_LINE, &["CmdOrCtrl+Shift+K"]),
            (c::MOVE_LINE_UP, &["Alt+Up"]),
            (c::MOVE_LINE_DOWN, &["Alt+Down"]),
            (c::MOVE_STATEMENT_UP, &[]),
            (c::MOVE_STATEMENT_DOWN, &[]),
            (c::SELECT_NEXT, &["CmdOrCtrl+D"]),
            (c::SELECT_ALL_OCCURRENCES, &["CmdOrCtrl+Shift+L"]),
            (c::UNSELECT_OCCURRENCE, &[]),
            // ⌘H hides the app on macOS: ⌘R there, Ctrl+H elsewhere.
            (c::REPLACE, &["CmdOrCtrl+Alt+F", "mac:CmdOrCtrl+R", "pc:CmdOrCtrl+H"]),
            // On Windows Alt+← / Alt+→ (Ctrl+Shift+− is Zoom Out there).
            (c::NAVIGATE_BACK, &["mac:Ctrl+-", "pc:Alt+Left"]),
            (c::NAVIGATE_FORWARD, &["mac:Ctrl+Shift+-", "pc:Alt+Right"]),
            (c::TOGGLE_PROJECT, &["CmdOrCtrl+B"]),
            (c::TAB_RENAME, &["F2"]),
            (c::FOLD, &["CmdOrCtrl+Alt+["]),
            (c::UNFOLD, &["CmdOrCtrl+Alt+]"]),
            (c::TOGGLE_DEBUG, &[]),
            (c::REVEAL_IN_PROJECT, &[]),
            (c::EXTEND_SELECTION, &["Ctrl+Shift+Alt+Right"]),
            (c::SHRINK_SELECTION, &["Ctrl+Shift+Alt+Left"]),
            (c::GOTO_IMPLEMENTATION, &["CmdOrCtrl+F12"]),
            (c::TYPE_HIERARCHY, &[]),
        ],
    },
    Preset {
        id: "zed",
        name: "Zed",
        overrides: &[
            (c::SEARCH_FILES, &["CmdOrCtrl+P"]),
            (c::GOTO_DEFINITION, &["F12"]),
            (c::GOTO_LINE, &["Ctrl+G"]),
            // ⌃G is Go to Line: Find Next is ⌘G on macOS, F3 elsewhere (Ctrl+G there).
            (c::FIND_NEXT, &["mac:CmdOrCtrl+G", "F3"]),
            (c::FIND_PREV, &["mac:CmdOrCtrl+Shift+G", "Shift+F3"]),
            (c::DUPLICATE, &["CmdOrCtrl+Shift+D"]),
            (c::DELETE_LINE, &["CmdOrCtrl+Shift+K"]),
            (c::MOVE_LINE_UP, &["Alt+Up"]),
            (c::MOVE_LINE_DOWN, &["Alt+Down"]),
            (c::MOVE_STATEMENT_UP, &[]),
            (c::MOVE_STATEMENT_DOWN, &[]),
            (c::SELECT_NEXT, &["CmdOrCtrl+D"]),
            (c::SELECT_ALL_OCCURRENCES, &["CmdOrCtrl+Shift+L"]),
            (c::UNSELECT_OCCURRENCE, &[]),
            // ⌘H hides the app on macOS: ⌘R there, Ctrl+H elsewhere.
            (c::REPLACE, &["CmdOrCtrl+Alt+F", "mac:CmdOrCtrl+R", "pc:CmdOrCtrl+H"]),
            // On Windows Alt+← / Alt+→ (Ctrl+Shift+− is Zoom Out there).
            (c::NAVIGATE_BACK, &["mac:Ctrl+-", "pc:Alt+Left"]),
            (c::NAVIGATE_FORWARD, &["mac:Ctrl+Shift+-", "pc:Alt+Right"]),
            (c::TOGGLE_PROJECT, &["CmdOrCtrl+Shift+E"]),
            (c::TAB_RENAME, &["F2"]),
            (c::FOLD, &["CmdOrCtrl+Alt+["]),
            (c::UNFOLD, &["CmdOrCtrl+Alt+]"]),
            (c::TOGGLE_DEBUG, &[]),
            (c::REVEAL_IN_PROJECT, &[]),
            (c::EXTEND_SELECTION, &["Ctrl+Shift+Alt+Right"]),
            (c::SHRINK_SELECTION, &["Ctrl+Shift+Alt+Left"]),
            (c::GOTO_IMPLEMENTATION, &["CmdOrCtrl+F12"]),
            (c::TYPE_HIERARCHY, &[]),
        ],
    },
    Preset {
        id: "sublime",
        name: "Sublime Text",
        overrides: &[
            (c::SEARCH_FILES, &["CmdOrCtrl+P"]),
            (c::GOTO_DEFINITION, &["F12"]),
            (c::GOTO_LINE, &["Ctrl+G"]),
            // ⌃G is Go to Line: Find Next is ⌘G on macOS, F3 elsewhere (Ctrl+G there).
            (c::FIND_NEXT, &["mac:CmdOrCtrl+G", "F3"]),
            (c::FIND_PREV, &["mac:CmdOrCtrl+Shift+G", "Shift+F3"]),
            (c::DUPLICATE, &["CmdOrCtrl+Shift+D"]),
            (c::DELETE_LINE, &["Ctrl+Shift+K"]),
            (c::MOVE_LINE_UP, &["Alt+Up"]),
            (c::MOVE_LINE_DOWN, &["Alt+Down"]),
            (c::MOVE_STATEMENT_UP, &[]),
            (c::MOVE_STATEMENT_DOWN, &[]),
            (c::SELECT_NEXT, &["CmdOrCtrl+D"]),
            (c::SELECT_ALL_OCCURRENCES, &["Alt+F3"]),
            (c::UNSELECT_OCCURRENCE, &[]),
            // ⌘H hides the app on macOS: ⌘R there, Ctrl+H elsewhere.
            (c::REPLACE, &["CmdOrCtrl+Alt+F", "mac:CmdOrCtrl+R", "pc:CmdOrCtrl+H"]),
            // On Windows Alt+− / Alt+Shift+− (Ctrl+Shift+− is Zoom Out there).
            (c::NAVIGATE_BACK, &["mac:Ctrl+-", "pc:Alt+-"]),
            (c::NAVIGATE_FORWARD, &["mac:Ctrl+Shift+-", "pc:Alt+Shift+-"]),
            (c::FOLD, &["CmdOrCtrl+Alt+["]),
            (c::UNFOLD, &["CmdOrCtrl+Alt+]"]),
            (c::TOGGLE_DEBUG, &[]),
            (c::REVEAL_IN_PROJECT, &[]),
            (c::EXTEND_SELECTION, &["Ctrl+Shift+Alt+Right"]),
            (c::SHRINK_SELECTION, &["Ctrl+Shift+Alt+Left"]),
            (c::GOTO_IMPLEMENTATION, &["CmdOrCtrl+F12"]),
            (c::TYPE_HIERARCHY, &[]),
        ],
    },
];

/// The active preset (index in `PRESETS`), shared by every window.
static CURRENT: AtomicUsize = AtomicUsize::new(0);

pub fn current() -> &'static Preset {
    PRESETS.get(CURRENT.load(Ordering::Relaxed)).unwrap_or(&PRESETS[0])
}

/// Makes `id` the active preset. `false` if there is no such preset.
pub fn select(id: &str) -> bool {
    // `lumen`: the default preset's id in settings saved before it was renamed.
    let id = if id == "lumen" { "default" } else { id };
    match PRESETS.iter().position(|p| p.id == id) {
        Some(i) => {
            CURRENT.store(i, Ordering::Relaxed);
            true
        }
        None => false,
    }
}

/// The saved preset and the user's own shortcuts, applied at startup.
pub fn load() {
    select(&crate::config::settings().get(&crate::settings::KEYMAP));
    load_user();
}

/// A preset with this id exists (or a saved id from before a rename).
pub fn exists(id: &str) -> bool {
    id == "lumen" || PRESETS.iter().any(|p| p.id == id)
}

/// Chooses and saves a preset. `false` if it didn't change.
pub fn choose(id: &str) -> bool {
    if current().id == id || !select(id) {
        return false;
    }
    if let Err(e) = crate::config::settings().set(&crate::settings::KEYMAP, id.to_owned()) {
        tracing::warn!(error = %e, "cannot save the keymap");
    }
    true
}

/// The modules' shortcuts per preset (contributed).
static MODULE_PRESETS: RwLock<Vec<PresetShortcuts>> = RwLock::new(Vec::new());

/// The user's own shortcuts (loaded at startup, kept as changed).
static USER: RwLock<UserShortcuts> = RwLock::new(UserShortcuts(std::collections::BTreeMap::new()));

/// The modules' shortcuts per preset, once they're registered.
pub fn set_module_presets(presets: Vec<PresetShortcuts>) {
    // As the places: a command's shortcuts in a preset are its module's, only added to.
    if let Ok(mut all) = MODULE_PRESETS.write() {
        crate::commands::merge_by(&mut all, presets, |p| (p.preset, p.command));
    }
}

/// Every preset's shortcuts: the app's, then the modules'.
fn presets() -> Vec<PresetShortcuts> {
    let app = PRESETS.iter().flat_map(|p| {
        p.overrides.iter().map(|(command, shortcuts)| PresetShortcuts {
            preset: p.id,
            command: *command,
            shortcuts,
        })
    });
    let modules = MODULE_PRESETS.read().map(|m| m.clone()).unwrap_or_default();
    app.chain(modules).collect()
}

/// Reads the user's own shortcuts again (at startup, after they change).
pub fn load_user() {
    if let Ok(mut user) = USER.write() {
        *user = crate::settings::user_shortcuts();
    }
}

/// Saves a command's own shortcuts (`None`: back to the preset's) and uses them.
pub fn set_user(command: CommandId, shortcuts: Option<Vec<String>>) {
    if let Err(e) = crate::settings::set_user_shortcuts(command.0, shortcuts) {
        tracing::warn!(error = %e, "cannot save the shortcuts");
    }
    load_user();
}

/// What `f` sees: the layers as they are now, for macOS (`mac`) or the other systems.
fn with_layers<R>(mac: bool, f: impl FnOnce(&Layers<'_>) -> R) -> R {
    let presets = presets();
    let user = USER.read().map(|u| u.clone()).unwrap_or_default();
    f(&Layers { preset: current().id, presets: &presets, user: &user, mac })
}

/// The system the app runs on is macOS.
const MAC: bool = cfg!(target_os = "macos");

/// Where the modules' commands' keys work (the others: everywhere).
static PLACES: RwLock<Vec<(CommandId, When)>> = RwLock::new(Vec::new());

/// Where the modules' commands work, once they're registered. A command's place is its
/// handler's, the same whichever modules load: the list only grows (registered again — the
/// tests' windows, a selection without some modules — nothing known is taken away, so a
/// window never sees another's partial list).
pub fn set_places(places: Vec<(CommandId, When)>) {
    if let Ok(mut all) = PLACES.write() {
        crate::commands::merge_by(&mut all, places, |(id, _)| *id);
    }
}

/// The place of the editor of `language`'s files, when a module's command works there.
pub fn editor_language(language: &str) -> Option<When> {
    let places = PLACES.read().ok()?;
    places.iter().map(|(_, w)| *w).find(|w| matches!(w, When::EditorLanguage(l) if *l == language))
}

/// Where a command's keys work.
fn when(id: CommandId) -> When {
    let places = PLACES.read();
    places
        .ok()
        .and_then(|p| p.iter().find(|(c, _)| *c == id).map(|(_, w)| *w))
        .unwrap_or(When::Everywhere)
}

/// The shortcuts of command `id` (the first one is shown in menus).
pub fn shortcuts(id: CommandId) -> Vec<String> {
    shortcuts_on(id, MAC)
}

/// The shortcuts of command `id` on macOS (`mac`) or on the other systems.
pub fn shortcuts_on(id: CommandId, mac: bool) -> Vec<String> {
    let Some(spec) = crate::commands::spec_of(id) else { return Vec::new() };
    with_layers(mac, |layers| layers.shortcuts(&spec).0)
}

/// Every command (the app's and the modules'), its specs.
fn all_specs() -> Vec<CommandSpec> {
    let modules = crate::commands::contributed().into_iter().map(|(s, _)| s);
    SPECS.iter().cloned().chain(modules).collect()
}

/// Every command's shortcuts through the layers (for the keymap and the menus).
pub fn bindings() -> Vec<Binding> {
    bindings_on(MAC)
}

/// The same, on macOS (`mac`) or on the other systems.
fn bindings_on(mac: bool) -> Vec<Binding> {
    let specs = all_specs();
    with_layers(mac, |layers| command_model::bindings::resolve(&specs, layers, when))
}

/// The keymap of the bindings (the same keys in the same place are said in the log).
pub fn keymap(mac: bool) -> Result<command_model::Keymap, command_model::ParseChordError> {
    let bindings = bindings_on(mac);
    for conflict in command_model::bindings::conflicts(&bindings, mac) {
        tracing::debug!(shortcut = conflict.shortcut, commands = ?conflict.commands, "shortcut conflict");
    }
    command_model::Keymap::from_bindings(&bindings, mac)
}

#[cfg(test)]
#[path = "../tests/unit/keymaps.rs"]
mod tests;
