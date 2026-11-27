//! Drawing the structure's modal: its form (fields, boxes, lists to choose from), then
//! what will run (the statements, or a CSV file's changes in words), what's lost (ticked
//! to confirm) and what SQLite rewrites, the footer's buttons, and the open list over all.

use database::danger::{AlterColumnLoss, Danger, DropKind};
use database::sql::WriteKind;
use database::sqlite::ObjectKind;
use i18n::{t, tr};
use kernel::{Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_components::ViewerPaint;
use ui_components::ds::{self, ButtonKind, ButtonSpec, Check, Ctx, Tone, button_size, radius};
use ui_kit::{TextInputStyle, draw_text};

use super::form::{Kind, Text};
use super::modal::{Choice, Hit, Popup, SchemaView, Tick};

/// The tools a part of the modal draws with.
pub(super) struct Pen<'a, 'b> {
    pub layer: &'a mut Layer,
    pub text: &'a mut TextSystem,
    pub icons: &'a icons::Icons,
    pub ds: &'a Ctx<'b>,
}

/// A field's height, a labeled group's (its label over its field, and the gap after).
const FIELD_H: f32 = 34.0;
const GROUP_H: f32 = 66.0;
/// A new table's column row, a list row.
const ROW_H: f32 = 36.0;
const LIST_H: f32 = 32.0;
const TITLE_H: f32 = 30.0;
/// The room under the strips (the window's footer follows).
const BOTTOM_PAD: f32 = 12.0;
/// The row confirming what's lost.
const CONFIRM_H: f32 = 30.0;
const PAD: f32 = 24.0;
/// The column being edited: its row's accent alpha.
const COLUMN_CHOSEN: f32 = 0.06;
/// An index's column under the mouse: the text's alpha over it.
const INDEX_HOVER: f32 = 0.04;
/// A column's place in the index: its badge's accent alpha and the room beside its number.
const PLACE_FILL: f32 = 0.16;
const PLACE_PAD: f32 = 3.0;
/// The NN / PK / AI / UQ columns' names spelled out (their headers' and boxes' tooltips).
const TICK_TIPS: [&str; 4] =
    ["schema.tip_not_null", "schema.tip_primary", "schema.tip_auto", "schema.tip_unique"];

/// SQL's words drawn in the accent color.
const KEYWORDS: [&str; 44] = [
    "CREATE",
    "TABLE",
    "INDEX",
    "UNIQUE",
    "ON",
    "ALTER",
    "RENAME",
    "TO",
    "COLUMN",
    "ADD",
    "DROP",
    "VIEW",
    "TRIGGER",
    "INSERT",
    "INTO",
    "SELECT",
    "FROM",
    "WHERE",
    "PRIMARY",
    "KEY",
    "FOREIGN",
    "REFERENCES",
    "NOT",
    "NULL",
    "DEFAULT",
    "CHECK",
    "BEGIN",
    "IMMEDIATE",
    "COMMIT",
    "PRAGMA",
    "AUTOINCREMENT",
    "WITHOUT",
    "ROWID",
    "CONSTRAINT",
    "DELETE",
    "UPDATE",
    "CASCADE",
    "SET",
    "RESTRICT",
    "NO",
    "ACTION",
    "COLLATE",
    "ASC",
    "DESC",
];

fn object_kind(kind: ObjectKind) -> String {
    t(match kind {
        ObjectKind::Index => "schema.object_index",
        ObjectKind::Trigger => "schema.object_trigger",
        ObjectKind::View => "schema.object_view",
    })
}

fn drop_kind(kind: DropKind) -> String {
    t(match kind {
        DropKind::Table => "schema.object_table",
        DropKind::View => "schema.object_view",
        DropKind::Index => "schema.object_index",
        DropKind::Schema => "schema.object_schema",
        DropKind::Database => "schema.object_database",
        DropKind::Function => "schema.object_function",
        DropKind::Procedure => "schema.object_procedure",
        DropKind::Trigger => "schema.object_trigger",
        DropKind::Sequence => "schema.object_sequence",
        DropKind::Type => "schema.object_type",
        DropKind::Other => "schema.object_other",
    })
}

/// What's lost, in words.
pub fn danger_text(danger: &Danger) -> String {
    let rows = |n: &Option<u64>| match n {
        Some(n) => tr!("schema.rows", count = *n),
        None => t("schema.rows_unknown"),
    };
    match danger {
        Danger::DropTable { table, rows: n } => {
            tr!("schema.lose_table", table = table.to_string(), rows = rows(n))
        }
        Danger::DropColumn { column, rows: n, .. } => {
            tr!("schema.lose_column", column = column.as_str(), rows = rows(n))
        }
        Danger::AlterColumn { column, loss, .. } => tr!(
            match loss {
                AlterColumnLoss::Conversion => "schema.lose_conversion",
                AlterColumnLoss::Resize => "schema.lose_resize",
                AlterColumnLoss::NotNull => "schema.lose_not_null",
            },
            column = column.as_str()
        ),
        Danger::MassDelete { rows, .. } => tr!("schema.lose_rows", count = *rows),
        Danger::LostObject { kind, name, .. } => {
            tr!("schema.lose_object", kind = object_kind(*kind), name = name.as_str())
        }
        Danger::DropIndex { name, .. } => tr!("schema.lose_index", name = name.as_str()),
        Danger::UnfilteredWrite { kind, table } => tr!(
            match kind {
                WriteKind::Delete => "schema.lose_unfiltered_delete",
                _ => "schema.lose_unfiltered_update",
            },
            table = table.to_string()
        ),
        Danger::DropObject { kind, name, cascade } => tr!(
            if *cascade { "schema.lose_drop_cascade" } else { "schema.lose_drop" },
            kind = drop_kind(*kind),
            name = name.as_str()
        ),
        Danger::Truncate { table } => tr!("schema.lose_truncate", table = table.to_string()),
        Danger::AlterDrop { table, column } => {
            tr!("schema.lose_drop_column", table = table.to_string(), column = column.as_str())
        }
    }
}

impl SchemaView {
    fn style(&self, ds: &Ctx<'_>, small: bool) -> TextInputStyle {
        let p = ds.palette;
        TextInputStyle {
            font: ds.ui_font(if small { -0.5 } else { 0.0 }),
            background: p.input_background,
            border: p.input_border,
            border_focused: p.accent,
            text: p.text,
            placeholder: p.text_muted,
            selection: p.selection,
            caret: p.caret,
            radius: ds.px(radius::MD),
            border_width: ds.line(),
            padding: ds.px(if small { 9.0 } else { 12.0 }),
            caret_width: ds.px(1.5).max(1.0),
            lead: 0.0,
            trail: 0.0,
        }
    }

    /// The form's height for what it holds (logical pixels).
    fn form_extent(&self) -> f32 {
        let Some(form) = &self.form else { return 0.0 };
        match &form.kind {
            Kind::CreateTable => {
                GROUP_H
                    + TITLE_H
                    + 26.0
                    + form.columns.len() as f32 * ROW_H
                    + 12.0
                    + GROUP_H
                    + TITLE_H
                    + form.keys.len() as f32 * (ROW_H + 8.0)
                    + 36.0
            }
            Kind::AddColumn | Kind::AlterColumn(_) if form.texts().len() > 1 => 3.0 * GROUP_H,
            Kind::CreateIndex => {
                let lists = form.own_columns().len().div_ceil(2).max(1);
                GROUP_H + TITLE_H + lists as f32 * LIST_H + 14.0 + GROUP_H
            }
            Kind::AddForeignKey => 3.0 * GROUP_H,
            k if k.drops() => 44.0,
            _ => GROUP_H,
        }
    }

    pub(super) fn paint_modal(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, icons, .. } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.anchors.clear();
        self.bounds = b;
        if self.form.is_none() {
            return;
        }
        let x0 = b.min_x() + px(PAD);
        let width = b.size.width - 2.0 * px(PAD);

        // From the bottom up (the footer, its buttons, is the window's): the strips
        // (failure, what's lost and its confirmation, rewrites), what runs, and the form gets
        // the rest from the top.
        let bottom = b.max_y() - px(BOTTOM_PAD);
        let dangers = self.dangers();
        let rewritten = match self.plan() {
            Some(Ok(Some(plan))) => plan.rewritten.clone(),
            _ => Vec::new(),
        };
        let line_h = px(22.0);
        let strip = |lines: usize| if lines == 0 { 0.0 } else { px(12.0) + lines as f32 * line_h };
        let failed_h = if self.failed.is_some() { strip(1) } else { 0.0 };
        let danger_h = strip(dangers.len());
        let confirm_h = if dangers.is_empty() { 0.0 } else { px(CONFIRM_H) };
        let info_h = strip(usize::from(!rewritten.is_empty()));
        let strips_h: f32 = [failed_h, danger_h, confirm_h, info_h]
            .iter()
            .filter(|h| **h > 0.0)
            .map(|h| h + px(8.0))
            .sum();
        let top = b.min_y() + px(12.0);
        // What runs gets the room its lines need (a few at least, at most half), the form
        // the rest; a form taller than that scrolls.
        let avail = bottom - strips_h - top;
        // Wrapped at about 8 logical pixels a character.
        let per_line = ((width - px(28.0)) / px(8.0)).max(20.0);
        let lines: f32 = self
            .preview_lines()
            .0
            .iter()
            .map(|l| (l.chars().count() as f32 / per_line).ceil().max(1.0))
            .sum();
        let lines = lines.clamp(2.0, 8.0);
        let preview_need = px(38.0) + lines * line_h + px(16.0);
        let extent = px(self.form_extent());
        let preview_min = px(38.0) + 2.0 * line_h + px(16.0);
        // A form that fits beside a few lines of what runs is shown whole (what runs
        // scrolls); a larger one shares the room.
        let form_h = if extent <= avail - preview_min {
            extent
        } else {
            extent.min((avail - preview_need).max(avail * 0.5))
        }
        .max(px(44.0));
        let form_area = Rect::new(b.min_x(), top, b.size.width, form_h);
        self.form_area = form_area;

        // The form, scrolled within its area.
        let before = self.hits.len();
        {
            let layer = scene.push_layer(Some(form_area));
            let mut pen = Pen { layer, text, icons, ds: &ds };
            let mut y = top - self.form_scroll.round();
            self.paint_form(&mut pen, x0, width, &mut y);
            let used = y + self.form_scroll.round() - top;
            self.form_max = (used - form_h).max(0.0);
            self.form_scroll = self.form_scroll.min(self.form_max);
        }
        for (r, _) in &mut self.hits[before..] {
            *r = r.intersect(&form_area).unwrap_or_default();
        }
        for r in self.anchors.values_mut() {
            *r =
                Rect::new(r.min_x(), r.min_y().max(form_area.min_y()), r.size.width, r.size.height);
        }
        self.form_bar.paint(
            scene,
            p,
            scale,
            form_area,
            self.form_max + form_h,
            self.form_scroll.round(),
        );

        // What runs.
        let mut y = form_area.max_y() + px(8.0);
        let layer = scene.push_layer(Some(b));
        ds::divider_card(layer, &ds, b.min_x(), b.max_x(), y - px(4.0));
        let sql = self.form.as_ref().is_some_and(|f| f.ctx.sql());
        let label = if sql { t("schema.preview_sql") } else { t("schema.preview_changes") };
        let small = ds.ui_font(-1.0).with_weight(Font::SEMIBOLD);
        let shaped = text.shape_line(&label.to_uppercase(), &small);
        draw_text(
            layer,
            &shaped,
            x0,
            ds::optical_baseline(y + px(4.0), px(20.0), &small),
            p.text_muted,
        );
        y += px(24.0);
        let card_bottom = bottom - strips_h - px(4.0);
        let card = Rect::new(x0, y, width, (card_bottom - y).max(px(40.0)));
        self.preview_area = card;
        self.paint_preview(scene, text, &ds, card, sql);

        // The strips.
        let mut y = card.max_y() + px(8.0);
        let layer = scene.push_layer(Some(b));
        let mut pen = Pen { layer, text, icons, ds: &ds };
        if let Some(failed) = self.failed.clone() {
            let r = Rect::new(x0, y, width, failed_h);
            self.strip(&mut pen, r, p.danger, "alert", &[failed], false);
            y = r.max_y() + px(8.0);
        }
        if !dangers.is_empty() {
            let r = Rect::new(x0, y, width, danger_h);
            let lines: Vec<String> = dangers.iter().map(danger_text).collect();
            self.strip(&mut pen, r, p.danger, "alert", &lines, false);
            y = r.max_y() + px(8.0);
            // What's lost, confirmed before it can run.
            let r = Rect::new(x0, y, width, confirm_h);
            self.paint_confirm(&mut pen, r);
            y = r.max_y() + px(8.0);
        }
        if !rewritten.is_empty() {
            let r = Rect::new(x0, y, width, info_h);
            let names: Vec<String> =
                rewritten.iter().map(|o| format!("{} {}", object_kind(o.kind), o.name)).collect();
            let line = tr!("schema.rewritten", objects = names.join(", "));
            self.strip(&mut pen, r, p.accent, "refresh", &[line], false);
        }

        // The open list, over everything (the shared picker): a choice's, or the
        // suggestions completing a field.
        match self.popup.and_then(|popup| self.anchors.get(&popup).copied()) {
            Some(anchor) => {
                let search = t("datasources.find_option");
                self.picker.paint(scene, text, icons, &ds, (anchor, b), &search);
            }
            None => self.picker.close(),
        }
    }

    /// A strip of lines with an icon (the last one a box to tick when `confirm`).
    fn strip(
        &mut self,
        pen: &mut Pen<'_, '_>,
        r: Rect,
        tone: Rgba,
        icon: &str,
        lines: &[String],
        confirm: bool,
    ) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        pen.layer.quad(Quad::filled(r, tone.with_alpha(0.10)).rounded(px(radius::MD)));
        let font = ds.ui_font(-0.5);
        let line_h = px(22.0);
        let mut y = r.min_y() + px(6.0);
        for (i, line) in lines.iter().enumerate() {
            let last = confirm && i + 1 == lines.len();
            let mid = y + line_h / 2.0;
            let mut x = r.min_x() + px(12.0);
            if last {
                let state = if self.confirmed { Check::On } else { Check::Off };
                let check = ds::checkbox(pen.layer, pen.text, ds, x, mid, state, true);
                self.hits.push((
                    Rect::new(r.min_x(), y, r.size.width, line_h),
                    Hit::Tick(Tick::Confirm),
                ));
                x = check.max_x() + px(10.0);
            } else if let Some(id) = pen.icons.id(if i == 0 { icon } else { "minus" }) {
                let s = px(14.0);
                let at = Rect::new(x + px(1.0), (mid - s / 2.0).round(), s, s);
                ui_kit::draw_icon(pen.layer, pen.icons, id, at, tone);
                x += px(26.0);
            }
            let room = r.max_x() - px(12.0) - x;
            let shown = ds::ellipsize(pen.text, line, &font, room.max(0.0));
            let shaped = pen.text.shape_line(&shown, &font);
            let color = if last { ds.palette.text } else { tone.mix(ds.palette.text, 0.25) };
            draw_text(pen.layer, &shaped, x, ds::optical_baseline(y, line_h, &font), color);
            y += line_h;
        }
    }

    /// What the preview shows: the statements that will run, the changes in words (a
    /// source without SQL), or why nothing can run yet (`true`: drawn muted).
    fn preview_lines(&self) -> (Vec<String>, bool) {
        match (self.changes(), self.plan()) {
            (Err(why), _) => (vec![why], true),
            (Ok(_), None) => (vec![t("schema.planning")], true),
            (Ok(_), Some(Err(e))) => {
                (vec![tr!("schema.plan_failed", why = crate::grid_source::message(e))], true)
            }
            (Ok(_), Some(Ok(Some(plan)))) => (plan.script.statements(), false),
            (Ok(changes), Some(Ok(None))) => (super::summary(&changes), false),
        }
    }

    /// The statements that will run (keywords lit), or the changes in words, or why
    /// nothing can run yet.
    fn paint_preview(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        card: Rect,
        sql: bool,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(card));
        layer.quad(
            Quad::filled(card, p.editor_background)
                .rounded(px(radius::MD))
                .bordered(ds.line(), p.border),
        );
        let inner = Rect::new(
            card.min_x() + px(14.0),
            card.min_y() + px(8.0),
            card.size.width - px(28.0),
            card.size.height - px(16.0),
        );
        let (lines, muted) = self.preview_lines();
        let line_h = px(22.0);
        let font = if sql && !muted { ds.code_font(-1.0) } else { ds.ui_font(-0.5) };
        let metrics = text.metrics(&font);
        let space = text.shape_line(" ", &font).width;
        let indent = 2.0 * space;
        // Each line's words, colored (SQL's keywords lit, its strings as strings), then
        // wrapped at the card's width (a long `CREATE TABLE` goes on, indented).
        let string = p.syntax.as_ref().map_or(p.text, |s| s.string);
        let mut rows: Vec<Vec<(f32, std::sync::Arc<text_layout::ShapedLine>, Rgba)>> = Vec::new();
        for line in &lines {
            let mut words: Vec<(String, Rgba)> = Vec::new();
            if muted || !sql {
                let shown = if muted { line.clone() } else { format!("• {line}") };
                let color = if muted { p.text_muted } else { p.text };
                words.extend(shown.split(' ').map(|w| (w.to_owned(), color)));
            } else {
                let mut quoted = false;
                for word in line.split(' ') {
                    let bare = word.trim_matches(|c: char| !c.is_ascii_alphabetic());
                    let in_string = quoted || word.starts_with('\'');
                    if word.starts_with('\'') {
                        quoted =
                            !(word.len() > 1 && word.trim_end_matches([',', ')']).ends_with('\''));
                    } else if quoted && word.trim_end_matches([',', ')', ';']).ends_with('\'') {
                        quoted = false;
                    }
                    let color = if in_string {
                        string
                    } else if KEYWORDS.contains(&bare) && bare == word.trim_matches(['(', ')', ','])
                    {
                        p.accent
                    } else if word.starts_with('"') {
                        p.text
                    } else {
                        p.text.mix(p.text_muted, 0.3)
                    };
                    words.push((word.to_owned(), color));
                }
            }
            let mut row = Vec::new();
            let mut x = 0.0;
            for (word, color) in words {
                let shaped = text.shape_line(&word, &font);
                if !row.is_empty() && x + space + shaped.width > inner.size.width {
                    rows.push(std::mem::take(&mut row));
                    x = indent;
                } else if !row.is_empty() {
                    x += space;
                }
                let w = shaped.width;
                row.push((x, shaped, color));
                x += w;
            }
            rows.push(row);
        }
        let content = rows.len() as f32 * line_h;
        self.preview_max = (content - inner.size.height).max(0.0);
        self.preview_scroll = self.preview_scroll.clamp(0.0, self.preview_max);
        let code = scene.push_layer(Some(inner));
        for (i, row) in rows.iter().enumerate() {
            let top = inner.min_y() + i as f32 * line_h - self.preview_scroll;
            if top + line_h < inner.min_y() || top > inner.max_y() {
                continue;
            }
            let base = ui_kit::centered_baseline(top, line_h, &metrics);
            for (x, shaped, color) in row {
                draw_text(code, shaped, inner.min_x() + x, base, *color);
            }
        }
        self.preview_bar.paint(scene, p, ds.scale, card, content + px(16.0), self.preview_scroll);
    }

    /// The box confirming what's lost (in `r`).
    fn paint_confirm(&mut self, pen: &mut Pen<'_, '_>, r: Rect) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let mid = r.min_y() + r.size.height / 2.0;
        let font = ds.ui_font(-0.5);
        let label = ds::ellipsize(pen.text, &t("schema.confirm"), &font, r.size.width - px(40.0));
        let state = if self.confirmed { Check::On } else { Check::Off };
        let check = ds::checkbox(pen.layer, pen.text, ds, r.min_x(), mid, state, true);
        let shaped = pen.text.shape_line(&label, &font);
        let lx = check.max_x() + px(10.0);
        let base = ds::optical_baseline(r.min_y(), r.size.height, &font);
        draw_text(pen.layer, &shaped, lx, base, ds.palette.danger);
        let hit = Rect::new(
            r.min_x() - px(4.0),
            r.min_y(),
            lx + shaped.width - r.min_x() + px(8.0),
            r.size.height,
        );
        self.hits.push((hit, Hit::Tick(Tick::Confirm)));
    }

    /// A field's label over it.
    fn label(&self, pen: &mut Pen<'_, '_>, x: f32, y: f32, label: &str) {
        let font = pen.ds.ui_font(-1.0);
        let shaped = pen.text.shape_line(label, &font);
        draw_text(
            pen.layer,
            &shaped,
            x,
            ds::optical_baseline(y, pen.ds.px(18.0), &font),
            pen.ds.palette.text_muted,
        );
    }

    /// A text field in `r` (its hit; the anchor of its suggestions).
    fn input(
        &mut self,
        pen: &mut Pen<'_, '_>,
        which: Text,
        r: Rect,
        placeholder: &str,
        small: bool,
    ) {
        let style = self.style(pen.ds, small);
        let focused = self.focus == Some(which);
        if let Some(input) = self.form.as_ref().and_then(|f| f.input(which)) {
            input.paint(pen.layer, pen.text, r, focused, placeholder, &style);
        }
        self.hits.push((r, Hit::Field(which)));
        self.anchors.insert(Popup::Suggest(which), r);
    }

    /// A labeled field at (`x`, `y`), `w` wide.
    fn group(
        &mut self,
        pen: &mut Pen<'_, '_>,
        which: Text,
        (x, y, w): (f32, f32, f32),
        label: &str,
        placeholder: &str,
    ) {
        self.label(pen, x, y, label);
        let r = Rect::new(x, y + pen.ds.px(22.0), w, pen.ds.px(FIELD_H));
        self.input(pen, which, r, placeholder, false);
    }

    /// A box with its label; its hit takes both.
    fn tick_box(
        &mut self,
        pen: &mut Pen<'_, '_>,
        tick: Tick,
        x: f32,
        mid: f32,
        on: bool,
        label: &str,
    ) -> f32 {
        let ds = pen.ds;
        let state = if on { Check::On } else { Check::Off };
        let r = ds::checkbox(pen.layer, pen.text, ds, x, mid, state, true);
        let mut right = r.max_x();
        if !label.is_empty() {
            let font = ds.ui_font(-0.5);
            let shaped = pen.text.shape_line(label, &font);
            let lx = r.max_x() + ds.px(8.0);
            draw_text(
                pen.layer,
                &shaped,
                lx,
                ds::optical_baseline(mid - ds.px(10.0), ds.px(20.0), &font),
                ds.palette.text,
            );
            right = lx + shaped.width;
        }
        let hit = Rect::new(x - ds.px(4.0), mid - ds.px(12.0), right - x + ds.px(8.0), ds.px(24.0));
        self.hits.push((hit, Hit::Tick(tick)));
        right
    }

    /// A list to choose from, shown as a field with its chosen value(s) and a chevron.
    fn dropdown(
        &mut self,
        pen: &mut Pen<'_, '_>,
        choice: Choice,
        r: Rect,
        value: &str,
        placeholder: &str,
    ) {
        let spec = ds::DropdownSpec {
            value,
            open: self.popup == Some(Popup::Choice(choice)),
            hovered: self.hover == Some(Hit::Choice(choice)),
            enabled: true,
            note: None,
            placeholder: Some(placeholder),
        };
        ds::dropdown(pen.layer, pen.text, pen.icons, pen.ds, r, spec);
        self.hits.push((r, Hit::Choice(choice)));
        self.anchors.insert(Popup::Choice(choice), r);
    }

    /// An icon's button (take a row out, add one): the shared one, its hit and its tooltip
    /// kept.
    fn icon_button(&mut self, pen: &mut Pen<'_, '_>, icon: &str, r: Rect, hit: Hit, tip: String) {
        let spec = ButtonSpec::new(ButtonKind::Ghost)
            .icon(icon)
            .hovered(self.hover.as_ref() == Some(&hit));
        ds::button(pen.layer, pen.text, pen.icons, pen.ds, r, spec);
        self.hits.push((r, hit));
        self.tips.push((r, tip));
    }

    /// A section's title, with what it says after it (muted).
    fn title(&self, pen: &mut Pen<'_, '_>, x: f32, y: f32, title: &str, detail: Option<&str>) {
        let ds = pen.ds;
        let font = ds.ui_font(0.0).with_weight(Font::SEMIBOLD);
        let shaped = pen.text.shape_line(title, &font);
        let base = ds::optical_baseline(y, ds.px(22.0), &font);
        draw_text(pen.layer, &shaped, x, base, ds.palette.text);
        if let Some(detail) = detail {
            let small = ds.ui_font(-1.0);
            let d = pen.text.shape_line(detail, &small);
            draw_text(pen.layer, &d, x + shaped.width + ds.px(10.0), base, ds.palette.text_muted);
        }
    }

    fn paint_form(&mut self, pen: &mut Pen<'_, '_>, x0: f32, width: f32, y: &mut f32) {
        let Some(kind) = self.form.as_ref().map(|f| f.kind.clone()) else { return };
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let half = ((width - px(16.0)) / 2.0).floor();
        match &kind {
            Kind::CreateTable => self.paint_create(pen, x0, width, y),
            Kind::RenameTable => {
                self.group(pen, Text::TableName, (x0, *y, half), &t("schema.table_name"), "");
                *y += px(GROUP_H);
            }
            Kind::CreateDatabase => {
                self.group(pen, Text::TableName, (x0, *y, half), &t("schema.database_name"), "");
                *y += px(GROUP_H);
            }
            Kind::CreateSchema => {
                self.group(pen, Text::TableName, (x0, *y, half), &t("schema.schema_name"), "");
                *y += px(GROUP_H);
            }
            Kind::AddColumn | Kind::AlterColumn(_) => self.paint_column(pen, x0, width, y),
            Kind::MoveColumn(_) => {
                self.label(pen, x0, *y, &t("schema.position"));
                let r = Rect::new(x0, *y + px(22.0), half, px(FIELD_H));
                let value = self
                    .options(Choice::Position)
                    .into_iter()
                    .find(|(_, on)| *on)
                    .map(|(l, _)| l)
                    .unwrap_or_default();
                self.dropdown(pen, Choice::Position, r, &value, "");
                *y += px(GROUP_H);
            }
            Kind::CreateIndex => self.paint_index(pen, x0, width, y),
            Kind::AddForeignKey => {
                let name_w = half;
                self.group(
                    pen,
                    Text::KeyName(0),
                    (x0, *y, name_w),
                    &t("schema.key_name"),
                    &t("schema.optional"),
                );
                *y += px(GROUP_H);
                self.paint_key(pen, 0, x0, width, y, true);
            }
            k => {
                // Something dropped: said in a sentence (what it loses is listed below).
                let what = match k {
                    Kind::DropTable => tr!(
                        "schema.drop_table_text",
                        table = self
                            .form
                            .as_ref()
                            .and_then(|f| f.table())
                            .map(|t| t.to_string())
                            .unwrap_or_default()
                    ),
                    Kind::DropColumn(c) => tr!("schema.drop_column_text", column = c.as_str()),
                    Kind::DropIndex(n) => tr!("schema.drop_index_text", name = n.as_str()),
                    Kind::DropForeignKey(key) => tr!(
                        "schema.drop_key_text",
                        columns = key.columns.join(", "),
                        table = key.table.to_string(),
                        references = key.references.join(", ")
                    ),
                    _ => String::new(),
                };
                let font = ds.ui_font(0.0);
                let shown = ds::ellipsize(pen.text, &what, &font, width);
                let shaped = pen.text.shape_line(&shown, &font);
                draw_text(
                    pen.layer,
                    &shaped,
                    x0,
                    ds::optical_baseline(*y + px(6.0), px(24.0), &font),
                    ds.palette.text,
                );
                *y += px(44.0);
            }
        }
    }

    /// One column's definition (added or changed).
    fn paint_column(&mut self, pen: &mut Pen<'_, '_>, x0: f32, width: f32, y: &mut f32) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let half = ((width - px(16.0)) / 2.0).floor();
        let x1 = x0 + half + px(16.0);
        let more = self.form.as_ref().is_some_and(|f| f.texts().len() > 1);
        self.group(pen, Text::Name(0), (x0, *y, half), &t("schema.column_name"), "");
        if !more {
            *y += px(GROUP_H);
            return;
        }
        self.group(
            pen,
            Text::Type(0),
            (x1, *y, half),
            &t("schema.column_type"),
            &t("schema.type_any"),
        );
        *y += px(GROUP_H);
        self.group(
            pen,
            Text::Default(0),
            (x0, *y, half),
            &t("schema.column_default"),
            &t("schema.default_hint"),
        );
        self.group(
            pen,
            Text::Collate(0),
            (x1, *y, half),
            &t("schema.column_collate"),
            &t("schema.optional"),
        );
        *y += px(GROUP_H);
        self.group(
            pen,
            Text::Generated(0),
            (x0, *y, half),
            &t("schema.column_generated"),
            &t("schema.optional"),
        );
        let (not_null, stored) = self
            .form
            .as_ref()
            .and_then(|f| f.columns.first())
            .map_or((false, false), |c| (c.not_null, c.stored));
        let mid = *y + px(22.0) + px(FIELD_H) / 2.0;
        let right = self.tick_box(pen, Tick::NotNull(0), x1, mid, not_null, &t("schema.not_null"));
        self.tick_box(pen, Tick::Stored(0), right + px(28.0), mid, stored, &t("schema.stored"));
        *y += px(GROUP_H);
    }

    /// A new table: its name, its columns as rows, the chosen column's details, its keys.
    fn paint_create(&mut self, pen: &mut Pen<'_, '_>, x0: f32, width: f32, y: &mut f32) {
        let ds = pen.ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let Some(form) = &self.form else { return };
        let sqlite = form.ctx.dialect.as_ref().is_some_and(|d| d.name() == "sqlite");
        let without_rowid = form.without_rowid;
        let count = form.columns.len();
        let keys = form.keys.len();
        let half = ((width - px(16.0)) / 2.0).floor();
        self.group(pen, Text::TableName, (x0, *y, half), &t("schema.table_name"), "");
        if sqlite {
            let mid = *y + px(22.0) + px(FIELD_H) / 2.0;
            self.tick_box(
                pen,
                Tick::WithoutRowid,
                x0 + half + px(16.0),
                mid,
                without_rowid,
                &t("schema.without_rowid"),
            );
        }
        *y += px(GROUP_H);

        // The columns: name, type, default, then NN / PK / AI / UQ, and ×.
        self.title(pen, x0, *y, &t("schema.columns"), None);
        *y += px(TITLE_H);
        let tick_w = px(40.0);
        let remove_w = px(30.0);
        let fixed = 4.0 * tick_w + remove_w + px(24.0);
        let flex = width - fixed;
        let (name_w, type_w, default_w) =
            ((flex * 0.36).floor(), (flex * 0.30).floor(), (flex * 0.34).floor() - px(16.0));
        let cols = [x0, x0 + name_w + px(8.0), x0 + name_w + type_w + px(16.0)];
        let ticks_x = cols[2] + default_w + px(16.0);
        let head = ds.ui_font(-1.5).with_weight(Font::SEMIBOLD);
        let heads = [
            (cols[0], t("schema.column_name")),
            (cols[1], t("schema.column_type")),
            (cols[2], t("schema.column_default")),
        ];
        for (x, label) in heads {
            let shaped = pen.text.shape_line(&label.to_uppercase(), &head);
            draw_text(
                pen.layer,
                &shaped,
                x + px(2.0),
                ds::optical_baseline(*y, px(20.0), &head),
                p.text_muted,
            );
        }
        let spelled = TICK_TIPS.map(t);
        for (i, label) in ["NN", "PK", "AI", "UQ"].iter().enumerate() {
            let shaped = pen.text.shape_line(label, &head);
            let cx = ticks_x + i as f32 * tick_w + tick_w / 2.0;
            let cell = Rect::new(ticks_x + i as f32 * tick_w, *y, tick_w, px(20.0));
            self.tips.push((cell, spelled[i].clone()));
            draw_text(
                pen.layer,
                &shaped,
                (cx - shaped.width / 2.0).round(),
                ds::optical_baseline(*y, px(20.0), &head),
                p.text_muted,
            );
        }
        *y += px(26.0);
        let field_h = px(30.0);
        for i in 0..count {
            let row = Rect::new(x0 - px(8.0), *y, width + px(16.0), px(ROW_H));
            let chosen = self.selected == i;
            ds::RowHighlight::new(chosen, false)
                .selected_alpha(COLUMN_CHOSEN)
                .paint(pen.layer, ds, row);
            self.hits.push((row, Hit::SelectColumn(i)));
            let fy = *y + (px(ROW_H) - field_h) / 2.0;
            self.input(
                pen,
                Text::Name(i),
                Rect::new(cols[0], fy, name_w, field_h),
                &t("schema.new_column"),
                true,
            );
            self.input(pen, Text::Type(i), Rect::new(cols[1], fy, type_w, field_h), "", true);
            self.input(pen, Text::Default(i), Rect::new(cols[2], fy, default_w, field_h), "", true);
            let Some(c) = self.form.as_ref().and_then(|f| f.columns.get(i)) else { break };
            let blank = c.is_blank();
            let states = [c.not_null, c.primary, c.auto, c.unique];
            let mid = *y + px(ROW_H) / 2.0;
            if !blank {
                let ticks = [Tick::NotNull(i), Tick::Primary(i), Tick::Auto(i), Tick::Unique(i)];
                for (j, (tick, on)) in ticks.into_iter().zip(states).enumerate() {
                    let cx = ticks_x + j as f32 * tick_w + tick_w / 2.0;
                    self.tick_box(pen, tick, (cx - px(8.0)).round(), mid, on, "");
                    let cell =
                        Rect::new(ticks_x + j as f32 * tick_w, mid - px(12.0), tick_w, px(24.0));
                    self.tips.push((cell, spelled[j].clone()));
                }
                let at = (ticks_x + 4.0 * tick_w + px(4.0), mid);
                let r = ds::square_button(pen.ds, at, ds::button_size::MEDIUM);
                self.icon_button(pen, "x", r, Hit::RemoveColumn(i), t("schema.tip_remove_column"));
            }
            *y += px(ROW_H);
        }
        *y += px(12.0);
        // The chosen column's details.
        let i = self.selected.min(count.saturating_sub(1));
        let name = self
            .form
            .as_ref()
            .and_then(|f| f.columns.get(i))
            .map(|c| c.name.text().trim().to_owned())
            .unwrap_or_default();
        let third = ((width - px(32.0)) / 3.0).floor();
        let label = |key: &str| {
            if name.is_empty() { t(key) } else { format!("{} · {name}", t(key)) }
        };
        self.group(
            pen,
            Text::Check(i),
            (x0, *y, third),
            &label("schema.column_check"),
            &t("schema.optional"),
        );
        self.group(
            pen,
            Text::Collate(i),
            (x0 + third + px(16.0), *y, third),
            &t("schema.column_collate"),
            &t("schema.optional"),
        );
        let gx = x0 + 2.0 * (third + px(16.0));
        let stored = self.form.as_ref().and_then(|f| f.columns.get(i)).is_some_and(|c| c.stored);
        self.label(pen, gx, *y, &t("schema.column_generated"));
        let stored_w = px(84.0);
        let r = Rect::new(gx, *y + px(22.0), third - stored_w, px(FIELD_H));
        self.input(pen, Text::Generated(i), r, &t("schema.optional"), false);
        let mid = r.min_y() + r.size.height / 2.0;
        self.tick_box(pen, Tick::Stored(i), r.max_x() + px(12.0), mid, stored, &t("schema.stored"));
        *y += px(GROUP_H);

        // The foreign keys.
        let detail = (keys == 0).then(|| t("database.none"));
        self.title(pen, x0, *y, &t("schema.foreign_keys"), detail.as_deref());
        *y += px(TITLE_H);
        for k in 0..keys {
            self.paint_key(pen, k, x0, width, y, false);
        }
        let add = t("schema.add_key");
        let spec = ButtonSpec::new(ButtonKind::Ghost)
            .icon("plus")
            .label(&add)
            .tone(Tone::Accent)
            .hovered(self.hover == Some(Hit::AddKey));
        let h = px(button_size::LARGE);
        let r = Rect::new(x0 - px(6.0), *y, ds::button_width(pen.text, ds, &spec, h), h);
        ds::button(pen.layer, pen.text, pen.icons, ds, r, spec);
        self.hits.push((r, Hit::AddKey));
        *y += px(36.0);
    }

    /// A foreign key's row: its columns → the table and its columns, the actions (× to
    /// take it out of a new table; its own lines, labeled, when it's the form).
    fn paint_key(
        &mut self,
        pen: &mut Pen<'_, '_>,
        k: usize,
        x0: f32,
        width: f32,
        y: &mut f32,
        alone: bool,
    ) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let Some(row) = self.form.as_ref().and_then(|f| f.keys.get(k)) else { return };
        let columns = row.columns.join(", ");
        let target = row.target.as_ref().map(ToString::to_string).unwrap_or_default();
        let references = row.references.join(", ");
        let (on_delete, on_update) = (row.on_delete.sql(), row.on_update.sql());
        if alone {
            let third = ((width - px(64.0)) / 3.0).floor();
            let h = px(FIELD_H);
            self.label(pen, x0, *y, &t("schema.key_columns"));
            self.label(pen, x0 + third + px(32.0), *y, &t("schema.key_table"));
            self.label(pen, x0 + 2.0 * (third + px(32.0)), *y, &t("schema.key_references"));
            let fy = *y + px(22.0);
            self.dropdown(
                pen,
                Choice::KeyColumns(k),
                Rect::new(x0, fy, third, h),
                &columns,
                &t("schema.choose"),
            );
            self.arrow(pen, x0 + third, fy, h);
            self.dropdown(
                pen,
                Choice::KeyTarget(k),
                Rect::new(x0 + third + px(32.0), fy, third, h),
                &target,
                &t("schema.choose"),
            );
            self.dropdown(
                pen,
                Choice::KeyReferences(k),
                Rect::new(x0 + 2.0 * (third + px(32.0)), fy, third, h),
                &references,
                &t("schema.choose"),
            );
            *y += px(GROUP_H);
            let half = ((width - px(16.0)) / 2.0).floor();
            self.label(pen, x0, *y, &t("schema.on_delete"));
            self.label(pen, x0 + half + px(16.0), *y, &t("schema.on_update"));
            let fy = *y + px(22.0);
            self.dropdown(pen, Choice::OnDelete(k), Rect::new(x0, fy, half, h), on_delete, "");
            self.dropdown(
                pen,
                Choice::OnUpdate(k),
                Rect::new(x0 + half + px(16.0), fy, half, h),
                on_update,
                "",
            );
            *y += px(GROUP_H);
            return;
        }
        // One line: columns → table (columns), on delete, on update, ×.
        let h = px(30.0);
        let fy = *y + (px(ROW_H) - h) / 2.0;
        let remove_w = px(30.0);
        let flex = width - remove_w - px(24.0) - 3.0 * px(8.0) - px(28.0);
        let w = [0.22, 0.2, 0.2, 0.19, 0.19].map(|s: f32| (flex * s).floor());
        let mut x = x0;
        self.dropdown(
            pen,
            Choice::KeyColumns(k),
            Rect::new(x, fy, w[0], h),
            &columns,
            &t("schema.key_columns"),
        );
        x += w[0];
        self.arrow(pen, x, fy, h);
        x += px(28.0);
        self.dropdown(
            pen,
            Choice::KeyTarget(k),
            Rect::new(x, fy, w[1], h),
            &target,
            &t("schema.key_table"),
        );
        x += w[1] + px(8.0);
        self.dropdown(
            pen,
            Choice::KeyReferences(k),
            Rect::new(x, fy, w[2], h),
            &references,
            &t("schema.key_references"),
        );
        x += w[2] + px(8.0);
        let del = format!("{} {on_delete}", t("schema.on_delete_short"));
        self.dropdown(pen, Choice::OnDelete(k), Rect::new(x, fy, w[3], h), &del, "");
        x += w[3] + px(8.0);
        let upd = format!("{} {on_update}", t("schema.on_update_short"));
        self.dropdown(pen, Choice::OnUpdate(k), Rect::new(x, fy, w[4], h), &upd, "");
        let at = (x0 + width - px(28.0), *y + px(ROW_H) / 2.0);
        let r = ds::square_button(pen.ds, at, ds::button_size::MEDIUM);
        self.icon_button(pen, "x", r, Hit::RemoveKey(k), t("schema.tip_remove_key"));
        *y += px(ROW_H) + px(8.0);
    }

    /// `→` between a key's columns and what they point to.
    fn arrow(&self, pen: &mut Pen<'_, '_>, x: f32, top: f32, h: f32) {
        let ds = pen.ds;
        if let Some(id) = pen.icons.id("arrow-right") {
            let s = ds.px(14.0);
            let at = Rect::new((x + ds.px(7.0)).round(), (top + (h - s) / 2.0).round(), s, s);
            ui_kit::draw_icon(pen.layer, pen.icons, id, at, ds.palette.text_muted);
        }
    }

    /// An index: its name and uniqueness, its columns (picked in order, each ascending or
    /// descending), a partial index's condition.
    fn paint_index(&mut self, pen: &mut Pen<'_, '_>, x0: f32, width: f32, y: &mut f32) {
        let ds = pen.ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let Some(form) = &self.form else { return };
        let unique = form.unique;
        let all = form.own_columns();
        let chosen = form.index_columns.clone();
        let half = ((width - px(16.0)) / 2.0).floor();
        self.group(pen, Text::IndexName, (x0, *y, half), &t("schema.index_name"), "");
        let mid = *y + px(22.0) + px(FIELD_H) / 2.0;
        self.tick_box(
            pen,
            Tick::IndexUnique,
            x0 + half + px(16.0),
            mid,
            unique,
            &t("schema.unique"),
        );
        *y += px(GROUP_H);
        self.title(pen, x0, *y, &t("schema.index_columns"), Some(&t("schema.index_columns_hint")));
        *y += px(TITLE_H);
        let font = ds.ui_font(-0.5);
        // The columns in two lists side by side (down the first, then the second).
        let per_list = all.len().div_ceil(2).max(1);
        let cell_w = half;
        for (i, name) in all.iter().enumerate() {
            let cx = x0 + if i < per_list { 0.0 } else { half + px(16.0) };
            let cy = *y + (i % per_list) as f32 * px(LIST_H);
            let r = Rect::new(cx - px(8.0), cy, cell_w + px(8.0), px(LIST_H));
            let place = chosen.iter().position(|(c, _)| c == name);
            let hit = Hit::IndexColumn(name.clone());
            let hovered = self.hover.as_ref() == Some(&hit);
            ds::RowHighlight::new(false, hovered)
                .hover(ds::HoverFill::Faint(INDEX_HOVER))
                .radius(radius::SM)
                .paint(pen.layer, ds, r);
            let mid = r.min_y() + r.size.height / 2.0;
            let state = if place.is_some() { Check::On } else { Check::Off };
            ds::Checkbox::new(state).hovered(hovered).paint(pen.layer, pen.text, ds, cx, mid);
            let order_right = cx + cell_w;
            let room = cell_w - px(28.0) - px(160.0);
            let shown = ds::ellipsize(pen.text, name, &font, room.max(px(40.0)));
            let shaped = pen.text.shape_line(&shown, &font);
            draw_text(
                pen.layer,
                &shaped,
                cx + px(28.0),
                ds::optical_baseline(r.min_y(), r.size.height, &font),
                p.text,
            );
            self.hits
                .push((Rect::new(r.min_x(), r.min_y(), cell_w - px(140.0), r.size.height), hit));
            if let Some(at) = place {
                // Its place in the index, and its order (a click flips it).
                let number = format!("{}", at + 1);
                ds::Badge::new(&number)
                    .color(p.accent)
                    .fill_alpha(PLACE_FILL)
                    .pad(PLACE_PAD)
                    .min_width(ds::BadgeSize::Regular.height())
                    .centered()
                    .paint(pen.layer, pen.text, ds, order_right - px(150.0), mid);
                let desc = chosen[at].1;
                let labels = ["ASC", "DESC"];
                let hovered = matches!(&self.hover, Some(Hit::IndexOrder(c)) if c == name)
                    .then_some(usize::from(!desc));
                let rects = ds::segmented(
                    pen.layer,
                    pen.text,
                    ds,
                    ds::SegmentedSpec {
                        right: order_right,
                        mid,
                        labels: &labels,
                        selected: usize::from(desc),
                        hovered,
                    },
                );
                for (j, r) in rects.into_iter().enumerate() {
                    if j != usize::from(desc) {
                        self.hits.push((r, Hit::IndexOrder(name.clone())));
                    }
                }
            }
        }
        *y += per_list as f32 * px(LIST_H);
        *y += px(14.0);
        self.group(
            pen,
            Text::Where,
            (x0, *y, width),
            &t("schema.index_where"),
            &t("schema.index_where_hint"),
        );
        *y += px(GROUP_H);
    }
}
