//! A block's row of cards (`SettingsRow::Cards`): as the themes' on Appearance, each a
//! picture over its name and tag, the chosen one outlined, a click choosing it. The
//! pictures are the module's ([`CardPreview`]): painted after the content's layer (they
//! push layers of their own), clipped to the content; one live — the hovered card, else
//! the chosen one — asked again at its pace while the page shows, the others still. When
//! no card of a preview shows any more (another page, the window closed), it's told to let
//! go of what it made.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ui_components::{CardPaint, CardPreview};
use workbench_model::SettingsCard;

use super::rows::{BlockRows, Labelled};
use super::*;

/// A card's picture to paint once the content's layer is done.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PendingCard {
    pub preview: &'static str,
    pub value: i64,
    pub bounds: Rect,
    pub live: bool,
}

/// What the cards' pictures need between frames.
#[derive(Debug, Default)]
pub(super) struct CardPictures {
    pub previews: Vec<Arc<dyn CardPreview>>,
    pub pending: Vec<PendingCard>,
    /// The previews painted in the last frame (told to let go once none is).
    pub shown: Vec<&'static str>,
    /// The live picture's last frame and its pace.
    pub live: Option<(Instant, Duration)>,
}

/// Cards on a line at most.
const COLUMNS: usize = 4;

impl SettingsView {
    /// The modules' card previews (given once by the app).
    pub fn set_card_previews(&mut self, previews: Vec<Arc<dyn CardPreview>>) {
        self.pictures.previews = previews;
    }

    /// When the live card's next frame is due (`None`: none shows).
    pub fn card_deadline(&self) -> Option<Instant> {
        if !self.is_open() {
            return None;
        }
        self.pictures.live.map(|(at, every)| at + every)
    }

    /// Every preview shown lets go of what it made (the window closed).
    pub(super) fn release_cards(&mut self) {
        for id in std::mem::take(&mut self.pictures.shown) {
            if let Some(p) = self.pictures.previews.iter().find(|p| p.id() == id) {
                p.release();
            }
        }
        self.pictures.live = None;
        self.pictures.pending.clear();
    }

    /// A row of cards, under its label, as many on a line as fit (at most four).
    pub(super) fn paint_cards_row(
        &mut self,
        rows: &mut BlockRows<'_, '_>,
        row: Labelled<'_>,
        cards: &[SettingsCard],
        current: i64,
        preview: &'static str,
    ) {
        let ctx = rows.ctx;
        let palette = ctx.palette;
        let px = |v: f32| ctx.px(v);
        let gap = px(14.0);
        let room = rows.width - 2.0 * px(ui_components::form::ROW_PAD);
        let cols = cards.len().clamp(1, COLUMNS);
        let card_w = ((room - gap * (cols - 1) as f32) / cols as f32).floor();
        let picture_h = ((card_w - px(16.0)) * 0.625).round();
        let card_h = picture_h + px(44.0);
        let lines = cards.len().div_ceil(cols);
        let control_h = (card_h * lines as f32 + gap * (lines - 1) as f32) / ctx.scale;
        let area = self.setting_row(rows, &row, room, control_h);
        let hovered = cards.iter().map(|c| c.value).find(|v| {
            self.hover
                == Some(SettingsAction::Choose { block: rows.block, key: row.key, value: *v })
        });
        // One live picture: the hovered card's, else the chosen one's.
        let live = hovered.unwrap_or(current);
        let font = ctx.ui_font(0.0);
        for (i, card) in cards.iter().enumerate() {
            let (c, r) = (i % cols, i / cols);
            let rect = Rect::new(
                area.control.min_x() + c as f32 * (card_w + gap),
                area.control.min_y() + r as f32 * (card_h + gap),
                card_w,
                card_h,
            );
            let action =
                SettingsAction::Choose { block: rows.block, key: row.key, value: card.value };
            let selected = card.value == current;
            let (border, width) = if selected {
                (palette.accent, px(2.0))
            } else if hovered == Some(card.value) {
                (palette.text_muted.with_alpha(0.6), ctx.line())
            } else {
                (palette.border, ctx.line())
            };
            rows.layer.quad(
                Quad::filled(rect, ctx.fade(palette.popup_background))
                    .rounded(px(12.0))
                    .bordered(width, ctx.fade(border)),
            );
            let picture = Rect::new(
                rect.min_x() + px(8.0),
                rect.min_y() + px(8.0),
                card_w - px(16.0),
                picture_h,
            );
            rows.layer
                .quad(Quad::filled(picture, ctx.fade(palette.window_background)).rounded(px(4.0)));
            if self.pictures.previews.iter().any(|p| p.id() == preview) {
                let live = card.value == live && !self.pictures.pending.iter().any(|p| p.live);
                self.pictures.pending.push(PendingCard {
                    preview,
                    value: card.value,
                    bounds: picture,
                    live,
                });
            }
            let name_row =
                Rect::new(rect.min_x(), picture.max_y(), card_w, rect.max_y() - picture.max_y());
            let baseline = ds::optical_baseline(name_row.min_y(), name_row.size.height, &font);
            let tag_w = card
                .tag
                .as_ref()
                .map_or(0.0, |t| rows.text.shape_line(t, &ctx.ui_font(-1.5)).width + px(36.0));
            let name =
                ds::ellipsize(rows.text, &card.label, &font, (card_w - px(14.0) - tag_w).max(0.0));
            let label = rows.text.shape_line(&name, &font);
            draw_text(
                rows.layer,
                &label,
                rect.min_x() + px(14.0),
                baseline,
                ctx.fade(palette.text),
            );
            if let Some(tag) = &card.tag {
                ds::badge(
                    rows.layer,
                    rows.text,
                    ctx,
                    ds::BadgeSpec {
                        label: tag,
                        x: rect.max_x() - px(12.0),
                        mid: name_row.min_y() + name_row.size.height / 2.0,
                        align_right: true,
                        tone: ds::Tone::Accent,
                    },
                );
            }
            self.block_hits.push((rect, action));
        }
    }

    /// The pictures recorded while the content was painted, each in its own layers clipped
    /// to the content (and to its card's picture); the previews no card shows any more let
    /// go.
    pub(super) fn paint_card_pictures(
        &mut self,
        scene: &mut Scene,
        text: &mut TextSystem,
        palette: &Palette,
        scale: f32,
    ) {
        let pending = std::mem::take(&mut self.pictures.pending);
        let content = self.content;
        let mut shown: Vec<&'static str> = Vec::new();
        let mut live = None;
        for card in &pending {
            let Some(preview) = self.pictures.previews.iter().find(|p| p.id() == card.preview)
            else {
                continue;
            };
            let Some(clip) = card.bounds.intersect(&content) else { continue };
            if !shown.contains(&card.preview) {
                shown.push(card.preview);
            }
            preview.paint(
                card.value,
                card.live,
                CardPaint { bounds: card.bounds, clip, scale, scene, text, palette },
            );
            if card.live {
                live = Some((Instant::now(), preview.frame_interval()));
            }
        }
        for id in &self.pictures.shown {
            if !shown.contains(id)
                && let Some(p) = self.pictures.previews.iter().find(|p| p.id() == *id)
            {
                p.release();
            }
        }
        self.pictures.shown = shown;
        self.pictures.live = live;
        self.pictures.pending = pending;
        self.pictures.pending.clear();
    }
}
