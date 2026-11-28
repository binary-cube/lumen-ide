//! What a sleeping view says over its last screen: a card with a moon, its title ("Session
//! asleep"), why (after how long left alone, or by hand) and the memory it freed when known,
//! and how to wake it — a "Wake Up ↵" button, or any key, or a click in it. While it comes
//! back, the same card says so (no button). Drawn from the theme's tokens; told to screen
//! readers as a group with its button.

use kernel::geometry::Rect;
use render_api::{Layer, Quad};
use text_layout::TextSystem;

use crate::ds::{self, ButtonKind, ButtonSpec, Elevation, button_size, radius};

/// The card's width at most, its padding, its icon, its lines' heights (logical pixels).
const CARD_WIDTH: f32 = 380.0;
const CARD_PAD: f32 = 18.0;
const MOON: f32 = 40.0;
/// The moon in its circle (logical pixels), and how strongly the circle is tinted.
const MOON_ICON: f32 = 20.0;
const MOON_TINT: f32 = 0.14;
const TITLE_LINE: f32 = 22.0;
const LINE: f32 = 18.0;
const BUTTON_H: f32 = button_size::LARGE;
/// From the area's edges, and the narrowest the card gets (logical pixels).
const CARD_MARGIN: f32 = ds::space::MD;
const CARD_MIN: f32 = 160.0;
/// Under the moon, over the button and under it (logical pixels).
const MOON_GAP: f32 = ds::space::SM;
const BUTTON_ABOVE: f32 = 14.0;
const BUTTON_BELOW: f32 = 6.0;

/// What the card says.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AsleepCard {
    pub title: String,
    /// Why it sleeps (empty: nothing said).
    pub why: String,
    /// The button's label (`None`: no button, it's coming back).
    pub button: Option<String>,
    /// The line under the button.
    pub how: String,
}

/// `card` drawn in the middle of `area`: where its button went (`None`: none).
pub fn paint_card(
    layer: &mut Layer,
    (text, icons): (&mut TextSystem, &icons::Icons),
    ctx: &ds::Ctx<'_>,
    area: Rect,
    card: &AsleepCard,
) -> Option<Rect> {
    let palette = ctx.palette;
    let px = |v: f32| ctx.px(v);
    let with_button = card.button.is_some();
    let width = px(CARD_WIDTH).min((area.size.width - 2.0 * px(CARD_MARGIN)).max(px(CARD_MIN)));
    let height = 2.0 * px(CARD_PAD)
        + px(MOON)
        + px(MOON_GAP)
        + px(TITLE_LINE)
        + px(LINE)
        + if with_button {
            px(BUTTON_ABOVE) + px(BUTTON_H) + px(BUTTON_BELOW) + px(LINE)
        } else {
            0.0
        };
    let bounds = Rect::new(
        (area.min_x() + (area.size.width - width) / 2.0).round(),
        (area.min_y() + (area.size.height - height) / 2.0).max(area.min_y()).round(),
        width,
        height.round(),
    );
    let describe = || {
        use ui_kit::access::{Described, Role};
        Described::new(Role::Group, card.title.as_str(), bounds).value(card.why.as_str())
    };
    let mut button = None;
    ui_kit::access::group(describe, || {
        // Opaque whatever the theme's popovers are: nothing under it shows through.
        let surface = palette.popup_background.with_alpha(1.0);
        ds::surface_with(layer, ctx, bounds, Elevation::Medium, radius::LG, surface);
        let center = |w: f32| bounds.min_x() + (bounds.size.width - w) / 2.0;
        let mut y = bounds.min_y() + px(CARD_PAD);
        // The moon in a soft accent circle (the hints' look).
        let circle = Rect::new(center(px(MOON)), y, px(MOON), px(MOON));
        layer.quad(
            Quad::filled(circle, palette.accent.with_alpha(MOON_TINT)).rounded(px(MOON) / 2.0),
        );
        if let Some(id) = icons.id(super::ASLEEP_ICON) {
            let at = Rect::new(
                center(px(MOON_ICON)),
                y + (px(MOON) - px(MOON_ICON)) / 2.0,
                px(MOON_ICON),
                px(MOON_ICON),
            );
            ui_kit::draw_icon(layer, icons, id, at, palette.accent);
        }
        y += px(MOON) + px(MOON_GAP);
        let title_font = ctx.ui_font(2.0).with_weight(600);
        let shaped = text.shape_line(&card.title, &title_font);
        let base = ds::optical_baseline(y, px(TITLE_LINE), &title_font);
        ui_kit::draw_text(layer, &shaped, center(shaped.width), base, palette.text);
        y += px(TITLE_LINE);
        let small = ctx.ui_font(0.0);
        let room = bounds.size.width - 2.0 * px(CARD_PAD);
        let said = |(layer, text): (&mut Layer, &mut TextSystem), line: &str, y: f32| {
            let line = ds::ellipsize(text, line, &small, room);
            let shaped = text.shape_line(&line, &small);
            let base = ds::optical_baseline(y, px(LINE), &small);
            ui_kit::draw_text(layer, &shaped, center(shaped.width), base, palette.text_muted);
        };
        if !card.why.is_empty() {
            said((&mut *layer, &mut *text), &card.why, y);
        }
        y += px(LINE);
        let Some(label) = &card.button else { return };
        y += px(BUTTON_ABOVE);
        let spec = ButtonSpec::new(ButtonKind::Primary).label(label).key("↵");
        let w = ds::button_width(text, ctx, &spec, px(BUTTON_H));
        let at = Rect::new(center(w).round(), y, w, px(BUTTON_H));
        ds::button(layer, text, icons, ctx, at, spec);
        button = Some(at);
        y += px(BUTTON_H) + px(BUTTON_BELOW);
        said((layer, text), &card.how, y);
    });
    button
}

/// How much of the background covers a sleeping view's last screen under its card (the
/// screen still seen, plainly behind it).
pub const SCRIM_ALPHA: f32 = 0.78;

/// A sleeping view's last screen (`picture`) in `area`, under its card: drawn at its own
/// size in pixels from the area's top left — never stretched or shrunk, whatever the area
/// became since (a smaller one cuts it at its edge, a larger one shows `background` beyond
/// it) — then dimmed with `background`. `layer` clips to `area`.
pub fn paint_picture(
    layer: &mut Layer,
    area: Rect,
    picture: Option<&render_api::ImageData>,
    background: render_api::Rgba,
) {
    layer.quad(Quad::filled(area, background));
    if let Some(picture) = picture {
        let at = Rect::new(area.min_x(), area.min_y(), picture.width as f32, picture.height as f32);
        layer.image(picture, at);
    }
    layer.quad(Quad::filled(area, background.with_alpha(SCRIM_ALPHA)));
}
