//! The open projects' list (a modal of the module's): a field filtering them by name, path
//! or number, and their cards in a grid — each a picture of its window (taken by the app,
//! [`workbench_model::pictures`]; until there's one, its color and initials), its name,
//! where it is, its shortcut; this window's marked. The
//! arrows or the mouse choose, ↵ or a click brings its window to the front (and closes the
//! list), Esc closes it.

use std::path::Path;

use kernel::{Point, Rect};
use render_api::{ImageData, Quad};
use text_layout::Font;
use ui_components::control::{Control, Controls};
use ui_components::ds::{self, Ctx, radius};
use ui_components::{
    AppRequest, KeyReach, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind, ViewerPaint,
    ViewerProvider,
};
use ui_kit::{TextInput, draw_text};
use workbench_model::ProjectWindow;

/// The extension its view is opened by.
pub const EXTENSION: &str = "view-open-projects";

/// What the list is opened with: the windows' projects (this one's marked).
#[derive(Debug, Clone)]
pub struct SwitcherData {
    pub projects: Vec<ProjectWindow>,
}

#[derive(Debug)]
pub(crate) struct Provider;

impl ViewerProvider for Provider {
    fn id(&self) -> &'static str {
        "open-projects"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(SwitcherView::default()))
    }
}

/// Sizes, logical pixels.
const WIDTH: f32 = 820.0;
const PAD: f32 = 24.0;
const FIELD_H: f32 = 40.0;
const GAP: f32 = 16.0;
const COLUMNS: usize = 3;
/// A card's picture's height to its width, and the text under it.
const PICTURE: f32 = 0.56;
const CAPTION_H: f32 = 58.0;
/// The rows shown at once (more scroll).
const ROWS: usize = 2;

#[derive(Debug, Default)]
pub struct SwitcherView {
    projects: Vec<ProjectWindow>,
    filter: TextInput,
    /// Its controls' state of use (see [`SwitcherView::with_controls`]).
    controls: Controls,
    /// The projects the filter keeps (their places), and the one chosen among them.
    shown: Vec<usize>,
    selected: usize,
    /// The first row in view.
    first_row: usize,
    cards: Vec<(Rect, usize)>,
    hover: Option<usize>,
    action: Option<ViewerAction>,
    /// The windows' pictures as images (each window's, and when it was taken): made once
    /// per picture, their textures given back when replaced or when the list closes.
    pictures: Vec<(u64, u64, ImageData)>,
}

impl Drop for SwitcherView {
    fn drop(&mut self) {
        for (_, _, image) in &self.pictures {
            image.release();
        }
    }
}

/// The filter as the list's control: by the keyboard only (no caret is put with the
/// mouse), it takes what's typed (control characters left out) and erases a character at a
/// time (⌫, Delete), ⌘A selecting it; the other keys (Tab among them) choose the cards, so
/// it keeps Tab and leaves them. `edited`: its text changed (the list is filtered again).
struct FilterPart<'a> {
    filter: &'a mut TextInput,
    edited: &'a mut bool,
}

impl Control for FilterPart<'_> {
    fn focusable(&self) -> bool {
        true
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, _reach: KeyReach) -> bool {
        match key {
            ViewerKey::Backspace => self.filter.backspace(false),
            ViewerKey::Delete => self.filter.delete(),
            _ => return false,
        }
        *self.edited = true;
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        if typed.is_empty() {
            return false;
        }
        self.filter.insert(&typed);
        *self.edited = true;
        true
    }

    fn selected_text(&self) -> Option<String> {
        let selected = self.filter.selected_text();
        (!selected.is_empty()).then(|| selected.to_owned())
    }

    fn command(&mut self, id: &str) -> bool {
        if id != "select_all" {
            return false;
        }
        self.filter.select_all();
        true
    }
}

/// The filter's place among the list's controls.
const FILTER: usize = 0;

impl SwitcherView {
    /// Its controls (the filter), with their state of use; the list is filtered again when
    /// the filter's text changed.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut edited = false;
        let mut filter = FilterPart { filter: &mut self.filter, edited: &mut edited };
        let mut parts: [&mut dyn Control; 1] = [&mut filter];
        let answer = f(&mut self.controls, &mut parts);
        if edited {
            self.refilter();
        }
        answer
    }

    /// The projects the filter keeps: by name, path or number; when nothing's typed, this
    /// window's chosen (the frame where you are; the arrows go to the others).
    fn refilter(&mut self) {
        let typed = self.filter.text().trim().to_lowercase();
        self.shown = (0..self.projects.len())
            .filter(|&i| {
                let p = &self.projects[i];
                typed.is_empty()
                    || p.name.to_lowercase().contains(&typed)
                    || p.path.to_string_lossy().to_lowercase().contains(&typed)
                    || p.number.is_some_and(|n| n.to_string() == typed)
            })
            .collect();
        let current = self.shown.iter().position(|&i| self.projects[i].current);
        self.selected = match current {
            Some(at) if typed.is_empty() => at,
            _ => 0,
        };
        self.first_row = 0;
    }

    /// Each project's picture as the app keeps it, the images made for new ones; `true`:
    /// one changed (drawn again).
    fn sync_pictures(&mut self) -> bool {
        let mut changed = false;
        for project in &self.projects {
            let Some(picture) = workbench_model::pictures::of(project.window) else { continue };
            let made = self.pictures.iter().position(|(w, ..)| *w == project.window);
            if made.is_some_and(|at| self.pictures[at].1 == picture.taken) {
                continue;
            }
            let image = ImageData::gpu_only(picture.width, picture.height, picture.rgba.to_vec());
            match made {
                Some(at) => {
                    self.pictures[at].2.release();
                    self.pictures[at] = (project.window, picture.taken, image);
                }
                None => self.pictures.push((project.window, picture.taken, image)),
            }
            changed = true;
        }
        changed
    }

    /// The image of window `window`'s picture, once there is one.
    fn picture(&self, window: u64) -> Option<&ImageData> {
        self.pictures.iter().find(|(w, ..)| *w == window).map(|(.., image)| image)
    }

    /// The chosen card's project.
    pub fn chosen(&self) -> Option<&ProjectWindow> {
        self.shown.get(self.selected).map(|&i| &self.projects[i])
    }

    /// The projects shown, in order.
    pub fn shown(&self) -> Vec<&ProjectWindow> {
        self.shown.iter().map(|&i| &self.projects[i]).collect()
    }

    /// The chosen one's window to the front (this window's: the list just closes).
    fn take_chosen(&mut self) {
        self.action = Some(match self.chosen() {
            Some(p) if !p.current => ViewerAction::Request(AppRequest::FocusProject(p.window)),
            _ => ViewerAction::Close,
        });
    }

    /// The choice moved by `delta` cards (wrapping), its row brought into view.
    fn step(&mut self, delta: isize) {
        let len = self.shown.len();
        if len == 0 {
            return;
        }
        self.selected = ui_kit::step_index(self.selected, delta, len);
        let row = self.selected / COLUMNS;
        if row < self.first_row {
            self.first_row = row;
        } else if row >= self.first_row + ROWS {
            self.first_row = row + 1 - ROWS;
        }
    }

    /// The rows the cards take.
    fn rows(&self) -> usize {
        self.shown.len().div_ceil(COLUMNS).clamp(1, ROWS)
    }

    /// A card's height for a width of `card_w`.
    fn card_height(card_w: f32) -> f32 {
        (card_w * PICTURE).round() + CAPTION_H
    }

    fn paint_card(
        &self,
        layer: &mut render_api::Layer,
        text: &mut text_layout::TextSystem,
        ds: &Ctx<'_>,
        r: Rect,
        project: &ProjectWindow,
        lit: (bool, bool),
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let (selected, hovered) = lit;
        let fill = if hovered { p.text.with_alpha(0.06) } else { p.text.with_alpha(0.03) };
        let (border, width) =
            if selected { (p.accent, px(2.0)) } else { (p.card_border, ds.line()) };
        layer.quad(Quad::filled(r, fill).rounded(px(radius::LG)).bordered(width, border));
        // Its picture: its window's, whole, on its color; none yet: its initials.
        let inset = px(8.0);
        let picture_h = ((r.size.width) * PICTURE).round() - inset;
        let pic =
            Rect::new(r.min_x() + inset, r.min_y() + inset, r.size.width - 2.0 * inset, picture_h);
        let color = ui_components::tag_colors::project(&project.name);
        layer.quad(Quad::filled(pic, color.with_alpha(0.16)).rounded(px(radius::MD)));
        if let Some(image) = self.picture(project.window) {
            layer.image(image, fitted(pic, image.width, image.height));
        } else {
            let big = Font::new(p.ui_font.clone(), px(28.0)).with_weight(Font::SEMIBOLD);
            let initials = text.shape_line(&ds::initials(&project.name), &big);
            let base = ds::optical_baseline(pic.min_y(), pic.size.height, &big);
            draw_text(
                layer,
                &initials,
                (pic.min_x() + (pic.size.width - initials.width) / 2.0).round(),
                base,
                color,
            );
        }
        // Its shortcut, in the picture's corner; this window's said.
        let right = pic.max_x() - px(8.0);
        let mid = pic.min_y() + px(18.0);
        if let Some(keys) = project.number.and_then(crate::shortcut_of) {
            ds::keycap(layer, text, ds, &keys, right, mid, false);
        }
        if project.current {
            let label = i18n::t("projects.this_window");
            let small = ds.ui_font(-1.5);
            let shaped = text.shape_line(&label, &small);
            let pill =
                Rect::new(pic.min_x() + px(8.0), mid - px(10.0), shaped.width + px(14.0), px(20.0));
            layer.quad(Quad::filled(pill, p.accent.with_alpha(0.18)).rounded(px(10.0)));
            draw_text(
                layer,
                &shaped,
                pill.min_x() + px(7.0),
                ds::optical_baseline(pill.min_y(), pill.size.height, &small),
                p.accent,
            );
        }
        // Its name and where it is.
        let room = r.size.width - 2.0 * px(12.0);
        let x = r.min_x() + px(12.0);
        let name_font = ds.ui_font(0.0).with_weight(Font::SEMIBOLD);
        let name = ds::ellipsize_file(text, &project.name, &name_font, room);
        let shaped = text.shape_line(&name, &name_font);
        let top = pic.max_y() + px(8.0);
        draw_text(layer, &shaped, x, ds::optical_baseline(top, px(22.0), &name_font), p.text);
        let small = ds.ui_font(-1.0);
        let path = home_relative(&project.path);
        let path = ds::ellipsize_path(text, &path, &small, room);
        let shaped = text.shape_line(&path, &small);
        draw_text(
            layer,
            &shaped,
            x,
            ds::optical_baseline(top + px(22.0), px(20.0), &small),
            p.text_muted,
        );
    }
}

/// A picture of `width` × `height` pixels made as large as fits in `area`, centered (the
/// whole window seen), on whole pixels.
fn fitted(area: Rect, width: u32, height: u32) -> Rect {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let scale = (area.size.width / w).min(area.size.height / h);
    let (fit_w, fit_h) = ((w * scale).round(), (h * scale).round());
    Rect::new(
        (area.min_x() + (area.size.width - fit_w) / 2.0).round(),
        (area.min_y() + (area.size.height - fit_h) / 2.0).round(),
        fit_w,
        fit_h,
    )
}

/// `path` with the home folder as `~`.
fn home_relative(path: &Path) -> String {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    match home.and_then(|h| path.strip_prefix(h).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~/{}", rest.to_string_lossy()),
        None => path.to_string_lossy().into_owned(),
    }
}

impl Viewer for SwitcherView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(data) = data.downcast_ref::<SwitcherData>() {
            self.projects = data.projects.clone();
            self.refilter();
            self.sync_pictures();
            // The keyboard in the filter from the start.
            self.with_controls(|c, parts| c.focus(parts, Some(FILTER)));
        }
    }

    fn subtitle(&self) -> Option<String> {
        Some(i18n::t("projects.switcher_subtitle"))
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some("folder")
    }

    fn key_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        vec![
            (vec!["↑", "↓", "←", "→", "⇥"], i18n::t("projects.hint_choose")),
            (vec!["↵"], i18n::t("projects.hint_switch")),
        ]
    }

    fn modal_width(&self) -> Option<f32> {
        Some(WIDTH)
    }

    fn modal_height(&self) -> Option<f32> {
        let card_w = (WIDTH - 2.0 * PAD - GAP * (COLUMNS as f32 - 1.0)) / COLUMNS as f32;
        let rows = self.rows() as f32;
        Some(PAD + FIELD_H + GAP + rows * Self::card_height(card_w) + (rows - 1.0) * GAP + PAD)
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let b = ctx.bounds;
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let px = |v: f32| ds.px(v);
        self.cards.clear();
        let ViewerPaint { text, scene, icons, .. } = ctx;
        let layer = scene.push_layer(Some(b));
        let field = Rect::new(
            b.min_x() + px(PAD),
            b.min_y() + px(PAD),
            b.size.width - 2.0 * px(PAD),
            px(FIELD_H),
        );
        let placeholder = i18n::t("projects.filter");
        ds::search_input(
            &mut ui_components::LayerPainter::new(layer, text, icons, ds),
            field,
            &self.filter,
            true,
            &placeholder,
        );
        let top = field.max_y() + px(GAP);
        if self.shown.is_empty() {
            let area = Rect::new(b.min_x(), top, b.size.width, b.max_y() - top);
            let none = if self.projects.len() < 2 { "projects.only_this" } else { "projects.none" };
            ds::empty_state(layer, text, &ds, area, &i18n::t(none), None);
            return;
        }
        let card_w =
            ((field.size.width - px(GAP) * (COLUMNS as f32 - 1.0)) / COLUMNS as f32).floor();
        let card_h = Self::card_height(card_w / ds.scale) * ds.scale;
        let first = self.first_row * COLUMNS;
        let visible: Vec<(usize, usize)> =
            self.shown.iter().copied().enumerate().skip(first).take(ROWS * COLUMNS).collect();
        for (at, i) in visible {
            let slot = at - first;
            let (row, column) = (slot / COLUMNS, slot % COLUMNS);
            let r = Rect::new(
                (field.min_x() + column as f32 * (card_w + px(GAP))).round(),
                (top + row as f32 * (card_h + px(GAP))).round(),
                card_w,
                card_h,
            );
            let lit = (at == self.selected, self.hover == Some(at));
            let project = self.projects[i].clone();
            self.paint_card(layer, text, &ds, r, &project, lit);
            self.cards.push((r, at));
        }
    }

    fn text_input(&mut self, text: &str) -> bool {
        self.with_controls(|c, parts| c.text(parts, text)).is_some()
    }

    /// Tab goes to the next card as → does, ⇧Tab to the one before as ←.
    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        match key {
            ViewerKey::Tab => {
                self.step(if reach.extend { -1 } else { 1 });
                true
            }
            _ => self.edit_key(key),
        }
    }

    /// The filter's keys first (erasing), then the cards'.
    fn edit_key(&mut self, key: ViewerKey) -> bool {
        if self.with_controls(|c, parts| c.key(parts, key, KeyReach::default())).is_some() {
            return true;
        }
        match key {
            ViewerKey::Escape => self.action = Some(ViewerAction::Close),
            ViewerKey::Enter => self.take_chosen(),
            ViewerKey::Left => self.step(-1),
            ViewerKey::Right => self.step(1),
            ViewerKey::Up => self.step(-(COLUMNS as isize)),
            ViewerKey::Down => self.step(COLUMNS as isize),
            _ => return false,
        }
        true
    }

    fn command(&mut self, id: &str) -> bool {
        self.with_controls(|c, parts| c.command(parts, id)).is_some()
    }

    /// Read from `&self`: the filter lends its field by its place.
    fn selected_text(&self) -> Option<String> {
        self.controls.selected_text_with(|i| (i == FILTER).then_some(&self.filter as &dyn Control))
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        let (_, at) = *self.cards.iter().find(|(r, _)| r.contains(p))?;
        self.selected = at;
        self.take_chosen();
        self.action.take()
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.cards.iter().find(|(r, _)| r.contains(p)).map(|(_, at)| *at);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.cards.iter().any(|(r, _)| r.contains(p)).then_some(ViewerCursor::Pointer)
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        let rows = self.shown.len().div_ceil(COLUMNS);
        let before = self.first_row;
        if dy > 0.0 && self.first_row + ROWS < rows {
            self.first_row += 1;
        } else if dy < 0.0 && self.first_row > 0 {
            self.first_row -= 1;
        }
        self.first_row != before
    }

    fn poll(&mut self) -> bool {
        // A picture taken since (this window's, asked for as the list opened).
        self.sync_pictures()
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

#[cfg(test)]
#[path = "../tests/unit/view.rs"]
mod tests;
