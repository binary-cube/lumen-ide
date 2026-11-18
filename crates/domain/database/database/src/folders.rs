//! Data sources' folders and colors: a list's sources grouped in folders (and subfolders),
//! each with a name and, if the user gave it one, a color — the production servers in red,
//! the development ones in green. A source takes its folder's color (the nearest folder up
//! that has one) unless it has its own. Pure: the lists are edited here, kept by a
//! [`crate::DataSourceStore`], drawn by the explorer (a color is a name, resolved to the
//! theme's own shade there).
//!
//! A folder belongs to one list ([`Scope`]): a source in it is in that list too. Taking a
//! folder away never takes its sources: they (and its subfolders) move up to its parent.

use std::collections::HashSet;
use std::fmt;

use kernel::Rgba;

use crate::source::{DataSource, Scope};

/// A folder's identity: a UUID v4, never its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FolderId(u128);

impl FolderId {
    /// A new identity (UUID v4).
    pub fn new() -> Self {
        Self(kernel::id::new_uuid())
    }

    pub const fn from_u128(id: u128) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u128 {
        self.0
    }

    /// From its saved text; `None` if it isn't a UUID.
    pub fn parse(text: &str) -> Option<Self> {
        kernel::id::parse_uuid(text).map(Self)
    }
}

impl Default for FolderId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for FolderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&kernel::id::uuid_text(self.0))
    }
}

/// A source's or a folder's color: one of a few named ones (drawn in each theme's own
/// shade, readable on dark and light), or any the user picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceColor {
    Red,
    Orange,
    Yellow,
    Green,
    Teal,
    Blue,
    Purple,
    Gray,
    /// One picked: `0xRRGGBB`.
    Custom(u32),
}

impl SourceColor {
    /// The named ones, in the order they're offered.
    pub const NAMED: [Self; 8] = [
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Teal,
        Self::Blue,
        Self::Purple,
        Self::Gray,
    ];

    /// How it's saved: its name, or `#rrggbb` for one picked.
    pub fn name(self) -> String {
        match self {
            Self::Custom(rgb) => format!("#{:06x}", rgb & 0xff_ffff),
            named => named.key().to_owned(),
        }
    }

    /// The named one's key (`red`…); `custom` for one picked.
    pub fn key(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Orange => "orange",
            Self::Yellow => "yellow",
            Self::Green => "green",
            Self::Teal => "teal",
            Self::Blue => "blue",
            Self::Purple => "purple",
            Self::Gray => "gray",
            Self::Custom(_) => "custom",
        }
    }

    /// From its saved text (a name, any case, or `#rrggbb`); `None` if it's neither.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.starts_with('#') {
            let [r, g, b, _] = Rgba::from_hex(text)?.to_rgba8();
            return Some(Self::Custom(u32::from_be_bytes([0, r, g, b])));
        }
        let lower = text.to_ascii_lowercase();
        let lower = if lower == "grey" { "gray".to_owned() } else { lower };
        Self::NAMED.into_iter().find(|c| c.key() == lower)
    }
}

/// A folder of a list's sources.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceFolder {
    pub id: FolderId,
    pub name: String,
    /// The folder it's in; `None`: at the list's top.
    pub parent: Option<FolderId>,
    pub color: Option<SourceColor>,
    /// Marked as production: every source in it (and in its subfolders) is too.
    pub production: bool,
    pub scope: Scope,
}

impl SourceFolder {
    /// A new folder (a new identity) at `scope`'s top.
    pub fn new(name: impl Into<String>, scope: Scope) -> Self {
        Self {
            id: FolderId::new(),
            name: name.into(),
            parent: None,
            color: None,
            production: false,
            scope,
        }
    }
}

/// One list as it's kept: its sources (in order) and its folders.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SourceList {
    pub sources: Vec<DataSource>,
    pub folders: Vec<SourceFolder>,
}

impl SourceList {
    pub fn new(sources: Vec<DataSource>, folders: Vec<SourceFolder>) -> Self {
        Self { sources, folders }
    }

    /// Only sources (a list from before folders).
    pub fn of(sources: Vec<DataSource>) -> Self {
        Self { sources, folders: Vec::new() }
    }
}

/// Folder `id` among `folders`.
pub fn folder(folders: &[SourceFolder], id: FolderId) -> Option<&SourceFolder> {
    folders.iter().find(|f| f.id == id)
}

/// The folders from the top down to `id` (itself last); empty when it isn't there. A
/// loop (a broken file) ends the walk.
pub fn folder_path(folders: &[SourceFolder], id: FolderId) -> Vec<&SourceFolder> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut at = Some(id);
    while let Some(id) = at {
        if !seen.insert(id) {
            break;
        }
        let Some(f) = folder(folders, id) else { break };
        out.push(f);
        at = f.parent;
    }
    out.reverse();
    out
}

/// Folder `id`'s color: its own, else the nearest folder's up that has one.
pub fn folder_color(folders: &[SourceFolder], id: FolderId) -> Option<SourceColor> {
    folder_path(folders, id).into_iter().rev().find_map(|f| f.color)
}

/// The color `source` is shown in: its own, else its folder's ([`folder_color`]).
pub fn effective_color(source: &DataSource, folders: &[SourceFolder]) -> Option<SourceColor> {
    source.color.or_else(|| source.folder.and_then(|id| folder_color(folders, id)))
}

/// The nearest folder from `id` up (itself included) marked as production, if any.
pub fn production_folder(folders: &[SourceFolder], id: FolderId) -> Option<&SourceFolder> {
    folder_path(folders, id).into_iter().rev().find(|f| f.production)
}

/// Folder `id` is production: itself, or a folder it's in, is marked so.
pub fn folder_production(folders: &[SourceFolder], id: FolderId) -> bool {
    production_folder(folders, id).is_some()
}

/// `source` is production: marked so itself, or in a folder that is ([`folder_production`]).
/// Inherited like the color, but never taken away by a source inside: a production folder's
/// sources are all production (safety comes before comfort).
pub fn effective_production(source: &DataSource, folders: &[SourceFolder]) -> bool {
    source.production || source.folder.is_some_and(|id| folder_production(folders, id))
}

/// Every folder inside `id` (its subfolders, theirs…), not itself.
pub fn descendants(folders: &[SourceFolder], id: FolderId) -> Vec<FolderId> {
    let mut out: Vec<FolderId> = Vec::new();
    let mut queue = vec![id];
    while let Some(at) = queue.pop() {
        for f in folders.iter().filter(|f| f.parent == Some(at)) {
            if f.id != id && !out.contains(&f.id) {
                out.push(f.id);
                queue.push(f.id);
            }
        }
    }
    out
}

/// Folder `id` can go into `parent` (`None`: the top): never into itself or one inside it,
/// never into another list's folder.
pub fn can_move_folder(folders: &[SourceFolder], id: FolderId, parent: Option<FolderId>) -> bool {
    let Some(me) = folder(folders, id) else { return false };
    match parent {
        None => true,
        Some(p) => {
            p != id
                && folder(folders, p).is_some_and(|f| f.scope == me.scope)
                && !descendants(folders, id).contains(&p)
        }
    }
}

/// Folder `id` moved into `parent` when it can be ([`can_move_folder`]); whether it was.
pub fn move_folder(folders: &mut [SourceFolder], id: FolderId, parent: Option<FolderId>) -> bool {
    if !can_move_folder(folders, id, parent) {
        return false;
    }
    let Some(me) = folders.iter_mut().find(|f| f.id == id) else { return false };
    if me.parent == parent {
        return false;
    }
    me.parent = parent;
    true
}

/// Folder `id` taken away: its subfolders and its sources move up to its parent (none is
/// ever taken with it). Whether it was there.
pub fn delete_folder(
    folders: &mut Vec<SourceFolder>,
    sources: &mut [DataSource],
    id: FolderId,
) -> bool {
    let Some(at) = folders.iter().position(|f| f.id == id) else { return false };
    let gone = folders.remove(at);
    for f in folders.iter_mut().filter(|f| f.parent == Some(id)) {
        f.parent = gone.parent;
    }
    for s in sources.iter_mut().filter(|s| s.folder == Some(id)) {
        s.folder = gone.parent;
    }
    true
}

/// The folders' and sources' places made sound (a list read back, or edited by hand): a
/// folder of another list, or one in a folder that isn't there (or in a loop), goes to the
/// top; a source in a folder that isn't there (or another list's) is at the top. Whether
/// anything changed.
pub fn repair(list: &mut SourceList) -> bool {
    let mut changed = false;
    let known: Vec<(FolderId, Scope)> = list.folders.iter().map(|f| (f.id, f.scope)).collect();
    let scope_of = |id: FolderId| known.iter().find(|(i, _)| *i == id).map(|(_, s)| *s);
    for i in 0..list.folders.len() {
        let (id, scope, parent) = {
            let f = &list.folders[i];
            (f.id, f.scope, f.parent)
        };
        let Some(parent) = parent else { continue };
        let broken = scope_of(parent) != Some(scope)
            || parent == id
            || descendants(&list.folders, id).contains(&parent);
        if broken {
            list.folders[i].parent = None;
            changed = true;
        }
    }
    for s in &mut list.sources {
        if let Some(id) = s.folder
            && scope_of(id) != Some(s.scope)
        {
            s.folder = None;
            changed = true;
        }
    }
    changed
}

/// The sources in folder `id` and every folder inside it.
pub fn sources_in<'a>(
    folders: &[SourceFolder],
    sources: &'a [DataSource],
    id: FolderId,
) -> Vec<&'a DataSource> {
    let mut all = descendants(folders, id);
    all.push(id);
    sources.iter().filter(|s| s.folder.is_some_and(|f| all.contains(&f))).collect()
}

/// The folders right inside `parent` (`None`: the top) of `scope`, by name (any case).
pub fn children(
    folders: &[SourceFolder],
    scope: Scope,
    parent: Option<FolderId>,
) -> Vec<&SourceFolder> {
    let mut out: Vec<&SourceFolder> =
        folders.iter().filter(|f| f.scope == scope && f.parent == parent).collect();
    out.sort_by_key(|f| f.name.to_lowercase());
    out
}

#[cfg(test)]
#[path = "../tests/unit/folders.rs"]
mod tests;
