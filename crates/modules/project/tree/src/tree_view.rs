//! The project tree panel (left).
//!
//! Draws only the visible rows; clicking a directory expands or
//! collapses it, clicking a file opens it.

use icons::{IconId, Icons};
use kernel::{Rect, Rgba};
use render_api::{Layer, Point, Quad, Scene};
use text_layout::{Font, FontFamily, FontMetrics, TextSystem};
use ui_kit::{TextInput, TextInputStyle, centered_baseline, draw_icon, draw_text};
use workspace::marks::{PathMark, PathMarks};
use workspace::{FileTree, Mark, NodeId, NodeKind, VisibleRow};

use ui_components::Palette;
use ui_components::ds;
use ui_components::tree::{self as tree_row, TreeList};

/// A folder a drag would drop into: its highlight's corners and its outline's width
/// (logical pixels).
const DROP_RADIUS: f32 = 6.0;
const DROP_OUTLINE: f32 = 1.5;
/// The name field of a new entry, in its row: its corners and the room before its text
/// (logical pixels).
const FIELD_RADIUS: f32 = 5.0;
const FIELD_PADDING: f32 = 6.0;

/// The panel's appearance.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectTreeStyle {
    pub background: Rgba,
    pub foreground: Rgba,
    pub muted: Rgba,
    pub selection: Rgba,
    pub hover: Rgba,
    pub folder: Rgba,
    pub indent_guide: Rgba,
    /// The name field for a new file / directory.
    pub input_background: Rgba,
    pub input_border: Rgba,
    pub error: Rgba,
    pub caret: Rgba,
    pub text_selection: Rgba,
    /// The marks beside a name ([`Mark`]): only here, changed here, changed on the other side
    /// (both sides: `error`).
    pub local: Rgba,
    pub changed: Rgba,
    pub behind: Rgba,
    /// A row's note to look at (a remote file the web server can't read).
    pub warning: Rgba,
    pub font_family: FontFamily,
    /// In logical pixels.
    pub font_size: f32,
    pub row_height: f32,
    pub indent: f32,
}

impl ProjectTreeStyle {
    pub fn from_palette(p: &Palette) -> Self {
        Self {
            background: p.window_background,
            foreground: p.text,
            muted: p.text_muted,
            selection: p.tree_selection,
            hover: p.tree_hover,
            folder: p.folder_icon,
            indent_guide: p.indent_guide,
            input_background: p.input_background,
            input_border: p.accent,
            error: p.danger,
            caret: p.caret,
            text_selection: p.selection,
            local: p.vcs_added,
            changed: p.vcs_modified,
            behind: p.accent,
            warning: p.warning,
            font_family: p.ui_font.clone(),
            font_size: p.ui_font_size,
            row_height: 28.0,
            indent: 16.0,
        }
    }
}

impl Default for ProjectTreeStyle {
    fn default() -> Self {
        Self::from_palette(&Palette::dark())
    }
}

/// What the app must do after a click in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeAction {
    Toggle(NodeId),
    Open(NodeId),
}

/// The rows' marks kept at most: more (a huge tree scrolled through) starts again.
const MARKED_MOST: usize = 4096;

/// A row's mark, its own, found by the modules ([`ProjectTreeView::path_mark`]).
type RowMark = Option<(PathMark, bool, bool)>;

/// The rows' marks worked out, and what they were worked out from: the tree's root and the
/// dependencies' generation (the user's marks changing empties it).
#[derive(Debug, Default)]
struct MarkedRows {
    root: Option<std::path::PathBuf>,
    generation: u64,
    rows: std::collections::HashMap<NodeId, RowMark>,
}

#[derive(Debug)]
pub struct ProjectTreeView {
    /// What the user marked in the project.
    path_marks: std::cell::RefCell<Option<std::sync::Arc<PathMarks>>>,
    /// Each row's mark as worked out ([`Self::path_mark`]): a node's path never changes (an
    /// id is never reused for another entry), so it's kept while the tree, the user's marks
    /// and the dependencies stay.
    marked: std::cell::RefCell<MarkedRows>,
    style: ProjectTreeStyle,
    bounds: Rect,
    scale: f32,
    scroll_y: f32,
    /// Horizontal scroll, for long names in deep directories.
    scroll_x: f32,
    /// The content width of the visible rows (from the last paint), for limiting
    /// the horizontal scroll.
    content_width: std::cell::Cell<f32>,
    selected: Option<NodeId>,
    /// The rows selected besides `selected` (⌘ / Ctrl-click, ⇧-click), and where a ⇧-click's
    /// range starts from.
    also: Vec<NodeId>,
    anchor: Option<NodeId>,
    hovered: Option<NodeId>,
    /// The folder a dragged entry would be dropped into (highlighted).
    drop_target: Option<NodeId>,
    /// The names cut short to fit (`…`), with where they're drawn: the whole name shows on
    /// hover (from the last paint).
    cut: std::cell::RefCell<Vec<(Rect, NodeId)>>,
    /// The marks drawn beside names (a cloud, a mark), for their tooltips (from the last
    /// paint).
    marks: std::cell::RefCell<Vec<(Rect, NodeId)>>,
    /// The rows' notes on their right (their whole text in a tooltip).
    notes: std::cell::RefCell<Vec<(Rect, NodeId)>>,
    /// The new file / directory whose name is typed directly in the tree.
    pending: Option<PendingEntry>,
    /// Its scrollbar, hidden at rest (shown while scrolled or hovered).
    bar: std::cell::RefCell<ui_components::EdgeScrollbar>,
    /// The theme's colors (the scrollbar's).
    palette: Palette,
}

/// An input row in the tree: the name of a new file or directory,
/// under the `parent` directory — or an entry's new name, edited in its own row.
#[derive(Debug)]
pub struct PendingEntry {
    pub parent: NodeId,
    pub is_dir: bool,
    pub input: TextInput,
    /// The name is invalid (or already exists): the border is red.
    pub error: bool,
    /// The entry renamed (its row holds the field); `None`: a new entry.
    pub renaming: Option<NodeId>,
}

impl ProjectTreeView {
    pub fn new(style: ProjectTreeStyle) -> Self {
        Self {
            path_marks: std::cell::RefCell::default(),
            marked: std::cell::RefCell::default(),
            style,
            bounds: Rect::default(),
            scale: 1.0,
            scroll_y: 0.0,
            scroll_x: 0.0,
            content_width: std::cell::Cell::new(0.0),
            selected: None,
            also: Vec::new(),
            anchor: None,
            hovered: None,
            drop_target: None,
            cut: std::cell::RefCell::new(Vec::new()),
            marks: std::cell::RefCell::new(Vec::new()),
            notes: std::cell::RefCell::new(Vec::new()),
            pending: None,
            bar: std::cell::RefCell::new(ui_components::EdgeScrollbar::auto_hiding()),
            palette: Palette::dark(),
        }
    }

    /// The theme's colors (its scrollbar's).
    pub fn set_palette(&mut self, palette: &Palette) {
        self.palette = palette.clone();
    }

    /// A press on its scrollbar (when shown): the rows scrolled there. `true` if it was.
    pub fn bar_press(&mut self, p: Point) -> bool {
        match self.bar.get_mut().press(p) {
            Some(offset) => {
                self.scroll_y = offset.round();
                true
            }
            None => false,
        }
    }

    /// Its scrollbar's thumb dragged. `true` if it is.
    pub fn bar_drag(&mut self, p: Point) -> bool {
        match self.bar.get_mut().drag(p) {
            Some(offset) => {
                self.scroll_y = offset.round();
                true
            }
            None => false,
        }
    }

    pub fn bar_release(&mut self) -> bool {
        self.bar.get_mut().release()
    }

    /// The pointer over its scrollbar's lane (it shows). `true` if it changed.
    pub fn bar_hover(&mut self, p: Point) -> bool {
        self.bar.get_mut().hover(p)
    }

    /// When its scrollbar next needs a frame (fading).
    pub fn next_frame(&self) -> Option<std::time::Instant> {
        self.bar.borrow().next_frame()
    }

    /// Starts creating a file / directory in `parent` (which must be expanded).
    pub fn begin_new(&mut self, parent: NodeId, is_dir: bool, tree: &mut FileTree) {
        self.pending = Some(PendingEntry {
            parent,
            is_dir,
            input: TextInput::default(),
            error: false,
            renaming: None,
        });
        self.reveal(parent, tree);
    }

    /// Starts renaming `id` in its own row: its name in the field, a file's without its
    /// extension selected (a folder's whole). The root has no row to rename.
    pub fn begin_rename(&mut self, id: NodeId, tree: &mut FileTree) -> bool {
        let path = tree.path(id);
        let Some(parent) = path.parent().and_then(|p| tree.find(p)) else { return false };
        let is_dir = tree.kind(id) == NodeKind::Directory;
        let name = tree.name(id);
        let stem = match name.rfind('.') {
            Some(dot) if dot > 0 && !is_dir => dot,
            _ => name.len(),
        };
        let mut input = TextInput::new(name);
        input.select(0..stem);
        self.pending =
            Some(PendingEntry { parent, is_dir, input, error: false, renaming: Some(id) });
        self.reveal(id, tree);
        true
    }

    pub fn pending(&self) -> Option<&PendingEntry> {
        self.pending.as_ref()
    }

    pub fn pending_mut(&mut self) -> Option<&mut PendingEntry> {
        self.pending.as_mut()
    }

    pub fn cancel_new(&mut self) -> Option<PendingEntry> {
        self.pending.take()
    }

    /// The input row's position (display index and depth), if any: a new entry's, inserted
    /// below its folder (a rename's field is in the entry's own row).
    fn pending_slot(&self, tree: &FileTree) -> Option<(usize, u16)> {
        let pending = self.pending.as_ref().filter(|p| p.renaming.is_none())?;
        let rows = tree.rows();
        let index = rows.iter().position(|r| r.id == pending.parent)?;
        Some((index + 1, rows[index].depth + 1))
    }

    pub fn selected(&self) -> Option<NodeId> {
        self.selected
    }

    pub fn set_style(&mut self, style: ProjectTreeStyle) {
        self.style = style;
    }

    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn set_bounds(&mut self, bounds: Rect, scale: f32) {
        self.bounds = bounds;
        self.scale = scale;
    }

    /// Only `id` selected (the others let go).
    pub fn select(&mut self, id: Option<NodeId>) {
        self.selected = id;
        self.also.clear();
        self.anchor = id;
    }

    /// Selects the node and scrolls until it is visible (the row must be displayed).
    pub fn reveal(&mut self, id: NodeId, tree: &mut FileTree) {
        self.select(Some(id));
        let row_h = self.row_height();
        let Some(index) = tree.visible_rows().iter().position(|r| r.id == id) else { return };
        let top = index as f32 * row_h;
        let view = (self.bounds.size.height - self.top_padding()).max(row_h);
        if top < self.scroll_y {
            self.scroll_y = top.round();
        } else if top + row_h > self.scroll_y + view {
            // Centered, so the surrounding context is visible too.
            self.scroll_y = (top - view / 2.0).max(0.0).round();
        }
    }

    pub fn scroll_by_pixels(&mut self, dy: f32, tree: &mut FileTree) -> bool {
        self.scroll_by(0.0, dy, tree)
    }

    /// Scroll on both axes (physical pixels). `true` if it moved.
    pub fn scroll_by(&mut self, dx: f32, dy: f32, tree: &mut FileTree) -> bool {
        let before = (self.scroll_x, self.scroll_y);
        // The rows (with a new entry's input row) between the padding above and as much
        // below: at the end, the last row shows whole, clear of the panel's edge.
        let rows = tree.visible_rows().len() + usize::from(self.pending_slot(tree).is_some());
        let content = rows as f32 * self.row_height() + 2.0 * self.top_padding();
        let max_y = (content - self.bounds.size.height).max(0.0);
        self.scroll_y = (self.scroll_y + dy).clamp(0.0, max_y).round();
        // Long names are cut short (`…`), never scrolled to.
        let _ = dx;
        self.scroll_x = 0.0;
        (self.scroll_x, self.scroll_y) != before
    }

    /// The row under point `p` (physical pixels) and the corresponding action.
    pub fn click(&mut self, p: Point, tree: &mut FileTree) -> Option<TreeAction> {
        let id = self.row_at(p, tree)?;
        self.select(Some(id));
        Some(match tree.kind(id) {
            NodeKind::Directory => TreeAction::Toggle(id),
            NodeKind::File => TreeAction::Open(id),
        })
    }

    /// Whether row `id` is selected (alone or among others).
    pub fn is_selected(&self, id: NodeId) -> bool {
        self.selected == Some(id) || self.also.contains(&id)
    }

    /// The rows selected that are shown, top to bottom (one hidden since — its folder
    /// collapsed, its entry gone — isn't).
    pub fn selection(&self, tree: &FileTree) -> Vec<NodeId> {
        if self.also.is_empty() {
            let shown = |id: &NodeId| tree.rows().iter().any(|r| r.id == *id);
            return self.selected.filter(shown).into_iter().collect();
        }
        tree.rows().iter().map(|r| r.id).filter(|id| self.is_selected(*id)).collect()
    }

    /// A press with ⌘ / Ctrl (`toggle`): the row under `p` added to the selection or taken
    /// out of it; with ⇧ (`range`): the rows from the last one clicked to it selected. Nothing
    /// opens. `false`: no row there.
    pub fn select_more(&mut self, p: Point, tree: &mut FileTree, toggle: bool) -> bool {
        let Some(id) = self.row_at(p, tree) else { return false };
        if toggle {
            if !self.is_selected(id) {
                self.also.extend(self.selected.replace(id));
            } else if self.selected == Some(id) {
                // The lead taken out: the last other one leads.
                self.selected = self.also.pop();
            } else {
                self.also.retain(|a| *a != id);
            }
            self.anchor = Some(id);
            return true;
        }
        let rows = tree.visible_rows();
        let at = |id: Option<NodeId>| id.and_then(|id| rows.iter().position(|r| r.id == id));
        let Some(to) = at(Some(id)) else { return false };
        let from = at(self.anchor.or(self.selected)).unwrap_or(to);
        let (a, b) = (from.min(to), from.max(to));
        self.also = rows[a..=b].iter().map(|r| r.id).filter(|r| *r != id).collect();
        self.selected = Some(id);
        true
    }

    /// Updates the row under the cursor. Returns `true` if a redraw is needed.
    pub fn hover(&mut self, p: Point, tree: &mut FileTree) -> bool {
        let hovered = self.row_at(p, tree);
        let changed = hovered != self.hovered;
        self.hovered = hovered;
        changed
    }

    /// The name under `p` if it was cut short to fit (its whole name is shown then).
    pub fn cut_name_at(&self, p: Point) -> Option<(Rect, NodeId)> {
        self.cut.borrow().iter().find(|(r, _)| r.contains(p)).copied()
    }

    /// The entry whose note (on its row's right) is under `p`.
    pub fn note_at(&self, p: Point) -> Option<(Rect, NodeId)> {
        self.notes.borrow().iter().find(|(r, _)| r.contains(p)).copied()
    }

    /// The entry whose mark (beside its name) is under `p`.
    pub fn mark_at(&self, p: Point) -> Option<(Rect, NodeId)> {
        self.marks.borrow().iter().find(|(r, _)| r.contains(p)).copied()
    }

    /// Highlights the folder a dragged entry would go into. `true` if it changed.
    pub fn set_drop_target(&mut self, id: Option<NodeId>) -> bool {
        std::mem::replace(&mut self.drop_target, id) != id
    }

    /// The row under point `p` (nothing selected).
    pub fn node_at(&self, p: Point, tree: &mut FileTree) -> Option<NodeId> {
        self.row_at(p, tree)
    }

    fn row_at(&self, p: Point, tree: &mut FileTree) -> Option<NodeId> {
        if !self.bounds.contains(p) {
            return None;
        }
        let y = p.y - self.bounds.min_y() - self.top_padding() + self.scroll_y;
        tree.refresh_visible();
        let list = TreeList::new(self.row_height(), tree.rows().len());
        let mut slot = list.slot_at(y)?;
        if let Some((at, _)) = self.pending_slot(tree) {
            if slot == at {
                return None;
            }
            if slot > at {
                slot -= 1;
            }
        }
        // The row renamed is its field's (a click there edits the name).
        let id = tree.rows().get(slot)?.id;
        (self.pending.as_ref().and_then(|p| p.renaming) != Some(id)).then_some(id)
    }

    /// `file_icon(name)` gives a file's icon and color (by language), `folder_color(name)`
    /// a folder's color by what it holds (`None`: the folders' own).
    pub fn paint(
        &self,
        tree: &mut FileTree,
        text: &mut TextSystem,
        scene: &mut Scene,
        icons: &Icons,
        file_icon: &dyn Fn(&str) -> (IconId, Rgba),
        folder_color: &dyn Fn(&str) -> Option<Rgba>,
    ) {
        let b = self.bounds;
        let s = &self.style;
        let px = |v: f32| (v * self.scale).round();
        let row_height = self.row_height();
        let inset = px(8.0);
        let font = Font::new(s.font_family.clone(), px(s.font_size));
        let ctx = ds::Ctx::new(&self.palette, self.scale);
        let frame = Frame {
            scale: self.scale,
            layout: tree_row::EntryLayout::new(self.scale, s.row_height, s.indent),
            metrics: text.metrics(&font),
            font,
            content_clip: Rect::new(
                b.min_x(),
                b.min_y(),
                (b.size.width - inset - px(6.0)).max(0.0),
                b.size.height,
            ),
            icons,
            file_icon,
            folder_color,
        };

        // The pills (selection, hover) span the full width; the content scrolls
        // horizontally in a separate layer, clipped before the pills' edge.
        tree.refresh_visible();
        let pending_slot = self.pending_slot(tree);
        // The rows below the input row move down by one row.
        let shift = |r: usize| r + usize::from(pending_slot.is_some_and(|(at, _)| r >= at));
        let rows = tree.rows();
        let range = TreeList::new(row_height, rows.len()).visible(self.scroll_y, b.size.height);
        let first = range.start;
        let visible = &rows[range];
        let origin_y = b.min_y() + self.top_padding() - self.scroll_y;
        let top_of = |i: usize| origin_y + shift(first + i) as f32 * row_height;
        let pill_at = |i: usize| frame.layout.pill(b, top_of(i));

        self.cut.borrow_mut().clear();
        self.marks.borrow_mut().clear();
        self.notes.borrow_mut().clear();
        let pills = scene.push_layer(Some(b));
        pills.quad(Quad::filled(b, s.background));
        for (i, row) in visible.iter().enumerate() {
            if self.drop_target == Some(row.id) {
                ds::RowHighlight::new(true, false)
                    .selected_fill(s.selection)
                    .radius(DROP_RADIUS)
                    .outline(s.input_border)
                    .outline_width(DROP_OUTLINE)
                    .paint(pills, &ctx, pill_at(i));
            } else if self.is_selected(row.id) {
                tree_row::highlight(pills, pill_at(i), s.selection, frame.layout.pill_radius());
            } else if self.hovered == Some(row.id) {
                tree_row::highlight(pills, pill_at(i), s.hover, frame.layout.pill_radius());
            }
        }
        let layer = scene.push_layer(Some(frame.content_clip));
        let mut content_width: f32 = 0.0;
        let renaming = self.pending.as_ref().and_then(|p| p.renaming);
        let mut renamed_row = None;
        for (i, row) in visible.iter().enumerate() {
            // The row renamed: its field instead of its name.
            if renaming == Some(row.id) {
                renamed_row = Some((top_of(i), row.depth));
                continue;
            }
            let item = tree_row::Item {
                name: tree.name(row.id),
                bounds: Rect::new(b.min_x(), top_of(i), b.size.width, row_height),
                level: usize::from(row.depth),
                expanded: (tree.kind(row.id) == NodeKind::Directory)
                    .then(|| tree.is_expanded(row.id)),
                selected: self.is_selected(row.id),
            };
            let told = tree_row::item(item);
            let right = self.paint_row(layer, text, tree, &frame, row, top_of(i));
            drop(told);
            content_width = content_width.max(right);
        }
        self.content_width.set(content_width);

        if let (Some(pending), Some((at, depth))) = (&self.pending, pending_slot) {
            let top = origin_y + at as f32 * row_height;
            self.paint_pending(scene, text, &frame, pending, top, depth);
        }
        if let (Some(pending), Some((top, depth))) = (&self.pending, renamed_row) {
            self.paint_pending(scene, text, &frame, pending, top, depth);
        }
        // The scrollbar, over the rows' right padding: nothing at rest.
        let content = rows.len() as f32 * row_height + 2.0 * self.top_padding();
        self.bar.borrow_mut().paint(scene, &self.palette, self.scale, b, content, self.scroll_y);
    }

    /// A row of the tree at `top`: its indentation guides, its chevron and icon, its name
    /// (cut short to fit) and its mark; returns how wide the content is up to its name's
    /// end (for the horizontal scroll).
    /// What the user marked the project as (`None`: nothing).
    pub fn set_path_marks(&self, marks: Option<std::sync::Arc<PathMarks>>) {
        let mut current = self.path_marks.borrow_mut();
        let same = match (current.as_ref(), marks.as_ref()) {
            (Some(was), Some(now)) => std::sync::Arc::ptr_eq(was, now),
            (was, now) => was.is_none() && now.is_none(),
        };
        if !same {
            *current = marks;
            self.marked.borrow_mut().rows.clear();
        }
    }

    /// `id`'s mark, and whether it's its own (not a parent's): the user's, else a dependency
    /// folder's (`vendor`, `node_modules`, Composer's `vendor-dir`: a library, found by the
    /// modules, plan 74g) — the third says it's that one.
    fn path_mark(&self, tree: &FileTree, id: NodeId) -> Option<(PathMark, bool, bool)> {
        // The user's marks changing empties it (`set_path_marks`).
        let generation = kernel::paths::generation();
        let mut marked = self.marked.borrow_mut();
        let same =
            marked.root.as_deref() == Some(tree.root_path()) && marked.generation == generation;
        if !same || marked.rows.len() >= MARKED_MOST {
            marked.rows.clear();
            if marked.root.as_deref() != Some(tree.root_path()) {
                marked.root = Some(tree.root_path().to_path_buf());
            }
            marked.generation = generation;
        }
        *marked.rows.entry(id).or_insert_with(|| self.work_out_mark(tree, id))
    }

    /// [`Self::path_mark`] worked out (the path built, the marks and dependencies asked).
    fn work_out_mark(&self, tree: &FileTree, id: NodeId) -> Option<(PathMark, bool, bool)> {
        let path = tree.path(id);
        let relative =
            path.strip_prefix(tree.root_path()).ok()?.to_string_lossy().replace('\\', "/");
        let marks = self.path_marks.borrow();
        if let Some(marks) = marks.as_ref()
            && let Some(mark) = marks.of(&relative)
        {
            return Some((mark, marks.own(&relative).is_some(), false));
        }
        let own = kernel::paths::dependency_at(tree.root_path(), std::path::Path::new(&relative))?;
        Some((PathMark::Library, own, true))
    }

    fn paint_row(
        &self,
        layer: &mut Layer,
        text: &mut TextSystem,
        tree: &FileTree,
        f: &Frame<'_>,
        row: &VisibleRow,
        top: f32,
    ) -> f32 {
        let b = self.bounds;
        let s = &self.style;
        let (row_height, padding) = (f.layout.row_height, f.layout.padding);
        let px = |v: f32| f.px(v);
        // Indentation guides: one vertical line for each parent level.
        let left = b.min_x() - self.scroll_x;
        f.layout.guides(layer, left, usize::from(row.depth), top, s.indent_guide);

        let baseline = centered_baseline(top, row_height, &f.metrics);
        let x = f.layout.x(left, usize::from(row.depth));
        // Not on disk yet (a remote project's, brought when opened): dimmed, a cloud after.
        let absent = tree.is_absent(row.id);
        let mark = mark_icon(s, tree.mark(row.id), absent);
        // What the user marked it as: an excluded entry stays, faint.
        let marked = self.path_mark(tree, row.id);
        let excluded = marked.is_some_and(|(mark, ..)| mark == PathMark::Excluded);
        let tint = marked.and_then(|(mark, ..)| mark_tint(s, mark));
        let icon_rect =
            self.paint_row_icon(layer, tree, f, row.id, (x, top), (absent || excluded, tint));
        let label_x = f.layout.label_x(icon_rect);
        // On the right: what a module says of it (muted, small), when the name keeps room.
        let mut right = f.content_clip.max_x() - px(6.0);
        let tag = marked.filter(|(_, own, _)| *own).and_then(|(mark, _, found)| match found {
            // Found by the modules, not marked: said so.
            true => Some("marks.tag_library_found"),
            false => mark_tag(mark),
        });
        if let Some(tag) = tag {
            let mut small = f.font.clone();
            small.size *= 0.86;
            let shaped = text.shape_line(&i18n::t(tag), &small);
            let at = right - shaped.width;
            if at - label_x > px(90.0) {
                draw_text(layer, &shaped, at, baseline, tint.unwrap_or(s.muted));
                right = at - px(10.0);
            }
        }
        if let Some(note) = tree.note(row.id) {
            let mut small = f.font.clone();
            small.size *= 0.86;
            let shaped = text.shape_line(&note.text, &small);
            let at = right - shaped.width;
            if at - label_x > px(90.0) {
                let tint = if note.warn { s.warning } else { s.muted };
                draw_text(layer, &shaped, at, baseline, tint);
                self.notes
                    .borrow_mut()
                    .push((Rect::new(at, top, shaped.width, row_height), row.id));
                right = at - px(10.0);
            }
        }
        // As much of the name as fits; the whole name on hover.
        let name = tree.name(row.id);
        let mark_room = if mark.is_some() { px(18.0) } else { 0.0 };
        let room = (right - label_x - mark_room).max(0.0);
        let fitted = ui_components::ds::ellipsize_file(text, name, &f.font, room);
        if fitted != name {
            let shown = Rect::new(label_x, top, room, row_height);
            self.cut.borrow_mut().push((shown, row.id));
        }
        let shaped = text.shape_line(&fitted, &f.font);
        let selected = self.is_selected(row.id);
        let color =
            if selected || row.depth == 0 { s.foreground } else { s.foreground.with_alpha(0.86) };
        let color = match tree.mark(row.id) {
            _ if absent || excluded => s.muted,
            Mark::None => color,
            Mark::Local => s.local,
            Mark::Changed => s.changed,
            Mark::Behind => color,
            Mark::Conflict => s.error,
        };
        draw_text(layer, &shaped, label_x, baseline, color);
        if let Some((name, tint)) = mark
            && let Some(id) = f.icons.id(name)
        {
            let size = px(12.0);
            let center = (top + (row_height - size) / 2.0).round();
            let at = Rect::new((label_x + shaped.width + px(6.0)).round(), center, size, size);
            draw_icon(layer, f.icons, id, at, tint);
            let hover = Rect::new(at.min_x() - px(3.0), top, size + px(6.0), row_height);
            self.marks.borrow_mut().push((hover, row.id));
        }
        label_x + shaped.width + self.scroll_x - b.min_x() + padding
    }

    /// A row's chevron (a directory's) and icon (dimmed when not on disk: `absent`), starting
    /// at `x`; returns where the icon is.
    fn paint_row_icon(
        &self,
        layer: &mut Layer,
        tree: &FileTree,
        f: &Frame<'_>,
        id: NodeId,
        (x, top): (f32, f32),
        (absent, tint): (bool, Option<Rgba>),
    ) -> Rect {
        let s = &self.style;
        // A marked folder shows its mark's color whole; so it's the plain folder, not a
        // set's picture for its name (those keep their own colors).
        let marked_folder = tree.kind(id) == NodeKind::Directory && tint.is_some();
        let faint = absent && !marked_folder;
        let dim = |c: Rgba| if faint { c.with_alpha(c.a * 0.5) } else { c };
        let entry = if tree.kind(id) == NodeKind::Directory {
            let name = if marked_folder { "" } else { tree.name(id) };
            // Ours: colored by what it holds (sources, tests, libraries…).
            let color = tint.or_else(|| (f.folder_color)(name)).unwrap_or(s.folder);
            tree_row::EntryIcon::Folder { name, expanded: tree.is_expanded(id), color }
        } else {
            let (icon, color) = (f.file_icon)(tree.name(id));
            tree_row::EntryIcon::File { icon, color }
        };
        tree_row::entry_icon(layer, f.icons, &f.layout, (x, top), entry, (s.muted, &dim))
    }

    /// The input row of a new file / directory at `top`: its icon and the name field.
    fn paint_pending(
        &self,
        scene: &mut Scene,
        text: &mut TextSystem,
        f: &Frame<'_>,
        pending: &PendingEntry,
        top: f32,
        depth: u16,
    ) {
        let b = self.bounds;
        let s = &self.style;
        let px = |v: f32| f.px(v);
        let (row_height, content_clip) = (f.layout.row_height, f.content_clip);
        let x = f.layout.x(b.min_x() - self.scroll_x, usize::from(depth));
        let icon = px(16.0);
        let chevron = px(12.0);
        let icon_rect = Rect::new(
            (x + chevron + px(4.0)).round(),
            (top + (row_height - icon) / 2.0).round(),
            icon,
            icon,
        );
        let name = if pending.is_dir { "folder" } else { "file" };
        let layer = scene.push_layer(Some(content_clip));
        if let Some(id) = f.icons.id(name) {
            draw_icon(
                layer,
                f.icons,
                id,
                icon_rect,
                if pending.is_dir { s.folder } else { s.muted },
            );
        }
        let field = Rect::new(
            icon_rect.max_x() + px(5.0),
            top + px(3.0),
            (content_clip.max_x() - icon_rect.max_x() - px(10.0)).max(px(40.0)),
            row_height - px(6.0),
        );
        // The shared field, fitted to a row: smaller corners, less room before the text.
        let ctx = ds::Ctx::new(&self.palette, self.scale);
        let style = TextInputStyle {
            radius: px(FIELD_RADIUS),
            padding: px(FIELD_PADDING),
            ..ds::field_style(&ctx, f.font.clone(), pending.error)
        };
        let placeholder =
            i18n::t(if pending.is_dir { "files.new_folder" } else { "files.new_file" });
        let layer = scene.push_layer(Some(field));
        pending.input.paint(layer, text, field, true, &placeholder, &style);
    }

    fn top_padding(&self) -> f32 {
        (6.0 * self.scale).round()
    }

    fn row_height(&self) -> f32 {
        (self.style.row_height * self.scale).round()
    }
}

/// The tree's geometry, font and icons for one paint, shared by its parts.
struct Frame<'a> {
    scale: f32,
    /// Where a row's parts go (the files' trees share it).
    layout: tree_row::EntryLayout,
    font: Font,
    metrics: FontMetrics,
    /// The content's clip: before the pills' edge.
    content_clip: Rect,
    icons: &'a Icons,
    file_icon: &'a dyn Fn(&str) -> (IconId, Rgba),
    folder_color: &'a dyn Fn(&str) -> Option<Rgba>,
}

impl Frame<'_> {
    /// Logical pixels to whole physical ones.
    fn px(&self, v: f32) -> f32 {
        (v * self.scale).round()
    }
}

/// The folder's color for a mark (`None`: as any other).
fn mark_tint(s: &ProjectTreeStyle, mark: PathMark) -> Option<Rgba> {
    match mark {
        PathMark::Excluded => Some(s.warning),
        PathMark::Library => Some(s.folder.with_alpha(s.folder.a * 0.55)),
        PathMark::Generated => Some(s.muted),
        PathMark::Normal => None,
    }
}

/// The key of the words at the end of a marked entry's row.
fn mark_tag(mark: PathMark) -> Option<&'static str> {
    match mark {
        PathMark::Excluded => Some("marks.tag_excluded"),
        PathMark::Library => Some("marks.tag_library"),
        PathMark::Generated => Some("marks.tag_generated"),
        PathMark::Normal => None,
    }
}

/// Where a remote project's copy stands: the icon after the name and its tint.
fn mark_icon(s: &ProjectTreeStyle, mark: Mark, absent: bool) -> Option<(&'static str, Rgba)> {
    match mark {
        _ if absent => Some(("cloud", s.muted.with_alpha(0.7))),
        Mark::None => None,
        Mark::Local => Some(("plus", s.local)),
        Mark::Changed => Some(("arrow-up", s.changed)),
        Mark::Behind => Some(("arrow-down", s.behind)),
        Mark::Conflict => Some(("alert", s.error)),
    }
}

#[cfg(test)]
#[path = "../tests/unit/tree_view.rs"]
mod tests;
