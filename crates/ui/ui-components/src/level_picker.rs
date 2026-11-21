//! Choosing the language version (e.g. PHP 8.3) from the status bar: a popover above
//! the item, with the scope (for the project / only for the active file), a filter and
//! the versions as pills (Auto / project's first). Keyboard navigation: ← → ↑ ↓, ↵, esc.
//! A module shows it as its popover ([`LEVEL_PICKER_EXTENSION`]); the choice goes back to
//! the app as [`AppRequest::SetLevel`](crate::AppRequest::SetLevel).

use std::time::Duration;

use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_kit::{KeyHint, draw_text};

use crate::anim::Tween;
use crate::control::{Control, Press};
use crate::ds::{self, Ctx, Elevation, radius};
use crate::search_field::SearchField;
use crate::{KeyReach, Painter, ViewerCursor, ViewerKey};

/// The extension of the picker's popover view (`open_overlay`, given [`LevelChoices`]).
pub const LEVEL_PICKER_EXTENSION: &str = "view-level-picker";

/// The scope's segmented control: its height (logical pixels).
const SCOPE_H: f32 = 34.0;

/// What the popover shows, and the status bar item it opens above.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelChoices {
    /// The item it opens above.
    pub anchor: Rect,
    /// The language name (e.g. `PHP`).
    pub name: String,
    /// The versions, from oldest to newest.
    pub levels: Vec<String>,
    /// The project's level and whether it is chosen automatically.
    pub project: String,
    pub auto: bool,
    /// The detected version and where from (e.g. `composer.json`).
    pub detected: Option<String>,
    pub source: Option<String>,
    /// The active file and its own level, if it has one.
    pub file_name: Option<String>,
    pub file: Option<String>,
}

/// The scope of the choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelScope {
    Project,
    File,
}

/// What was chosen: for the project (`None` = automatic) or only for the file (`None` = same
/// as the project).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LevelPick {
    Project(Option<String>),
    File(Option<String>),
}

/// What a click on the picker did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LevelClick {
    /// A level was chosen.
    Pick(LevelPick),
    /// Inside the picker, nothing chosen (the scope, the search field's ×, empty space).
    Inside,
    /// Outside the picker (it closes).
    Outside,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Scope(LevelScope),
    Chip(usize),
}

#[derive(Debug, Default)]
pub struct LevelPicker {
    open: bool,
    anchor: Rect,
    viewport: Rect,
    scale: f32,
    scope: Option<LevelScope>,
    /// Its filter (the app's search field: ×, ⌘A; the popover's one field, the caret its).
    pub query: SearchField,
    selected: usize,
    /// The pills from the last paint: (area, level; `None` = Auto / same as the project).
    chips: Vec<(Rect, Option<String>)>,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    panel: Rect,
    columns: usize,
    appear: Option<Tween>,
}

impl LevelPicker {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the popover above its item (from the status bar).
    pub fn open(&mut self, choices: &LevelChoices) {
        self.open = true;
        self.anchor = choices.anchor;
        self.query.set_text("");
        self.scope =
            Some(if choices.file.is_some() { LevelScope::File } else { LevelScope::Project });
        self.selected = 0;
        let mut appear = Tween::new(0.0, Duration::from_millis(130));
        appear.retarget(1.0);
        self.appear = Some(appear);
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn set_viewport(&mut self, viewport: Rect, scale: f32) {
        self.viewport = viewport;
        self.scale = scale;
    }

    pub fn is_animating(&self) -> bool {
        self.appear.is_some_and(|a| a.animating())
    }

    pub fn contains(&self, p: Point) -> bool {
        self.open && self.panel.contains(p)
    }

    fn scope(&self) -> LevelScope {
        self.scope.unwrap_or(LevelScope::Project)
    }

    fn pick(&self, i: usize) -> Option<LevelPick> {
        let (_, level) = self.chips.get(i)?;
        Some(match self.scope() {
            LevelScope::Project => LevelPick::Project(level.clone()),
            LevelScope::File => LevelPick::File(level.clone()),
        })
    }

    /// Click: a pill (the choice), the scope, ×; outside, the picker closes.
    pub fn click(&mut self, p: Point) -> LevelClick {
        if !self.panel.contains(p) {
            return LevelClick::Outside;
        }
        match self.hits.iter().find(|(r, _)| r.contains(p)).map(|(_, h)| *h) {
            Some(Hit::Scope(scope)) => {
                self.scope = Some(scope);
                self.selected = 0;
                LevelClick::Inside
            }
            Some(Hit::Chip(i)) => self.pick(i).map_or(LevelClick::Inside, LevelClick::Pick),
            None => {
                self.press_field(p, 1);
                LevelClick::Inside
            }
        }
    }

    /// A press on the filter (`clicks`-th of a series): × empties it, else the caret put
    /// (two a word, three all of it). `true` if it was on the field.
    pub fn press_field(&mut self, p: Point, clicks: u32) -> bool {
        if !self.query.hit(p) {
            return false;
        }
        let press = Control::press(&mut self.query, p, clicks);
        if press == Press::Took && self.query.take_cleared() {
            self.selected = 0;
        }
        true
    }

    /// The mouse dragged after a press on the filter: what it covers selected.
    pub fn drag(&mut self, p: Point) -> bool {
        self.open && Control::drag(&mut self.query, p)
    }

    /// One of the filter's editing keys (Backspace, Delete, Home, End; ← → are the pills',
    /// the view chooses): `true` if the field used it.
    pub fn field_key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let taken = Control::key(&mut self.query, key, reach);
        if taken && matches!(key, ViewerKey::Backspace | ViewerKey::Delete) {
            self.selected = 0;
        }
        taken
    }

    /// An edit command on the filter (`select_all`, `cut`, `undo`, `redo`).
    pub fn command(&mut self, id: &str) -> bool {
        let taken = self.open && Control::command(&mut self.query, id);
        if taken && id != "select_all" {
            self.selected = 0;
        }
        taken
    }

    /// What's selected in the filter (copied).
    pub fn selected_text(&self) -> Option<String> {
        Control::selected_text(&self.query)
    }

    /// The cursor at `p`: a hand on a pill, a scope or ×, the text's in the filter.
    pub fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if self.hits.iter().any(|(r, _)| r.contains(p)) {
            return Some(ViewerCursor::Pointer);
        }
        self.query.hit(p).then(|| Control::cursor(&self.query, p)).flatten()
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.hits.iter().find(|(r, _)| r.contains(p)).map(|(_, h)| *h);
        let changed = hover != self.hover;
        self.hover = hover;
        let clear = Control::hover(&mut self.query, Some(p));
        changed | clear
    }

    /// `p` is on a pill, a scope or the filter's ×.
    pub fn over_clickable(&self, p: Point) -> bool {
        self.cursor_at(p) == Some(ViewerCursor::Pointer)
    }

    pub fn type_text(&mut self, typed: &str) {
        Control::text(&mut self.query, typed);
        self.selected = 0;
    }

    pub fn backspace(&mut self) {
        self.field_key(ViewerKey::Backspace, KeyReach::default());
    }

    /// ← → (one step) and ↑ ↓ (one row), wrapping from the end to the start.
    pub fn step(&mut self, delta: isize, row: bool) {
        let n = self.chips.len() as isize;
        if n == 0 {
            return;
        }
        let delta = if row { delta * self.columns.max(1) as isize } else { delta };
        self.selected = (self.selected as isize + delta).rem_euclid(n) as usize;
    }

    /// Tab: the other scope (if there is a file).
    pub fn toggle_scope(&mut self, has_file: bool) {
        if has_file {
            self.scope = Some(match self.scope() {
                LevelScope::Project => LevelScope::File,
                LevelScope::File => LevelScope::Project,
            });
            self.selected = 0;
        }
    }

    pub fn confirm(&self) -> Option<LevelPick> {
        self.pick(self.selected)
    }

    pub fn paint(&mut self, choices: &LevelChoices, painter: Painter<'_>) {
        let Painter { text, scene, palette, icons } = painter;
        self.hits.clear();
        self.chips.clear();
        if !self.open {
            return;
        }
        let t = self.appear.map_or(1.0, |a| a.value());
        let ctx = Ctx::new(palette, self.scale).with_opacity(t);
        let px = |v: f32| ctx.px(v);
        let scope = if choices.file_name.is_none() { LevelScope::Project } else { self.scope() };
        let chips = chip_list(choices, scope, &ui_kit::ListFilter::new(self.query.text()));
        if self.selected >= chips.len() {
            self.selected = 0;
        }

        // Dimensions: fixed width, height by the number of pill rows.
        let w = px(380.0);
        let columns = 3usize;
        self.columns = columns;
        let chip_h = px(34.0);
        let gap = px(8.0);
        let rows = chips.len().div_ceil(columns).max(1);
        let header_h = px(56.0);
        let scope_h = if choices.file_name.is_some() { px(34.0) + px(12.0) } else { 0.0 };
        let field_h = px(36.0);
        let footer_h = px(36.0);
        let grid_h = rows as f32 * chip_h + (rows - 1) as f32 * gap;
        let h = header_h + scope_h + field_h + px(12.0) + grid_h + px(14.0) + footer_h;
        let v = self.viewport;
        let x = (self.anchor.max_x() - w).clamp(v.min_x() + px(8.0), v.max_x() - w - px(8.0));
        let lift = (1.0 - t) * px(6.0);
        let y = (self.anchor.min_y() - px(ds::POPOVER_GAP) - h + lift).max(v.min_y() + px(8.0));
        let panel = Rect::new(x.round(), y.round(), w, h);
        self.panel = panel;
        let frame = Frame {
            panel,
            pad: px(16.0),
            columns,
            chip_h,
            gap,
            field_h,
            footer_h,
            scope,
            small: ctx.ui_font(-1.5),
        };
        let layer = scene.push_layer(None);
        ds::surface(layer, &ctx, panel, Elevation::Medium, radius::LG);
        let layer = scene.push_layer(Some(panel));

        paint_header(choices, &frame, text, layer, &ctx);
        let mut y = panel.min_y() + header_h;
        if let Some(file) = &choices.file_name {
            y = self.paint_scope(file, y, &frame, (text, icons), layer, &ctx);
        }
        // The filter: the popover's one field, so its caret always shows.
        let field = Rect::new(
            panel.min_x() + frame.pad,
            y,
            panel.size.width - 2.0 * frame.pad,
            frame.field_h,
        );
        let placeholder = i18n::t("level.filter");
        self.query.paint(layer, text, icons, &ctx, (field, true), &placeholder);
        let y = y + field_h + px(12.0);

        let layer = scene.push_layer(Some(panel));
        self.paint_chips(chips, y, &frame, text, layer, &ctx);
        paint_footer(choices, &frame, text, layer, &ctx);
    }

    /// The scope: Project | Only <file>, at `y`; returns the y below it.
    fn paint_scope(
        &mut self,
        file: &str,
        y: f32,
        frame: &Frame,
        (text, icons): (&mut TextSystem, &icons::Icons),
        layer: &mut Layer,
        ctx: &Ctx<'_>,
    ) -> f32 {
        let px = |v: f32| ctx.px(v);
        let (panel, pad) = (frame.panel, frame.pad);
        let width = panel.size.width - 2.0 * pad;
        let scopes = [LevelScope::Project, LevelScope::File];
        let project = i18n::t("level.project");
        let only = i18n::tr!("level.only_file", name = file);
        let labels = [project.as_str(), only.as_str()];
        let hovered = scopes.iter().position(|s| self.hover == Some(Hit::Scope(*s)));
        let spec = ds::SegmentedSpec {
            right: 0.0,
            mid: y + px(SCOPE_H) / 2.0,
            labels: &labels,
            selected: scopes.iter().position(|s| *s == frame.scope).unwrap_or_default(),
            hovered,
        };
        let look = ds::SegmentedLook {
            height: SCOPE_H,
            place: ds::SegmentPlace::Fill { x: panel.min_x() + pad, width },
            ..Default::default()
        };
        let hits = &mut self.hits;
        ds::segmented_styled(layer, text, icons, ctx, (spec, look), |i, r| {
            hits.push((r, Hit::Scope(scopes[i])));
        });
        y + px(SCOPE_H) + px(12.0)
    }

    /// The pills, in rows from `y` (or "nothing found").
    fn paint_chips(
        &mut self,
        chips: Vec<(String, Option<String>, bool)>,
        y: f32,
        frame: &Frame,
        text: &mut TextSystem,
        layer: &mut Layer,
        ctx: &Ctx<'_>,
    ) {
        let px = |v: f32| ctx.px(v);
        let p = ctx.palette;
        let (panel, pad, columns) = (frame.panel, frame.pad, frame.columns);
        let (chip_h, gap) = (frame.chip_h, frame.gap);
        let w = panel.size.width;
        let chip_w = ((w - 2.0 * pad - gap * (columns - 1) as f32) / columns as f32).floor();
        let font = ctx.ui_font(-0.5);
        let strong = font.clone().with_weight(Font::SEMIBOLD);
        if chips.is_empty() {
            let small = &frame.small;
            let shaped = text.shape_line(&i18n::t("level.nothing"), small);
            draw_text(
                layer,
                &shaped,
                panel.min_x() + pad,
                ds::optical_baseline(y, chip_h, small),
                ctx.fade(p.text_muted),
            );
        }
        for (i, (label, level, on)) in chips.into_iter().enumerate() {
            let (row, col) = (i / columns, i % columns);
            let r = Rect::new(
                panel.min_x() + pad + col as f32 * (chip_w + gap),
                y + row as f32 * (chip_h + gap),
                chip_w,
                chip_h,
            );
            let focused = i == self.selected;
            let hovered = self.hover == Some(Hit::Chip(i));
            let (fill, border) = if on {
                (p.accent, p.accent)
            } else if focused || hovered {
                (p.accent.with_alpha(0.10), p.accent.with_alpha(0.6))
            } else {
                (p.text.with_alpha(0.035), p.border)
            };
            layer.quad(
                Quad::filled(r, ctx.fade(fill))
                    .rounded(px(radius::MD))
                    .bordered(ctx.line(), ctx.fade(border)),
            );
            let f = if on { &strong } else { &font };
            let label = ds::ellipsize(text, &label, f, r.size.width - px(14.0));
            let shaped = text.shape_line(&label, f);
            draw_text(
                layer,
                &shaped,
                (r.min_x() + (r.size.width - shaped.width) / 2.0).round(),
                ds::optical_baseline(r.min_y(), chip_h, f),
                ctx.fade(if on { Rgba::WHITE } else { p.text }),
            );
            self.chips.push((r, level));
            self.hits.push((r, Hit::Chip(i)));
        }
    }
}

/// The panel's geometry and the scope shown, shared by the pieces of [`LevelPicker::paint`].
struct Frame {
    panel: Rect,
    pad: f32,
    columns: usize,
    chip_h: f32,
    /// Between the pills.
    gap: f32,
    field_h: f32,
    footer_h: f32,
    scope: LevelScope,
    /// The small font (what was detected, "nothing found").
    small: Font,
}

/// The pills: Auto / same as the project, then the versions (newest first), those matching
/// `filter`: (label, level, chosen).
fn chip_list(
    choices: &LevelChoices,
    scope: LevelScope,
    filter: &ui_kit::ListFilter,
) -> Vec<(String, Option<String>, bool)> {
    let mut chips: Vec<(String, Option<String>, bool)> = Vec::new();
    let (first_label, first_on) = match scope {
        LevelScope::Project => {
            let label = match &choices.detected {
                Some(level) => i18n::tr!("level.auto_detected", level = level.as_str()),
                None => i18n::t("level.auto"),
            };
            (label, choices.auto)
        }
        LevelScope::File => (i18n::t("level.like_project"), choices.file.is_none()),
    };
    if filter.matches(&first_label) {
        chips.push((first_label, None, first_on));
    }
    for level in choices.levels.iter().rev() {
        if !filter.matches(level) {
            continue;
        }
        let on = match scope {
            LevelScope::Project => !choices.auto && choices.project == *level,
            LevelScope::File => choices.file.as_deref() == Some(level.as_str()),
        };
        chips.push((format!("{} {level}", choices.name), Some(level.clone()), on));
    }
    chips
}

/// The header: "Versiunea PHP" and what was detected.
fn paint_header(
    choices: &LevelChoices,
    frame: &Frame,
    text: &mut TextSystem,
    layer: &mut Layer,
    ctx: &Ctx<'_>,
) {
    let px = |v: f32| ctx.px(v);
    let (panel, pad) = (frame.panel, frame.pad);
    let title_font = ctx.ui_font(1.0).with_weight(Font::SEMIBOLD);
    let title =
        text.shape_line(&i18n::tr!("level.title", name = choices.name.as_str()), &title_font);
    draw_text(
        layer,
        &title,
        panel.min_x() + pad,
        panel.min_y() + px(26.0),
        ctx.fade(ctx.palette.text),
    );
    let small = &frame.small;
    let detail = match (&choices.detected, &choices.source) {
        (Some(level), Some(source)) => {
            i18n::tr!("level.detected_from", level = level.as_str(), source = source.as_str())
        }
        (Some(level), None) => i18n::tr!("level.detected", level = level.as_str()),
        _ => i18n::t("level.none_detected"),
    };
    let detail = ds::ellipsize(text, &detail, small, panel.size.width - 2.0 * pad);
    let shaped = text.shape_line(&detail, small);
    draw_text(
        layer,
        &shaped,
        panel.min_x() + pad,
        panel.min_y() + px(45.0),
        ctx.fade(ctx.palette.text_muted),
    );
}

/// The keys, in the footer.
fn paint_footer(
    choices: &LevelChoices,
    frame: &Frame,
    text: &mut TextSystem,
    layer: &mut Layer,
    ctx: &Ctx<'_>,
) {
    let (panel, footer_h) = (frame.panel, frame.footer_h);
    let w = panel.size.width;
    let footer = Rect::new(panel.min_x(), panel.max_y() - footer_h, w, footer_h);
    ds::divider(layer, ctx, footer.min_x(), footer.max_x(), footer.min_y());
    let [mv, pick, close, scope_hint] = [
        i18n::t("home.hint_move"),
        i18n::t("level.hint_pick"),
        i18n::t("home.hint_close"),
        i18n::t("level.hint_scope"),
    ];
    let mut hints: Vec<KeyHint<'_>> = vec![(&["←", "→"], &mv), (&["↵"], &pick)];
    if choices.file_name.is_some() {
        hints.push((&["tab"], &scope_hint));
    }
    hints.push((&["esc"], &close));
    let row = Rect::new(
        footer.min_x() + frame.pad,
        footer.min_y() + ctx.line(),
        w,
        footer_h - ctx.line(),
    );
    ui_kit::paint_key_hints(layer, text, &hints, row, &ctx.palette.key_hints(ctx.scale));
}

#[cfg(test)]
#[path = "../tests/unit/level_picker.rs"]
mod tests;
