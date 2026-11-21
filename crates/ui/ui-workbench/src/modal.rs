//! The app modal: the common frame for windows over the app (settings, lists,
//! forms). Configurable via [`ModalSpec`]: the size, the header (title, subtitle,
//! closing with the `esc` key, a field beside it), side navigation by sections (optional:
//! grouped under captions, as tabs when the window is narrow), the key footer (optional). Draws the dimmed backdrop, the surface, the header, the sections and
//! the footer, then returns the content area, where the caller draws.
//!
//! Interaction: [`Modal::hit`] tells what is under the mouse (close, a section, the content,
//! or outside: nothing — a modal never closes on a click outside), [`Modal::hover`] holds the hover state, and ↑ / ↓ move
//! the active section ([`Modal::step_section`]).

use std::time::Duration;

use icons::Icons;
use kernel::{Point, Rect};
use render_api::{Layer, Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_kit::{KeyHint, draw_icon, draw_text};

use crate::anim::Appear;
use crate::ds::{self, Ctx, Elevation, radius};
use crate::palette::Palette;
use ui_components::form::NavItem;

/// The modal's size (maximum width × height, in logical pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalSize {
    Small,
    Medium,
    Large,
    /// The settings: a navigation of many pages beside wide content.
    XLarge,
}

impl ModalSize {
    fn logical(self) -> (f32, f32) {
        match self {
            Self::Small => (460.0, 360.0),
            Self::Medium => (680.0, 520.0),
            Self::Large => (880.0, 620.0),
            Self::XLarge => (920.0, 720.0),
        }
    }
}

/// A section in the side navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModalSection<'a> {
    pub id: &'static str,
    pub label: &'a str,
    /// The icon name (from the app's set).
    pub icon: &'static str,
    /// The caption of the group it's in (shown over the group's first section; consecutive
    /// sections with the same caption are one group; empty: none) and the group's icon
    /// (its tab when narrow).
    pub group: &'a str,
    pub group_icon: &'static str,
}

/// The extra room above a group's caption, but the first's (logical pixels).
const NAV_GROUP_GAP: f32 = 8.0;

/// Narrower than this (logical pixels), a grouped navigation becomes tabs over the content
/// (its groups, then the chosen group's sections); also when its list wouldn't fit.
pub const NAV_TABS_BELOW: f32 = 760.0;
/// A section's row in the side navigation (logical pixels).
const NAV_ROW: f32 = 28.0;

/// A modal's configuration.
#[derive(Debug, Clone, Copy)]
pub struct ModalSpec<'a> {
    pub title: &'a str,
    pub subtitle: Option<&'a str>,
    /// An icon before the title, on an accent tile (the icon's name, from the app's set).
    pub icon: Option<&'static str>,
    pub size: ModalSize,
    /// The side navigation (empty: none).
    pub sections: &'a [ModalSection<'a>],
    /// Space reserved at the top of the navigation (logical pixels), e.g. for a search field.
    pub nav_header: f32,
    /// Room for a field in the header, left of `esc` (logical pixels wide; 0: none): see
    /// [`Modal::header_field`].
    pub header_field: f32,
    pub active: Option<&'static str>,
    /// The key footer (empty: no footer).
    pub hints: &'a [KeyHint<'a>],
    /// The footer's buttons, right of the keys (the first the rightmost): the footer is then
    /// as tall as a form's ([`ds::MODAL_FOOTER_H`]); a click on one is
    /// [`ModalHit::Button`].
    pub buttons: &'a [ui_components::ModalButton],
    /// What the footer says instead of the keys (something running, a question).
    pub status: Option<&'a ui_components::ModalStatus>,
    /// The content's own height (logical pixels), when it asks for one: the window then as
    /// tall as its header, that and its footer — never taller than its size. `None`: the
    /// size's height.
    pub fit: Option<f32>,
    /// The narrowest the content reads at (logical pixels), when it's a short one (a
    /// question): the window then as wide as that or as its title needs, never wider than its
    /// size. `None`: the size's width.
    pub fit_width: Option<f32>,
    /// With `fit_width`: whether a long title widens the window (a question's does); `false`
    /// keeps it as wide as asked, its title cut with `…`.
    pub title_widens: bool,
    /// The content's zoom (1 = 100%), when it has one: `− 100% +` on the footer's right.
    pub zoom: Option<f32>,
    /// It can be closed from its header (`esc`, clickable); `false`: no `esc` drawn and
    /// nothing there to click (work under way that mustn't be left).
    pub closable: bool,
}

/// What is under the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalHit {
    Close,
    Section(&'static str),
    Content,
    /// A footer's button (its index in [`ModalSpec::buttons`]).
    Button(usize),
    /// The footer's zoom: out, back to 100%, in.
    Zoom(ZoomStep),
}

/// A step of the footer's zoom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomStep {
    Out,
    Reset,
    In,
}

/// Where a footer's status line goes and how it looks.
struct StatusLine<'a> {
    /// The room left of the buttons (the footer's height).
    area: Rect,
    mid: f32,
    icon: Option<&'a str>,
    color: kernel::Rgba,
}

/// A footer's status: its icon, its text cut to the room. Returns where it was cut short
/// and all of it (its tooltip).
fn status_line(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &Icons,
    ctx: &Ctx<'_>,
    at: StatusLine<'_>,
    what: &str,
    font: &Font,
) -> Option<(Rect, String)> {
    let StatusLine { area, mid, icon, color } = at;
    let mut x = area.min_x();
    if let Some(id) = icon.and_then(|name| icons.id(name)) {
        let s = ctx.px(15.0);
        draw_icon(layer, icons, id, Rect::new(x, (mid - s / 2.0).round(), s, s), ctx.fade(color));
        x += s + ctx.px(8.0);
    }
    let shown = ds::ellipsize(text, what, font, (area.max_x() - x).max(0.0));
    let shaped = text.shape_line(&shown, font);
    let base = ds::optical_baseline(area.min_y(), area.size.height, font);
    draw_text(layer, &shaped, x, base, ctx.fade(color));
    (shown != what).then(|| (area, what.to_owned()))
}

#[derive(Debug, Default)]
pub struct Modal {
    open: bool,
    viewport: Rect,
    scale: f32,
    panel: Rect,
    close: Rect,
    content: Rect,
    sections: Vec<(Rect, &'static str)>,
    nav: Rect,
    /// The header's field (from the last paint; empty without one).
    header_field: Rect,
    hover: Option<ModalHit>,
    appear: Appear,
    /// The side navigation taller than its room scrolls (as the projects list does), with
    /// its bar.
    nav_scroll: ui_components::ScrollState,
    nav_overflows: bool,
    nav_bar: ui_components::EdgeScrollbar,
    /// The footer's buttons (from the last paint).
    buttons: Vec<Rect>,
    /// The footer's status where it was cut short, and all of it (its tooltip).
    status_tip: Option<(Rect, String)>,
    /// The footer's zoom steps (from the last paint).
    zoom: Vec<(Rect, ZoomStep)>,
}

impl Modal {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open(&mut self) {
        self.open = true;
        self.hover = None;
        self.appear.start(Duration::from_millis(150));
    }

    pub fn close(&mut self) {
        self.open = false;
        self.hover = None;
    }

    pub fn set_viewport(&mut self, viewport: Rect, scale: f32) {
        self.viewport = viewport;
        self.scale = scale;
    }

    pub fn is_animating(&self) -> bool {
        self.appear.animating()
    }

    /// The side navigation (from the last paint; empty without sections).
    pub fn nav(&self) -> Rect {
        self.nav
    }

    /// Whether `p` is over the side navigation: the wheel there is the navigation's, even
    /// at its end (it never moves the content instead).
    pub fn over_nav(&self, p: Point) -> bool {
        self.nav.size.width > 0.0 && self.nav.contains(p)
    }

    /// The side navigation scrolled by `dy` when `p` is over it and it's taller than its
    /// room. `true` if it scrolled.
    pub fn scroll_nav(&mut self, p: Point, dy: f32) -> bool {
        if !self.nav_overflows || !self.nav.contains(p) {
            return false;
        }
        self.nav_scroll.scroll_by(dy)
    }

    /// How far the side navigation is scrolled (physical pixels).
    pub fn nav_scroll(&self) -> f32 {
        self.nav_scroll.offset()
    }

    /// A press on the navigation's scrollbar: `true` if it took it.
    pub fn press_nav_bar(&mut self, p: Point) -> bool {
        if !self.nav_overflows {
            return false;
        }
        match self.nav_bar.press(p) {
            Some(offset) => {
                self.nav_scroll.scroll_to(offset);
                true
            }
            None => false,
        }
    }

    /// The navigation's scrollbar dragged to `p`: `true` if it moved.
    pub fn drag_nav_bar(&mut self, p: Point) -> bool {
        match self.nav_bar.drag(p) {
            Some(offset) => self.nav_scroll.scroll_to(offset),
            None => false,
        }
    }

    /// The navigation's scrollbar is dragged.
    pub fn nav_bar_held(&self) -> bool {
        self.nav_bar.is_dragging()
    }

    /// The mouse released: `true` if the navigation's scrollbar was held.
    pub fn release_nav_bar(&mut self) -> bool {
        self.nav_bar.release()
    }

    /// Its navigation bar as a control (`ui_components::control`).
    pub fn nav_grab(&mut self) -> NavGrab<'_> {
        NavGrab(self)
    }

    /// The header's field, left of `esc` (from the last paint; empty without one).
    pub fn header_field(&self) -> Rect {
        self.header_field
    }

    /// The content area (from the last paint).
    pub fn content(&self) -> Rect {
        self.content
    }

    /// What is at point `p`. A click outside the panel does nothing: the modal closes
    /// only with × or Esc.
    /// The footer's status under `p` when it was cut short: all of it (a tooltip).
    pub fn status_tip(&self, p: Point) -> Option<(Rect, String)> {
        self.status_tip.as_ref().filter(|(r, _)| r.contains(p)).cloned()
    }

    pub fn hit(&self, p: Point) -> Option<ModalHit> {
        if !self.open || !self.panel.contains(p) {
            return None;
        }
        if self.close.contains(p) {
            return Some(ModalHit::Close);
        }
        if let Some(i) = self.buttons.iter().position(|r| r.contains(p)) {
            return Some(ModalHit::Button(i));
        }
        if let Some((_, step)) = self.zoom.iter().find(|(r, _)| r.contains(p)) {
            return Some(ModalHit::Zoom(*step));
        }
        if let Some((_, id)) = self.sections.iter().find(|(r, _)| r.contains(p)) {
            return Some(ModalHit::Section(id));
        }
        self.content.contains(p).then_some(ModalHit::Content)
    }

    /// `true` if the item under the mouse changed (close / section).
    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p).filter(|h| *h != ModalHit::Content && (self.panel.contains(p)));
        let changed = hover != self.hover;
        self.hover = hover;
        let bar = self.nav_overflows && self.nav_bar.hover(p);
        changed || bar
    }

    /// The section next to the active one (↑ / ↓), if any.
    pub fn step_section(
        &self,
        sections: &[ModalSection<'_>],
        active: &str,
        delta: isize,
    ) -> Option<&'static str> {
        let i = sections.iter().position(|s| s.id == active)? as isize;
        let next = ui_kit::step_index(i as usize, delta, sections.len());
        Some(sections[next].id)
    }

    /// Draws the frame and returns the content area and the context (with the appearance
    /// opacity).
    pub fn paint<'p>(
        &mut self,
        spec: &ModalSpec<'_>,
        icons: &Icons,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &'p Palette,
    ) -> Option<(Rect, Ctx<'p>)> {
        self.sections.clear();
        self.buttons.clear();
        self.status_tip = None;
        if !self.open {
            return None;
        }
        let t = self.appear.value();
        let ctx = Ctx::new(palette, self.scale).with_opacity(t);
        let px = |v: f32| ctx.px(v);
        let v = self.viewport;
        let (w, h) = spec.size.logical();
        let width = match spec.fit_width {
            Some(least) => {
                // The title's own width with what's beside it: the margins, the icon's tile,
                // the gap before `esc` and its cap.
                let title_font = ctx.ui_font(3.0).with_weight(Font::SEMIBOLD);
                let title = if spec.title_widens {
                    text.shape_line(spec.title, &title_font).width
                } else {
                    0.0
                };
                let tile = if spec.icon.is_some() { px(38.0 + 14.0) } else { 0.0 };
                let chrome = px(22.0) * 2.0 + tile + px(16.0) + px(40.0);
                // The content inside the panel's border on both sides: as wide as it asked.
                let least = (px(least.min(w)) + 2.0 * ctx.line()).min(px(w));
                if spec.title_widens { (title + chrome).clamp(least, px(w)) } else { least }
            }
            None => px(w),
        }
        .min(v.size.width - px(48.0));
        let header_h = if spec.subtitle.is_some() { px(68.0) } else { px(56.0) };
        let footer_h = if !spec.buttons.is_empty() || spec.status.is_some() {
            px(ds::MODAL_FOOTER_H)
        } else if spec.hints.is_empty() {
            0.0
        } else {
            px(44.0)
        };
        let wanted =
            // The header's line and the border under the content.
            spec.fit.map_or(px(h), |fit| {
                (header_h + 2.0 * ctx.line() + px(fit) + footer_h).min(px(h))
            });
        let height = wanted.min(v.size.height - px(48.0));
        let lift = (1.0 - t) * px(10.0);
        let p = Rect::new(
            (v.min_x() + (v.size.width - width) / 2.0).round(),
            (v.min_y() + (v.size.height - height) / 2.0 + lift).round(),
            width.round(),
            height.round(),
        );
        self.panel = p;

        let layer = scene.push_layer(None);
        ds::backdrop(layer, &ctx, v);
        ds::surface(layer, &ctx, p, Elevation::High, radius::XL);
        let layer = scene.push_layer(Some(p));

        let frame = Frame {
            panel: p,
            pad: px(22.0),
            header_h,
            footer_h,
            // Inside the panel's border (the content never covers it, nor the footer's line).
            body: Rect::new(
                p.min_x() + ctx.line(),
                p.min_y() + header_h + ctx.line(),
                p.size.width - 2.0 * ctx.line(),
                p.size.height - header_h - 2.0 * ctx.line() - footer_h,
            ),
        };
        self.paint_header(spec, &frame, icons, text, layer, &ctx);
        if footer_h > 0.0 {
            self.paint_footer(spec, &frame, icons, text, scene, &ctx);
        }
        let content = if spec.sections.is_empty() {
            frame.body
        } else {
            self.paint_nav(spec, &frame, icons, text, scene, &ctx)
        };
        self.content = content;
        Some((content, ctx))
    }

    /// The footer's zoom, `− 100% +`, ending at `right` (each a step to click, lit on hover).
    fn paint_zoom(
        &mut self,
        layer: &mut Layer,
        (text, icons): (&mut TextSystem, &Icons),
        ctx: &Ctx<'_>,
        (right, mid): (f32, f32),
        level: f32,
    ) {
        let px = |v: f32| ctx.px(v);
        let font = ctx.ui_font(-0.5);
        let pct = format!("{}%", (level * 100.0).round());
        let widest = text.shape_line("000%", &font).width;
        let parts = [
            (ZoomStep::In, "+".to_owned(), px(22.0)),
            (ZoomStep::Reset, pct, widest + px(12.0)),
            (ZoomStep::Out, "−".to_owned(), px(22.0)),
        ];
        let h = px(24.0);
        let mut x = right;
        for (step, label, w) in parts {
            x -= w;
            let r = Rect::new(x.round(), (mid - h / 2.0).round(), w.round(), h);
            let spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
                .label(&label)
                .rest_tone(ds::Tone::Neutral)
                .hovered(self.hover == Some(ModalHit::Zoom(step)));
            ds::button(layer, text, icons, ctx, r, spec);
            self.zoom.push((r, step));
            x -= px(2.0);
        }
    }

    /// The header: the title (and subtitle, after an optional icon tile), closing with
    /// the `esc` key.
    fn paint_header(
        &mut self,
        spec: &ModalSpec<'_>,
        frame: &Frame,
        icons: &Icons,
        text: &mut TextSystem,
        layer: &mut Layer,
        ctx: &Ctx<'_>,
    ) {
        let px = |v: f32| ctx.px(v);
        let palette = ctx.palette;
        let (p, header_h, pad) = (frame.panel, frame.header_h, frame.pad);
        let mid = p.min_y() + header_h / 2.0;
        // Close: the `esc` key, clickable (with hover); the field left of it.
        let hovered = self.hover == Some(ModalHit::Close);
        self.close = if spec.closable {
            ds::keycap(layer, text, ctx, "esc", p.max_x() - pad, mid, hovered)
        } else {
            Rect::new(p.max_x() - pad, mid, 0.0, 0.0)
        };
        self.header_field = if spec.header_field > 0.0 {
            let w = px(spec.header_field).min(p.size.width * 0.45);
            Rect::new(
                (self.close.min_x() - px(14.0) - w).round(),
                (mid - px(17.0)).round(),
                w.round(),
                px(34.0),
            )
        } else {
            Rect::default()
        };
        let right = if spec.header_field > 0.0 {
            self.header_field.min_x() - px(16.0)
        } else {
            self.close.min_x() - px(16.0)
        };
        let title_font = ctx.ui_font(3.0).with_weight(Font::SEMIBOLD);
        let tm = text.metrics(&title_font);
        let sub_font = ctx.ui_font(-1.0);
        let sm = text.metrics(&sub_font);
        let block = tm.ascent
            + tm.descent
            + spec.subtitle.map_or(0.0, |_| px(4.0) + sm.ascent + sm.descent);
        let top = p.min_y() + (header_h - block) / 2.0;
        let mut text_x = p.min_x() + pad;
        if let Some(id) = spec.icon.and_then(|name| icons.id(name)) {
            let tile = px(38.0);
            let r = Rect::new(text_x, (mid - tile / 2.0).round(), tile, tile);
            layer.quad(
                Quad::filled(r, ctx.fade(palette.accent.with_alpha(0.12))).rounded(px(radius::LG)),
            );
            let size = px(20.0);
            let at = Rect::new(
                (r.min_x() + (tile - size) / 2.0).round(),
                (r.min_y() + (tile - size) / 2.0).round(),
                size,
                size,
            );
            draw_icon(layer, icons, id, at, ctx.fade(palette.accent));
            text_x = r.max_x() + px(14.0);
        }
        // Cut where the field (or `esc`) starts: never under it.
        let room = (right - text_x).max(0.0);
        let title = ds::ellipsize(text, spec.title, &title_font, room);
        let title = text.shape_line(&title, &title_font);
        draw_text(layer, &title, text_x, (top + tm.ascent).round(), ctx.fade(palette.text));
        if let Some(subtitle) = spec.subtitle {
            let subtitle = ds::ellipsize(text, subtitle, &sub_font, room);
            let shaped = text.shape_line(&subtitle, &sub_font);
            let y = top + tm.ascent + tm.descent + px(4.0) + sm.ascent;
            draw_text(layer, &shaped, text_x, y.round(), ctx.fade(palette.text_muted));
        }
        ds::divider(layer, ctx, p.min_x(), p.max_x(), p.min_y() + header_h);
    }

    /// The footer, its background clipped to the panel's shape: the keys (or the status) on
    /// its left, the buttons on its right.
    fn paint_footer(
        &mut self,
        spec: &ModalSpec<'_>,
        frame: &Frame,
        icons: &Icons,
        text: &mut TextSystem,
        scene: &mut Scene,
        ctx: &Ctx<'_>,
    ) {
        let (p, footer_h) = (frame.panel, frame.footer_h);
        let footer = KeyFooter { panel: p, height: footer_h, pad: frame.pad, radius: radius::XL };
        self.zoom.clear();
        if spec.buttons.is_empty() && spec.status.is_none() {
            paint_key_footer(scene, text, ctx, footer, p.size.width / 2.0, spec.hints);
            if let Some(level) = spec.zoom {
                let layer = scene.push_layer(Some(p));
                let mid = p.max_y() - footer_h / 2.0;
                self.paint_zoom(layer, (text, icons), ctx, (p.max_x() - frame.pad, mid), level);
            }
            return;
        }
        paint_key_footer(scene, text, ctx, footer, 0.0, &[]);
        let layer = scene.push_layer(Some(p));
        let foot = Rect::new(p.min_x(), p.max_y() - footer_h, p.size.width, footer_h);
        let mid = foot.min_y() + footer_h / 2.0;
        let buttons: Vec<ds::FooterButton<'_>> = spec
            .buttons
            .iter()
            .enumerate()
            .map(|(i, b)| ds::FooterButton {
                label: &b.label,
                main: b.main,
                danger: b.danger,
                enabled: b.enabled,
                hovered: self.hover == Some(ModalHit::Button(i)),
                separated: b.separated,
                focused: b.focused,
                keyless: b.keyless,
            })
            .collect();
        let (areas, left) =
            ds::footer_buttons(layer, text, ctx, p.max_x() - frame.pad, mid, &buttons);
        self.buttons = areas;
        let x = p.min_x() + frame.pad;
        let room = (left - x).max(0.0);
        let palette = ctx.palette;
        let area = Rect::new(x, foot.min_y(), room, footer_h);
        match spec.status {
            Some(ui_components::ModalStatus::Busy(what)) => {
                let what = ds::ellipsize(text, what, &ctx.ui_font(-0.5), room - ctx.px(24.0));
                ds::loader(layer, text, ctx, x, mid, &ds::Loader::with_text(&what));
            }
            Some(ui_components::ModalStatus::Warning(what)) => {
                let font = ctx.ui_font(-0.5).with_weight(Font::SEMIBOLD);
                let at = StatusLine { area, mid, icon: None, color: palette.warning };
                self.status_tip = status_line(layer, text, icons, ctx, at, what, &font);
            }
            Some(ui_components::ModalStatus::Done(what)) => {
                let at = StatusLine { area, mid, icon: Some("check"), color: palette.vcs_added };
                self.status_tip =
                    status_line(layer, text, icons, ctx, at, what, &ctx.ui_font(-0.5));
            }
            Some(ui_components::ModalStatus::Note(what)) => {
                let at = StatusLine { area, mid, icon: None, color: palette.text_muted };
                self.status_tip =
                    status_line(layer, text, icons, ctx, at, what, &ctx.ui_font(-0.5));
            }
            Some(ui_components::ModalStatus::Failed(what)) => {
                let at = StatusLine { area, mid, icon: Some("alert"), color: palette.danger };
                self.status_tip =
                    status_line(layer, text, icons, ctx, at, what, &ctx.ui_font(-0.5));
            }
            None => {
                let row = Rect::new(x, foot.min_y() + ctx.line(), room, footer_h - ctx.line());
                let style = ctx.palette.key_hints(ctx.scale);
                ui_kit::paint_key_hints(layer, text, spec.hints, row, &style);
            }
        }
    }

    /// The section navigation, on the left (its groups under their captions); as tabs over
    /// the content when the window is narrow or the list wouldn't fit. Returns the content
    /// area.
    fn paint_nav(
        &mut self,
        spec: &ModalSpec<'_>,
        frame: &Frame,
        icons: &Icons,
        text: &mut TextSystem,
        scene: &mut Scene,
        ctx: &Ctx<'_>,
    ) -> Rect {
        let px = |v: f32| ctx.px(v);
        let body = frame.body;
        let groups = group_runs(spec.sections);
        let grouped = groups.iter().any(|g| !g.caption.is_empty());
        let captions = groups.iter().filter(|g| !g.caption.is_empty()).count() as f32;
        let list_h = px(8.0 + spec.nav_header + 8.0)
            + spec.sections.len() as f32 * px(NAV_ROW + 2.0)
            + captions * px(ui_components::form::NAV_CAPTION + 4.0)
            + (captions - 1.0).max(0.0) * px(NAV_GROUP_GAP);
        // Tabs over the content only when the window is narrow; a list taller than its room
        // scrolls (its scrollbar in a lane of its own, right of the rows).
        if grouped && frame.panel.size.width < px(NAV_TABS_BELOW) {
            self.nav_overflows = false;
            self.nav_scroll = ui_components::ScrollState::default();
            return self.paint_nav_tabs(
                spec,
                frame,
                &groups,
                &mut ui_components::ScenePainter::new(scene, text, icons, *ctx),
            );
        }
        let nav_w = px(200.0);
        let nav = Rect::new(body.min_x(), body.min_y(), nav_w, body.size.height);
        let _ = Self::paint_nav_background(nav, frame, scene, ctx);
        self.nav = nav;
        self.nav_overflows = list_h > nav.size.height + 0.5;
        if !self.nav_overflows {
            self.nav_scroll = ui_components::ScrollState::default();
        }
        self.nav_scroll.set_viewport(nav.size.height);
        // Its scrollbar lies on the dividing line (no lane of its own beside the rows).
        let lane = 0.0;
        let layer = scene.push_layer(Some(nav));
        let hovered = match self.hover {
            Some(ModalHit::Section(id)) => Some(id),
            _ => None,
        };
        let (x, w) = (nav.min_x() + px(10.0), nav_w - px(20.0) - lane);
        let top = self.nav_scroll.origin(nav.min_y());
        let mut y = top + px(8.0) + px(spec.nav_header);
        for (i, group) in groups.iter().enumerate() {
            let sections = &spec.sections[group.range.clone()];
            if !group.caption.is_empty() {
                // More room above each group but the first.
                if i > 0 {
                    y += px(NAV_GROUP_GAP);
                }
                let open = sections.iter().any(|s| Some(s.id) == spec.active);
                y = ui_components::form::nav_caption(
                    &mut ui_components::LayerPainter::new(layer, text, icons, *ctx),
                    (x, y, w),
                    group.caption,
                    open,
                );
            }
            let items: Vec<NavItem<'_>> = sections
                .iter()
                .map(|s| NavItem { icon: s.icon, label: s.label, count: None, dot: false })
                .collect();
            let selected = sections.iter().position(|s| Some(s.id) == spec.active);
            let lit = sections.iter().position(|s| Some(s.id) == hovered);
            let mut hits = Vec::new();
            y = ui_components::form::nav_list(
                &mut ui_components::LayerPainter::new(layer, text, icons, *ctx),
                ui_components::form::NavPlace { x, y, width: w, row_h: NAV_ROW },
                ui_components::form::Nav::new(&items, selected.unwrap_or(usize::MAX), lit),
                |i, r| {
                    // Only what shows is clickable (rows scrolled out aren't).
                    if let Some(shown) = r.intersect(&nav) {
                        hits.push((shown, sections[i].id));
                    }
                },
            );
            self.sections.extend(hits);
            y += px(4.0);
        }
        if self.nav_overflows {
            // What was drawn, from its top (the scroll's limit).
            self.nav_scroll.measure(top, y + px(4.0));
            let (content, offset) = (self.nav_scroll.content(), self.nav_scroll.offset());
            // On the line between the navigation and the page: one thin line, no gap.
            let line = (nav.max_x() + ctx.line() / 2.0, nav.min_y(), nav.size.height);
            self.nav_bar.paint_on_line(scene, ctx.palette, self.scale, line, content, offset);
        }
        Rect::new(
            nav.max_x() + ctx.line(),
            body.min_y(),
            body.max_x() - nav.max_x() - ctx.line(),
            body.size.height,
        )
    }

    /// The navigation as tabs over the content: the groups, then the chosen group's
    /// sections (a group's tab opens its first). Returns the content area under them.
    fn paint_nav_tabs(
        &mut self,
        spec: &ModalSpec<'_>,
        frame: &Frame,
        groups: &[GroupRun<'_>],
        pen: &mut ui_components::ScenePainter<'_>,
    ) -> Rect {
        let (scene, text, icons, ctx) = (&mut *pen.scene, &mut *pen.text, pen.icons, &pen.ds);
        let px = |v: f32| ctx.px(v);
        let body = frame.body;
        let room = if spec.nav_header > 0.0 { px(12.0 + spec.nav_header) } else { 0.0 };
        self.nav = Rect::new(body.min_x(), body.min_y(), body.size.width, room);
        let layer = scene.push_layer(Some(frame.panel));
        let pad = px(16.0);
        let (x, w) = (body.min_x() + pad, body.size.width - 2.0 * pad);
        let active = spec.sections.iter().position(|s| Some(s.id) == spec.active);
        let chosen = active.and_then(|a| groups.iter().position(|g| g.range.contains(&a)));
        let hovered = match self.hover {
            Some(ModalHit::Section(id)) => spec.sections.iter().position(|s| s.id == id),
            _ => None,
        };
        let group_items: Vec<NavItem<'_>> = groups
            .iter()
            .map(|g| NavItem { icon: g.icon, label: g.caption, count: None, dot: false })
            .collect();
        let lit = hovered.and_then(|h| groups.iter().position(|g| g.range.start == h));
        let mut hits = Vec::new();
        let y = ui_components::form::nav_tabs(
            &mut ui_components::LayerPainter::new(layer, text, icons, *ctx),
            (x, body.min_y() + room + px(6.0), w),
            ui_components::form::Nav::new(&group_items, chosen.unwrap_or(usize::MAX), lit),
            |i, r| hits.push((r, spec.sections[groups[i].range.start].id)),
        );
        let shown = &groups[chosen.unwrap_or(0).min(groups.len().saturating_sub(1))];
        let sections = &spec.sections[shown.range.clone()];
        let items: Vec<NavItem<'_>> = sections
            .iter()
            .map(|s| NavItem { icon: s.icon, label: s.label, count: None, dot: false })
            .collect();
        let selected = sections.iter().position(|s| Some(s.id) == spec.active);
        let lit =
            hovered.and_then(|h| h.checked_sub(shown.range.start)).filter(|i| *i < items.len());
        let y = ui_components::form::nav_tabs(
            &mut ui_components::LayerPainter::new(layer, text, icons, *ctx),
            (x, y + px(2.0), w),
            ui_components::form::Nav::new(&items, selected.unwrap_or(usize::MAX), lit),
            |i, r| hits.push((r, sections[i].id)),
        );
        // A page's tab before its group's (the group's opens the same page).
        hits.reverse();
        self.sections.extend(hits);
        Rect::new(body.min_x(), y, body.size.width, (body.max_y() - y).max(0.0))
    }

    /// The navigation's background and its border on the right; returns the layer its
    /// sections are drawn on.
    fn paint_nav_background<'s>(
        nav: Rect,
        frame: &Frame,
        scene: &'s mut Scene,
        ctx: &Ctx<'_>,
    ) -> &'s mut Layer {
        let palette = ctx.palette;
        let nav_w = nav.size.width;
        // The navigation on the theme's side-panel color. With no footer it reaches the
        // panel's bottom, so its corner follows the panel's (the rect runs past the
        // navigation up and right, clipped to it, so only that corner is round); above a
        // footer it's square.
        let fill = ctx.fade(palette.sidebar_background);
        let under = scene.push_layer(Some(nav));
        // Inside the panel's border (it stays drawn on the left edge).
        if frame.footer_h > 0.0 {
            let inside = Rect::new(
                nav.min_x() + ctx.line(),
                nav.min_y(),
                nav_w - ctx.line(),
                nav.size.height,
            );
            under.quad(Quad::filled(inside, fill));
        } else {
            let r = ctx.px(radius::XL) - ctx.line();
            under.quad(
                Quad::filled(
                    Rect::new(
                        nav.min_x() + ctx.line(),
                        nav.min_y() - r,
                        nav_w + r,
                        nav.size.height + r - ctx.line(),
                    ),
                    fill,
                )
                .rounded(r),
            );
        }
        let layer = scene.push_layer(Some(frame.panel));
        ds::divider_vertical(layer, ctx, nav.max_x(), nav.min_y(), nav.max_y());
        layer
    }
}

/// Consecutive sections with the same group caption.
struct GroupRun<'a> {
    caption: &'a str,
    icon: &'static str,
    range: std::ops::Range<usize>,
}

fn group_runs<'a>(sections: &[ModalSection<'a>]) -> Vec<GroupRun<'a>> {
    let mut out: Vec<GroupRun<'a>> = Vec::new();
    for (i, s) in sections.iter().enumerate() {
        match out.last_mut() {
            Some(run) if run.caption == s.group => run.range.end = i + 1,
            _ => out.push(GroupRun { caption: s.group, icon: s.group_icon, range: i..i + 1 }),
        }
    }
    out
}

/// A key footer's place: the bottom `height` of `panel` (its corners `radius`, logical),
/// its hints from `pad` in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct KeyFooter {
    pub panel: Rect,
    pub height: f32,
    pub pad: f32,
    pub radius: f32,
}

/// The list windows' key footer (the modals', the menu drawer's): a faint strip at the
/// bottom of the panel, clipped to its rounded shape, a divider over it and the hints on it
/// (at most `width` wide).
pub(crate) fn paint_key_footer(
    scene: &mut Scene,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    footer: KeyFooter,
    width: f32,
    hints: &[KeyHint<'_>],
) {
    let (p, footer_h) = (footer.panel, footer.height);
    let foot = Rect::new(p.min_x(), p.max_y() - footer_h, p.size.width, footer_h);
    let fl = scene.push_layer(Some(foot));
    let inset = ctx.line();
    let shape =
        Rect::new(p.min_x() + inset, p.min_y(), p.size.width - 2.0 * inset, p.size.height - inset);
    fl.quad(
        Quad::filled(shape, ctx.fade(ctx.palette.text.with_alpha(0.022)))
            .rounded((ctx.px(footer.radius) - inset).max(0.0)),
    );
    let fl = scene.push_layer(Some(p));
    ds::divider(fl, ctx, foot.min_x(), foot.max_x(), foot.min_y());
    let row = Rect::new(
        foot.min_x() + footer.pad,
        foot.min_y() + ctx.line(),
        width,
        footer_h - ctx.line(),
    );
    ui_kit::paint_key_hints(fl, text, hints, row, &ctx.palette.key_hints(ctx.scale));
}

/// The panel's geometry, shared by the pieces of [`Modal::paint`].
struct Frame {
    panel: Rect,
    /// The header's and footer's side padding.
    pad: f32,
    header_h: f32,
    /// 0 without a footer.
    footer_h: f32,
    /// Between the header and the footer.
    body: Rect,
}

/// A modal's navigation bar as a control: pressed on its lane, its thumb is held (or the
/// navigation jumps there) and follows the pointer (see [`Modal::nav_grab`]).
#[derive(Debug)]
pub struct NavGrab<'a>(&'a mut Modal);

impl ui_components::control::Control for NavGrab<'_> {
    fn hit(&self, p: Point) -> bool {
        self.0.nav_overflows && self.0.nav_bar.contains(p)
    }

    fn press(&mut self, p: Point, _clicks: u32) -> ui_components::control::Press {
        if self.0.press_nav_bar(p) {
            ui_components::control::Press::Held
        } else {
            ui_components::control::Press::Ignored
        }
    }

    fn drag(&mut self, p: Point) -> bool {
        self.0.drag_nav_bar(p)
    }

    fn release(&mut self) -> bool {
        self.0.release_nav_bar()
    }
}

#[cfg(test)]
#[path = "../tests/unit/modal.rs"]
mod tests;
