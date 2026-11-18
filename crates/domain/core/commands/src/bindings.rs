//! Which shortcuts a command has, in layers: its own (the command's spec) → the chosen
//! preset's (the app's and the modules' — a module gives its commands' shortcuts per preset,
//! the app doesn't know them) → the user's (only the commands they changed, each with its
//! whole list). Each binding works somewhere ([`When`]): the same keys can mean another
//! command elsewhere; on the same keys in the same place, a [`Conflict`].

use std::collections::BTreeMap;

use crate::{CommandId, CommandSpec, KeyChord};

/// Where a binding works.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum When {
    /// Wherever the keys reach the app (the editor, a view, the terminal for its own keys).
    Everywhere,
    /// In the editor.
    Editor,
    /// In a view (its kind: `canvas`, `compare`, `terminal`…).
    View(&'static str),
    /// In the editor, on a file of that language (its id: `sql`): a part of [`When::Editor`]
    /// (the same keys there conflict), looked up before it.
    EditorLanguage(&'static str),
}

impl When {
    /// Both can be active at once (then the same keys there conflict).
    pub fn overlaps(self, other: Self) -> bool {
        let editor = |w: Self| matches!(w, Self::Editor | Self::EditorLanguage(_));
        self == other
            || self == Self::Everywhere
            || other == Self::Everywhere
            // A language's editor is the editor too (two languages never are at once).
            || (editor(self) && editor(other) && (self == Self::Editor || other == Self::Editor))
    }
}

/// A preset's shortcuts for a command (the app's presets and the modules', contributed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresetShortcuts {
    /// The preset's id (`jetbrains`, `vscode`…).
    pub preset: &'static str,
    pub command: CommandId,
    /// Its shortcuts there (empty: none).
    pub shortcuts: &'static [&'static str],
}

/// The user's own shortcuts: the commands they changed, each with its whole list (empty:
/// none), by command id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserShortcuts(pub BTreeMap<String, Vec<String>>);

impl UserShortcuts {
    pub fn get(&self, command: CommandId) -> Option<&[String]> {
        self.0.get(command.0).map(Vec::as_slice)
    }
}

/// Where a command's shortcuts come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Its own (the command's spec).
    Default,
    /// The chosen preset's.
    Preset,
    /// The user's.
    User,
}

/// The layers a command's shortcuts are resolved through.
#[derive(Debug, Clone, Copy)]
pub struct Layers<'a> {
    /// The chosen preset's id.
    pub preset: &'a str,
    /// Every preset's shortcuts (the app's and the modules').
    pub presets: &'a [PresetShortcuts],
    pub user: &'a UserShortcuts,
    /// The system the shortcuts are for (macOS, or the others): one written for the other
    /// system only (`mac:…`, `pc:…`) is left out, the rest as that system reads them.
    pub mac: bool,
}

impl Layers<'_> {
    /// A command's shortcuts (the first one is shown in menus) and where they come from.
    pub fn shortcuts(&self, spec: &CommandSpec) -> (Vec<String>, Origin) {
        if let Some(own) = self.user.get(spec.id) {
            return (own.to_vec(), Origin::User);
        }
        let preset = self.presets.iter().find(|p| p.preset == self.preset && p.command == spec.id);
        match preset {
            Some(p) => (self.owned(p.shortcuts), Origin::Preset),
            None => (self.owned(spec.shortcuts), Origin::Default),
        }
    }

    /// `shortcuts` on this system.
    fn owned(&self, shortcuts: &[&str]) -> Vec<String> {
        shortcuts
            .iter()
            .filter_map(|s| crate::shortcut_on(s, self.mac))
            .map(str::to_owned)
            .collect()
    }
}

/// A command's resolved shortcuts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub command: CommandId,
    pub shortcuts: Vec<String>,
    pub when: When,
    pub origin: Origin,
}

/// Every command's shortcuts through the layers; `when` says where each one works.
pub fn resolve(
    specs: &[CommandSpec],
    layers: &Layers<'_>,
    when: impl Fn(CommandId) -> When,
) -> Vec<Binding> {
    specs
        .iter()
        .map(|spec| {
            let (shortcuts, origin) = layers.shortcuts(spec);
            Binding { command: spec.id, shortcuts, when: when(spec.id), origin }
        })
        .collect()
}

/// The same keys for more commands in places active at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The keys, as the first command writes them.
    pub shortcut: String,
    pub commands: Vec<CommandId>,
}

/// The conflicts among `bindings` on this platform (`mac`: what `Ctrl` means), in the
/// bindings' order; shortcuts that don't parse are left out.
pub fn conflicts(bindings: &[Binding], mac: bool) -> Vec<Conflict> {
    let mut seen: Vec<(KeyChord, When, &str, CommandId)> = Vec::new();
    let mut out: Vec<Conflict> = Vec::new();
    for binding in bindings {
        for shortcut in &binding.shortcuts {
            let Ok(chord) = KeyChord::parse(shortcut).map(|c| c.for_platform(mac)) else {
                continue;
            };
            let clash = seen.iter().find(|(c, w, _, id)| {
                *c == chord && w.overlaps(binding.when) && *id != binding.command
            });
            if let Some((_, _, first, id)) = clash {
                match out.iter_mut().find(|c| c.shortcut == *first) {
                    Some(conflict) if !conflict.commands.contains(&binding.command) => {
                        conflict.commands.push(binding.command);
                    }
                    Some(_) => {}
                    None => out.push(Conflict {
                        shortcut: (*first).to_owned(),
                        commands: vec![*id, binding.command],
                    }),
                }
            }
            seen.push((chord, binding.when, shortcut, binding.command));
        }
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/bindings.rs"]
mod tests;
