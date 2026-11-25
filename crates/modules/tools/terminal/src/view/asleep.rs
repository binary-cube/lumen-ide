//! What a sleeping terminal says over its last screen: a card with a moon, "Session asleep",
//! why (after how long idle, or by hand) and the memory it freed when known, and how to wake
//! it — a "Wake Up ↵" button, or any key, or a click in it. While a session woken from a
//! sleep comes back, the same card says it's resuming (no button). Drawn from the theme's
//! tokens; told to screen readers as a group with its button.

use super::*;
use ui_components::ds::{self, ButtonKind, ButtonSpec, Elevation, button_size, radius};

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
/// From the grid's edges, and the narrowest the card gets (logical pixels).
const CARD_MARGIN: f32 = ds::space::MD;
const CARD_MIN: f32 = 160.0;
/// Under the moon, over the button and under it (logical pixels).
const MOON_GAP: f32 = ds::space::SM;
const BUTTON_ABOVE: f32 = 14.0;
const BUTTON_BELOW: f32 = 6.0;

/// Why it sleeps, as it fell asleep: by hand or after how long idle, what it held.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Slept {
    pub(super) by_hand: bool,
    pub(super) idle: Option<std::time::Duration>,
    pub(super) freed: Option<u64>,
}

impl Slept {
    /// Why, in words: by hand or after N minutes, and about how much it freed.
    pub(super) fn why(&self) -> String {
        let why = if self.by_hand {
            i18n::t("terminal.asleep_by_hand")
        } else {
            let minutes = self.idle.map_or(0, |d| d.as_secs() / 60);
            i18n::tr!("terminal.asleep_idle", minutes = minutes)
        };
        match self.freed {
            Some(bytes) => {
                let freed = i18n::tr!("terminal.asleep_freed", size = i18n::bytes(bytes).as_str());
                format!("{why} · {freed}")
            }
            None => why,
        }
    }
}

/// What the snapshots say it freed (a made-up figure, the same every run).
#[cfg(feature = "snapshot")]
const SNAPSHOT_FREED: u64 = 412 * 1024 * 1024;

impl TerminalView {
    /// The app's `--snapshot`: put to sleep by hand now, as if it held [`SNAPSHOT_FREED`]
    /// (no owner needed: it resumes the way it started).
    #[cfg(feature = "snapshot")]
    pub(super) fn asleep_for_snapshot(&mut self) -> bool {
        let Some(launch) = self.launch.clone() else { return false };
        if let Some(slot) = &self.renamed {
            asked(slot).memory = Some(SNAPSHOT_FREED);
        }
        self.fall_asleep(launch, true);
        self.asleep
    }

    /// The card over its last screen (see the module); `resuming`: it's coming back (no
    /// button). Where its button went is kept ([`Self::wake_button`]).
    pub(super) fn paint_asleep(
        &mut self,
        layer: &mut Layer,
        (text, icons): (&mut TextSystem, &icons::Icons),
        (palette, scale): (&Palette, f32),
        resuming: bool,
    ) {
        let ctx = ds::Ctx::new(palette, scale);
        let px = |v: f32| ctx.px(v);
        let area = self.grid_area;
        let width = px(CARD_WIDTH).min((area.size.width - 2.0 * px(CARD_MARGIN)).max(px(CARD_MIN)));
        let height = 2.0 * px(CARD_PAD)
            + px(MOON)
            + px(MOON_GAP)
            + px(TITLE_LINE)
            + px(LINE)
            + if resuming {
                0.0
            } else {
                px(BUTTON_ABOVE) + px(BUTTON_H) + px(BUTTON_BELOW) + px(LINE)
            };
        let card = Rect::new(
            (area.min_x() + (area.size.width - width) / 2.0).round(),
            (area.min_y() + (area.size.height - height) / 2.0).max(area.min_y()).round(),
            width,
            height.round(),
        );
        let title =
            if resuming { i18n::t("terminal.resuming") } else { i18n::t("terminal.asleep_title") };
        let why = self.slept.map(|s| s.why()).unwrap_or_default();
        let describe = || {
            use ui_kit::access::{Described, Role};
            Described::new(Role::Group, title.as_str(), card).value(why.as_str())
        };
        let mut button = None;
        ui_kit::access::group(describe, || {
            // Opaque whatever the theme's popovers are: nothing under it shows through.
            let surface = palette.popup_background.with_alpha(1.0);
            ds::surface_with(layer, &ctx, card, Elevation::Medium, radius::LG, surface);
            let center = |w: f32| card.min_x() + (card.size.width - w) / 2.0;
            let mut y = card.min_y() + px(CARD_PAD);
            // The moon in a soft accent circle (the hints' look).
            let circle = Rect::new(center(px(MOON)), y, px(MOON), px(MOON));
            let tint = palette.accent.with_alpha(MOON_TINT);
            layer.quad(Quad::filled(circle, tint).rounded(px(MOON) / 2.0));
            if let Some(id) = icons.id(ASLEEP_ICON) {
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
            let shaped = text.shape_line(&title, &title_font);
            let base = ds::optical_baseline(y, px(TITLE_LINE), &title_font);
            draw_text(layer, &shaped, center(shaped.width), base, palette.text);
            y += px(TITLE_LINE);
            let small = ctx.ui_font(0.0);
            let room = card.size.width - 2.0 * px(CARD_PAD);
            let said = |(layer, text): (&mut Layer, &mut TextSystem), line: &str, y: f32| {
                let line = ds::ellipsize(text, line, &small, room);
                let shaped = text.shape_line(&line, &small);
                let base = ds::optical_baseline(y, px(LINE), &small);
                draw_text(layer, &shaped, center(shaped.width), base, palette.text_muted);
            };
            if !why.is_empty() {
                said((&mut *layer, &mut *text), &why, y);
            }
            y += px(LINE);
            if resuming {
                return;
            }
            y += px(BUTTON_ABOVE);
            let label = i18n::t("terminal.wake_up");
            let spec = ButtonSpec::new(ButtonKind::Primary).label(&label).key("↵");
            let w = ds::button_width(text, &ctx, &spec, px(BUTTON_H));
            let at = Rect::new(center(w).round(), y, w, px(BUTTON_H));
            ds::button(layer, text, icons, &ctx, at, spec);
            button = Some(at);
            y += px(BUTTON_H) + px(BUTTON_BELOW);
            said((layer, text), &i18n::t("terminal.asleep_how"), y);
        });
        self.wake_button = button;
    }
}
