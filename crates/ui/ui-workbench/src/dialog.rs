//! The app's dialogs (instead of native ones): "Save changes?", "Where do you
//! open the project?" etc. A card centered over the window: the icon in a subtle
//! square, the title, the message (with `**highlights**`), and at the bottom, on a separate
//! footer, the buttons — the destructive action on the left, cancel and the primary action
//! on the right, each with its key (↵ / esc). Enter picks the primary button, Esc cancels,
//! ← / → / Tab move focus (the focus ring appears only during keyboard navigation).
//!
//! A dialog about many things (the files deleted) lists them under its message: their
//! common folder once, then each on its row (its folders muted, its name in the text
//! color), in a box that scrolls (the wheel, ↑ / ↓) past [`LIST_ROWS`] of them — only the
//! rows in sight are shaped. The card always fits the window: the list gives up height first,
//! so the buttons stay in sight.
//!
//! A dialog may have a checkbox (unchecked when it opens) for a riskier variant of its
//! action: checked, the dialog turns to danger, a warning shows below it and the primary
//! button takes its own label (e.g. "Delete from disk").
//!
//! The dialog is non-blocking: the answer (the button id) is picked up by the app, which
//! continues the action that requested it.

use std::sync::Arc;
use std::time::Duration;

use icons::Icons;
use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad, Scene};
use text_layout::{Font, FontMetrics, ShapedLine, TextSystem};
use ui_kit::{centered_baseline, draw_icon, draw_text};

use crate::anim::Appear;
use crate::ds::{self, Ctx, Elevation, Tone, radius};
use crate::palette::Palette;

/// The most entries a dialog's list shows at once (more scroll).
pub const LIST_ROWS: usize = 8;

/// The kind of a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// The recommended action (filled, accent color); Enter.
    Primary,
    Secondary,
    /// An action that loses something (e.g. "Nu salva").
    Destructive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogButton {
    pub id: &'static str,
    pub label: String,
    pub kind: ButtonKind,
}

impl DialogButton {
    pub fn new(id: &'static str, label: impl Into<String>, kind: ButtonKind) -> Self {
        Self { id, label: label.into(), kind }
    }
}

/// A dialog's checkbox: its label and, while checked, the warning shown and the primary
/// button's label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogCheck {
    pub label: String,
    pub warning: String,
    pub checked_label: String,
}

/// A dialog's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogSpec {
    pub tone: Tone,
    /// The icon name (from the app's set), e.g. `file-pen`, `folder-open`.
    pub icon: &'static str,
    pub title: String,
    /// The message; `**text**` is highlighted (e.g. the file name).
    pub message: String,
    /// The buttons, left to right (the primary one usually last).
    pub buttons: Vec<DialogButton>,
    /// The button chosen by Esc (a click outside does nothing).
    pub cancel: &'static str,
    /// A text field (optional): the initial value and what is selected on open.
    pub input: Option<(String, std::ops::Range<usize>)>,
    /// A checkbox (optional), unchecked on open.
    pub check: Option<DialogCheck>,
    /// The entries the dialog is about (paths, `/`-separated), listed under the message:
    /// their common folder once, then each on its row, in a box that scrolls past
    /// [`LIST_ROWS`] of them. Empty: no list.
    pub list: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Dialog {
    spec: Option<DialogSpec>,
    focus: usize,
    hover: Option<usize>,
    rects: Vec<Rect>,
    card: Rect,
    viewport: Rect,
    scale: f32,
    appear: Appear,
    /// Focus was moved from the keyboard (the focus ring is shown).
    keyboard: bool,
    /// The dialog's text field, if any.
    pub input: Option<ui_kit::TextInput>,
    checked: bool,
    /// The checkbox and its label (a click there toggles it).
    check_rect: Option<Rect>,
    /// The list's scroll (physical pixels from its top) and how far it goes, as last painted.
    list_scroll: f32,
    list_max: f32,
    /// A row of the list's height, as last painted (↑ / ↓ scroll by it).
    list_row: f32,
    list_bar: ui_components::EdgeScrollbar,
    /// Where the list's box ends, as last painted (tests).
    #[cfg(test)]
    list_bottom: f32,
}

impl Dialog {
    pub fn is_open(&self) -> bool {
        self.spec.is_some()
    }

    pub fn open(&mut self, spec: DialogSpec) {
        self.focus = spec.buttons.iter().position(|b| b.kind == ButtonKind::Primary).unwrap_or(0);
        self.input = spec.input.as_ref().map(|(value, select)| {
            let mut input = ui_kit::TextInput::new(value);
            input.select(select.clone());
            input
        });
        self.spec = Some(spec);
        self.checked = false;
        self.check_rect = None;
        self.hover = None;
        self.keyboard = false;
        self.list_scroll = 0.0;
        self.list_max = 0.0;
        self.list_bar = ui_components::EdgeScrollbar::default();
        self.appear.start(Duration::from_millis(140));
    }

    pub fn close(&mut self) {
        self.spec = None;
        self.rects.clear();
        self.check_rect = None;
    }

    /// The checkbox is checked.
    pub fn checked(&self) -> bool {
        self.checked
    }

    /// Toggles the checkbox (Space); `false` if there's none.
    pub fn toggle_check(&mut self) -> bool {
        if self.spec.as_ref().is_none_or(|s| s.check.is_none()) {
            return false;
        }
        self.checked = !self.checked;
        true
    }

    /// A click on the checkbox or its label toggles it.
    pub fn check_at(&mut self, p: Point) -> bool {
        self.check_rect.is_some_and(|r| r.contains(p)) && self.toggle_check()
    }

    /// The spec as shown: checked, in danger, its primary button with the checked label.
    fn shown(&self) -> Option<DialogSpec> {
        let mut spec = self.spec.clone()?;
        if self.checked
            && let Some(check) = &spec.check
        {
            let label = check.checked_label.clone();
            spec.tone = Tone::Danger;
            if let Some(b) = spec.buttons.iter_mut().find(|b| b.kind == ButtonKind::Primary) {
                b.label = label;
            }
        }
        Some(spec)
    }

    pub fn set_viewport(&mut self, viewport: Rect, scale: f32) {
        self.viewport = viewport;
        self.scale = scale;
    }

    pub fn is_animating(&self) -> bool {
        self.appear.animating()
    }

    /// The button under `p` (its id) or cancel (click outside the card).
    pub fn hit(&self, p: Point) -> Option<&'static str> {
        // Outside the card: nothing (a dialog closes only with a button or Esc).
        let spec = self.spec.as_ref()?;
        let i = self.rects.iter().position(|r| r.contains(p))?;
        spec.buttons.get(i).map(|b| b.id)
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.rects.iter().position(|r| r.contains(p));
        let changed = hover != self.hover;
        self.hover = hover;
        let bar = self.list_bar.hover(p);
        changed || bar
    }

    /// The wheel (physical pixels, positive = down): the list scrolls. `true` if it moved.
    pub fn scroll(&mut self, dy: f32) -> bool {
        let before = self.list_scroll;
        self.list_scroll = (self.list_scroll + dy).clamp(0.0, self.list_max);
        self.list_scroll != before
    }

    /// The card, as last painted.
    pub fn card(&self) -> Rect {
        self.card
    }

    /// How far its list is scrolled (physical pixels from its top).
    pub fn list_offset(&self) -> f32 {
        self.list_scroll
    }

    /// A press on the list's scrollbar: `true` if it took it (a drag follows: the list
    /// follows the thumb, or jumps where the track was pressed).
    pub fn scrollbar_press(&mut self, p: Point) -> bool {
        let Some(offset) = self.list_bar.press(p) else { return false };
        self.list_scroll = offset.clamp(0.0, self.list_max);
        true
    }

    pub fn scrollbar_held(&self) -> bool {
        self.list_bar.is_dragging()
    }

    /// The held thumb dragged to `p`: `true` if the list moved.
    pub fn scrollbar_drag(&mut self, p: Point) -> bool {
        let Some(offset) = self.list_bar.drag(p) else { return false };
        let before = self.list_scroll;
        self.list_scroll = offset.clamp(0.0, self.list_max);
        self.list_scroll != before
    }

    /// The mouse released: `true` if the scrollbar was held.
    pub fn scrollbar_release(&mut self) -> bool {
        self.list_bar.release()
    }

    /// ↑ / ↓: the list scrolls `rows` rows (negative: up). `true` if it moved.
    pub fn scroll_rows(&mut self, rows: f32) -> bool {
        self.scroll(rows * self.list_row)
    }

    /// A button or the checkbox is under the mouse (the cursor becomes a hand).
    pub fn over_button(&self, p: Point) -> bool {
        self.rects.iter().any(|r| r.contains(p)) || self.check_rect.is_some_and(|r| r.contains(p))
    }

    /// Enter: the focused button.
    pub fn confirm(&self) -> Option<&'static str> {
        self.spec.as_ref()?.buttons.get(self.focus).map(|b| b.id)
    }

    /// Esc.
    pub fn cancel(&self) -> Option<&'static str> {
        self.spec.as_ref().map(|s| s.cancel)
    }

    /// Moves focus between buttons (← / → / Tab).
    pub fn move_focus(&mut self, delta: isize) -> bool {
        let Some(spec) = &self.spec else { return false };
        if spec.buttons.is_empty() {
            return false;
        }
        self.focus = ui_kit::step_index(self.focus, delta, spec.buttons.len());
        self.keyboard = true;
        true
    }

    pub fn paint(
        &mut self,
        icons: &Icons,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        self.rects.clear();
        let Some(spec) = self.shown() else { return };
        let spec = &spec;
        let t = self.appear.value();
        let ctx = Ctx::new(palette, self.scale).with_opacity(t);
        let px = |v: f32| ctx.px(v);
        let v = self.viewport;
        let style = Style::new(ctx, text);

        let width = px(420.0).min(v.size.width - px(40.0));
        let pad = px(22.0);
        let icon_box = px(36.0);
        let text_x = pad + icon_box + px(14.0);
        let max_text = width - text_x - pad;
        let rows = wrap_message(&spec.message, (&style.body, &style.strong), max_text, text);
        // The title on as many rows as it needs (a long file name broken where it must).
        let title_rows = wrap_title(&spec.title, &style.title, max_text, text);
        let title_h = (style.title.size * 1.3).round();
        let line_h = (style.body.size * 1.5).round();
        let footer_h = px(58.0);
        let field_h = if self.input.is_some() { px(36.0) + px(14.0) } else { 0.0 };
        let warning = match &spec.check {
            Some(check) if self.checked => {
                wrap_message(&check.warning, (&style.body, &style.strong), max_text, text)
            }
            _ => Vec::new(),
        };
        let check_h = if spec.check.is_some() {
            px(34.0)
                + warning.len() as f32 * line_h
                + if warning.is_empty() { 0.0 } else { px(4.0) }
        } else {
            0.0
        };
        let tm = style.title_m;
        let folder = common_folder(&spec.list);
        let list = ListLayout::new(spec.list.len(), !folder.is_empty(), line_h, &ctx);
        let content_h = (tm.ascent + tm.descent)
            + title_rows.len().saturating_sub(1) as f32 * title_h
            + px(6.0)
            + rows.len() as f32 * line_h
            + field_h
            + check_h;
        // The list's section starts below the rest, edge to edge, down to the footer.
        let list_top = pad + content_h.max(icon_box) + pad;
        // The window's room: the list gives up rows first (down to two), the buttons stay.
        let room = v.size.height - 2.0 * px(16.0);
        let above = list.above;
        let list = list.within(room - list_top - footer_h - above);
        let height = list_top + list.height() + footer_h;
        let lift = (1.0 - t) * px(8.0);
        let top = (v.min_y() + v.size.height * 0.26)
            .min(v.max_y() - px(16.0) - height)
            .max(v.min_y() + px(16.0));
        let card = Rect::new(
            (v.min_x() + (v.size.width - width) / 2.0).round(),
            (top + lift).round(),
            width.round(),
            height.round(),
        );
        self.card = card;
        let geo = Geometry { card, pad, icon_box, text_x, max_text, line_h, footer_h };

        let layer = scene.push_layer(None);
        ds::backdrop(layer, &ctx, v);
        ds::surface(layer, &ctx, card, Elevation::High, radius::LG + 2.0);

        let layer = scene.push_layer(Some(card));
        paint_icon(spec, &geo, icons, layer, &ctx);
        let mut y = paint_message((&title_rows, title_h), &rows, &geo, &style, text, layer);
        if list.rows > 0 {
            let at = card.min_y() + list_top;
            self.paint_list(spec, folder, (at, &list), &geo, (&style, icons), text, scene);
        }
        // The text field (e.g. the new name), below the message.
        if let Some(input) = &mut self.input {
            let layer = scene.push_layer(Some(card));
            let field = paint_field(y, &geo, layer, &ctx);
            y = field.max_y() + px(4.0);
            paint_input(input, field, &style, text, scene);
        }
        // The checkbox, then (checked) its warning in the danger color.
        if let Some(check) = &spec.check {
            let layer = scene.push_layer(Some(card));
            let x = card.min_x() + geo.text_x;
            let mid = y + px(20.0);
            let state = if self.checked { ds::Check::On } else { ds::Check::Off };
            let boxed = ds::checkbox(layer, text, &ctx, x, mid, state, true);
            let label = text.shape_line(&check.label, &style.body);
            let lx = boxed.max_x() + px(8.0);
            let color = if self.checked { palette.danger } else { palette.text };
            draw_text(
                layer,
                &label,
                lx,
                centered_baseline(mid - px(10.0), px(20.0), &style.body_m),
                ctx.fade(color),
            );
            self.check_rect = Some(Rect::new(
                x - px(4.0),
                mid - px(12.0),
                lx + label.width - x + px(8.0),
                px(24.0),
            ));
            let mut wy = mid + px(18.0);
            for row in &warning {
                let mut wx = x;
                for (word, bold) in row {
                    let font = if *bold { &style.strong } else { &style.body };
                    let shaped = text.shape_line(word, font);
                    draw_text(
                        layer,
                        &shaped,
                        wx.round(),
                        (wy + style.body_m.ascent).round(),
                        ctx.fade(palette.danger),
                    );
                    wx += shaped.width;
                }
                wy += geo.line_h;
            }
        }

        let layer = paint_footer(&geo, scene, &ctx);
        let rects = layout_buttons(spec, &geo, &style, text);
        for (i, &r) in rects.iter().enumerate() {
            self.paint_button(spec, i, r, &style, text, layer);
        }
        self.rects = rects;
    }

    /// The list's section from `y` down to the footer, the card's whole width on its own
    /// color (a line above it): the common `folder` on its row, then the entries, only those
    /// in sight shaped.
    #[allow(clippy::too_many_arguments)]
    fn paint_list(
        &mut self,
        spec: &DialogSpec,
        folder: &str,
        (y, list): (f32, &ListLayout),
        geo: &Geometry,
        (style, icons): (&Style<'_>, &Icons),
        text: &mut TextSystem,
        scene: &mut Scene,
    ) {
        let ctx = &style.ctx;
        let px = |v: f32| ctx.px(v);
        let palette = ctx.palette;
        let card = geo.card;
        let x = card.min_x() + geo.pad;
        let width = card.size.width - 2.0 * geo.pad;
        let mut y = y;
        let layer = scene.push_layer(Some(card));
        ds::divider(layer, ctx, card.min_x(), card.max_x(), y);
        if !folder.is_empty() {
            y += list.gap;
            let size = px(14.0);
            let mid = y + geo.line_h / 2.0;
            if let Some(id) = icons.id("folder") {
                let r = Rect::new(x, (mid - size / 2.0).round(), size, size);
                draw_icon(layer, icons, id, r, ctx.fade(palette.text_muted));
            }
            let room = width - size - px(6.0);
            let shown = ds::ellipsize_start(text, folder.trim_end_matches('/'), &style.body, room);
            let shaped = text.shape_line(&shown, &style.body);
            let baseline = centered_baseline(y, geo.line_h, &style.body_m);
            draw_text(layer, &shaped, x + size + px(6.0), baseline, ctx.fade(palette.text_muted));
            y += geo.line_h;
        }
        let boxed = Rect::new(card.min_x(), y.round(), card.size.width, list.box_h.round());
        let content = spec.list.len() as f32 * list.row_h + 2.0 * list.inset;
        self.list_row = list.row_h;
        self.list_max = (content - boxed.size.height).max(0.0);
        self.list_scroll = self.list_scroll.clamp(0.0, self.list_max);
        let rows = scene.push_layer(Some(boxed));
        let first = ((self.list_scroll - list.inset) / list.row_h).floor().max(0.0) as usize;
        let room = width;
        for (i, entry) in spec.list.iter().enumerate().skip(first) {
            let top = boxed.min_y() + list.inset + i as f32 * list.row_h - self.list_scroll;
            if top > boxed.max_y() {
                break;
            }
            let entry = entry.get(folder.len()..).unwrap_or(entry);
            let shown = ds::ellipsize_path(text, entry, &style.body, room);
            let (dirs, name) = match shown.rfind('/') {
                Some(i) => shown.split_at(i + 1),
                None => ("", shown.as_str()),
            };
            let baseline = centered_baseline(top, list.row_h, &style.body_m);
            let mut rx = x;
            if !dirs.is_empty() {
                let shaped = text.shape_line(dirs, &style.body);
                draw_text(rows, &shaped, rx, baseline, ctx.fade(palette.text_muted));
                rx += shaped.width;
            }
            let shaped = text.shape_line(name, &style.body);
            draw_text(rows, &shaped, rx, baseline, ctx.fade(palette.text));
        }
        self.list_bar.paint(scene, palette, ctx.scale, boxed, content, self.list_scroll);
        #[cfg(test)]
        {
            self.list_bottom = boxed.max_y();
        }
    }

    /// A button (`spec.buttons[i]` at `r`): its fill by kind and hover, the focus ring
    /// (keyboard navigation only), the label and its key (↵ / esc).
    fn paint_button(
        &self,
        spec: &DialogSpec,
        i: usize,
        r: Rect,
        style: &Style<'_>,
        text: &mut TextSystem,
        layer: &mut Layer,
    ) {
        let ctx = &style.ctx;
        let px = |v: f32| ctx.px(v);
        let palette = ctx.palette;
        let button = &spec.buttons[i];
        let button_h = r.size.height;
        let (label, key, w) = measure(button, spec.cancel, style, text);
        let hovered = self.hover == Some(i);
        let (fill, border, color) = match button.kind {
            ButtonKind::Primary => {
                let base = if self.checked { palette.danger } else { palette.accent };
                let fill = if hovered { base.with_alpha(0.9) } else { base };
                (fill, Rgba::TRANSPARENT, Rgba::WHITE)
            }
            ButtonKind::Secondary => {
                let fill =
                    if hovered { palette.text.with_alpha(0.06) } else { palette.popup_background };
                (fill, palette.border, palette.text)
            }
            ButtonKind::Destructive => {
                let fill =
                    if hovered { palette.danger.with_alpha(0.08) } else { Rgba::TRANSPARENT };
                (fill, Rgba::TRANSPARENT, palette.danger)
            }
        };
        if self.keyboard && i == self.focus {
            let ring = px(2.0);
            let gap = px(2.0);
            let outer = Rect::new(
                r.min_x() - ring - gap,
                r.min_y() - ring - gap,
                r.size.width + 2.0 * (ring + gap),
                r.size.height + 2.0 * (ring + gap),
            );
            layer.quad(
                Quad::filled(outer, Rgba::TRANSPARENT)
                    .rounded(px(radius::SM + 1.0) + ring + gap)
                    .bordered(ring, ctx.fade(palette.accent.with_alpha(0.55))),
            );
        }
        layer.quad(
            Quad::filled(r, ctx.fade(fill))
                .rounded(px(radius::SM + 1.0))
                .bordered(ctx.line(), ctx.fade(border)),
        );
        let key_w = key.as_ref().map_or(0.0, |k| k.width + px(16.0));
        let lx = r.min_x() + (w - label.width - key_w) / 2.0;
        draw_text(
            layer,
            &label,
            lx.round(),
            centered_baseline(r.min_y(), button_h, &style.button_m),
            ctx.fade(color),
        );
        let Some(key) = key else { return };
        let kh = px(18.0);
        let kr = Rect::new(
            (lx + label.width + px(8.0)).round(),
            r.min_y() + (button_h - kh) / 2.0,
            key.width + px(8.0),
            kh,
        );
        let key_bg = if button.kind == ButtonKind::Primary {
            Rgba::WHITE.with_alpha(0.18)
        } else {
            palette.text.with_alpha(0.06)
        };
        let key_color = if button.kind == ButtonKind::Primary {
            Rgba::WHITE.with_alpha(0.85)
        } else {
            palette.text_muted
        };
        layer.quad(Quad::filled(kr, ctx.fade(key_bg)).rounded(px(4.0)));
        draw_text(
            layer,
            &key,
            kr.min_x() + px(4.0),
            centered_baseline(kr.min_y(), kh, &style.key_m),
            ctx.fade(key_color),
        );
    }
}

/// The drawing context and the dialog's fonts with their metrics, measured once per paint.
struct Style<'p> {
    ctx: Ctx<'p>,
    title: Font,
    title_m: FontMetrics,
    body: Font,
    strong: Font,
    body_m: FontMetrics,
    button: Font,
    button_m: FontMetrics,
    key: Font,
    key_m: FontMetrics,
}

impl<'p> Style<'p> {
    fn new(ctx: Ctx<'p>, text: &mut TextSystem) -> Self {
        let title = ctx.ui_font(1.5).with_weight(Font::SEMIBOLD);
        let title_m = text.metrics(&title);
        let body = ctx.ui_font(0.0);
        let strong = body.clone().with_weight(Font::SEMIBOLD);
        let body_m = text.metrics(&body);
        let button = ds::button_font(&ctx);
        let button_m = text.metrics(&button);
        let key = ctx.ui_font(-2.5);
        let key_m = text.metrics(&key);
        Self { ctx, title, title_m, body, strong, body_m, button, button_m, key, key_m }
    }
}

/// The card's geometry, shared by the pieces of [`Dialog::paint`].
struct Geometry {
    card: Rect,
    pad: f32,
    icon_box: f32,
    /// The text column's left edge (from the card's) and its width.
    text_x: f32,
    max_text: f32,
    line_h: f32,
    footer_h: f32,
}

/// A button's label, its key (↵ for the primary one, esc for cancel) and its width.
type Measured = (Arc<ShapedLine>, Option<Arc<ShapedLine>>, f32);

/// The message, with highlights, wrapped word by word (a `\n` starts a new row; two, an
/// empty one between): rows of (text, highlighted).
fn wrap_message(
    message: &str,
    (body, strong): (&Font, &Font),
    max_text: f32,
    text: &mut TextSystem,
) -> Vec<Vec<(String, bool)>> {
    let mut rows: Vec<Vec<(String, bool)>> = vec![Vec::new()];
    let mut x = 0.0;
    for (i, part) in message.split("**").enumerate() {
        let bold = i % 2 == 1;
        let font = if bold { strong } else { body };
        for (line, words) in part.split('\n').enumerate() {
            if line > 0 {
                rows.push(Vec::new());
                x = 0.0;
            }
            wrap_words(words, font, max_text, text, (&mut rows, &mut x), bold);
        }
    }
    rows
}

/// The title's rows within `max_text`: by words, a word wider than a row (a long file name)
/// broken between its characters.
fn wrap_title(title: &str, font: &Font, max_text: f32, text: &mut TextSystem) -> Vec<String> {
    let mut rows = vec![String::new()];
    for word in title.split_inclusive(' ') {
        let row = rows.last().cloned().unwrap_or_default();
        let joined = format!("{row}{word}");
        if text.shape_line(joined.trim_end(), font).width <= max_text {
            if let Some(last) = rows.last_mut() {
                *last = joined;
            }
            continue;
        }
        if !row.is_empty() {
            rows.push(String::new());
        }
        // The word alone: on its own row, or broken where it no longer fits.
        let mut pieces = break_word(word, font, max_text, text).into_iter();
        if let (Some(first), Some(last)) = (pieces.next(), rows.last_mut()) {
            *last = first;
        }
        rows.extend(pieces);
    }
    let mut rows: Vec<String> = rows.into_iter().map(|r| r.trim_end().to_owned()).collect();
    rows.retain(|r| !r.is_empty());
    if rows.is_empty() {
        rows.push(String::new());
    }
    rows
}

/// `words` wrapped onto `rows` (the last one `x` wide so far); a word wider than a row (a
/// long path) broken where it no longer fits — after a `/` when there's one.
fn wrap_words(
    words: &str,
    font: &Font,
    max_text: f32,
    text: &mut TextSystem,
    (rows, x): (&mut Vec<Vec<(String, bool)>>, &mut f32),
    bold: bool,
) {
    for word in words.split_inclusive(' ') {
        let w = text.shape_line(word.trim_end(), font).width;
        if w > max_text {
            let pieces = break_word(word.trim_start(), font, max_text, text);
            for piece in pieces {
                if *x > 0.0 {
                    rows.push(Vec::new());
                }
                *x = text.shape_line(&piece, font).width;
                if let Some(row) = rows.last_mut() {
                    row.push((piece, bold));
                }
            }
            continue;
        }
        if *x > 0.0 && *x + w > max_text {
            rows.push(Vec::new());
            *x = 0.0;
        }
        let word = if *x == 0.0 { word.trim_start() } else { word };
        *x += text.shape_line(word, font).width;
        if let Some(row) = rows.last_mut() {
            row.push((word.to_owned(), bold));
        }
    }
}

/// `word` in pieces each within `max_text` (a word wider than a row: a long path, a file
/// name): shaped once, cut where the next character no longer fits — after a `/` when the
/// row is at least half full by then (a path's folders stay whole), else there.
fn break_word(word: &str, font: &Font, max_text: f32, text: &mut TextSystem) -> Vec<String> {
    let shaped = text.shape_line(word, font);
    let mut pieces = Vec::new();
    let (mut from, mut left) = (0, 0.0);
    for glyph in &shaped.glyphs {
        if glyph.start <= from || glyph.x + glyph.advance - left <= max_text {
            continue;
        }
        let slash = word[from..glyph.start]
            .rfind('/')
            .map(|i| from + i + 1)
            .filter(|&cut| cut < glyph.start && shaped.x_at(cut) - left >= max_text / 2.0);
        let cut = slash.unwrap_or(glyph.start);
        pieces.push(word[from..cut].to_owned());
        (from, left) = (cut, shaped.x_at(cut));
    }
    if from < word.len() {
        pieces.push(word[from..].to_owned());
    }
    pieces
}

/// The list's heights: a row's, the space above it (and its folder's row), the box's.
struct ListLayout {
    rows: usize,
    row_h: f32,
    /// The box's padding at its top and bottom.
    inset: f32,
    /// The gap above the folder's row.
    gap: f32,
    /// Above the entries: the folder's row and its gap (none without a folder).
    above: f32,
    box_h: f32,
}

impl ListLayout {
    /// `entries` listed (at most [`LIST_ROWS`] in sight), with their folder's row or not.
    fn new(entries: usize, folder: bool, line_h: f32, ctx: &Ctx<'_>) -> Self {
        let (row_h, inset) = (ctx.px(24.0), ctx.px(4.0));
        let rows = entries.min(LIST_ROWS);
        let gap = ctx.px(8.0);
        let above = if entries == 0 || !folder { 0.0 } else { gap + line_h };
        let box_h = if rows == 0 { 0.0 } else { rows as f32 * row_h + 2.0 * inset };
        Self { rows, row_h, inset, gap, above, box_h }
    }

    /// Fitted to `room` for its box: rows given up while it doesn't fit, down to two.
    fn within(mut self, room: f32) -> Self {
        while self.rows > 2 && self.box_h > room {
            self.rows -= 1;
            self.box_h = self.rows as f32 * self.row_h + 2.0 * self.inset;
        }
        self
    }

    /// Its section's height, above the footer (nothing without entries).
    fn height(&self) -> f32 {
        if self.rows == 0 { 0.0 } else { self.above + self.box_h }
    }
}

/// The folder all `entries` are in (`src/Http/`, ending in `/`): shown once above them,
/// each row then only what's below it. Empty when they share none, or for one entry (shown
/// whole).
fn common_folder(entries: &[String]) -> &str {
    let [first, rest @ ..] = entries else { return "" };
    if rest.is_empty() {
        return "";
    }
    let folder_end = |upto: usize| first[..upto].rfind('/').map_or(0, |i| i + 1);
    let mut end = folder_end(first.len());
    for entry in rest {
        while end > 0
            && !(entry.len() > end && entry.as_bytes().starts_with(&first.as_bytes()[..end]))
        {
            end = folder_end(end - 1);
        }
    }
    &first[..end]
}

/// The icon in a rounded square, in the dialog's tone.
fn paint_icon(spec: &DialogSpec, geo: &Geometry, icons: &Icons, layer: &mut Layer, ctx: &Ctx<'_>) {
    let px = |v: f32| ctx.px(v);
    let (card, pad, icon_box) = (geo.card, geo.pad, geo.icon_box);
    let tone = spec.tone.color(ctx.palette);
    let ic = Rect::new(card.min_x() + pad, card.min_y() + pad, icon_box, icon_box);
    layer.quad(Quad::filled(ic, ctx.fade(tone.with_alpha(0.12))).rounded(px(radius::MD)));
    if let Some(id) = icons.id(spec.icon) {
        let size = px(19.0);
        let r = Rect::new(
            ic.min_x() + (icon_box - size) / 2.0,
            ic.min_y() + (icon_box - size) / 2.0,
            size,
            size,
        );
        draw_icon(layer, icons, id, r, ctx.fade(tone));
    }
}

/// The title and the wrapped message (highlights in the text color); returns the y below
/// them.
fn paint_message(
    (title, title_h): (&[String], f32),
    rows: &[Vec<(String, bool)>],
    geo: &Geometry,
    style: &Style<'_>,
    text: &mut TextSystem,
    layer: &mut Layer,
) -> f32 {
    let ctx = &style.ctx;
    let palette = ctx.palette;
    let card = geo.card;
    let tm = style.title_m;
    let mut y = card.min_y() + geo.pad + tm.ascent;
    for (i, row) in title.iter().enumerate() {
        if i > 0 {
            y += title_h;
        }
        let shaped = text.shape_line(row, &style.title);
        draw_text(layer, &shaped, card.min_x() + geo.text_x, y.round(), ctx.fade(palette.text));
    }
    y += tm.descent + ctx.px(6.0);
    for row in rows {
        let mut x = card.min_x() + geo.text_x;
        for (word, bold) in row {
            let font = if *bold { &style.strong } else { &style.body };
            let shaped = text.shape_line(word, font);
            let color = if *bold { palette.text } else { palette.text_muted };
            draw_text(
                layer,
                &shaped,
                x.round(),
                (y + style.body_m.ascent).round(),
                ctx.fade(color),
            );
            x += shaped.width;
        }
        y += geo.line_h;
    }
    y
}

/// The text field's box (e.g. for the new name), below the message (`y`); returns it.
fn paint_field(y: f32, geo: &Geometry, layer: &mut Layer, ctx: &Ctx<'_>) -> Rect {
    let px = |v: f32| ctx.px(v);
    let field =
        Rect::new(geo.card.min_x() + geo.text_x, (y + px(10.0)).round(), geo.max_text, px(36.0));
    layer.quad(
        Quad::filled(field, ctx.fade(ctx.palette.editor_background))
            .rounded(px(radius::SM + 1.0))
            .bordered(ctx.line(), ctx.fade(ctx.palette.accent.with_alpha(0.7))),
    );
    field
}

/// The text field's text, on its own layer clipped to the `field` box.
fn paint_input(
    input: &mut ui_kit::TextInput,
    field: Rect,
    style: &Style<'_>,
    text: &mut TextSystem,
    scene: &mut Scene,
) {
    let ctx = &style.ctx;
    let px = |v: f32| ctx.px(v);
    let palette = ctx.palette;
    let style = ui_kit::TextInputStyle {
        font: style.body.clone(),
        background: Rgba::TRANSPARENT,
        border: Rgba::TRANSPARENT,
        border_focused: Rgba::TRANSPARENT,
        text: ctx.fade(palette.text),
        placeholder: ctx.fade(palette.text_muted),
        selection: ctx.fade(palette.selection),
        caret: ctx.fade(palette.caret),
        radius: 0.0,
        border_width: 0.0,
        padding: px(10.0),
        caret_width: px(1.5).max(1.0),
        lead: 0.0,
    };
    let input_layer = scene.push_layer(Some(field));
    input.paint(input_layer, text, field, true, "", &style);
}

/// The footer: a subtle background and the separator; returns the layer the buttons
/// are drawn on.
fn paint_footer<'s>(geo: &Geometry, scene: &'s mut Scene, ctx: &Ctx<'_>) -> &'s mut Layer {
    let card = geo.card;
    let foot = Rect::new(card.min_x(), card.max_y() - geo.footer_h, card.size.width, geo.footer_h);
    // The footer background follows the card's rounded corners: a rectangle the size of
    // the card, with the same radius, clipped to the footer area.
    let footer_layer = scene.push_layer(Some(foot));
    let inset = ctx.line();
    let shape = Rect::new(
        card.min_x() + inset,
        card.min_y(),
        card.size.width - 2.0 * inset,
        card.size.height - inset,
    );
    footer_layer.quad(
        Quad::filled(shape, ctx.fade(ctx.palette.text.with_alpha(0.025)))
            .rounded(ctx.px(radius::LG + 2.0) - inset),
    );
    let layer = scene.push_layer(Some(card));
    ds::divider(layer, ctx, foot.min_x(), foot.max_x(), foot.min_y());
    layer
}

/// A button's label, key and width.
fn measure(b: &DialogButton, cancel: &str, style: &Style<'_>, text: &mut TextSystem) -> Measured {
    let px = |v: f32| style.ctx.px(v);
    let label = text.shape_line(&b.label, &style.button);
    let key = match b.kind {
        ButtonKind::Primary => Some("↵"),
        _ if b.id == cancel => Some("esc"),
        _ => None,
    }
    .map(|k| text.shape_line(k, &style.key));
    let key_w = key.as_ref().map_or(0.0, |k| k.width + px(8.0) + px(8.0));
    let w = (label.width + key_w + px(28.0)).max(px(84.0));
    (label, key, w)
}

/// The buttons' places in the footer, in the spec's order: destructive ones on the left,
/// the rest on the right (in the given order).
fn layout_buttons(
    spec: &DialogSpec,
    geo: &Geometry,
    style: &Style<'_>,
    text: &mut TextSystem,
) -> Vec<Rect> {
    let px = |v: f32| style.ctx.px(v);
    let (card, footer_h) = (geo.card, geo.footer_h);
    let button_h = px(32.0);
    let top = (card.max_y() - footer_h + (footer_h - button_h) / 2.0).round();
    let mut rects = vec![Rect::default(); spec.buttons.len()];
    let mut left = card.min_x() + geo.pad;
    let mut right = card.max_x() - geo.pad;
    let order: Vec<usize> = (0..spec.buttons.len()).collect();
    for &i in order.iter().filter(|&&i| spec.buttons[i].kind == ButtonKind::Destructive) {
        let (_, _, w) = measure(&spec.buttons[i], spec.cancel, style, text);
        rects[i] = Rect::new(left, top, w.round(), button_h);
        left += w + px(8.0);
    }
    for &i in order.iter().rev().filter(|&&i| spec.buttons[i].kind != ButtonKind::Destructive) {
        let (_, _, w) = measure(&spec.buttons[i], spec.cancel, style, text);
        rects[i] = Rect::new((right - w).round(), top, w.round(), button_h);
        right -= w + px(8.0);
    }
    rects
}

#[cfg(test)]
#[path = "../tests/unit/dialog.rs"]
mod tests;
