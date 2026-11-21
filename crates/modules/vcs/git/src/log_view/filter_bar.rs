//! The filter bar painted (over the commits, in their column): the text field with its `.*`
//! and `Cc`, the pills, and the open pill's list over the log; its presses, and what its
//! field typed in answered as a control (`parts::BarField` takes the keys and the text).

use super::filters::{Choice, Field, FilterKind};
use super::parts::FieldAnswer;
use super::*;
use ui_components::control::{Control, Press};
use ui_components::search_field::{SearchExtras, SearchField};

/// A line of the bar (logical pixels): it takes one, or two when narrow.
pub(super) const BAR_H: f32 = 44.0;
/// The bar's margins and the gap between its pills; the text field's least and widest
/// (logical pixels).
const PAD: f32 = 12.0;
const PILL_GAP: f32 = 6.0;
const FIELD_MIN: f32 = 220.0;
const FIELD_MAX: f32 = 280.0;
/// The room between an open list's rows and its scrollbar (logical pixels).
const LIST_BAR_GAP: f32 = 4.0;
/// A list's row height and how many show at most (logical pixels).
const ROW_H: f32 = 28.0;
const VISIBLE_ROWS: usize = 10;
/// A list's keys at its bottom (logical pixels).
const KEYS_H: f32 = 34.0;
/// The text's `.*` and `Cc`: each one's size and the gap between them (logical pixels);
/// the room they take in its field, on the right.
const TOGGLE_W: f32 = button_size::COMPACT;
const TOGGLE_H: f32 = 20.0;
const TOGGLE_GAP: f32 = 2.0;
const TOGGLES_W: f32 = TOGGLE_W * 2.0 + TOGGLE_GAP;

impl LogView {
    /// The pills as they show now: each kind, its label, whether it's set.
    fn pill_labels(&self) -> Vec<(FilterKind, String, bool)> {
        FilterKind::ALL
            .iter()
            .map(|kind| {
                let (label, set) = self.pill_label(*kind);
                (*kind, label, set)
            })
            .collect()
    }

    /// How many lines the bar takes in `width` (physical pixels): one when the text field
    /// (at its least) and every pill fit side by side; else two, the pills under the field.
    pub(super) fn filter_bar_lines(&self, text: &mut TextSystem, ds: &Ctx<'_>, width: f32) -> u8 {
        let px = |v: f32| ds.px(v);
        let pills: f32 = self
            .pill_labels()
            .iter()
            .map(|(_, label, set)| pill_width(text, ds, label, *set) + px(PILL_GAP))
            .sum();
        let needed = px(PAD) + px(FIELD_MIN) + px(PAD) + pills + px(PAD);
        if width >= needed { 1 } else { 2 }
    }

    /// The bar in `bar` (`lines` of [`BAR_H`]): the text field, then the pills — beside it,
    /// or under it on a second line; a pill that still doesn't fit is left out whole.
    pub(super) fn paint_filter_bar(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        (bar, lines): (Rect, u8),
    ) {
        let px = |v: f32| ds.px(v);
        let line_h = px(BAR_H);
        let first_mid = bar.min_y() + line_h / 2.0;
        let h = px(28.0);
        let labels = self.pill_labels();
        let field_w = if lines > 1 {
            (bar.size.width - px(PAD) * 2.0).max(0.0)
        } else {
            let pills: f32 = labels
                .iter()
                .map(|(_, label, set)| pill_width(text, ds, label, *set) + px(PILL_GAP))
                .sum();
            px(FIELD_MAX).min(bar.size.width - px(PAD) * 3.0 - pills).max(px(FIELD_MIN))
        };
        let field = Rect::new(bar.min_x() + px(PAD), (first_mid - h / 2.0).round(), field_w, h);
        let mut pen = ui_components::LayerPainter::new(layer, text, icons, *ds);
        self.paint_search_field(&mut pen, field, Field::Text);
        let (mut x, mid) = if lines > 1 {
            (bar.min_x() + px(PAD), first_mid + line_h)
        } else {
            (field.max_x() + px(PAD), first_mid)
        };
        let end = bar.max_x() - px(PAD);
        self.bar.pills.clear();
        for (kind, label, set) in labels {
            if x + pill_width(text, ds, &label, set) > end {
                break;
            }
            let open = self.bar.popup.as_ref().is_some_and(|p| p.kind == Some(kind));
            let pill = self.paint_pill(
                &mut ui_components::LayerPainter::new(layer, text, icons, *ds),
                (x, mid),
                (kind, &label),
                set,
                open,
            );
            self.bar.pills.push((kind, pill));
            x = pill.max_x() + px(PILL_GAP);
        }
    }

    /// One of the log's search fields in `field`, both the app's search field: the branches'
    /// filter ([`Field::Branches`]) and "Text or hash" ([`Field::Text`]), the latter with its
    /// own `.*` and `Cc` in the room it keeps on the right (× left of them), its border red
    /// when the regular expression isn't one.
    pub(super) fn paint_search_field(
        &mut self,
        pen: &mut ui_components::LayerPainter<'_>,
        field: Rect,
        which: Field,
    ) {
        let (layer, text, icons, ds) = (&mut *pen.layer, &mut *pen.text, pen.icons, &pen.ds);
        let focused = self.bar.focus == Some(which);
        if which == Field::Branches {
            self.hits.push((field, Hit::BranchSearch));
            let placeholder = t("git.branches_filter");
            self.bar.branch_query.paint(layer, text, icons, ds, (field, focused), &placeholder);
            return;
        }
        self.hits.push((field, Hit::TextField));
        let placeholder = t("git.filter_text");
        let extras = SearchExtras { trailing: TOGGLES_W, invalid: self.bar.invalid };
        let room = self.bar.text.paint_with(
            layer,
            text,
            icons,
            ds,
            (field, focused),
            &placeholder,
            extras,
        );
        let px = |v: f32| ds.px(v);
        let mid = room.min_y() + room.size.height / 2.0;
        // From the right: Cc, .* (the text's).
        let toggles =
            [(Hit::MatchCase, "Cc", self.bar.match_case), (Hit::Regex, ".*", self.bar.regex)];
        let mut right = room.max_x();
        for (hit, label, on) in toggles {
            let (w, h) = (px(TOGGLE_W), px(TOGGLE_H));
            let r = Rect::new(right - w, mid - h / 2.0, w, h);
            let spec = ButtonSpec::new(ButtonKind::Ghost)
                .label(label)
                .rest_tone(Tone::Neutral)
                .on(on)
                .hovered(self.hover == Some(hit));
            ds::button(layer, text, icons, ds, r, spec);
            self.hits.push((r, hit));
            right = r.min_x() - px(TOGGLE_GAP);
        }
    }

    /// A pill from `x` (its middle at `mid`): the shared chip, its label and ▾, in the accent
    /// with its × when `set`. Returns where it is.
    fn paint_pill(
        &mut self,
        pen: &mut ui_components::LayerPainter<'_>,
        (x, mid): (f32, f32),
        (kind, label): (FilterKind, &str),
        set: bool,
        open: bool,
    ) -> Rect {
        let (layer, text, icons, ds) = (&mut *pen.layer, &mut *pen.text, pen.icons, &pen.ds);
        let body = open || self.hover == Some(Hit::Pill(kind));
        let clear = self.hover == Some(Hit::ClearPill(kind));
        let spec = pill_spec(label, set, body, clear);
        let h = ds.px(button_size::MEDIUM);
        let w = ds::button_width(text, ds, &spec, h);
        let pill = Rect::new(x.round(), (mid - h / 2.0).round(), w, h);
        if !set {
            ds::button(layer, text, icons, ds, pill, spec);
            self.hits.push((pill, Hit::Pill(kind)));
            return pill;
        }
        let parts = ds::chip(layer, text, icons, ds, pill, spec);
        self.hits.push((parts.body, Hit::Pill(kind)));
        self.hits.push((parts.clear, Hit::ClearPill(kind)));
        pill
    }

    /// The open list, over the log under its pill (within `view`): its search field, its
    /// rows (those in view), the interval's fields for Date ▾.
    pub(super) fn paint_popup(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        view: Rect,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let Some(kind) = self.bar.popup.as_ref().and_then(|pp| pp.kind) else { return };
        let Some(anchor) = self.bar.pills.iter().find(|(k, _)| *k == kind).map(|(_, r)| *r) else {
            return;
        };
        let loading =
            matches!(kind, FilterKind::User | FilterKind::Paths) && self.bar.lists.is_none();
        let popup = self.bar.popup.as_ref().map_or(0, |pp| pp.rows.len());
        let more = self.bar.popup.as_ref().map_or(0, |pp| pp.more);
        let width = px(if kind == FilterKind::Paths { 420.0 } else { 320.0 });
        let pad = px(8.0);
        let search_h = if kind.searched() { px(30.0) + pad } else { 0.0 };
        let more_h = if more > 0 { px(ROW_H) } else { 0.0 };
        let keys_h = px(KEYS_H);
        let room = (view.max_y()
            - anchor.max_y()
            - px(ds::POPOVER_GAP)
            - pad * 2.0
            - search_h
            - more_h
            - keys_h)
            .max(px(ROW_H));
        let shown = if loading { 1 } else { popup.clamp(1, VISIBLE_ROWS) };
        let list_h =
            (shown as f32 * px(ROW_H)).min((room / px(ROW_H)).floor().max(1.0) * px(ROW_H));
        let height = pad * 2.0 + search_h + list_h + more_h + keys_h;
        let x = anchor.min_x().min(view.max_x() - width - px(8.0)).max(view.min_x() + px(8.0));
        let rect = Rect::new(x.round(), (anchor.max_y() + px(4.0)).round(), width, height.round());
        let layer = scene.push_layer(Some(view));
        ds::surface_with(layer, ds, rect, ds::Elevation::High, radius::LG, p.hover_background);
        let layer = scene.push_layer(Some(rect));
        self.hits.push((rect, Hit::Popup));
        let mut y = rect.min_y() + pad;
        let inner_x = rect.min_x() + pad;
        let inner_w = rect.size.width - pad * 2.0;
        let focus = self.bar.focus;
        let Some(popup) = &mut self.bar.popup else { return };
        popup.rect = rect;
        if kind.searched() {
            let field = Rect::new(inner_x, y, inner_w, px(30.0));
            let placeholder = t("git.filter_search");
            let focused = focus == Some(Field::Query);
            popup.query.paint(layer, text, icons, ds, (field, focused), &placeholder);
            self.hits.push((field, Hit::PopupQuery));
            y = field.max_y() + pad;
        }
        let list = Rect::new(rect.min_x(), y, rect.size.width, list_h);
        popup.list = list;
        let row_h = px(ROW_H);
        let content = popup.rows.len() as f32 * row_h;
        let max = (content - list.size.height).max(0.0);
        popup.scroll = popup.scroll.clamp(0.0, max);
        // While they scroll, the rows leave the scrollbar a lane of its own.
        let row_w = if max > 0.0 {
            let lane = ui_components::Scrollbar::track_thickness(ds.scale) + px(LIST_BAR_GAP);
            (list.max_x() - lane - inner_x).clamp(0.0, inner_w)
        } else {
            inner_w
        };
        let rows_layer = scene.push_layer(Some(list));
        if loading {
            let reading = t("git.filter_reading");
            let spinner = ds::Loader::with_text(&reading);
            ds::loader(
                rows_layer,
                text,
                ds,
                inner_x + px(8.0),
                list.min_y() + row_h / 2.0,
                &spinner,
            );
        } else if popup.rows.is_empty() {
            let shaped = text.shape_line(&t("git.filter_no_match"), &ds.ui_font(-0.5));
            draw_text(
                rows_layer,
                &shaped,
                inner_x + px(10.0),
                ds::optical_baseline(list.min_y(), row_h, &ds.ui_font(-0.5)),
                p.text_muted,
            );
        }
        let font = ds.ui_font(-0.5);
        let strong = font.clone().with_weight(Font::SEMIBOLD);
        let small = ds.ui_font(-1.5);
        let first = (popup.scroll / row_h) as usize;
        let count = (list.size.height / row_h).ceil() as usize + 1;
        let hover = self.hover;
        let mut hits = Vec::new();
        for i in first..(first + count).min(popup.rows.len()) {
            let row = &popup.rows[i];
            let top = (list.min_y() + i as f32 * row_h - popup.scroll).round();
            let r = Rect::new(inner_x, top, row_w, row_h);
            if row.choice == Choice::Header {
                let shaped = text.shape_line(
                    &row.label.to_uppercase(),
                    &small.clone().with_weight(Font::SEMIBOLD),
                );
                draw_text(
                    rows_layer,
                    &shaped,
                    r.min_x() + px(10.0),
                    ds::optical_baseline(top + px(4.0), row_h - px(4.0), &small),
                    p.text_muted,
                );
                continue;
            }
            // The shared check row draws the highlight and the ✓; its label, icon and detail
            // are this list's own (a path ellipsized from its start, a count on the right).
            let spec = ds::CheckRowSpec::new("", row.checked, hover == Some(Hit::PopupRow(i)))
                .active(popup.active == Some(i))
                .untinted();
            ds::check_row(rows_layer, text, icons, ds, r, spec);
            let mid = top + row_h / 2.0;
            let s = px(14.0);
            let mut tx = r.min_x() + px(30.0);
            if !row.icon.is_empty()
                && let Some(id) = icons.id(row.icon)
            {
                let color = if row.icon == "folder" { p.folder_icon } else { p.text_muted };
                ui_kit::draw_icon(
                    rows_layer,
                    icons,
                    id,
                    Rect::new(tx, (mid - s / 2.0).round(), s, s),
                    color,
                );
                tx += s + px(8.0);
            }
            let f = if row.checked { &strong } else { &font };
            let detail_w = if row.detail.is_empty() {
                0.0
            } else {
                text.shape_line(&row.detail, &small).width + px(10.0)
            };
            let room = (r.max_x() - tx - px(8.0) - detail_w).max(0.0);
            let label = if kind == FilterKind::Paths {
                ds::ellipsize_start(text, &row.label, f, room)
            } else {
                ds::ellipsize(text, &row.label, f, room)
            };
            let lit = if label == row.label { row.lit.as_slice() } else { &[] };
            let base = ds::optical_baseline(top, row_h, f);
            crate::files_tree::draw_lit(
                rows_layer,
                text,
                f,
                &label,
                (tx, base),
                (p.text, p.accent),
                lit,
            );
            if !row.detail.is_empty() {
                let shaped = text.shape_line(&row.detail, &small);
                draw_text(
                    rows_layer,
                    &shaped,
                    r.max_x() - px(8.0) - shaped.width,
                    ds::optical_baseline(top, row_h, &small),
                    p.text_muted,
                );
            }
            if let Some(visible) = r.intersect(&list) {
                hits.push((visible, Hit::PopupRow(i)));
            }
        }
        popup.bar.paint(scene, p, ds.scale, list, content, popup.scroll);
        let layer = scene.push_layer(Some(rect));
        let y = list.max_y();
        if more > 0 {
            let shaped = text.shape_line(&tr!("git.filter_more", count = more), &small);
            draw_text(
                layer,
                &shaped,
                inner_x + px(10.0),
                ds::optical_baseline(y, row_h, &small),
                p.text_muted,
            );
        }
        // The list's keys, at its bottom.
        let (choose, select, close) = (t("keys.choose"), t("picker.hint_select"), t("keys.close"));
        let hints: &[ui_kit::KeyHint<'_>] =
            &[(&["↑", "↓"], &choose), (&["↵"], &select), (&["esc"], &close)];
        let keys = Rect::new(rect.min_x(), rect.max_y() - keys_h, rect.size.width, keys_h);
        ds::footer_in_card(scene, text, ds, keys, hints, radius::LG);
        self.hits.extend(hits);
    }

    /// The field with the keys, to read (`None`: none).
    fn focused_field(&self) -> Option<&SearchField> {
        Some(match self.bar.focus? {
            Field::Text => &self.bar.text,
            Field::Branches => &self.bar.branch_query,
            Field::Query => &self.bar.popup.as_ref()?.query,
        })
    }

    /// What's selected in the field with the keys (Copy takes it).
    pub(super) fn field_selection(&self) -> Option<String> {
        let selected = self.focused_field()?.input.selected_text();
        (!selected.is_empty()).then(|| selected.to_owned())
    }

    /// The mouse dragged from a press in a field: its selection follows. `true` if one is
    /// being selected in.
    pub(super) fn drag_field(&mut self, p: Point) -> bool {
        if !self.field_drag {
            return false;
        }
        match self.focused_input() {
            Some(field) => Control::drag(field, p),
            None => false,
        }
    }

    /// The field with the keys (`None`: none).
    fn focused_input(&mut self) -> Option<&mut SearchField> {
        Some(match self.bar.focus? {
            Field::Text => &mut self.bar.text,
            Field::Branches => &mut self.bar.branch_query,
            Field::Query => &mut self.bar.popup.as_mut()?.query,
        })
    }

    /// What was typed went into field `field`: the text waits for a pause, a list's search
    /// and the branches' filter apply at once.
    fn typed_into(&mut self, field: Field) {
        match field {
            Field::Text => self.text_typed(),
            Field::Query => {
                if let Some(popup) = &mut self.bar.popup {
                    popup.scroll = 0.0;
                    popup.active = None;
                }
                self.refresh_rows();
            }
            Field::Branches => {
                self.branches_scroll = 0.0;
                self.rebuild_branches();
            }
        }
    }

    /// What the bar's field typed in answered as a control ([`super::parts::BarField`]):
    /// the filter it is follows, `↵` applies or chooses, the list moves or closes.
    pub(super) fn take_field_answer(&mut self) {
        let Some(answer) = self.field_answer.take() else { return };
        match answer {
            FieldAnswer::Typed(field) => self.typed_into(field),
            FieldAnswer::Entered(field) => self.enter_field(field),
            FieldAnswer::Stepped(by) => self.step_popup(by),
            FieldAnswer::Closed => self.close_popup(),
        }
    }

    /// `↵` in field `field`: the text applied now, the list's row with the keys (else its
    /// first) chosen, the first branch shown.
    fn enter_field(&mut self, field: Field) {
        match field {
            Field::Text => self.apply_text(),
            Field::Query => {
                let first = self.bar.popup.as_ref().and_then(|p| {
                    p.active.or_else(|| p.rows.iter().position(|r| r.choice != Choice::Header))
                });
                if let Some(i) = first {
                    self.choose_row(i);
                }
            }
            Field::Branches => {
                let first =
                    self.branch_rows.iter().position(|r| matches!(r.item, BranchItem::Ref { .. }));
                if let Some(i) = first {
                    self.choose_branch(i);
                }
            }
        }
    }

    /// The list's chosen row moved by `by` (headers skipped), kept in view.
    pub(super) fn step_popup(&mut self, by: isize) {
        let row_h = (ROW_H * self.scale.max(0.1)).round();
        let Some(popup) = &mut self.bar.popup else { return };
        let n = popup.rows.len() as isize;
        if n == 0 {
            return;
        }
        let mut i = popup.active.map_or(if by > 0 { -1 } else { n }, |a| a as isize);
        loop {
            i += by.signum();
            if i < 0 || i >= n {
                return;
            }
            if popup.rows[i as usize].choice != Choice::Header {
                break;
            }
        }
        popup.active = Some(i as usize);
        let top = i as f32 * row_h;
        let height = popup.list.size.height;
        if top < popup.scroll {
            popup.scroll = top;
        } else if top + row_h > popup.scroll + height {
            popup.scroll = top + row_h - height;
        }
    }

    /// A press on one of the bar's parts (or the list's). `true` if it was one of theirs.
    pub(super) fn press_filter(&mut self, hit: Hit, p: Point, clicks: u32) -> bool {
        // The caret placed in the field pressed (two clicks: a word, three: all); a drag
        // from there selects. A search field's × empties it (what it filters follows).
        self.field_drag = false;
        let place = |field: &mut SearchField| {
            let held = Control::press(field, p, clicks) == Press::Held;
            // A press changes the text only by ×: the change is told here, once.
            field.take_edited();
            (held, field.take_cleared())
        };
        match hit {
            Hit::TextField => {
                self.close_popup();
                self.bar.focus = Some(Field::Text);
                let cleared;
                (self.field_drag, cleared) = place(&mut self.bar.text);
                if cleared {
                    self.apply_text();
                }
            }
            Hit::Regex => {
                self.bar.regex = !self.bar.regex;
                self.apply_text();
            }
            Hit::MatchCase => {
                self.bar.match_case = !self.bar.match_case;
                self.apply_text();
            }
            Hit::Pill(kind) => self.toggle_popup(kind),
            Hit::ClearPill(kind) => {
                self.close_popup();
                self.clear_filter(kind);
            }
            Hit::PopupRow(i) => self.choose_row(i),
            Hit::PopupQuery => {
                self.bar.focus = Some(Field::Query);
                let Some(popup) = &mut self.bar.popup else { return true };
                let cleared;
                (self.field_drag, cleared) = place(&mut popup.query);
                if cleared {
                    self.typed_into(Field::Query);
                }
            }
            Hit::Popup => {}
            Hit::BranchSearch => {
                self.bar.focus = Some(Field::Branches);
                let cleared;
                (self.field_drag, cleared) = place(&mut self.bar.branch_query);
                if cleared {
                    self.typed_into(Field::Branches);
                }
            }
            _ => return false,
        }
        true
    }
}

impl LogView {
    /// The mouse at `p` over the bar's text, the branches' filter and the open list's search
    /// (their × lit under it): `true` if one changed.
    pub(super) fn hover_search_fields(&mut self, p: Point) -> bool {
        let over = |field: &SearchField| Some(p).filter(|p| field.area().contains(*p));
        let at = over(&self.bar.text);
        let mut changed = Control::hover(&mut self.bar.text, at);
        let at = over(&self.bar.branch_query);
        changed |= Control::hover(&mut self.bar.branch_query, at);
        if let Some(popup) = &mut self.bar.popup {
            let at = over(&popup.query);
            changed |= Control::hover(&mut popup.query, at);
        }
        changed
    }
}

/// The side panels' and the heads' tint: one gray over the view, up to the splitters'
/// middles (each splitter where two colors meet, never a line of its own).
pub(super) fn panel_tint(p: &ui_components::Palette) -> Rgba {
    p.text.with_alpha(0.02)
}

/// A pill's look: the shared chip when `set` (in the accent, with its ×), else a round soft
/// button with its ▾.
fn pill_spec(label: &str, set: bool, hovered: bool, clear_hovered: bool) -> ButtonSpec<'_> {
    if set {
        return ds::chip_spec(label, true, hovered, clear_hovered);
    }
    ButtonSpec::new(ButtonKind::Soft).label(label).round().menu(true).hovered(hovered)
}

/// A pill's width for `label` (with its × when `set`), as [`LogView::paint_pill`] draws it.
fn pill_width(text: &mut TextSystem, ds: &Ctx<'_>, label: &str, set: bool) -> f32 {
    let spec = pill_spec(label, set, false, false);
    ds::button_width(text, ds, &spec, ds.px(button_size::MEDIUM))
}
