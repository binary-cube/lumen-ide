//! Icon packs with other drawings for the interface, each a module (on / off in the
//! settings like any other): Lucide, Tabler, Phosphor, Material Symbols, Remix Icon,
//! Heroicons, Fluent, Iconoir, Bootstrap Icons, Octicons, Ionicons. Their data
//! (`data/<pack>.svgs`, made by `cargo xtask icon-packs`) holds only the drawings of our icon
//! names; it sits in the binary untouched until the pack is chosen — then its index is read
//! on the first icon drawn, and dropped when another pack is chosen. What a pack lacks is
//! drawn from our own set.
//!
//! The file icon sets (Material Icon Theme, Catppuccin, Symbols, vscode-icons) are modules
//! the same way: their rules and drawings (`data/<set>-files.map`, `.svgs`), read once
//! chosen.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use icons::IconPack;

/// A pack from its data: one drawing per line, `name<TAB>svg` (`#`: a comment).
#[derive(Debug)]
pub struct DataPack {
    id: &'static str,
    name: &'static str,
    credit: &'static str,
    data: &'static str,
    /// Its drawings by name, read on the first one asked (slices of the data: nothing
    /// copied); `None` until then, and again once released.
    index: Mutex<Option<HashMap<&'static str, &'static str>>>,
}

impl DataPack {
    pub const fn new(
        id: &'static str,
        name: &'static str,
        credit: &'static str,
        data: &'static str,
    ) -> Self {
        Self { id, name, credit, data, index: Mutex::new(None) }
    }

    /// Whether its index is read (it was drawn from since it was last released).
    pub fn is_read(&self) -> bool {
        self.index.lock().is_ok_and(|i| i.is_some())
    }

    fn read(data: &'static str) -> HashMap<&'static str, &'static str> {
        data.lines().filter(|l| !l.starts_with('#')).filter_map(|l| l.split_once('\t')).collect()
    }
}

impl IconPack for DataPack {
    fn id(&self) -> &'static str {
        self.id
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn credit(&self) -> &'static str {
        self.credit
    }

    fn svg(&self, name: &str) -> Option<Cow<'static, str>> {
        let mut index = self.index.lock().ok()?;
        let index = index.get_or_insert_with(|| Self::read(self.data));
        index.get(name).map(|svg| Cow::Borrowed(*svg))
    }

    fn release(&self) {
        if let Ok(mut index) = self.index.lock() {
            *index = None;
        }
    }
}

/// A file icon set from its data: its rules (`kind<TAB>what<TAB>icon`) and its drawings
/// (`icon<TAB>svg`), read on the first icon asked (slices of the data), dropped when released.
#[derive(Debug)]
pub struct DataFileSet {
    id: &'static str,
    name: &'static str,
    credit: &'static str,
    rules: &'static str,
    svgs: &'static str,
    index: Mutex<Option<FileIndex>>,
}

/// A file icon set's rules and drawings, by what they're for.
#[derive(Debug, Default)]
struct FileIndex {
    extensions: HashMap<&'static str, &'static str>,
    names: HashMap<&'static str, &'static str>,
    languages: HashMap<&'static str, &'static str>,
    folders: HashMap<&'static str, &'static str>,
    open: HashMap<&'static str, &'static str>,
    defaults: HashMap<&'static str, &'static str>,
    svgs: HashMap<&'static str, &'static str>,
}

impl DataFileSet {
    pub const fn new(
        id: &'static str,
        name: &'static str,
        credit: &'static str,
        rules: &'static str,
        svgs: &'static str,
    ) -> Self {
        Self { id, name, credit, rules, svgs, index: Mutex::new(None) }
    }

    pub fn is_read(&self) -> bool {
        self.index.lock().is_ok_and(|i| i.is_some())
    }

    fn read(&self) -> FileIndex {
        let mut index = FileIndex { svgs: DataPack::read(self.svgs), ..FileIndex::default() };
        for line in self.rules.lines().filter(|l| !l.starts_with('#')) {
            let mut parts = line.splitn(3, '\t');
            let (Some(kind), Some(what), Some(icon)) = (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let table = match kind {
                "e" => &mut index.extensions,
                "n" => &mut index.names,
                "l" => &mut index.languages,
                "f" => &mut index.folders,
                "o" => &mut index.open,
                _ => &mut index.defaults,
            };
            table.insert(what, icon);
        }
        index
    }

    /// `f` with its index (read once).
    fn with<T>(&self, f: impl FnOnce(&FileIndex) -> Option<T>) -> Option<T> {
        let mut index = self.index.lock().ok()?;
        f(index.get_or_insert_with(|| self.read()))
    }
}

impl icons::FileIconSet for DataFileSet {
    fn id(&self) -> &'static str {
        self.id
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn credit(&self) -> &'static str {
        self.credit
    }

    /// Its exact name first (`composer.json`), then its extensions, the longest first
    /// (`blade.php` before `php`), then its language; else the set's file icon.
    fn file(&self, name: &str, language: &str) -> Option<&'static str> {
        let lower = name.to_lowercase();
        self.with(|index| {
            if let Some(icon) = index.names.get(lower.as_str()) {
                return Some(*icon);
            }
            let by_extension = lower
                .match_indices('.')
                .map(|(i, _)| &lower[i + 1..])
                .find_map(|ext| index.extensions.get(ext));
            by_extension
                .or_else(|| index.languages.get(language))
                .or_else(|| index.defaults.get("file"))
                .copied()
        })
    }

    fn folder(&self, name: &str, open: bool) -> Option<&'static str> {
        let lower = name.to_lowercase();
        self.with(|index| {
            let (table, default) =
                if open { (&index.open, "folder-open") } else { (&index.folders, "folder") };
            table.get(lower.as_str()).or_else(|| index.defaults.get(default)).copied()
        })
    }

    fn svg(&self, key: &str) -> Option<Cow<'static, str>> {
        self.with(|index| index.svgs.get(key).map(|svg| Cow::Borrowed(*svg)))
    }

    fn release(&self) {
        if let Ok(mut index) = self.index.lock() {
            *index = None;
        }
    }
}

/// A file icon set's module: it contributes its set.
#[derive(Debug)]
pub struct FileSetModule {
    manifest: &'static ModuleManifest,
    set: fn() -> DataFileSet,
}

impl Module for FileSetModule {
    fn manifest(&self) -> &ModuleManifest {
        self.manifest
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn icons::FileIconSet>>(Arc::new((self.set)()));
        Ok(())
    }
}

/// Declares a file icon set: its module's manifest (`icons.files-<id>`) and how it's made
/// from its data (`data/<id>-files.map`, `.svgs`).
macro_rules! file_set {
    ($manifest:ident, $make:ident, $id:literal, $name:literal, $credit:literal, $what:literal) => {
        static $manifest: ModuleManifest = ModuleManifest {
            id: ModuleId::new_static(concat!("icons.files-", $id)),
            name: concat!($name, " file icons"),
            description: $what,
            depends_on: &[],
            info: extensions::ModuleInfo {
                icon: "file",
                author: Some($credit),
                ..extensions::ModuleInfo::BUILT_IN
            },
        };

        pub fn $make() -> DataFileSet {
            DataFileSet::new(
                $id,
                $name,
                $credit,
                include_str!(concat!("../data/", $id, "-files.map")),
                include_str!(concat!("../data/", $id, "-files.svgs")),
            )
        }
    };
}

file_set!(
    MATERIAL_FILES,
    material_files,
    "material",
    "Material Icon Theme",
    "Material Extensions · MIT",
    "Material Icon Theme's colored icons for files and folders (MIT)"
);
file_set!(
    CATPPUCCIN_FILES,
    catppuccin_files,
    "catppuccin",
    "Catppuccin",
    "Catppuccin · MIT",
    "Catppuccin's pastel icons for files and folders, its Mocha flavor (MIT)"
);
file_set!(
    SYMBOLS_FILES,
    symbols_files,
    "symbols",
    "Symbols",
    "Miguel Solorio · MIT",
    "Symbols' minimal icons for files and folders (MIT)"
);
file_set!(
    VSCODE_FILES,
    vscode_files,
    "vscode",
    "vscode-icons",
    "Roberto Huertas · MIT",
    "vscode-icons' colored icons for files and folders (MIT)"
);

/// Every file icon set's module, in their order in the settings.
pub fn file_set_modules() -> Vec<FileSetModule> {
    [
        (&MATERIAL_FILES, material_files as fn() -> DataFileSet),
        (&CATPPUCCIN_FILES, catppuccin_files),
        (&SYMBOLS_FILES, symbols_files),
        (&VSCODE_FILES, vscode_files),
    ]
    .into_iter()
    .map(|(manifest, set)| FileSetModule { manifest, set })
    .collect()
}

/// A pack's module: it contributes its pack.
#[derive(Debug)]
pub struct PackModule {
    manifest: &'static ModuleManifest,
    pack: fn() -> DataPack,
}

impl Module for PackModule {
    fn manifest(&self) -> &ModuleManifest {
        self.manifest
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn IconPack>>(Arc::new((self.pack)()));
        Ok(())
    }
}

/// Declares a pack: its module's manifest and how it's made from its data.
macro_rules! pack {
    ($manifest:ident, $make:ident, $id:literal, $name:literal, $credit:literal, $what:literal) => {
        static $manifest: ModuleManifest = ModuleManifest {
            id: ModuleId::new_static(concat!("icons.", $id)),
            name: concat!($name, " icons"),
            description: $what,
            depends_on: &[],
            info: extensions::ModuleInfo {
                icon: "image",
                author: Some($credit),
                ..extensions::ModuleInfo::BUILT_IN
            },
        };

        fn $make() -> DataPack {
            DataPack::new($id, $name, $credit, include_str!(concat!("../data/", $id, ".svgs")))
        }
    };
}

pack!(
    LUCIDE,
    lucide,
    "lucide",
    "Lucide",
    "Lucide Contributors · ISC",
    "Lucide's drawings for the interface's icons (ISC)"
);
pack!(
    TABLER,
    tabler,
    "tabler",
    "Tabler",
    "Paweł Kuna · MIT",
    "Tabler's drawings for the interface's icons (MIT)"
);
pack!(
    PHOSPHOR,
    phosphor,
    "phosphor",
    "Phosphor",
    "Phosphor Icons · MIT",
    "Phosphor's drawings for the interface's icons (MIT)"
);
pack!(
    MATERIAL,
    material,
    "material",
    "Material Symbols",
    "Google · Apache-2.0",
    "Material Symbols' drawings for the interface's icons (Apache-2.0)"
);
pack!(
    REMIX,
    remix,
    "remix",
    "Remix Icon",
    "Remix Design · Apache-2.0",
    "Remix Icon's drawings for the interface's icons (Apache-2.0)"
);
pack!(
    HEROICONS,
    heroicons,
    "heroicons",
    "Heroicons",
    "Tailwind Labs · MIT",
    "Heroicons' drawings for the interface's icons (MIT)"
);
pack!(
    FLUENT,
    fluent,
    "fluent",
    "Fluent",
    "Microsoft · MIT",
    "Fluent UI System Icons' drawings for the interface's icons (MIT)"
);

pack!(
    ICONOIR,
    iconoir,
    "iconoir",
    "Iconoir",
    "Luca Burgio · MIT",
    "Iconoir's drawings for the interface's icons (MIT)"
);
pack!(
    BOOTSTRAP,
    bootstrap,
    "bootstrap",
    "Bootstrap",
    "The Bootstrap Authors · MIT",
    "Bootstrap Icons' drawings for the interface's icons (MIT)"
);
pack!(
    OCTICONS,
    octicons,
    "octicons",
    "Octicons",
    "GitHub · MIT",
    "GitHub's Octicons for the interface's icons (MIT)"
);
pack!(
    IONICONS,
    ionicons,
    "ionicons",
    "Ionicons",
    "Ionic · MIT",
    "Ionicons' drawings for the interface's icons (MIT)"
);

/// Every pack's module, in their order in the settings.
pub fn modules() -> Vec<PackModule> {
    [
        (&LUCIDE, lucide as fn() -> DataPack),
        (&TABLER, tabler),
        (&PHOSPHOR, phosphor),
        (&MATERIAL, material),
        (&REMIX, remix),
        (&HEROICONS, heroicons),
        (&FLUENT, fluent),
        (&ICONOIR, iconoir),
        (&BOOTSTRAP, bootstrap),
        (&OCTICONS, octicons),
        (&IONICONS, ionicons),
    ]
    .into_iter()
    .map(|(manifest, pack)| PackModule { manifest, pack })
    .collect()
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
