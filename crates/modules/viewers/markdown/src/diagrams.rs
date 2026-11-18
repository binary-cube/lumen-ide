//! A ```` ```mermaid ```` block in the preview: its diagram once the render pool has it
//! (the one shown before while a new one is worked out, so nothing jumps while typing),
//! fitted to the width and centered; an error as a card with the line it's on (a click
//! opens the source there); a type not drawn yet as a card with its source.

use std::sync::Arc;

use kernel::{Point, Rect};
use text_layout::{Font, TextSystem};

use crate::jobs;
use crate::layout::{Hit, Item};
use crate::mermaid::scene::{Anchor, Prim, Scene};
use crate::mermaid::{self, Failure, Look, Rendered};
use crate::walk::Walk;
use diagram_text::mermaid::ErrorKind;

/// A diagram of the document, as the viewer needs it (open it larger, copy it).
#[derive(Clone)]
pub struct DiagramBlock {
    pub rect: Rect,
    pub scene: Option<Arc<Scene>>,
    pub source: String,
    pub title: Option<String>,
}

/// Lays out the diagram of `body` (the block's fence on `line` of the document, 0-based).
pub(crate) fn block(w: &mut Walk<'_>, body: &str, line: usize, x0: f32) {
    let ordinal = w.diagram_ordinal();
    let palette = w.b.palette;
    let look = Look::of(palette);
    let key = jobs::key(&(body, look.key()));
    let job = {
        let (body, look) = (body.to_owned(), look.clone());
        move || mermaid::render(&body, &look)
    };
    let outcome = w.caches.diagrams.get(key, job);
    let rendered = match outcome {
        Some(Ok(r)) => {
            w.caches.shown.insert(ordinal, r.clone());
            Some(r)
        }
        Some(Err(Failure::Error(error))) => {
            w.caches.shown.remove(&ordinal);
            failure_card(w, body, line, &error, x0);
            return;
        }
        // Being worked out: the one shown before, if any.
        None => w.caches.shown.get(&ordinal).cloned(),
    };
    let Some(r) = rendered else {
        placeholder(w, body, x0);
        return;
    };
    place(w, &r, key, body, x0, &look);
}

/// The diagram fitted to the width (never enlarged), centered.
fn place(w: &mut Walk<'_>, r: &Rendered, key: u64, body: &str, x0: f32, look: &Look) {
    let s = w.b.s;
    let room = w.b.right - x0;
    let fit = (room / (r.scene.width * s)).min(1.0);
    let k = s * fit;
    let (dw, dh) = ((r.scene.width * k).round(), (r.scene.height * k).round());
    let rect = Rect::new((x0 + (room - dw) / 2.0).round(), w.b.y + w.b.px(4.0), dw, dh);
    // The shapes: in tiles, made where they're shown (when painting).
    w.b.items.push(Item::Diagram {
        rect,
        scene: Arc::clone(&r.scene),
        k,
        key: jobs::key(&(key, k.to_bits())),
    });
    let origin = Point::new(rect.min_x(), rect.min_y());
    for item in text_items(&r.scene, look, origin, k, w.b.text) {
        w.b.items.push(item);
    }
    let index = w.b.diagrams.len();
    w.b.hits.push((rect, Hit::Diagram(index)));
    // Selected whole; its source is what it copies.
    w.b.whole(rect, body.trim_end_matches('\n').to_owned());
    for (link, url) in &r.scene.links {
        let at = Rect::new(
            origin.x + link.min_x() * k,
            origin.y + link.min_y() * k,
            link.size.width * k,
            link.size.height * k,
        );
        w.b.hits.push((at, Hit::Link(url.clone())));
    }
    w.b.diagrams.push(DiagramBlock {
        rect,
        scene: Some(Arc::clone(&r.scene)),
        source: body.to_owned(),
        title: r.title.clone(),
    });
    w.b.y = rect.max_y() + w.b.px(4.0);
}

/// The scene's texts as the preview's items, at scale `k` from `origin`.
pub fn text_items(
    scene: &Scene,
    look: &Look,
    origin: Point,
    k: f32,
    text: &mut TextSystem,
) -> Vec<Item> {
    let mut out = Vec::new();
    for prim in &scene.prims {
        let Prim::Text(t) = prim else { continue };
        let family = if t.mono { look.mono.clone() } else { look.font.clone() };
        let weight = if t.bold { Font::SEMIBOLD } else { Font::NORMAL };
        let font = Font::new(family, (t.size * k * 4.0).round() / 4.0)
            .with_weight(weight)
            .with_italic(t.italic);
        let metrics = text.metrics(&font);
        let width = text.shape_line(&t.text, &font).width;
        let x = origin.x + t.at.x * k;
        let x = match t.anchor {
            Anchor::Start => x,
            Anchor::Middle => x - width / 2.0,
            Anchor::End => x - width,
        };
        let baseline = origin.y + t.at.y * k + (metrics.ascent - metrics.descent) / 2.0;
        out.push(Item::Text {
            x: x.round(),
            baseline: baseline.round(),
            text: t.text.clone(),
            font,
            color: t.color,
        });
    }
    out
}

/// While the first diagram of a block is worked out: a quiet box of a usual height.
fn placeholder(w: &mut Walk<'_>, body: &str, x0: f32) {
    let b = &mut w.b;
    let rect = Rect::new(x0, b.y, b.right - x0, b.px(120.0));
    b.whole(rect, body.trim_end_matches('\n').to_owned());
    b.items.push(Item::Rect { rect, color: b.palette.text.with_alpha(0.03), radius: b.px(8.0) });
    let font = Font::new(b.palette.ui_font.clone(), b.body);
    let label = i18n::t("markdown.mermaid_drawing");
    let width = b.text.shape_line(&label, &font).width;
    let baseline = rect.center().y + b.px(5.0);
    let color = b.palette.text_muted;
    b.items.push(Item::Text {
        x: (rect.center().x - width / 2.0).round(),
        baseline,
        text: label,
        font,
        color,
    });
    b.y = rect.max_y();
}

/// A block that can't be drawn: why (its line), and its source with that line marked
/// (clicking it opens the source there).
fn failure_card(
    w: &mut Walk<'_>,
    body: &str,
    fence: usize,
    error: &diagram_text::mermaid::Error,
    x0: f32,
) {
    let b = &mut w.b;
    let palette = b.palette;
    let unsupported = matches!(error.kind, ErrorKind::Unsupported(_) | ErrorKind::UnknownType(_));
    let accent = if unsupported { palette.text_muted } else { palette.danger };
    let pad = b.px(14.0);
    let top = b.y;
    let title_font = Font::new(palette.ui_font.clone(), b.body).with_weight(Font::SEMIBOLD);
    let code = Font::new(palette.code_font.clone(), (palette.code_font_size * b.s).round());
    let line_h = (code.size * 1.5).round();
    let title_h = (b.body * 1.6).round();
    let lines: Vec<&str> = body.trim_end_matches('\n').split('\n').collect();
    let height = pad * 2.0 + title_h + b.px(6.0) + lines.len() as f32 * line_h;
    let rect = Rect::new(x0, top, b.right - x0, height);
    let radius = b.px(8.0);
    b.items.push(Item::Rect { rect, color: accent.with_alpha(0.06), radius });
    let hairline = b.px(1.0).max(1.0);
    b.items.push(Item::Border { rect, color: accent.with_alpha(0.55), width: hairline, radius });
    let icon = b.px(16.0);
    let icon_rect = Rect::new(x0 + pad, top + pad + (title_h - icon) / 2.0, icon, icon);
    b.items.push(Item::Icon {
        rect: icon_rect,
        name: if unsupported { "info" } else { "circle-alert" },
        color: accent,
    });
    let title = mermaid::describe(error);
    let metrics = b.text.metrics(&title_font);
    let baseline =
        top + pad + ((title_h - (metrics.ascent + metrics.descent)) / 2.0 + metrics.ascent).round();
    b.items.push(Item::Text {
        x: x0 + pad + icon + b.px(8.0),
        baseline,
        text: title,
        font: title_font,
        color: accent,
    });
    let code_metrics = b.text.metrics(&code);
    let first = top + pad + title_h + b.px(6.0);
    for (i, line) in lines.iter().enumerate() {
        let row_top = first + i as f32 * line_h;
        let number = i + 1;
        if number == error.line {
            let row = Rect::new(x0 + hairline, row_top, rect.size.width - 2.0 * hairline, line_h);
            b.items.push(Item::Rect { rect: row, color: accent.with_alpha(0.14), radius: 0.0 });
            b.hits.push((row, Hit::Line(fence + number)));
        }
        let baseline = row_top
            + ((line_h - (code_metrics.ascent + code_metrics.descent)) / 2.0 + code_metrics.ascent)
                .round();
        let gutter = format!("{number:>3}");
        b.items.push(Item::Text {
            x: x0 + pad,
            baseline,
            text: gutter,
            font: code.clone(),
            color: palette.text_muted.with_alpha(0.7),
        });
        if !line.trim().is_empty() {
            let x = x0 + pad + code_metrics.advance * 5.0;
            b.items.push(Item::Text {
                x,
                baseline,
                text: line.replace('\t', "    "),
                font: code.clone(),
                color: palette.code_text,
            });
        }
    }
    b.code_blocks.push((rect, body.trim_end_matches('\n').to_owned()));
    b.whole(rect, body.trim_end_matches('\n').to_owned());
    b.y = rect.max_y();
}
