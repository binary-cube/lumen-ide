//! Painting the log: the commits with their graph, the selected commit's details and files, the
//! wrapped message.

use super::*;

impl LogView {
    /// The toolbar: the title and what the log shows, then its buttons on the right.
    pub(super) fn paint_toolbar(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        bar: Rect,
    ) {
        layer.quad(Quad::filled(
            Rect::new(bar.min_x(), bar.max_y(), bar.size.width, ds.line()),
            ds.palette.card_border,
        ));
        let x = self.paint_heading(layer, text, ds, bar);
        let x = self.paint_where(layer, text, ds, bar, x);
        self.paint_buttons(layer, text, icons, ds, bar, x);
    }

    /// The toolbar's title and a loader while reading (a refresh, the next page). Returns
    /// where they end.
    pub(super) fn paint_heading(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        bar: Rect,
    ) -> f32 {
        let px = |v: f32| ds.px(v);
        let mid = bar.min_y() + bar.size.height / 2.0;
        let title_font = ds.ui_font(2.0).with_weight(Font::SEMIBOLD);
        let heading = if self.path.is_some() { t("git.history_title") } else { t("git.log_title") };
        let title = text.shape_line(&heading, &title_font);
        let mut x = bar.min_x() + px(20.0);
        draw_text(
            layer,
            &title,
            x,
            ds::optical_baseline(bar.min_y(), bar.size.height, &title_font),
            ds.palette.text,
        );
        x += title.width + px(12.0);
        // Reading (a refresh, the next page): a small loader by the title.
        if self.loading.is_some() {
            let spinner = ds::Loader { size: 14.0, ..ds::Loader::default() };
            let r = ds::loader(layer, text, ds, x, mid, &spinner);
            x = r.max_x() + px(12.0);
        }
        x
    }

    /// From `x`: the file whose history this is, or the branch and how it stands against
    /// its upstream. Returns where they end.
    pub(super) fn paint_where(
        &self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        bar: Rect,
        mut x: f32,
    ) -> f32 {
        let px = |v: f32| ds.px(v);
        let mid = bar.min_y() + bar.size.height / 2.0;
        if let Some(path) = &self.path {
            let r = ds::badge(
                layer,
                text,
                ds,
                ds::BadgeSpec { label: path, x, mid, align_right: false, tone: Tone::Accent },
            );
            x = r.max_x();
        } else if let Some(sync) = &self.sync {
            let branch = ds::badge(
                layer,
                text,
                ds,
                ds::BadgeSpec {
                    label: &sync.branch,
                    x,
                    mid,
                    align_right: false,
                    tone: Tone::Accent,
                },
            );
            x = branch.max_x() + px(8.0);
            let badge = crate::sync::sync_badge(sync);
            let label = match (&sync.upstream, badge.is_empty()) {
                (None, _) => t("git.no_upstream"),
                (Some(up), true) => tr!("git.in_sync", upstream = up.as_str()),
                (Some(up), false) => format!("{badge} · {up}"),
            };
            ds::badge(
                layer,
                text,
                ds,
                ds::BadgeSpec { label: &label, x, mid, align_right: false, tone: Tone::Neutral },
            );
        }
        x
    }

    /// Buttons on the right: Refresh, Fetch, Pull, Push — an icon and a label; Push
    /// stands out when there are commits to send, Pull when there are some to bring.
    /// Those that would come too close to `x` are left out.
    pub(super) fn paint_buttons(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        bar: Rect,
        x: f32,
    ) {
        let px = |v: f32| ds.px(v);
        let mid = bar.min_y() + bar.size.height / 2.0;
        let mut right = bar.max_x() - px(16.0);
        let (ahead, behind) = self.sync.as_ref().map_or((0, 0), |s| (s.ahead, s.behind));
        // From the right: the remote's (Push, Pull, Fetch, Refresh), a separator, Commit.
        let buttons = [
            Some((t("git.push"), "arrow-up", Hit::Command(crate::PUSH.0), ahead > 0)),
            Some((t("git.pull"), "arrow-down", Hit::Command(crate::PULL.0), behind > 0)),
            Some((t("git.fetch"), "download", Hit::Command(crate::FETCH.0), false)),
            Some((t("git.refresh"), "refresh", Hit::Refresh, false)),
            None,
            Some((t("git.commit_button"), "git-commit", Hit::Command(crate::COMMIT.0), false)),
        ];
        let h = px(ds::TOOLBAR_BUTTON_H);
        for button in buttons {
            let Some((label, icon, hit, lit)) = button else {
                if right - px(16.0) < x + px(160.0) {
                    break;
                }
                ds::divider_vertical(
                    layer,
                    ds,
                    (right - px(4.0)).round(),
                    mid - px(11.0),
                    mid + px(11.0),
                );
                right -= px(16.0);
                continue;
            };
            let w = ds::toolbar_button_width(text, ds, &label);
            let r = Rect::new((right - w).round(), (mid - h / 2.0).round(), w, h);
            if r.min_x() < x + px(160.0) {
                break;
            }
            let hovered = self.hover == Some(hit);
            let spec = ds::ToolbarButton { label: &label, icon, accent: lit, hovered };
            ds::toolbar_button(layer, text, icons, ds, r, spec);
            self.hits.push((r, hit));
            right = r.min_x() - px(8.0);
        }
    }

    pub(super) fn paint_rows(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        font: &Font,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let row_h = self.row_h;
        let lane_w = px(14.0);
        let widest = self.rows.iter().map(|r| r.graph.width).max().unwrap_or(1).min(MAX_LANES);
        let graph_x = self.list.min_x() + px(14.0);
        let graph_w = widest as f32 * lane_w + px(10.0);
        let small = ds.ui_font(-1.5);
        let date_w = if self.list.size.width < px(560.0) { px(96.0) } else { px(120.0) };
        // A narrow list keeps the summary: no author column.
        let author_w = if self.list.size.width < px(560.0) {
            0.0
        } else {
            px(150.0).min(self.list.size.width * 0.2)
        };
        let first = (self.scroll / row_h).floor() as usize;
        let visible = (self.list.size.height / row_h).ceil() as usize + 2;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        let lane_x = |lane: usize| graph_x + lane.min(MAX_LANES - 1) as f32 * lane_w + lane_w / 2.0;
        let line_w = px(1.6).max(1.0);
        for i in first..(first + visible).min(self.rows.len()) {
            let top = (self.list.min_y() + i as f32 * row_h - self.scroll).round();
            let row = Rect::new(self.list.min_x(), top, self.list.size.width, row_h);
            let selected = self.chosen.contains(&i);
            if selected {
                layer.quad(Quad::filled(row, p.accent.with_alpha(0.16)));
            } else if self.hover == Some(Hit::Row(i)) {
                layer.quad(Quad::filled(row, p.text.with_alpha(0.04)));
            }
            // Only the visible part: a row under the header must not take its clicks.
            if let Some(hit) = row.intersect(&self.list) {
                self.hits.push((hit, Hit::Row(i)));
            }
            let LogRow { entry, graph } = &self.rows[i];
            let mid = top + row_h / 2.0;
            paint_graph(layer, graph, top, row_h, line_w, &lane_x, p);
            // The commit: a dot (HEAD: a ring).
            let node = Point::new(lane_x(graph.column), mid);
            let r = px(4.5);
            let color = lane_color(p, graph.column);
            let head = entry.refs.iter().any(|r| r.kind == RefKind::Head);
            layer.quad(
                Quad::filled(Rect::new(node.x - r, node.y - r, r * 2.0, r * 2.0), color).rounded(r),
            );
            if head {
                let inner = r - px(2.0);
                layer.quad(
                    Quad::filled(
                        Rect::new(node.x - inner, node.y - inner, inner * 2.0, inner * 2.0),
                        p.editor_background,
                    )
                    .rounded(inner),
                );
            }

            // References, then the summary.
            let mut x = graph_x + graph_w;
            let text_right = self.list.max_x() - date_w - author_w - px(12.0);
            for label in entry.refs.iter().filter(|r| r.kind != RefKind::Head) {
                // The summary keeps room: the references give way.
                if x > text_right - px(200.0) {
                    break;
                }
                let tone = match label.kind {
                    RefKind::Branch => Tone::Accent,
                    _ => Tone::Neutral,
                };
                let name = label.name.clone();
                let badge = ds::badge(
                    layer,
                    text,
                    ds,
                    ds::BadgeSpec { label: &name, x, mid, align_right: false, tone },
                );
                x = badge.max_x() + px(6.0);
            }
            let summary =
                ds::ellipsize(text, &entry.commit.summary, font, (text_right - x).max(0.0));
            let shaped = text.shape_line(&summary, font);
            let base = ds::optical_baseline(top, row_h, font);
            draw_text(
                layer,
                &shaped,
                x.round(),
                base,
                if selected { p.text } else { p.text.with_alpha(0.92) },
            );

            // Author and age, right-aligned columns.
            let small_base = ds::optical_baseline(top, row_h, &small);
            if author_w > 0.0 {
                let author = ds::ellipsize(text, &entry.commit.author, &small, author_w - px(12.0));
                let shaped = text.shape_line(&author, &small);
                draw_text(
                    layer,
                    &shaped,
                    (self.list.max_x() - date_w - author_w).round(),
                    small_base,
                    p.text_muted,
                );
            }
            let age = i18n::ago(now - entry.commit.time);
            let shaped = text.shape_line(&age, &small);
            draw_text(
                layer,
                &shaped,
                (self.list.max_x() - px(14.0) - shaped.width).round(),
                small_base,
                p.text_muted,
            );
        }
    }
}

/// The lanes' colors (a lane keeps its color: lines are easy to follow).
pub(super) fn lane_color(p: &Palette, lane: usize) -> Rgba {
    let colors = [
        p.accent,
        p.terminal_ansi[2],
        p.terminal_ansi[3],
        p.terminal_ansi[5],
        p.terminal_ansi[6],
        p.terminal_ansi[1],
        p.terminal_ansi[4],
    ];
    colors[lane % colors.len()]
}

/// A line `a → b`, `w` thick, as round dots (any angle, crisp enough at row scale).
pub(super) fn stroke(layer: &mut Layer, a: Point, b: Point, w: f32, color: Rgba) {
    if (a.x - b.x).abs() < 0.5 {
        let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
        layer.quad(Quad::filled(Rect::new(a.x - w / 2.0, y0, w, y1 - y0), color));
        return;
    }
    let len = (b.x - a.x).hypot(b.y - a.y);
    let steps = (len / (w * 0.45).max(0.5)).ceil() as usize;
    for s in 0..=steps {
        let t = s as f32 / steps.max(1) as f32;
        // Eased across: the line leaves and arrives vertically (a soft S).
        let e = t * t * (3.0 - 2.0 * t);
        let q = Point::new(a.x + (b.x - a.x) * e, a.y + (b.y - a.y) * t);
        layer.quad(
            Quad::filled(Rect::new(q.x - w / 2.0, q.y - w / 2.0, w, w), color).rounded(w / 2.0),
        );
    }
}

/// `message` wrapped at words to `width` (a word longer than the line is cut); the first
/// paragraph bold. All of it: the details scroll.
pub(super) fn wrap_message(
    text: &mut TextSystem,
    message: &str,
    bold: &Font,
    body: &Font,
    width: f32,
) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    let mut in_title = true;
    for paragraph in message.lines() {
        if paragraph.trim().is_empty() {
            in_title = false;
            if out.last().is_some_and(|(l, _)| !l.is_empty()) {
                out.push((String::new(), false));
            }
            continue;
        }
        let font = if in_title { bold } else { body };
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let candidate =
                if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
            if line.is_empty() || text.shape_line(&candidate, font).width <= width {
                line = candidate;
            } else {
                out.push((ds::ellipsize(text, &line, font, width), in_title));
                line = word.to_owned();
            }
        }
        out.push((ds::ellipsize(text, &line, font, width), in_title));
    }
    while out.last().is_some_and(|(l, _)| l.is_empty()) {
        out.pop();
    }
    out
}

/// A row's graph: lanes running through, the lines from the commit's children above and to
/// its parents below.
pub(super) fn paint_graph(
    layer: &mut Layer,
    graph: &GraphRow,
    top: f32,
    row_h: f32,
    w: f32,
    lane_x: &dyn Fn(usize) -> f32,
    p: &Palette,
) {
    let (mid, bottom) = (top + row_h / 2.0, top + row_h);
    let node = lane_x(graph.column);
    for &lane in &graph.through {
        let x = lane_x(lane);
        stroke(layer, Point::new(x, top), Point::new(x, bottom), w, lane_color(p, lane));
    }
    for &lane in &graph.from_above {
        stroke(layer, Point::new(lane_x(lane), top), Point::new(node, mid), w, lane_color(p, lane));
    }
    for &lane in &graph.to_below {
        stroke(
            layer,
            Point::new(node, mid),
            Point::new(lane_x(lane), bottom),
            w,
            lane_color(p, lane),
        );
    }
}
