//! The structure of the shown table: its columns (type, nullability, default, key, what
//! else they are), its indexes and its foreign keys — each a small table of its own, under
//! a title with how many and, where the driver can, a button adding one; a row's
//! right-click menu changes or drops it (the structure's modal, [`crate::schema`]).

use database::{FkAction, Structure};
use kernel::Rect;
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_components::ds::{self, ButtonKind, ButtonSpec, Ctx, Tone, button_size};
use ui_kit::draw_text;

use super::{DataView, Hit};

/// The focused row: its fill's alpha of the accent.
const FOCUS_FILL: f32 = 0.14;

/// A section of the structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Columns,
    Indexes,
    Keys,
}

/// What a section's title button adds.
fn add_label(part: Part) -> String {
    i18n::t(match part {
        Part::Columns => "schema.add_column",
        Part::Indexes => "schema.add_index",
        Part::Keys => "schema.add_key",
    })
}

/// A section of the structure: its title and its table.
struct Section {
    title: String,
    heads: Vec<String>,
    /// Each column's share of the width.
    shares: Vec<f32>,
    rows: Vec<Vec<Cell>>,
    empty: String,
    /// The row pointed at (lit).
    focus: Option<usize>,
}

/// A cell: its text, and how it's drawn.
#[derive(Debug, Clone)]
pub(crate) struct Cell {
    pub text: String,
    pub mono: bool,
    pub muted: bool,
    pub key: bool,
}

impl Cell {
    fn plain(text: impl Into<String>) -> Self {
        Self { text: text.into(), mono: false, muted: false, key: false }
    }

    fn mono(text: impl Into<String>) -> Self {
        Self { mono: true, ..Self::plain(text) }
    }

    fn muted(text: impl Into<String>) -> Self {
        Self { muted: true, ..Self::plain(text) }
    }
}

fn action(a: FkAction) -> String {
    a.sql().to_owned()
}

/// What a column is besides its type (its key's place, auto-increment, generated,
/// collation), as the structure lists it.
pub(crate) fn column_notes(structure: &Structure, column: &database::ColumnInfo) -> String {
    let mut notes = Vec::new();
    if column.in_key {
        let key = structure.primary_key();
        if key.len() > 1 {
            let at = key.iter().position(|k| *k == column.name).map_or(0, |i| i + 1);
            notes.push(i18n::tr!("database.col_key_part", place = at));
        } else {
            notes.push(i18n::t("database.col_primary"));
        }
    }
    if column.auto_increment {
        notes.push(i18n::t("database.col_auto"));
    }
    if let Some(g) = &column.generated {
        notes.push(i18n::tr!(
            if g.stored { "database.col_stored" } else { "database.col_virtual" },
            expression = g.expression.as_str()
        ));
    }
    if let Some(c) = &column.collation {
        notes.push(format!("COLLATE {c}"));
    }
    notes.join(" · ")
}

fn sections(structure: &Structure, focus: Option<&str>) -> Vec<Section> {
    let t = i18n::t;
    let columns = Section {
        title: i18n::tr!("database.columns", count = structure.columns.len()),
        heads: vec![
            t("database.col_name"),
            t("database.col_type"),
            t("database.col_null"),
            t("database.col_default"),
            t("database.col_notes"),
        ],
        shares: vec![0.22, 0.18, 0.1, 0.18, 0.32],
        rows: structure
            .columns
            .iter()
            .map(|c| {
                vec![
                    Cell { key: c.in_key, ..Cell::plain(c.name.clone()) },
                    Cell::mono(c.declared_type.clone()),
                    Cell::muted(if c.nullable { t("database.yes") } else { t("database.no") }),
                    Cell::mono(c.default.clone().unwrap_or_default()),
                    Cell::muted(column_notes(structure, c)),
                ]
            })
            .collect(),
        empty: t("database.none"),
        focus: focus.and_then(|f| {
            let columns = &structure.columns;
            columns
                .iter()
                .position(|c| c.name == f)
                .or_else(|| columns.iter().position(|c| c.name.eq_ignore_ascii_case(f)))
        }),
    };
    let indexes = Section {
        title: i18n::tr!("database.indexes", count = structure.indexes.len()),
        heads: vec![
            t("database.col_name"),
            t("database.index_columns"),
            t("database.index_unique"),
        ],
        shares: vec![0.35, 0.45, 0.2],
        rows: structure
            .indexes
            .iter()
            .map(|i| {
                let mut name = i.name.clone();
                if i.implicit {
                    name.push_str(&format!(" ({})", t("database.index_implicit")));
                }
                vec![
                    Cell::plain(name),
                    Cell::mono(i.columns.join(", ")),
                    Cell::muted(if i.unique { t("database.yes") } else { t("database.no") }),
                ]
            })
            .collect(),
        empty: t("database.none"),
        focus: None,
    };
    let keys = Section {
        title: i18n::tr!("database.foreign_keys", count = structure.foreign_keys.len()),
        heads: vec![
            t("database.col_name"),
            t("database.fk_columns"),
            t("database.fk_references"),
            t("database.fk_actions"),
        ],
        shares: vec![0.2, 0.2, 0.3, 0.3],
        rows: structure
            .foreign_keys
            .iter()
            .map(|f| {
                vec![
                    Cell::plain(f.name.clone().unwrap_or_else(|| "—".to_owned())),
                    Cell::mono(f.columns.join(", ")),
                    Cell::mono(format!("{} ({})", f.table, f.references.join(", "))),
                    Cell::muted(i18n::tr!(
                        "database.fk_on",
                        update = action(f.on_update),
                        delete = action(f.on_delete)
                    )),
                ]
            })
            .collect(),
        empty: t("database.none"),
        focus: None,
    };
    vec![columns, indexes, keys]
}

impl DataView {
    pub(super) fn paint_structure(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        area: Rect,
    ) {
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(area));
        let structure = self.grid.as_ref().and_then(|g| g.source().structure().cloned());
        let Some(structure) = structure else {
            let label = i18n::t("grid.loading");
            let loader = ds::Loader::with_text(&label);
            let w = loader.width(text, ds);
            ds::loader(
                layer,
                text,
                ds,
                area.min_x() + (area.size.width - w) / 2.0,
                area.min_y() + area.size.height / 2.0,
                &loader,
            );
            return;
        };
        let sections = sections(&structure, self.structure_focus.as_deref());
        let can = self.schema_changes();
        let view = self.table().is_some_and(|t| t.kind == database::TableKind::View);
        let adds = [
            (Part::Columns, can.columns),
            (Part::Indexes, can.indexes),
            (Part::Keys, can.foreign_keys),
        ];
        let row_h = px(30.0);
        let content: f32 = sections
            .iter()
            .map(|s| px(56.0) + row_h * (s.rows.len().max(1) as f32 + 1.0) + px(16.0))
            .sum::<f32>()
            + px(24.0);
        // The column pointed at, brought into view once (its row centered when it can be).
        if std::mem::take(&mut self.structure_reveal)
            && let Some(row) = sections.first().and_then(|s| s.focus)
        {
            let top = px(20.0) + px(32.0) + row_h * (row as f32 + 1.0);
            self.structure_scroll = top - (area.size.height - row_h) / 2.0;
        }
        self.structure_scroll =
            self.structure_scroll.clamp(0.0, (content - area.size.height).max(0.0));
        let mut y = area.min_y() + px(20.0) - self.structure_scroll;
        let x = area.min_x() + px(24.0);
        let width = (area.size.width - px(48.0)).max(px(200.0));
        for (section, (part, can)) in sections.iter().zip(adds) {
            let add = (can && !view).then(|| add_label(part));
            let hovered = self.hover == Some(Hit::Add(part));
            let painted = paint_section(
                layer,
                text,
                icons,
                ds,
                section,
                (x, y, width),
                add.as_deref().map(|label| (label, hovered)),
            );
            y = painted.bottom;
            if let Some(r) = painted.add {
                self.hits.push((r, Hit::Add(part)));
                if painted.compact {
                    self.tips.push((r, add_label(part)));
                }
            }
            for r in painted.keys {
                if r.intersect(&area).is_some() {
                    self.tips.push((r, i18n::t("database.tip_primary_key")));
                }
            }
            for (i, r) in painted.rows.into_iter().enumerate() {
                if let Some(r) = r.intersect(&area) {
                    self.hits.push((r, Hit::Row(part, i)));
                }
            }
        }
        let scroll = self.structure_scroll;
        self.structure_bar.paint(scene, ds.palette, ds.scale, area, content, scroll);
    }
}

/// Where a section was drawn: its end, its rows, its title's button (only its icon), the
/// keys' marks.
struct Painted {
    bottom: f32,
    rows: Vec<Rect>,
    add: Option<Rect>,
    compact: bool,
    keys: Vec<Rect>,
}

/// A section at (`x`, `y`), `width` wide; `add`: its title's button (its label, hovered).
fn paint_section(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ds: &Ctx<'_>,
    section: &Section,
    (x, mut y, width): (f32, f32, f32),
    add: Option<(&str, bool)>,
) -> Painted {
    let p = ds.palette;
    let px = |v: f32| ds.px(v);
    let title_font = ds.ui_font(0.5).with_weight(Font::SEMIBOLD);
    let mut add_rect = None;
    let mut compact = false;
    let mut title_room = width;
    if let Some((label, hovered)) = add {
        // A quiet button on the title's line, at the right: `+ Column` (only `+` when the
        // title wouldn't fit beside it, its label then in its tooltip).
        let spec = ButtonSpec::new(ButtonKind::Ghost)
            .icon("plus")
            .label(label)
            .tone(Tone::Accent)
            .hovered(hovered);
        let h = px(button_size::MEDIUM);
        let full = ds::button_width(text, ds, &spec, h);
        let title_w = text.shape_line(&section.title, &title_font).width;
        compact = title_w + px(12.0) + full > width;
        let (w, spec) =
            if compact { (h, ButtonSpec { label: None, ..spec }) } else { (full, spec) };
        let r = Rect::new((x + width - w).round(), (y - px(2.0)).round(), w, h);
        ds::button(layer, text, icons, ds, r, spec);
        title_room = r.min_x() - x - px(12.0);
        add_rect = Some(r);
    }
    let title = ds::ellipsize(text, &section.title, &title_font, title_room.max(0.0));
    let shaped = text.shape_line(&title, &title_font);
    draw_text(layer, &shaped, x, (y + px(18.0)).round(), p.text);
    y += px(32.0);
    let mut row_rects = Vec::new();
    let row_h = px(30.0);
    let rows = section.rows.len().max(1);
    let card = Rect::new(x, y, width, row_h * (rows as f32 + 1.0));
    layer.quad(
        Quad::filled(card, p.text.with_alpha(0.015))
            .rounded(px(ds::radius::MD))
            .bordered(ds.line(), p.border),
    );
    let head = Rect::new(x, y, width, row_h);
    layer.quad(Quad::filled(
        Rect::new(x + ds.line(), y + ds.line(), width - 2.0 * ds.line(), row_h - ds.line()),
        p.text.with_alpha(0.025),
    ));
    let head_font = ds.ui_font(-1.5).with_weight(Font::SEMIBOLD);
    let cell_font = ds.ui_font(-0.5);
    let mono = ds.code_font(-1.5);
    let mut edges = vec![x];
    let mut acc = x;
    for share in &section.shares {
        acc += width * share;
        edges.push(acc);
    }
    for (i, h) in section.heads.iter().enumerate() {
        let room = edges[i + 1] - edges[i] - px(24.0);
        let line = ds::ellipsize(text, &h.to_uppercase(), &head_font, room.max(0.0));
        let shaped = text.shape_line(&line, &head_font);
        draw_text(
            layer,
            &shaped,
            edges[i] + px(12.0),
            ds::optical_baseline(head.min_y(), row_h, &head_font),
            p.text_muted,
        );
    }
    y += row_h;
    if section.rows.is_empty() {
        ds::divider(layer, ds, x, x + width, y);
        let shaped = text.shape_line(&section.empty, &cell_font);
        draw_text(
            layer,
            &shaped,
            x + px(12.0),
            ds::optical_baseline(y, row_h, &cell_font),
            p.text_muted,
        );
        y += row_h;
    }
    let mut keys = Vec::new();
    for (r, row) in section.rows.iter().enumerate() {
        row_rects.push(Rect::new(x, y, width, row_h));
        if section.focus == Some(r) {
            let lit = Rect::new(x + ds.line(), y, width - 2.0 * ds.line(), row_h);
            ds::RowHighlight::new(true, false)
                .selected_alpha(FOCUS_FILL)
                .radius(0.0)
                .paint(layer, ds, lit);
            layer.quad(Quad::filled(Rect::new(x + ds.line(), y, px(3.0), row_h), p.accent));
        }
        ds::divider(layer, ds, x, x + width, y);
        for (i, cell) in row.iter().enumerate() {
            let mut cx = edges[i] + px(12.0);
            let mut room = edges[i + 1] - edges[i] - px(24.0);
            if cell.key
                && let Some(id) = icons.id("key")
            {
                let s = px(12.0);
                let mark = Rect::new(cx, (y + (row_h - s) / 2.0).round(), s, s);
                ui_kit::draw_icon(layer, icons, id, mark, p.warning);
                keys.push(mark);
                cx += s + px(6.0);
                room -= s + px(6.0);
            }
            let font = if cell.mono { &mono } else { &cell_font };
            let line = ds::ellipsize(text, &cell.text, font, room.max(0.0));
            let shaped = text.shape_line(&line, font);
            let color = if cell.muted { p.text_muted } else { p.text };
            draw_text(layer, &shaped, cx, ds::optical_baseline(y, row_h, font), color);
        }
        y += row_h;
    }
    Painted { bottom: y + px(24.0), rows: row_rects, add: add_rect, compact, keys }
}
