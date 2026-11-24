//! A step's card: its fonts, its lines (the object, the times, the rows, the cost), the
//! card and a folded one drawn, the tooltip's text.

use super::*;

/// The cards' fonts at scale `k` (diagram → screen).
#[derive(Debug)]
pub struct Fonts {
    title: Font,
    title_strong: Font,
    line: Font,
    line_strong: Font,
    code: Font,
    pill: Font,
}

impl Fonts {
    pub fn new(p: &Palette, k: f32) -> Self {
        let size = |delta: f32| ((p.ui_font_size + delta) * k).round().max(5.0);
        let title = Font::new(p.ui_font.clone(), size(-0.5));
        Self {
            title_strong: title.clone().with_weight(Font::SEMIBOLD),
            title,
            line: Font::new(p.ui_font.clone(), size(-1.5)),
            line_strong: Font::new(p.ui_font.clone(), size(-1.5)).with_weight(Font::SEMIBOLD),
            code: Font::new(p.code_font.clone(), ((p.code_font_size - 2.0) * k).round().max(5.0)),
            pill: Font::new(p.ui_font.clone(), size(-2.5)).with_weight(Font::SEMIBOLD),
        }
    }
}

/// How a card stands out.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Look {
    pub(super) selected: bool,
    pub(super) hovered: bool,
    pub(super) hot: bool,
    pub(super) dim: bool,
    pub(super) hot_spot: bool,
}

/// A piece of a card's line.
enum Piece {
    Text(String, Rgba, bool),
    Code(String),
    Pill(String, Rgba),
}

/// A step's lines (the same count for every card of the profile).
fn lines_of(report: &ProfileReport, id: NodeId, p: &Palette) -> Vec<(Vec<Piece>, Option<Piece>)> {
    let node = report.profile.node(id);
    let d = report.derived(id);
    let mut out = Vec::new();
    // What it works on.
    let mut what: Vec<Piece> = Vec::new();
    if let Some(object) = node.object.as_ref().filter(|o| o.kind != ObjectKind::Index) {
        what.push(Piece::Text(object.label(), p.text, true));
        if let Some(index) = &node.index {
            what.push(Piece::Text(index.clone(), p.text_muted, false));
        }
    } else if let Some(index) = &node.index {
        what.push(Piece::Text(index.clone(), p.text, true));
    } else if let Some(cond) = tree::condition(node) {
        what.push(Piece::Code(cond.to_owned()));
    } else if let Some(sub) = &node.subplan_name {
        what.push(Piece::Text(sub.clone(), p.text_muted, false));
    } else {
        what.push(Piece::Text(tree::op_kind_text(node.op), p.text_muted, false));
    }
    out.push((what, None));
    let measured = report.totals.measure.is_some();
    let timed = report.totals.measure == Some(database::plan::Measure::Time);
    if measured {
        let (own, all, share) = if timed {
            (d.self_ms.map(figures::ms), d.inclusive_ms.map(figures::ms), d.self_pct)
        } else {
            (
                d.self_cost.map(|c| figures::number(c.round())),
                node.estimates.total_cost.map(|c| figures::number(c.round())),
                d.cost_pct,
            )
        };
        let mut left = Vec::new();
        match own {
            Some(own) => left.push(Piece::Text(own, p.text, true)),
            None => left.push(Piece::Text("—".into(), p.text_muted, false)),
        }
        if let Some(all) = all {
            left.push(Piece::Text(format!("/ {all}"), p.text_muted, false));
        }
        let right = share.map(|s| Piece::Pill(figures::share(s), p.heat(s as f32)));
        out.push((left, right));
    }
    let has_rows =
        report.profile.nodes.iter().any(|n| n.estimates.rows.is_some() || n.actuals.rows.is_some());
    if has_rows {
        let est = node.estimates.rows.map(|r| r * node.actuals.loops.unwrap_or(1.0).max(1.0));
        let mut left = Vec::new();
        let mut right = None;
        match (est, d.total_rows) {
            (Some(est), Some(actual)) => {
                left.push(Piece::Text(figures::count(est), p.text_muted, false));
                left.push(Piece::Text("→".into(), p.text_muted, false));
                left.push(Piece::Text(figures::count(actual), p.text, true));
                let ratio = d.misestimate.unwrap_or(1.0);
                let off = if ratio >= 1.0 { ratio } else { 1.0 / ratio.max(1e-9) };
                if off >= 10.0 {
                    let l = if off >= 1000.0 { Level::Critical } else { Level::Warning };
                    right = Some(Piece::Pill(figures::factor(ratio), l.color(p)));
                }
            }
            (Some(est), None) => left.push(Piece::Text(figures::count(est), p.text, false)),
            (None, Some(actual)) => left.push(Piece::Text(figures::count(actual), p.text, true)),
            (None, None) => left.push(Piece::Text("—".into(), p.text_muted, false)),
        }
        left.insert(0, Piece::Text(i18n::t("profile.view.graph_rows"), p.text_muted, false));
        out.push((left, right));
    }
    let has_cost = report.profile.nodes.iter().any(|n| n.estimates.total_cost.is_some());
    if has_cost {
        let mut left = vec![Piece::Text(i18n::t("profile.view.graph_cost"), p.text_muted, false)];
        match node.estimates.total_cost {
            Some(c) => left.push(Piece::Text(figures::number(c.round()), p.text, false)),
            None => left.push(Piece::Text("—".into(), p.text_muted, false)),
        }
        let loops = node.actuals.loops.filter(|l| *l > 1.0);
        let right = loops.map(|l| {
            Piece::Text(
                i18n::tr!("profile.view.graph_loops", loops = figures::count(l).as_str()),
                p.text_muted,
                false,
            )
        });
        out.push((left, right));
    }
    out
}

pub(super) fn paint_card(
    pen: &mut ui_components::LayerPainter<'_>,
    fonts: &Fonts,
    (r, k): (Rect, f32),
    (report, id): (&ProfileReport, NodeId),
    finding: Option<&Finding>,
    look: Look,
) {
    let (layer, text, ds) = (&mut *pen.layer, &mut *pen.text, &pen.ds);
    let icons = pen.icons;
    let p = ds.palette;
    let node = report.profile.node(id);
    let d = report.derived(id);
    let share = d.self_pct.or(d.cost_pct);
    let heat = share.map_or(p.text_muted.with_alpha(0.3), |s| p.heat(s as f32));
    let severity = finding.map(|f| level(f.severity));
    let rad = (8.0 * k).round().max(2.0);
    let opacity = if look.dim { 0.35 } else { 1.0 };
    let fade = |c: Rgba| c.with_alpha(c.a * opacity);
    if look.selected {
        ds::surface(layer, ds, diagram::inflate(r, 1.0), ds::Elevation::Medium, rad);
    }
    let (border, bw) = if look.selected {
        (p.accent, (2.0 * ds.px(1.0)).round())
    } else if let Some(l) = severity {
        (l.color(p), (1.5 * ds.px(1.0)).round().max(1.0))
    } else if look.hovered {
        (p.accent.with_alpha(0.6), ds.line())
    } else if look.hot {
        (p.accent.with_alpha(0.45), ds.line())
    } else {
        (p.card_border, ds.line())
    };
    // The heat on top: the card filled with it, its body over all but a stripe.
    layer.quad(Quad::filled(r, fade(heat)).rounded(rad).bordered(bw, fade(border)));
    let stripe = (STRIPE * k).round().max(2.0);
    let body = Rect::new(
        r.min_x() + bw,
        r.min_y() + stripe,
        r.size.width - 2.0 * bw,
        r.size.height - stripe - bw,
    );
    let fill = match severity {
        Some(l) => p.editor_background.mix(l.color(p), 0.06),
        None => p.editor_background,
    };
    layer.quad(Quad::filled(body, fade(fill)).rounded((rad - bw).max(1.0)));
    if r.size.width < 70.0 * ds.px(1.0) {
        return;
    }
    // The header: the kind's icon, the engine's words, the sign in the corner.
    let header =
        Rect::new(r.min_x(), r.min_y() + stripe, r.size.width, (HEADER_H * k).round() - stripe);
    let mid = header.min_y() + header.size.height / 2.0;
    let s = (14.0 * k).round();
    let mut x = r.min_x() + 10.0 * k;
    if let Some(icon) = icons.id(tree::op_icon(node.op)) {
        let color = if look.hot { p.accent } else { p.text_muted };
        draw_icon(
            layer,
            icons,
            icon,
            Rect::new(x.round(), (mid - s / 2.0).round(), s, s),
            fade(color),
        );
    }
    x += s + 7.0 * k;
    let mut right = r.max_x() - 10.0 * k;
    let sign = match (severity, look.hot_spot) {
        (Some(l), _) => Some((l.icon(), l.color(p))),
        (None, true) => Some(("flame", heat)),
        _ => None,
    };
    if let Some((icon, color)) = sign
        && let Some(sid) = icons.id(icon)
    {
        let ss = (15.0 * k).round();
        let sr = Rect::new((right - ss).round(), (mid - ss / 2.0).round(), ss, ss);
        draw_icon(layer, icons, sid, sr, fade(color));
        right = sr.min_x() - 6.0 * k;
    }
    let mut words = node.name.clone();
    if let Some(join) = node.join
        && join != database::profile::node::JoinKind::Inner
        && !words.contains(join.words())
    {
        words = format!("{} {words}", join.words());
    }
    let title_font = if look.hot { &fonts.title_strong } else { &fonts.title };
    let shown = ds::ellipsize(text, &words, title_font, (right - x).max(0.0));
    let shaped = text.shape_line(&shown, title_font);
    draw_text(
        layer,
        &shaped,
        x.round(),
        ds::optical_baseline(header.min_y(), header.size.height, title_font),
        fade(p.text),
    );
    if node.actuals.never_executed {
        // Never run: said in its words' place when there's room after them.
        let note = text.shape_line(&i18n::t("profile.view.never_executed"), &fonts.pill);
        if x + shaped.width + 8.0 * k + note.width < right {
            draw_text(
                layer,
                &note,
                (x + shaped.width + 8.0 * k).round(),
                ds::optical_baseline(header.min_y(), header.size.height, &fonts.pill),
                fade(p.text_muted),
            );
        }
    }
    let divider_y = (r.min_y() + HEADER_H * k).round();
    ds::divider_in(
        layer,
        &ds.with_opacity(opacity),
        (r.min_x() + bw, r.max_x() - bw, divider_y),
        p.card_border.with_alpha(CARD_RULE),
    );
    if k < 0.42 * ds.px(1.0) {
        return;
    }
    // The lines.
    let mut y = divider_y + 4.0 * k;
    let lh = LINE_H * k;
    for (left, right_piece) in lines_of(report, id, p) {
        let base = ds::optical_baseline(y, lh, &fonts.line);
        let mut lx = r.min_x() + 12.0 * k;
        let mut end = r.max_x() - 12.0 * k;
        if let Some(piece) = right_piece {
            match piece {
                Piece::Pill(label, color) => {
                    // Zoomed with the card (its `k` over the screen's scale), dimmed with it.
                    let pr = ds::Badge::new(&label)
                        .color(readable_on(color, p))
                        .fill(color.with_alpha(CARD_PILL_FILL))
                        .font(&fonts.pill)
                        .size(ds::BadgeSize::Custom(CARD_PILL_H))
                        .pad(PILL_PAD)
                        .zoom(k / ds.scale)
                        .paint_right(layer, text, &ds.with_opacity(opacity), end, y + lh / 2.0);
                    end = pr.min_x() - 6.0 * k;
                }
                Piece::Text(label, color, _) => {
                    let shaped = text.shape_line(&label, &fonts.line);
                    draw_text(layer, &shaped, (end - shaped.width).round(), base, fade(color));
                    end -= shaped.width + 6.0 * k;
                }
                Piece::Code(_) => {}
            }
        }
        for (i, piece) in left.into_iter().enumerate() {
            if i > 0 {
                lx += 5.0 * k;
            }
            let room = end - lx;
            if room < 12.0 * k {
                break;
            }
            let (label, font, color) = match piece {
                Piece::Text(label, color, strong) => {
                    (label, if strong { &fonts.line_strong } else { &fonts.line }, color)
                }
                Piece::Code(label) => (label, &fonts.code, p.text_muted),
                Piece::Pill(label, color) => (label, &fonts.pill, color),
            };
            let shown = ds::ellipsize(text, &label, font, room);
            let shaped = text.shape_line(&shown, font);
            draw_text(layer, &shaped, lx.round(), base, fade(color));
            lx += shaped.width;
        }
        y += lh;
    }
}

/// A pill's words on a tint of `color`: the color itself, deepened on a light theme.
fn readable_on(color: Rgba, p: &Palette) -> Rgba {
    if p.is_dark() { color.mix(Rgba::WHITE, 0.15) } else { color.mix(Rgba::BLACK, 0.25) }
}

pub(super) fn paint_folded(
    pen: &mut ui_components::LayerPainter<'_>,
    fonts: &Fonts,
    (r, k): (Rect, f32),
    (report, under): (&ProfileReport, NodeId),
    count: usize,
    hovered: bool,
) {
    let (layer, text, ds) = (&mut *pen.layer, &mut *pen.text, &pen.ds);
    let p = ds.palette;
    let rad = r.size.height / 2.0;
    let border = if hovered { p.accent.with_alpha(0.7) } else { p.card_border };
    layer.quad(
        Quad::filled(r, p.text.with_alpha(if hovered { 0.07 } else { 0.035 }))
            .rounded(rad)
            .bordered(ds.line(), border),
    );
    let total = report.totals.time_ms.or(report.totals.cost).unwrap_or(0.0);
    let d = report.derived(under);
    let measure_time = report.totals.measure == Some(database::plan::Measure::Time);
    let weight =
        if measure_time { d.inclusive_ms } else { report.profile.node(under).estimates.total_cost };
    let label = match weight.filter(|_| total > 0.0) {
        Some(w) => i18n::tr!(
            "profile.view.folded_share",
            count = count as i64,
            share = figures::share((w / total).min(1.0)).as_str()
        ),
        None => i18n::tr!("profile.view.folded", count = count as i64),
    };
    let shown = ds::ellipsize(text, &label, &fonts.line, r.size.width - 20.0 * k);
    let shaped = text.shape_line(&shown, &fonts.line);
    draw_text(
        layer,
        &shaped,
        (r.min_x() + (r.size.width - shaped.width) / 2.0).round(),
        ds::optical_baseline(r.min_y(), r.size.height, &fonts.line),
        p.text_muted,
    );
}

/// A card's tooltip: the step's words and kind, its exact figures, its finding.
pub fn card_tip(report: &ProfileReport, id: NodeId, finding: Option<&Finding>) -> String {
    let node = report.profile.node(id);
    let d = report.derived(id);
    let mut lines = vec![export::head(node), tree::op_kind_text(node.op)];
    if let (Some(own), Some(share)) = (d.self_ms, d.self_pct) {
        lines.push(i18n::tr!(
            "profile.view.self_tip",
            own = figures::ms_exact(own).as_str(),
            share = figures::share_exact(share).as_str(),
            total = figures::ms_exact(report.totals.time_ms.unwrap_or(0.0)).as_str()
        ));
    } else if let (Some(own), Some(share)) = (d.self_cost, d.cost_pct) {
        lines.push(i18n::tr!(
            "profile.view.self_cost_tip",
            own = figures::number(own).as_str(),
            share = figures::share_exact(share).as_str()
        ));
    }
    if let Some(total) = d.inclusive_ms {
        let loops = node.actuals.loops.unwrap_or(1.0).max(1.0);
        lines.push(if loops > 1.0 {
            i18n::tr!(
                "profile.view.total_tip",
                total = figures::ms_exact(total).as_str(),
                per = figures::ms_exact(node.actuals.total_ms.unwrap_or(0.0)).as_str(),
                loops = figures::count(loops).as_str()
            )
        } else {
            format!("{}: {}", i18n::t("profile.view.col.total_time"), figures::ms_exact(total))
        });
    }
    let est = node.estimates.rows.map(|r| r * node.actuals.loops.unwrap_or(1.0).max(1.0));
    match (est, d.total_rows) {
        (Some(est), Some(actual)) => lines.push(i18n::tr!(
            "profile.view.rows_tip",
            expected = figures::count(est).as_str(),
            actual = figures::count(actual).as_str(),
            factor = figures::factor(d.misestimate.unwrap_or(1.0)).as_str()
        )),
        (Some(est), None) => lines
            .push(i18n::tr!("profile.view.rows_est_tip", expected = figures::count(est).as_str())),
        _ => {}
    }
    if let Some(total) = node.estimates.total_cost {
        lines.push(i18n::tr!(
            "profile.view.cost_tip",
            startup = figures::number(node.estimates.startup_cost.unwrap_or(0.0)).as_str(),
            total = figures::number(total).as_str()
        ));
    }
    if d.approx {
        lines.push(i18n::t("profile.view.approx"));
    }
    if let Some(f) = finding {
        let t = analysis::texts(f);
        lines.push(format!("{} · {}", analysis::severity(f.severity), t.title));
    }
    lines.join("\n")
}
