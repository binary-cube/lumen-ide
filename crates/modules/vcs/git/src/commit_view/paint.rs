//! The screen's parts: its header, the lists, the message.

use super::*;

impl CommitView {
    /// The header at `y`: the title, the branch, the refresh link.
    pub(super) fn paint_header(
        &mut self,
        layer: &mut render_api::Layer,
        text: &mut text_layout::TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        page: &Page,
        y: f32,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let Page { x0, width, ref small, .. } = *page;
        let title_font = ds.ui_font(4.0).with_weight(Font::SEMIBOLD);
        let title = text.shape_line(&t("git.commit_title"), &title_font);
        draw_text(layer, &title, x0, y + title_font.size, p.text);
        if let Some(branch) = &self.branch {
            ds::badge(
                layer,
                text,
                ds,
                ds::BadgeSpec {
                    label: branch,
                    x: x0 + title.width + px(space::MD),
                    mid: y + px(14.0),
                    align_right: false,
                    tone: Tone::Accent,
                },
            );
        }
        let refresh = text.shape_line(&t("git.refresh"), small);
        let rx = x0 + width - refresh.width;
        let size = px(14.0);
        if let Some(id) = icons.id("refresh") {
            let at = Rect::new(rx - px(6.0) - size, y + (px(28.0) - size) / 2.0, size, size);
            ui_kit::draw_icon(layer, icons, id, at, p.accent);
        }
        let room = refresh.width + px(18.0) + size;
        let refresh_rect = Rect::new(rx - px(12.0) - size, y, room, px(28.0));
        draw_text(layer, &refresh, rx, ds::optical_baseline(y, px(28.0), small), p.accent);
        self.hits.push((refresh_rect, Hit::Refresh));
        // Stash…, before it (while there's something to put aside).
        if !self.files.is_empty() {
            let stash = text.shape_line(&t("git.stash_link"), small);
            let sx = refresh_rect.min_x() - px(14.0) - stash.width;
            if let Some(id) = icons.id("archive-down") {
                let at = Rect::new(sx - px(6.0) - size, y + (px(28.0) - size) / 2.0, size, size);
                ui_kit::draw_icon(layer, icons, id, at, p.accent);
            }
            let room = stash.width + px(18.0) + size;
            let stash_rect = Rect::new(sx - px(12.0) - size, y, room, px(28.0));
            draw_text(layer, &stash, sx, ds::optical_baseline(y, px(28.0), small), p.accent);
            self.hits.push((stash_rect, Hit::Stash));
        }
    }

    /// A list in `card`: its head (a checkbox for all of it, its name, how many) and its files.
    pub(super) fn paint_section(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut text_layout::TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        (card, staged): (Rect, bool),
    ) {
        let px = |v: f32| ds.px(v);
        let p = ds.palette;
        let which = if staged { STAGED } else { UNSTAGED };
        let list = self.list(staged);
        let layer = scene.push_layer(Some(card));
        let name = if staged { t("git.staged_files") } else { t("git.unstaged_files") };
        let body = paint_card(layer, text, ds, card, (&name, px(36.0)));
        // Its keys at its bottom, when there's room for rows above them.
        let foot_h = px(LIST_KEYS_H);
        let foot = (body.size.height >= foot_h * 3.0)
            .then(|| Rect::new(body.min_x(), body.max_y() - foot_h, body.size.width, foot_h));
        let body = match foot {
            Some(_) => {
                Rect::new(body.min_x(), body.min_y(), body.size.width, body.size.height - foot_h)
            }
            None => body,
        };
        let head = Rect::new(card.min_x() + px(4.0), card.min_y(), px(30.0), px(CARD_HEAD));
        checkbox(layer, text, ds, head, staged && !list.is_empty(), !list.is_empty());
        ds::badge(
            layer,
            text,
            ds,
            ds::BadgeSpec {
                label: &list.len().to_string(),
                x: card.max_x() - px(10.0),
                mid: card.min_y() + px(CARD_HEAD) / 2.0,
                align_right: true,
                tone: if list.is_empty() { Tone::Neutral } else { Tone::Accent },
            },
        );
        if !list.is_empty() {
            self.hits.push((head, Hit::All(staged)));
        }
        let row_h = px(28.0);
        self.panes[which] = body;
        let content = list.len() as f32 * row_h + px(8.0);
        // Moved to by key: the shown file's row in view.
        if self.reveal
            && let Some(at) = self.shown_place(staged)
        {
            let top = px(4.0) + at as f32 * row_h;
            let room = (body.size.height - row_h - px(4.0)).max(0.0);
            // Its top in view, else its bottom (a list too short for a row: its top).
            let (lo, hi) = ((top - room).max(0.0), (top - px(4.0)).max(0.0));
            let scroll = &mut self.scrolls[which];
            *scroll = scroll.clamp(lo.min(hi), hi);
        }
        self.scrolls[which] = self.scrolls[which].min((content - body.size.height).max(0.0));
        let scroll = self.scrolls[which];
        if list.is_empty() {
            let empty = if staged { t("git.nothing_staged") } else { t("git.nothing_unstaged") };
            ds::empty_state(layer, text, ds, body, &empty, None);
        }
        let rows = scene.push_layer(Some(body));
        for (at, i) in list.into_iter().enumerate() {
            let y = body.min_y() + px(4.0) + at as f32 * row_h - scroll;
            if y + row_h >= body.min_y() && y <= body.max_y() {
                let row = Rect::new(body.min_x() + px(4.0), y, body.size.width - px(8.0), row_h);
                self.paint_file(rows, (text, icons), ds, (body, row), (i, staged));
            }
        }
        self.bars[which].set_auto_hiding(true);
        self.bars[which].paint(scene, p, ds.scale, body, content, scroll);
        if let Some(foot) = foot {
            self.paint_list_keys(scene, text, ds, (foot, staged));
        }
    }

    /// File `i`'s `row` in a list: its checkbox, state, name and folder; on what isn't staged,
    /// the button throwing its changes away.
    pub(super) fn paint_file(
        &mut self,
        layer: &mut render_api::Layer,
        (text, icons): (&mut text_layout::TextSystem, &icons::Icons),
        ds: &Ctx<'_>,
        (list, row): (Rect, Rect),
        (i, staged): (usize, bool),
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let file = &self.files[i];
        let state = if staged { file.staged } else { file.unstaged };
        let Some(state) = state else { return };
        let list_shown = self.selected.as_ref().is_some_and(|(_, s)| *s == staged);
        let shown = list_shown && self.selected.as_ref().is_some_and(|(q, _)| *q == file.path);
        if list_shown && (shown || self.marked.contains(&file.path)) {
            let alpha = if shown && self.marked.len() > 1 { 0.2 } else { 0.12 };
            layer.quad(Quad::filled(row, p.accent.with_alpha(alpha)).rounded(px(radius::SM)));
        } else if self.hover == Some(Hit::Row(i, staged)) {
            layer.quad(Quad::filled(row, p.text.with_alpha(0.04)).rounded(px(radius::SM)));
        }
        checkbox(layer, text, ds, row, staged, true);
        let room = row.size.width - px(30.0);
        let line = Rect::new(row.min_x() + px(30.0), row.min_y(), room, row.size.height);
        let path = file.path.as_str();
        // Its state as a badge where the row's mark is (the mark itself left empty).
        let color = p.text_muted;
        ds::file_row(layer, text, ds, line, ds::FileRow { path, mark: "", color, tag: None });
        let side = px(ds::CHANGE_BADGE);
        let badge = Rect::new(
            (line.min_x() + px(3.0)).round(),
            (row.min_y() + (row.size.height - side) / 2.0).round(),
            side,
            side,
        );
        ds::change_badge(layer, icons, ds, badge, crate::files_tree::change_state(state));
        self.badges.push((badge.intersect(&list).unwrap_or_default(), state));
        let visible = |r: Rect| r.intersect(&list).unwrap_or_default();
        self.hits.push((visible(row), Hit::Row(i, staged)));
        let check = Rect::new(row.min_x(), row.min_y(), px(28.0), row.size.height);
        self.hits.push((visible(check), Hit::Check(i, staged)));
    }

    /// The message and the button at `fy`; the outcome under them.
    /// The message (three lines at least, ten at most, then it scrolls), the Commit button
    /// beside its top, what happened under it. Returns its height.
    pub(super) fn paint_message(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut text_layout::TextSystem,
        ds: &Ctx<'_>,
        footer: Rect,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let (x0, fy, small) = (footer.min_x(), footer.min_y(), ds.ui_font(-1.0));
        let small = &small;
        let (commit, committing) = (t("git.commit"), t("git.committing"));
        let button_w = ds::primary_width(text, ds, &[&commit, &committing]);
        // In a column (a short screen): the button under the message, beside Amend.
        let column = footer.size.width < px(MESSAGE_COLUMN + 1.0);
        let below = if column { px(ds::BUTTON_H) + px(space::SM) } else { 0.0 };
        let field_w =
            if column { footer.size.width } else { footer.size.width - button_w - px(space::MD) };
        // Too short for the message and what's under it: nothing, rather than out of the view.
        let field_h = footer.size.height - px(36.0) - below;
        if field_h < px(32.0) {
            return;
        }
        let field = Rect::new(x0, fy, field_w, field_h);
        let spec = MultilineSpec {
            placeholder: &t("git.message_placeholder"),
            focused: self.keys == Keys::Message,
            subject_limit: Some(72),
        };
        self.message.paint(scene, text, ds, field, spec);
        self.hits.push((field, Hit::Message));
        let layer = scene.push_layer(Some(footer));
        let button = if column {
            let y = field.max_y() + px(space::SM);
            Rect::new(field.max_x() - button_w, y, button_w, px(ds::BUTTON_H))
        } else {
            Rect::new(field.max_x() + px(space::MD), fy, button_w, px(ds::BUTTON_H))
        };
        let label = if self.commit_rx.is_some() { committing } else { commit };
        let hovered = self.hover == Some(Hit::Commit);
        ds::primary_button(layer, text, ds, button, &label, self.can_commit(), hovered);
        self.hits.push((button, Hit::Commit));
        // Its keys under it (in a column, the footer says them).
        if !column {
            let hint = text.shape_line(&t("git.commit_keys"), &ds.ui_font(-2.0));
            draw_text(
                layer,
                &hint,
                button.min_x(),
                ds::optical_baseline(button.max_y() + px(4.0), px(18.0), &ds.ui_font(-2.0)),
                p.text_muted,
            );
        }
        // Amend last commit (its message brought in); a warning when it was pushed — in a
        // column, beside the button.
        let row = if column {
            let w = (button.min_x() - x0 - px(space::SM)).max(0.0);
            Rect::new(x0, button.min_y() + (button.size.height - px(28.0)) / 2.0, w, px(28.0))
        } else {
            Rect::new(x0, field.max_y() + px(8.0), field.size.width, px(28.0))
        };
        let mid = row.min_y() + row.size.height / 2.0;
        let state = if self.amend { ds::Check::On } else { ds::Check::Off };
        let check = ds::checkbox(layer, text, ds, row.min_x() + px(2.0), mid, state, true);
        let font = ds.ui_font(-0.5);
        let label = text.shape_line(&t("git.amend"), &font);
        let lx = check.max_x() + px(8.0);
        draw_text(
            layer,
            &label,
            lx,
            ds::optical_baseline(row.min_y(), row.size.height, &font),
            p.text,
        );
        self.hits.push((
            Rect::new(row.min_x(), row.min_y(), lx + label.width - row.min_x(), row.size.height),
            Hit::Amend,
        ));
        if self.amend && self.amend_pushed {
            let warn = text.shape_line(&t("git.amend_pushed"), small);
            let wx = lx + label.width + px(16.0);
            draw_text(
                layer,
                &warn,
                wx,
                ds::optical_baseline(row.min_y(), row.size.height, small),
                p.warning,
            );
        }
        if let Some((outcome, error)) = &self.outcome {
            let shaped = text.shape_line(outcome, small);
            draw_text(
                layer,
                &shaped,
                x0,
                ds::optical_baseline(row.max_y() + px(4.0), px(24.0), small),
                if *error { p.danger } else { p.vcs_added },
            );
        }
    }
}

impl CommitView {
    /// A list's keys at its card's bottom: moving through it, staging (or unstaging) the
    /// file shown, opening it, going to the message.
    pub(super) fn paint_list_keys(
        &self,
        scene: &mut render_api::Scene,
        text: &mut text_layout::TextSystem,
        ds: &Ctx<'_>,
        (foot, staged): (Rect, bool),
    ) {
        let move_label = if staged { t("git.keys_unstage") } else { t("git.keys_stage") };
        let (choose, open, message) = (t("keys.choose"), t("keys.open"), t("git.keys_message"));
        let hints: &[ui_kit::KeyHint<'_>] = &[
            (&["↑", "↓"], &choose),
            (&["space"], &move_label),
            (&["↵"], &open),
            (&["⇥"], &message),
        ];
        ds::footer_in_card(scene, text, ds, foot, hints, radius::MD);
    }
}

impl CommitView {
    /// The repository's places in `area` (docked): each section's head (its icon, its name
    /// in capitals), its rows under it — the files' status lit (this screen), the current
    /// branch marked with what it has to push; they scroll.
    pub(super) fn paint_places(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut text_layout::TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        area: Rect,
    ) {
        use places::{Place, SECTIONS, Work};
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(area));
        layer.quad(Quad::filled(area, p.text.with_alpha(0.025)).rounded(px(radius::MD)));
        let field = Rect::new(
            area.min_x() + px(8.0),
            area.min_y() + px(8.0),
            (area.size.width - px(16.0)).max(0.0),
            px(PLACES_FILTER_H),
        );
        self.paint_places_filter(&mut LayerPainter::new(layer, text, icons, *ds), field);
        // The rows under the field, clipped to what's left.
        let top = field.max_y() + px(4.0);
        let area = Rect::new(area.min_x(), top, area.size.width, (area.max_y() - top).max(0.0));
        self.places_area = area;
        let layer = scene.push_layer(Some(area));
        if self.place_rows.is_empty() {
            ds::empty_state(layer, text, ds, area, &t("git.places_none"), None);
            return;
        }
        let row_h = px(28.0);
        let content = self.place_rows.len() as f32 * row_h + px(8.0);
        self.places_scroll = self.places_scroll.min((content - area.size.height).max(0.0));
        let (font, small) = (ds.ui_font(-0.5), ds.ui_font(-2.5).with_weight(Font::SEMIBOLD));
        let strong = font.clone().with_weight(Font::SEMIBOLD);
        let icon = px(15.0);
        let mut y = area.min_y() - self.places_scroll;
        for (i, row) in self.place_rows.iter().enumerate() {
            let r = Rect::new(area.min_x() + px(6.0), y, area.size.width - px(12.0), row_h);
            y += row_h;
            if r.max_y() < area.min_y() || r.min_y() > area.max_y() {
                continue;
            }
            let x = r.min_x() + px(8.0) + row.depth as f32 * px(14.0);
            let mid = r.min_y() + row_h / 2.0;
            let lit = matches!(row.place, Place::Work(Work::FileStatus));
            if lit {
                layer.quad(Quad::filled(r, p.accent.with_alpha(0.14)).rounded(px(radius::SM)));
            } else if self.hover == Some(Hit::Place(i)) {
                layer.quad(Quad::filled(r, p.text.with_alpha(0.05)).rounded(px(radius::SM)));
            }
            let draw_icon = |layer: &mut render_api::Layer, name: &str, at: f32, color| {
                if let Some(id) = icons.id(name) {
                    let rect = Rect::new(at, (mid - icon / 2.0).round(), icon, icon);
                    ui_kit::draw_icon(layer, icons, id, rect, color);
                }
            };
            let label_x = x + icon + px(8.0);
            let room = (r.max_x() - px(8.0) - label_x).max(0.0);
            let baseline = ds::optical_baseline(r.min_y(), row_h, &font);
            let mut label = |layer: &mut render_api::Layer, value: &str, f: &Font, color| {
                ds::draw_ellipsized(layer, text, value, f, (label_x, baseline), room, color);
            };
            match &row.place {
                Place::Section { key, expanded } => {
                    let (_, _, name, glyph) =
                        SECTIONS.iter().find(|(k, ..)| k == key).copied().unwrap_or(SECTIONS[0]);
                    draw_icon(layer, glyph, x, p.text_muted);
                    let caps = t(name).to_uppercase();
                    let b = ds::optical_baseline(r.min_y(), row_h, &small);
                    ds::draw_ellipsized(
                        layer,
                        text,
                        &caps,
                        &small,
                        (label_x, b),
                        room,
                        p.text_muted,
                    );
                    if !expanded {
                        let chevron = Rect::new(
                            r.max_x() - px(20.0),
                            (mid - px(6.0)).round(),
                            px(12.0),
                            px(12.0),
                        );
                        if let Some(id) = icons.id("chevron-right") {
                            ui_kit::draw_icon(layer, icons, id, chevron, p.text_muted);
                        }
                    }
                }
                Place::Work(work) => {
                    let (name, glyph) = match work {
                        Work::FileStatus => ("git.places_file_status", "git-commit"),
                        Work::History => ("git.places_history", "clock"),
                        Work::Search => ("git.places_search", "search"),
                    };
                    draw_icon(layer, glyph, x, if lit { p.accent } else { p.text_muted });
                    label(layer, &t(name), &font, p.text);
                }
                Place::Folder { label: name, expanded, .. } => {
                    let glyph = if *expanded { "chevron-down" } else { "chevron-right" };
                    draw_icon(layer, glyph, x, p.text_muted);
                    label(layer, name, &font, p.text);
                }
                Place::Ref { label: name, current, ahead, kind, .. } => {
                    if *current {
                        let d = px(7.0);
                        let dot = Rect::new(x + (icon - d) / 2.0, (mid - d / 2.0).round(), d, d);
                        layer.quad(Quad::filled(dot, p.accent).rounded(d / 2.0));
                    } else if *kind == version_control::RefKind::Tag {
                        draw_icon(layer, "tag", x, p.text_muted);
                    }
                    let f = if *current { &strong } else { &font };
                    label(layer, name, f, p.text);
                    if *ahead > 0 {
                        ds::badge(
                            layer,
                            text,
                            ds,
                            ds::BadgeSpec {
                                label: &format!("{ahead}↑"),
                                x: r.max_x() - px(6.0),
                                mid,
                                align_right: true,
                                tone: Tone::Neutral,
                            },
                        );
                    }
                }
                Place::Stash(stash) => {
                    draw_icon(layer, "layers", x, p.text_muted);
                    label(layer, &stash.message, &font, p.text);
                }
                Place::Submodule(path) => {
                    draw_icon(layer, "folder", x, p.text_muted);
                    label(layer, path, &font, p.text);
                }
            }
            if let Some(visible) = r.intersect(&area) {
                self.hits.push((visible, Hit::Place(i)));
            }
        }
    }

    /// The places' filter in `field`: the magnifier, what's typed (or its hint), its × once
    /// something is.
    fn paint_places_filter(&mut self, pen: &mut LayerPainter<'_>, field: Rect) {
        let focused = self.keys == Keys::Places;
        ds::search_input(pen, field, &self.places_filter, focused, &t("git.places_filter"));
        self.hits.push((field, Hit::PlacesFilter));
        if !self.places_filter.text().is_empty() {
            let (cx, mid) =
                (field.max_x() - pen.ds.px(16.0), field.min_y() + field.size.height / 2.0);
            let hovered = self.hover == Some(Hit::ClearPlacesFilter);
            let clear = ds::clear_button(pen.layer, pen.icons, &pen.ds, cx, mid, hovered);
            self.hits.push((clear, Hit::ClearPlacesFilter));
        }
    }
}

/// The page's column and the fonts its parts share.
pub(super) struct Page {
    pub(super) x0: f32,
    pub(super) width: f32,
    pub(super) small: Font,
}

/// A card in `rect` with `title` at `title_x` in its head: its body.
pub(super) fn paint_card(
    layer: &mut render_api::Layer,
    text: &mut text_layout::TextSystem,
    ds: &Ctx<'_>,
    rect: Rect,
    (title, title_x): (&str, f32),
) -> Rect {
    let p = ds.palette;
    let px = |v: f32| ds.px(v);
    let (r, edge) = (px(radius::MD), ds.line());
    // The card: a light tint in its outline; its head a shade deeper (opaque, so its rounded
    // top and square bottom meet without a seam), a line under it.
    let body = p.editor_background.mix(p.text, 0.015);
    layer.quad(Quad::filled(rect, body).rounded(r).bordered(edge, p.card_border));
    let head = px(CARD_HEAD);
    let shade = p.editor_background.mix(p.text, 0.04);
    let inner = |y: f32, h: f32| Rect::new(rect.min_x() + edge, y, rect.size.width - 2.0 * edge, h);
    layer.quad(Quad::filled(inner(rect.min_y() + edge, head - edge), shade).rounded(r - edge));
    layer.quad(Quad::filled(inner(rect.min_y() + head / 2.0, head / 2.0), shade));
    let line = Rect::new(rect.min_x(), rect.min_y() + head - edge, rect.size.width, edge);
    layer.quad(Quad::filled(line, p.card_border));
    let font = ds.ui_font(-0.5).with_weight(Font::SEMIBOLD);
    let shaped = text.shape_line(title, &font);
    let baseline = ds::optical_baseline(rect.min_y(), head, &font);
    draw_text(layer, &shaped, rect.min_x() + title_x, baseline, p.text);
    Rect::new(rect.min_x(), rect.min_y() + head, rect.size.width, rect.size.height - head)
}

/// A checkbox at the start of `row`.
pub(super) fn checkbox(
    layer: &mut render_api::Layer,
    text: &mut text_layout::TextSystem,
    ds: &Ctx<'_>,
    row: Rect,
    on: bool,
    enabled: bool,
) {
    let state = if on { ds::Check::On } else { ds::Check::Off };
    let mid = row.min_y() + row.size.height / 2.0;
    ds::checkbox(layer, text, ds, row.min_x() + ds.px(6.0), mid, state, enabled);
}
