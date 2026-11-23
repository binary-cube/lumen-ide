//! A tree's rows, drawn the same way wherever one shows (the project tree, a remote
//! project's folders, the git branches, the task manager's groups): the chevron of a node
//! that folds, the indentation guides, a row's highlight (selection, hover), and for a list
//! of rows of one height ([`TreeList`]) which rows show and which one is under the pointer.
//!
//! Each tree keeps its own geometry (row height, indentation, where the chevron goes): these
//! only draw and hit-test it, so the pixels stay those of each tree.

use std::ops::Range;

use icons::Icons;
use kernel::{Rect, Rgba};
use render_api::{Layer, Quad};

/// The chevron's icon: down for an open node, right for a folded one.
pub fn chevron_icon(expanded: bool) -> &'static str {
    if expanded { "chevron-down" } else { "chevron-right" }
}

/// A tree's row as a screen reader reads it (see [`item`]).
#[derive(Debug, Clone, Copy)]
pub struct Item<'a> {
    /// What its row says (an entry's name).
    pub name: &'a str,
    /// The whole row.
    pub bounds: Rect,
    /// Its depth (the top level is 0).
    pub level: usize,
    /// Open or folded; `None`: nothing under it.
    pub expanded: Option<bool>,
    pub selected: bool,
}

/// A tree's row told as one item (its name, its depth, open or not, chosen) while the guard
/// returned lives (drop it once the row is painted): its texts that say its name are its
/// own, its others (a note) read under it, its chevron says nothing more.
pub fn item(row: Item<'_>) -> Option<ui_kit::access::Opened> {
    ui_kit::access::open(|| {
        use ui_kit::access::{Described, Role};
        let mut node = Described::new(Role::TreeItem, row.name, row.bounds)
            .level(row.level)
            .selected(row.selected);
        node.state.expanded = row.expanded;
        node
    })
}

/// The chevron of a node that folds, in `at` (nothing when the icon set has none). To a
/// screen reader it's the tree's item, open or folded, named by the text on its row — unless
/// it's drawn in a row told whole ([`item`]).
pub fn chevron(layer: &mut Layer, icons: &Icons, at: Rect, expanded: bool, color: Rgba) {
    if !ui_kit::access::inside(ui_kit::access::Role::TreeItem) {
        ui_kit::access::node(|| {
            let mut node = ui_kit::access::Described::new(ui_kit::access::Role::TreeItem, "", at);
            node.state.expanded = Some(expanded);
            node
        });
    }
    if let Some(id) = icons.id(chevron_icon(expanded)) {
        ui_kit::draw_icon(layer, icons, id, at, color);
    }
}

/// The indentation guides across a row (`top`, `height`): one vertical line `width` wide
/// for each level in `levels`, at `x_of(level)` (rounded to a whole pixel).
pub fn guides(
    layer: &mut Layer,
    levels: Range<usize>,
    x_of: impl Fn(usize) -> f32,
    (top, height): (f32, f32),
    width: f32,
    color: Rgba,
) {
    for level in levels {
        let x = x_of(level).round();
        layer.quad(Quad::filled(Rect::new(x, top, width, height), color));
    }
}

/// A row's highlight (chosen, under the mouse): a rounded fill.
pub fn highlight(layer: &mut Layer, at: Rect, color: Rgba, radius: f32) {
    layer.quad(Quad::filled(at, color).rounded(radius));
}

/// A scrolled list of rows of one height (`row_height`, physical pixels), `len` of them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TreeList {
    pub row_height: f32,
    pub len: usize,
}

impl TreeList {
    pub fn new(row_height: f32, len: usize) -> Self {
        Self { row_height, len }
    }

    /// The rows a view `height` tall shows when scrolled by `scroll`: from the first one cut
    /// at the top, one more than fit (the last one cut at the bottom).
    pub fn visible(&self, scroll: f32, height: f32) -> Range<usize> {
        let first = ((scroll / self.row_height) as usize).min(self.len);
        let count = (height / self.row_height).ceil() as usize + 1;
        first..(first + count).min(self.len)
    }

    /// The slot `y` falls in, measured from the first row's top: `None` above it; it may be
    /// past the last row (a tree with an extra row of its own counts it itself).
    pub fn slot_at(&self, y: f32) -> Option<usize> {
        (y >= 0.0).then(|| (y / self.row_height) as usize)
    }

    /// The row `y` falls on, measured from the first row's top.
    pub fn row_at(&self, y: f32) -> Option<usize> {
        self.slot_at(y).filter(|&i| i < self.len)
    }
}

/// A file tree's rows laid out as the project tree lays them out (physical pixels for one
/// scale): where a row's chevron, icon and name go at each depth, its indentation guides,
/// its highlight. Shared by every tree of files (the project's, a commit's changed files),
/// so they look the same.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntryLayout {
    pub scale: f32,
    pub row_height: f32,
    pub indent: f32,
    /// The content's left padding.
    pub padding: f32,
    /// A hairline's width.
    pub line: f32,
}

impl EntryLayout {
    /// The project tree's: rows `row_height` tall, `indent` per level (logical pixels).
    pub fn new(scale: f32, row_height: f32, indent: f32) -> Self {
        let px = |v: f32| (v * scale).round();
        Self {
            scale,
            row_height: px(row_height),
            indent: px(indent),
            padding: px(14.0),
            line: scale.round().max(1.0),
        }
    }

    fn px(&self, v: f32) -> f32 {
        (v * self.scale).round()
    }

    /// Where a row at `depth` starts (its chevron), in a tree whose left edge is `left`.
    pub fn x(&self, left: f32, depth: usize) -> f32 {
        left + self.padding + depth as f32 * self.indent
    }

    /// The guide of level `level`.
    pub fn guide_x(&self, left: f32, level: usize) -> f32 {
        left + self.padding + level as f32 * self.indent + self.indent * 0.3
    }

    /// A row's guides (`top`): one per parent level below `depth`.
    pub fn guides(&self, layer: &mut Layer, left: f32, depth: usize, top: f32, color: Rgba) {
        guides(
            layer,
            1..depth,
            |l| self.guide_x(left, l),
            (top, self.row_height),
            self.line,
            color,
        );
    }

    fn center(&self, top: f32, size: f32) -> f32 {
        (top + (self.row_height - size) / 2.0).round()
    }

    /// A folder's chevron, from `x`.
    pub fn chevron_rect(&self, x: f32, top: f32) -> Rect {
        let size = self.px(12.0);
        Rect::new(x.round(), self.center(top, size), size, size)
    }

    /// The entry's icon, from `x`.
    pub fn icon_rect(&self, x: f32, top: f32) -> Rect {
        let (chevron, icon) = (self.px(12.0), self.px(16.0));
        Rect::new((x + chevron + self.px(4.0)).round(), self.center(top, icon), icon, icon)
    }

    /// Where the name starts, after the icon.
    pub fn label_x(&self, icon: Rect) -> f32 {
        (icon.max_x() + self.px(7.0)).round()
    }

    /// A row's highlight (chosen, under the mouse) across `bounds`, at `top`.
    pub fn pill(&self, bounds: Rect, top: f32) -> Rect {
        let inset = self.px(8.0);
        Rect::new(
            bounds.min_x() + inset,
            top + self.line,
            bounds.size.width - 2.0 * inset,
            self.row_height - 2.0 * self.line,
        )
    }

    /// The highlight's corners.
    pub fn pill_radius(&self) -> f32 {
        self.px(6.0)
    }
}

/// What a tree entry is, for its chevron and icon.
#[derive(Debug, Clone, Copy)]
pub enum EntryIcon<'a> {
    /// A folder (its chevron, its icon by the icon set: `name`'s, colored `color` unless the
    /// set draws its own).
    Folder {
        name: &'a str,
        expanded: bool,
        color: Rgba,
    },
    File {
        icon: icons::IconId,
        color: Rgba,
    },
}

/// An entry's chevron (a folder's, in `chevron`) and icon from `x` at `top`, each color
/// passed through `dim`; returns where the icon is.
pub fn entry_icon(
    layer: &mut Layer,
    icons: &Icons,
    layout: &EntryLayout,
    (x, top): (f32, f32),
    entry: EntryIcon<'_>,
    (chevron_color, dim): (Rgba, &dyn Fn(Rgba) -> Rgba),
) -> Rect {
    let icon_rect = layout.icon_rect(x, top);
    match entry {
        EntryIcon::Folder { name, expanded, color } => {
            chevron(layer, icons, layout.chevron_rect(x, top), expanded, chevron_color);
            // The icons' registry decides (a set's icon for `src`, `tests`…, else ours).
            let (folder, tint) = icons.folder(name, expanded);
            let color = match tint {
                icons::Tint::Own => Rgba::WHITE,
                _ => color,
            };
            ui_kit::draw_icon(layer, icons, folder, icon_rect, dim(color));
        }
        EntryIcon::File { icon, color } => {
            ui_kit::draw_icon(layer, icons, icon, icon_rect, dim(color));
        }
    }
    icon_rect
}

/// The files' icons and colors by name (by the icon set and the languages' rules), for the
/// icon set and style they were found with: emptied when either changes (or [`Self::clear`]:
/// the theme, the project). Asked for every visible row on every frame.
#[derive(Debug)]
pub struct FileIcons {
    stamp: Option<(Option<u8>, icons::IconLook)>,
    icons: kernel::GenerationalCache<Box<str>, (icons::IconId, Rgba)>,
}

impl Default for FileIcons {
    fn default() -> Self {
        Self::new(512)
    }
}

impl FileIcons {
    /// Remembering up to `limit` names per generation.
    pub fn new(limit: usize) -> Self {
        Self { stamp: None, icons: kernel::GenerationalCache::new(limit) }
    }

    pub fn clear(&mut self) {
        self.icons.clear();
    }

    /// Asked with this icon set and style: what was found with another goes.
    pub fn stamp(&mut self, icons: &Icons) {
        let stamp = Some((icons.file_set(), icons.look()));
        if self.stamp != stamp {
            self.icons.clear();
            self.stamp = stamp;
        }
    }

    /// File `name`'s icon and color: the icon set's, else ours by its language (from the
    /// project's `queries`; none: the plain file's).
    pub fn icon(
        &mut self,
        name: &str,
        icons: &Icons,
        palette: &crate::Palette,
        queries: Option<&dyn workbench_model::ProjectQueries>,
    ) -> (icons::IconId, Rgba) {
        if let Some(&found) = self.icons.get(name) {
            return found;
        }
        let found = match queries {
            Some(q) => {
                let (icon, language) = q.file_icon(name);
                let (id, tint) = icons.file(name, icon, &language);
                (id, palette.tint(&tint))
            }
            None => (icons.id_or_file("file"), palette.file_icon("")),
        };
        self.icons.insert(name.into(), found);
        found
    }
}

#[cfg(test)]
#[path = "../tests/unit/tree.rs"]
mod tests;
