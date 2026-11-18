//! Choosing the language version (e.g. PHP 8.3) from the status bar: a popover above
//! the item, with the scope (for the project / only for the active file), a filter and
//! the versions as pills (Auto / project's first). Keyboard navigation: ← → ↑ ↓, ↵, esc.
//! A module shows it as its popover ([`LEVEL_PICKER_EXTENSION`]); the choice goes back to
//! the app as [`AppRequest::SetLevel`](crate::AppRequest::SetLevel).

use std::time::Duration;

use icons::Icons;
use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_kit::{KeyHint, TextInput, TextInputStyle, draw_text};

use crate::Painter;
use crate::anim::Tween;
use crate::ds::{self, Ctx, Elevation, radius};

/// The extension of the picker's popover view (`open_overlay`, given [`LevelChoices`]).
pub const LEVEL_PICKER_EXTENSION: &str = "view-level-picker";

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
    Clear,
}

#[derive(Debug, Default)]
pub struct LevelPicker {
    open: bool,
    anchor: Rect,
    viewport: Rect,
    scale: f32,
    scope: Option<LevelScope>,
    pub query: TextInput,
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
            Some(Hit::Clear) => {
                self.query.set_text("");
                LevelClick::Inside
            }
            None => LevelClick::Inside,
        }
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.hits.iter().find(|(r, _)| r.contains(p)).map(|(_, h)| *h);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    pub fn over_clickable(&self, p: Point) -> bool {
        self.hits.iter().any(|(r, _)| r.contains(p))
    }

    pub fn type_text(&mut self, typed: &str) {
        self.query.insert(typed);
        self.selected = 0;
    }

    pub fn backspace(&mut self) {
        self.query.backspace(false);
        self.selected = 0;
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
            y = self.paint_scope(file, y, &frame, text, layer, &ctx);
        }
        let input = self.paint_field(y, &frame, icons, layer, &ctx);
        self.paint_query(input, text, scene, &ctx);
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
        text: &mut TextSystem,
        layer: &mut Layer,
        ctx: &Ctx<'_>,
    ) -> f32 {
        let px = |v: f32| ctx.px(v);
        let p = ctx.palette;
        let (panel, pad) = (frame.panel, frame.pad);
        let seg = Rect::new(panel.min_x() + pad, y, panel.size.width - 2.0 * pad, px(34.0));
        layer.quad(Quad::filled(seg, ctx.fade(p.text.with_alpha(0.05))).rounded(px(radius::MD)));
        let half = ((seg.size.width - px(4.0)) / 2.0).floor();
        let font = ctx.ui_font(-1.0);
        for (i, (label, s)) in [
            (i18n::t("level.project"), LevelScope::Project),
            (i18n::tr!("level.only_file", name = file), LevelScope::File),
        ]
        .into_iter()
        .enumerate()
        {
            let r = Rect::new(
                seg.min_x() + px(2.0) + i as f32 * half,
                seg.min_y() + px(2.0),
                half,
                seg.size.height - px(4.0),
            );
            let on = frame.scope == s;
            if on {
                layer.quad(
                    Quad::filled(r, ctx.fade(p.popup_background))
                        .rounded(px(radius::SM + 1.0))
                        .bordered(ctx.line(), ctx.fade(p.border)),
                );
            } else if self.hover == Some(Hit::Scope(s)) {
                layer.quad(
                    Quad::filled(r, ctx.fade(p.text.with_alpha(0.05)))
                        .rounded(px(radius::SM + 1.0)),
                );
            }
            let label = ds::ellipsize(text, &label, &font, r.size.width - px(16.0));
            let shaped = text.shape_line(&label, &font);
            draw_text(
                layer,
                &shaped,
                (r.min_x() + (r.size.width - shaped.width) / 2.0).round(),
                ds::optical_baseline(r.min_y(), r.size.height, &font),
                ctx.fade(if on { p.text } else { p.text_muted }),
            );
            self.hits.push((r, Hit::Scope(s)));
        }
        seg.max_y() + px(12.0)
    }

    /// The filter's box at `y` (with × when it has text); returns where its text goes.
    fn paint_field(
        &mut self,
        y: f32,
        frame: &Frame,
        icons: &Icons,
        layer: &mut Layer,
        ctx: &Ctx<'_>,
    ) -> Rect {
        let px = |v: f32| ctx.px(v);
        let (panel, pad) = (frame.panel, frame.pad);
        let field = Rect::new(panel.min_x() + pad, y, panel.size.width - 2.0 * pad, frame.field_h);
        let mut input = ds::search_field(layer, icons, ctx, field, true);
        if !self.query.text().is_empty() {
            let mid = field.min_y() + field.size.height / 2.0;
            let r = ds::clear_button(
                layer,
                icons,
                ctx,
                field.max_x() - px(17.0),
                mid,
                self.hover == Some(Hit::Clear),
            );
            input.size.width = (r.min_x() - px(6.0) - input.min_x()).max(0.0);
            self.hits.push((r, Hit::Clear));
        }
        input
    }

    /// The filter's text, on its own layer clipped to `input`.
    fn paint_query(
        &mut self,
        input: Rect,
        text: &mut TextSystem,
        scene: &mut Scene,
        ctx: &Ctx<'_>,
    ) {
        let p = ctx.palette;
        let style = TextInputStyle {
            font: ctx.ui_font(-0.5),
            background: Rgba::TRANSPARENT,
            border: Rgba::TRANSPARENT,
            border_focused: Rgba::TRANSPARENT,
            text: ctx.fade(p.text),
            placeholder: ctx.fade(p.text_muted),
            selection: ctx.fade(p.selection),
            caret: ctx.fade(p.caret),
            radius: 0.0,
            border_width: 0.0,
            padding: 0.0,
            caret_width: ctx.px(1.5).max(1.0),
            lead: 0.0,
        };
        let input_layer = scene.push_layer(Some(input));
        self.query.paint(input_layer, text, input, true, &i18n::t("level.filter"), &style);
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
