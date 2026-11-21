//! The details on the right: on top the chosen commits' changed files as a
//! tree (`crate::files_tree`), under them the commit — its hash and date, its author, its
//! whole message, the branches holding it — or, with several chosen, how many and their
//! summaries; "Select a commit…" while none is.

use super::*;

/// The share of the panel the files take at first (the rest: the commit), their least
/// height and the commit's (logical pixels).
pub(super) const FILES_SHARE: f32 = 0.55;
const FILES_MIN: f32 = 100.0;
const COMMIT_MIN: f32 = 90.0;

impl LogView {
    pub(super) fn paint_details(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        panel: Rect,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        // Its tint is the view's (under the splitter, up to the seam's middle).
        let layer = scene.push_layer(Some(panel));
        if self.chosen.is_empty() {
            self.details_panel = Rect::default();
            self.detail_room = Rect::default();
            ds::empty_state(layer, text, ds, panel, &t("git.select_commit"), None);
            return;
        }
        self.details_area = panel;
        let files_h = self.files_height(panel);
        let files = Rect::new(
            panel.min_x() + ds.line(),
            panel.min_y(),
            panel.size.width - ds.line(),
            files_h,
        );
        self.hits.push((files, Hit::Files));
        let loading = self.details_all.is_empty() && self.details_loading.is_some();
        if loading {
            let area = Rect::new(files.min_x(), files.min_y(), files.size.width, px(120.0));
            ds::empty_state(layer, text, ds, area, &t("git.log_loading"), None);
        } else {
            let queries = self.queries.clone();
            let focused = self.files_focused();
            self.tree.paint(scene, text, icons, ds, (files, focused), queries.as_deref());
        }
        let below = Rect::new(
            panel.min_x(),
            files.max_y(),
            panel.size.width,
            (panel.max_y() - files.max_y()).max(0.0),
        );
        let layer = scene.push_layer(Some(below));
        ds::divider(layer, ds, below.min_x(), below.max_x(), below.min_y());
        self.details_panel = below;
        if self.chosen.len() > 1 {
            self.paint_several(scene, text, ds, below);
        } else {
            self.paint_commit(scene, (text, icons), ds, below);
        }
        // The separator between them: dragged, a double click back to the default.
        let strip = px(8.0);
        let area = Rect::new(
            below.min_x(),
            (below.min_y() - strip / 2.0).round(),
            below.size.width,
            strip,
        );
        let style = SplitterStyle::from_palette(p);
        let splitter = self.files_split.get_or_insert_with(|| {
            Splitter::new(style.clone(), ui_components::Orientation::Horizontal)
        });
        splitter.set_style(style);
        splitter.set_bounds(area, ds.scale);
        splitter.paint(scene);
    }

    /// The files' height in `panel`: their share, leaving both parts their least room.
    pub(super) fn files_height(&self, panel: Rect) -> f32 {
        let s = self.scale.max(0.1);
        let share = if self.files_share > 0.0 { self.files_share } else { FILES_SHARE };
        let max = (panel.size.height - COMMIT_MIN * s).max(0.0);
        (panel.size.height * share).max(FILES_MIN * s).min(max).round()
    }

    /// The separator dragged to `y` (physical): the files' share follows.
    pub(super) fn drag_files_split(&mut self, y: f32) {
        let panel = self.details_area;
        if panel.size.height <= 0.0 {
            return;
        }
        self.files_share = ((y - panel.min_y()) / panel.size.height).clamp(0.1, 0.9);
    }

    /// Back to the default share (a double click on the separator).
    pub(super) fn reset_files_split(&mut self) {
        self.files_share = FILES_SHARE;
        self.keep_widths();
    }

    /// One commit: its id and date, its author (with `esc`), then what scrolls — its whole
    /// message, the branches holding it.
    fn paint_commit(
        &mut self,
        scene: &mut render_api::Scene,
        (text, icons): (&mut TextSystem, &icons::Icons),
        ds: &Ctx<'_>,
        below: Rect,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(below));
        let close = ds::keycap(
            layer,
            text,
            ds,
            "esc",
            below.max_x() - px(14.0),
            below.min_y() + px(22.0),
            self.hover == Some(Hit::CloseDetails),
        );
        self.hits.push((close, Hit::CloseDetails));
        let x = below.min_x() + px(18.0);
        let Some(details) = self.details.clone() else {
            self.detail_room = Rect::default();
            return;
        };
        let lane = if self.detail_scroll.overflows() { px(10.0) } else { 0.0 };
        let width = below.size.width - px(36.0) - lane;
        let mut y = below.min_y() + px(12.0);
        let small = ds.ui_font(-1.5);
        let body = ds.ui_font(-0.5);
        let mono = ds.code_font(-1.5);
        let (yy, m, d) = crate::local_time::date(details.commit.time);
        let id = text.shape_line(&details.commit.id[..details.commit.id.len().min(10)], &mono);
        draw_text(layer, &id, x, ds::optical_baseline(y, px(20.0), &mono), p.accent);
        let when = text.shape_line(&format!("{yy}-{m:02}-{d:02}"), &small);
        if x + id.width + px(10.0) + when.width < close.min_x() - px(8.0) {
            draw_text(
                layer,
                &when,
                x + id.width + px(10.0),
                ds::optical_baseline(y, px(20.0), &small),
                p.text_muted,
            );
        }
        y += px(26.0);
        let author = if details.email.is_empty() {
            details.commit.author.clone()
        } else {
            format!("{} <{}>", details.commit.author, details.email)
        };
        let author = ds::ellipsize(text, &author, &small, width);
        let shaped = text.shape_line(&author, &small);
        draw_text(layer, &shaped, x, ds::optical_baseline(y, px(18.0), &small), p.text_muted);
        y += px(22.0);
        // A file's history: every file of the commit, or that one only.
        if self.path.is_some() && details.files.len() > 1 {
            let label = if self.all_files { t("git.only_this_file") } else { t("git.all_files") };
            let spec = ButtonSpec::new(ButtonKind::Ghost)
                .label(&label)
                .tone(Tone::Accent)
                .hovered(self.hover == Some(Hit::AllFiles));
            let h = px(button_size::SMALL);
            let link = Rect::new(x - px(6.0), y, ds::button_width(text, ds, &spec, h), h);
            ds::button(layer, text, icons, ds, link, spec);
            self.hits.push((link, Hit::AllFiles));
            y += px(26.0);
        }
        let room = Rect::new(below.min_x(), y, below.size.width, (below.max_y() - y).max(0.0));
        self.detail_room = room;
        self.detail_scroll.set_viewport(room.size.height);
        let top = self.detail_scroll.origin(room.min_y());
        let layer = scene.push_layer(Some(room));
        let mut y = top + px(4.0);
        let title_font = body.clone().with_weight(Font::SEMIBOLD);
        let key = (details.commit.id.clone(), width.to_bits());
        if self.message.as_ref().is_none_or(|(k, _)| *k != key) {
            let lines =
                super::paint::wrap_message(text, &details.message, &title_font, &body, width);
            self.message = Some((key, lines));
        }
        let line_h = px(20.0);
        for (line, bold) in self.message.as_ref().map(|(_, l)| l.as_slice()).unwrap_or_default() {
            if y + line_h >= room.min_y() && y <= room.max_y() {
                let font = if *bold { &title_font } else { &body };
                let shaped = text.shape_line(line, font);
                draw_text(layer, &shaped, x, ds::optical_baseline(y, line_h, font), p.text);
            }
            y += line_h;
        }
        // The branches holding it.
        if !self.containing.is_empty() {
            y += px(10.0);
            let names = self.containing.join(", ");
            let label = tr!("git.contained_in", names = names.as_str());
            let lines = ds::wrap(text, &label, &small, width, 4);
            for line in lines {
                let shaped = text.shape_line(&line, &small);
                draw_text(
                    layer,
                    &shaped,
                    x,
                    ds::optical_baseline(y, px(18.0), &small),
                    p.text_muted,
                );
                y += px(18.0);
            }
        }
        self.detail_scroll.measure(top, y + px(10.0));
        let (content, offset) = (self.detail_scroll.content(), self.detail_scroll.offset());
        self.detail_bar.paint(scene, p, ds.scale, room, content, offset);
    }

    /// Several commits: how many, then each one's hash and summary (scrolling).
    fn paint_several(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        below: Rect,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(below));
        let x = below.min_x() + px(18.0);
        let strong = ds.ui_font(-0.5).with_weight(Font::SEMIBOLD);
        let title = tr!("git.commits_selected", count = self.chosen.len());
        let shaped = text.shape_line(&title, &strong);
        draw_text(
            layer,
            &shaped,
            x,
            ds::optical_baseline(below.min_y() + px(10.0), px(22.0), &strong),
            p.text,
        );
        let room = Rect::new(
            below.min_x(),
            below.min_y() + px(40.0),
            below.size.width,
            (below.size.height - px(40.0)).max(0.0),
        );
        self.detail_room = room;
        self.detail_scroll.set_viewport(room.size.height);
        let top = self.detail_scroll.origin(room.min_y());
        let layer = scene.push_layer(Some(room));
        let mono = ds.code_font(-1.5);
        let body = ds.ui_font(-0.5);
        let width = below.size.width - px(36.0);
        let mut y = top;
        let line_h = px(22.0);
        for &i in &self.chosen {
            let Some(row) = self.rows.get(i) else { continue };
            if y + line_h >= room.min_y() && y <= room.max_y() {
                let id = text.shape_line(row.entry.commit.short(), &mono);
                draw_text(layer, &id, x, ds::optical_baseline(y, line_h, &mono), p.accent);
                let sx = x + id.width + px(10.0);
                let summary = ds::ellipsize(
                    text,
                    &row.entry.commit.summary,
                    &body,
                    (x + width - sx).max(0.0),
                );
                let shaped = text.shape_line(&summary, &body);
                draw_text(layer, &shaped, sx, ds::optical_baseline(y, line_h, &body), p.text);
            }
            y += line_h;
        }
        self.detail_scroll.measure(top, y + px(10.0));
        let (content, offset) = (self.detail_scroll.content(), self.detail_scroll.offset());
        self.detail_bar.paint(scene, p, ds.scale, room, content, offset);
    }
}
