//! Painting the wizard: the connection's form, the folder chosen on the server, its contents as a
//! tree, the footer.

use super::*;
use ui_components::ds::{ButtonKind, ButtonSpec, button_size};
use ui_components::tree;

/// A row of the lists (the folders completing a path, a folder's entries): the chosen one in
/// the accent, the one under the mouse faint.
fn lit_row(chosen: bool, hovered: bool) -> ds::RowHighlight {
    ds::RowHighlight::new(chosen, hovered)
        .selected_alpha(ROW_CHOSEN)
        .hover(ds::HoverFill::Faint(ROW_HOVER))
}

/// The lists' rows: the chosen one's accent alpha and the hovered one's text alpha.
const ROW_CHOSEN: f32 = 0.14;
const ROW_HOVER: f32 = 0.05;
/// A path's chip: its fill's alpha of the accent; the folder's chip on the form, the chosen
/// path's in the footer (height and room on each side, logical pixels).
const CHIP_FILL: f32 = 0.08;
const FOLDER_CHIP_H: f32 = 28.0;
const FOLDER_CHIP_PAD: f32 = 10.0;
const SELECTED_CHIP_H: f32 = 26.0;
const SELECTED_CHIP_PAD: f32 = 9.0;

/// A quiet icon button beside a field or a row (the shared ghost one): muted at rest, lit
/// under the mouse.
fn quiet_icon(icon: &str) -> ButtonSpec<'_> {
    ButtonSpec::new(ButtonKind::Ghost).icon(icon).rest_tone(ds::Tone::Neutral)
}

impl ConnectView {
    pub(super) fn style(&self, ds: &Ctx<'_>) -> TextInputStyle {
        TextInputStyle { padding: ds.px(12.0), ..ds::field_style(ds, ds.ui_font(0.0), false) }
    }

    /// The folders completing the focused path (the local copy's, the server's), in a list
    /// under its field.
    pub(super) fn paint_suggestions(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
    ) {
        if self.suggestions.is_empty() {
            return;
        }
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let field = self.completed_field;
        let row_h = px(30.0);
        let list = Rect::new(
            field.min_x(),
            field.max_y() + px(4.0),
            field.size.width,
            self.suggestions.len() as f32 * row_h + px(8.0),
        );
        let layer = scene.push_layer(None);
        ds::surface(layer, ds, list, ds::Elevation::High, radius::MD);
        let font = ds.ui_font(-0.5);
        let mono = ds.ui_font(-1.0);
        for (i, path) in self.suggestions.iter().enumerate() {
            let r = Rect::new(
                list.min_x() + px(4.0),
                list.min_y() + px(4.0) + i as f32 * row_h,
                list.size.width - px(8.0),
                row_h,
            );
            lit_row(self.suggested == Some(i), self.hover == Some(Hit::Suggestion(i)))
                .radius(radius::SM)
                .paint(layer, ds, r);
            if let Some(id) = icons.id("folder") {
                let s = px(15.0);
                ui_kit::draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new(r.min_x() + px(8.0), r.min_y() + (row_h - s) / 2.0, s, s),
                    p.folder_icon,
                );
            }
            let (parent, name) =
                path.rsplit_once(['/', std::path::MAIN_SEPARATOR]).unwrap_or(("", path));
            let shaped = text.shape_line(name, &font);
            let x = r.min_x() + px(32.0);
            draw_text(layer, &shaped, x, ds::optical_baseline(r.min_y(), row_h, &font), p.text);
            let room = r.max_x() - px(10.0) - (x + shaped.width + px(10.0));
            if room > px(40.0) {
                let shown = ds::ellipsize_start(text, parent, &mono, room);
                let dim = text.shape_line(&shown, &mono);
                draw_text(
                    layer,
                    &dim,
                    r.max_x() - px(10.0) - dim.width,
                    ds::optical_baseline(r.min_y(), row_h, &mono),
                    p.text_muted,
                );
            }
            self.hits.push((r, Hit::Suggestion(i)));
        }
    }

    /// A label and its field at (`x`, `y`), `w` wide.
    pub(super) fn field(&mut self, pen: Pen<'_, '_>, f: Field, x: f32, y: f32, w: f32) {
        let Pen { layer, text, icons, ds } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let key = match f {
            Field::Host => "remote.host",
            Field::Port => "remote.port",
            Field::User => "remote.user",
            Field::Secret if self.auth == AuthKind::Key => "remote.passphrase",
            Field::Secret => "remote.password",
            Field::Key => "remote.key",
            Field::Name => "remote.name",
            Field::Label => "remote.connection_name",
            Field::Local => "remote.local",
            Field::Path => "remote.step_folder",
        };
        let label_font = ds.ui_font(-1.0);
        let label = text.shape_line(&t(key), &label_font);
        draw_text(layer, &label, x, ds::optical_baseline(y, px(18.0), &label_font), p.text_muted);
        let field_h = px(38.0);
        let r = Rect::new(x, y + px(22.0), w, field_h);
        // Editing: the kept password is never shown; empty, it stays.
        let kept = t("remote.secret_kept");
        let placeholder = match f {
            Field::Host => "example.com",
            Field::Key => "~/.ssh/id_ed25519",
            Field::Secret if self.editing.is_some() => kept.as_str(),
            _ => "",
        };
        // Editing, the project's folder stays: shown, not typed in.
        let fixed = self.editing.is_some() && matches!(f, Field::Name | Field::Local);
        // What the field holds, as an icon before the text (in the accent while typed in).
        let icon = match f {
            Field::Host => Some("globe"),
            Field::Port => Some("hash"),
            Field::User => Some("user"),
            Field::Secret => Some("lock"),
            Field::Key => Some("key"),
            Field::Name | Field::Label => Some("tag"),
            Field::Local => Some("folder"),
            Field::Path => None,
        };
        let focused = self.focused() == f && !self.on_list && !fixed;
        let mut style = self.style(ds);
        if icon.is_some() {
            style.lead = px(24.0);
        }
        if self.invalid == Some(f) {
            (style.border, style.border_focused) = (p.danger, p.danger);
        }
        if fixed {
            (style.background, style.text) = (p.text.with_alpha(0.03), p.text_muted);
        }
        self.input(f).paint(layer, text, r, focused, placeholder, &style);
        if let Some(id) = icon.and_then(|name| icons.id(name)) {
            let s = px(16.0);
            let at = Rect::new(
                (r.min_x() + px(12.0)).round(),
                (r.min_y() + (field_h - s) / 2.0).round(),
                s,
                s,
            );
            ui_kit::draw_icon(layer, icons, id, at, if focused { p.accent } else { p.text_muted });
        }
        if fixed {
            return;
        }
        self.hits.push((r, Hit::Field(f)));
        // The eye: shows / hides the secret.
        if f == Field::Secret {
            let size = px(button_size::SMALL);
            let eye = Rect::new(
                r.max_x() - size - px(8.0),
                r.min_y() + (field_h - size) / 2.0,
                size,
                size,
            );
            let icon = if self.input(f).is_masked() { "eye" } else { "eye-off" };
            let spec = quiet_icon(icon).hovered(self.hover == Some(Hit::Eye));
            ds::button(layer, text, icons, ds, eye, spec);
            self.hits.push((eye, Hit::Eye));
        }
        // The local folder: chosen in the system's picker too.
        if f == Field::Local {
            self.completed_field = r;
            let size = px(button_size::MEDIUM);
            let browse = Rect::new(
                r.max_x() - size - px(6.0),
                r.min_y() + (field_h - size) / 2.0,
                size,
                size,
            );
            let spec = quiet_icon("folder-open").hovered(self.hover == Some(Hit::Browse));
            ds::button(layer, text, icons, ds, browse, spec);
            self.hits.push((browse, Hit::Browse));
        }
    }

    /// A row of choices (pills) under a label.
    pub(super) fn choices(
        &mut self,
        pen: Pen<'_, '_>,
        x0: f32,
        y: &mut f32,
        label: &str,
        options: Vec<(String, bool, Hit)>,
    ) {
        let Pen { layer, text, ds, .. } = pen;
        let px = |v: f32| ds.px(v);
        let label_font = ds.ui_font(-1.0);
        let shaped = text.shape_line(label, &label_font);
        draw_text(
            layer,
            &shaped,
            x0,
            ds::optical_baseline(*y, px(18.0), &label_font),
            ds.palette.text_muted,
        );
        *y += px(24.0);
        // One of a few, side by side (a segmented control).
        let names: Vec<&str> = options.iter().map(|(name, _, _)| name.as_str()).collect();
        let selected = options.iter().position(|(_, on, _)| *on).unwrap_or(0);
        let hovered = options.iter().position(|(_, _, hit)| self.hover == Some(*hit));
        let right = x0 + ds::segmented_width(text, ds, &names);
        let rects = ds::segmented(
            layer,
            text,
            ds,
            ds::SegmentedSpec { right, mid: *y + px(16.0), labels: &names, selected, hovered },
        );
        for (r, (_, _, hit)) in rects.into_iter().zip(options) {
            self.hits.push((r, hit));
        }
        *y += px(46.0);
    }

    pub(super) fn paint_connection(&mut self, pen: Pen<'_, '_>, column: Column, y: &mut f32) {
        let Pen { layer, text, icons, ds } = pen;
        let Column { x0, width } = column;
        let px = |v: f32| ds.px(v);
        if self.editing.is_some() {
            let fonts = Fonts::of(ds);
            self.paint_root_chip(Pen { layer, text, icons, ds }, column, &fonts, y);
        }
        let protocols = PROTOCOLS
            .iter()
            .enumerate()
            .map(|(i, pr)| {
                (
                    t(&format!("remote.protocol_{}", pr.id().replace('-', "_"))),
                    *pr == self.protocol,
                    Hit::Protocol(i),
                )
            })
            .collect();
        self.choices(Pen { layer, text, icons, ds }, x0, y, &t("remote.protocol"), protocols);
        let gap = px(12.0);
        let port_w = px(110.0);
        self.field(Pen { layer, text, icons, ds }, Field::Host, x0, *y, width - port_w - gap);
        self.field(Pen { layer, text, icons, ds }, Field::Port, x0 + width - port_w, *y, port_w);
        *y += px(72.0);
        if self.protocol == Protocol::Sftp {
            let auths = [
                (AuthKind::Password, "remote.auth_password"),
                (AuthKind::Key, "remote.auth_key"),
                (AuthKind::Agent, "remote.auth_agent"),
            ]
            .iter()
            .map(|(k, key)| (t(key), *k == self.auth, Hit::Auth(*k)))
            .collect();
            self.choices(Pen { layer, text, icons, ds }, x0, y, &t("remote.auth"), auths);
        }
        let half = (width - gap) / 2.0;
        self.field(Pen { layer, text, icons, ds }, Field::User, x0, *y, half);
        if self.auth != AuthKind::Agent {
            self.field(Pen { layer, text, icons, ds }, Field::Secret, x0 + half + gap, *y, half);
        }
        *y += px(72.0);
        if self.auth == AuthKind::Key && self.protocol == Protocol::Sftp {
            self.field(Pen { layer, text, icons, ds }, Field::Key, x0, *y, width);
            *y += px(72.0);
        }
        // An unknown server: its fingerprint and "Trust".
        if let Some(fingerprint) = self.unknown.clone() {
            let p = ds.palette;
            let mono = ds.code_font(-1.0);
            let shown = ds::ellipsize(text, &fingerprint, &mono, width - px(24.0));
            let shaped = text.shape_line(&shown, &mono);
            let card = Rect::new(x0, *y, width, px(40.0));
            layer.quad(
                Quad::filled(card, p.accent.with_alpha(0.08))
                    .rounded(px(radius::MD))
                    .bordered(ds.line(), p.accent.with_alpha(0.4)),
            );
            draw_text(
                layer,
                &shaped,
                x0 + px(12.0),
                ds::optical_baseline(*y, px(40.0), &mono),
                p.text,
            );
            *y += px(52.0);
        }
    }

    pub(super) fn paint_folder(
        &mut self,
        pen: Pen<'_, '_>,
        column: Column,
        y: &mut f32,
        body: Rect,
    ) {
        let Pen { layer, text, icons, ds } = pen;
        let Column { x0, width } = column;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(-0.5);
        let mono = ds.code_font(-0.5);
        // "Up", and where we are (typed: gone there with ↵).
        let field_h = px(38.0);
        let up = Rect::new(x0, *y, px(38.0), field_h);
        let spec =
            ButtonSpec::new(ButtonKind::Soft).icon("arrow-up").hovered(self.hover == Some(Hit::Up));
        ds::button(layer, text, icons, ds, up, spec);
        self.hits.push((up, Hit::Up));
        let r = Rect::new(up.max_x() + px(8.0), *y, width - up.size.width - px(8.0), field_h);
        let style = TextInputStyle { font: mono, ..self.style(ds) };
        self.input(Field::Path).paint(layer, text, r, !self.on_list, "/", &style);
        self.hits.push((r, Hit::Field(Field::Path)));
        self.completed_field = r;
        *y += field_h + px(10.0);
        if self.waiting > 0 && self.listing.is_empty() {
            let loading = t("remote.listing");
            ds::loader(layer, text, ds, x0, *y + px(14.0), &ds::Loader::with_text(&loading));
            *y += px(36.0);
            return;
        }
        // The folder's entries, in a card.
        let row_h = px(34.0);
        let inset = px(6.0);
        if !self.listing.is_empty() {
            let card = Rect::new(x0, *y, width, self.listing.len() as f32 * row_h + 2.0 * inset);
            layer.quad(
                Quad::filled(card, Rgba::TRANSPARENT)
                    .rounded(px(radius::LG))
                    .bordered(ds.line(), p.card_border),
            );
            *y += inset;
        }
        let (x0, width) = (x0 + inset, width - 2.0 * inset);
        for (i, entry) in self.listing.iter().enumerate() {
            let top = *y;
            *y += row_h;
            if top + row_h < body.min_y() || top > body.max_y() {
                continue;
            }
            let row = Rect::new(x0, top, width, row_h);
            let name = entry.path.rsplit('/').next().unwrap_or(&entry.path);
            let chosen = self.on_list && self.highlight == Some(i);
            lit_row(chosen, entry.is_dir && self.hover == Some(Hit::Enter(i)))
                .paint(layer, ds, row);
            let (icon, color) = if entry.is_dir {
                ("folder", p.folder_icon)
            } else {
                ("file", p.text_muted.with_alpha(0.6))
            };
            if let Some(id) = icons.id(icon) {
                let s = px(16.0);
                ui_kit::draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new(x0 + px(8.0), top + (row_h - s) / 2.0, s, s),
                    color,
                );
            }
            let label = text.shape_line(name, &font);
            let label_color = if entry.is_dir { p.text } else { p.text_muted };
            draw_text(
                layer,
                &label,
                x0 + px(34.0),
                ds::optical_baseline(top, row_h, &font),
                label_color,
            );
            if entry.is_dir {
                // A folder opens: a chevron says so.
                let s = px(12.0);
                let at = Rect::new(row.max_x() - px(12.0) - s, top + (row_h - s) / 2.0, s, s);
                tree::chevron(layer, icons, at, false, p.text_muted);
                self.hits.push((row, Hit::Enter(i)));
            }
        }
        if !self.listing.is_empty() {
            *y += inset;
        }
    }

    /// The last step: the folder, how it's worked on, the transfers, the tree, where it's
    /// saved.
    pub(super) fn paint_contents(&mut self, mut pen: Pen<'_, '_>, column: Column, y: &mut f32) {
        let fonts = Fonts::of(pen.ds);
        self.paint_root_chip(pen.lend(), column, &fonts, y);
        self.paint_modes(pen.lend(), column, &fonts, y);
        self.paint_transfers(pen.lend(), column, &fonts, y);
        let auto = (self.auto_upload, "remote.auto_upload", Hit::AutoUpload);
        self.paint_option(pen.lend(), column, &fonts, y, auto);
        let moves = (self.moves, "remote.moves", Hit::Moves);
        self.paint_option(pen.lend(), column, &fonts, y, moves);
        *y += pen.ds.px(8.0);
        self.paint_write_as(pen.lend(), column, &fonts, y);
        self.paint_tree(pen.lend(), column, &fonts, y);
        self.paint_exclusions(pen.lend(), column, &fonts, y);
        self.paint_saving(pen, column, y);
    }

    /// The chosen folder, in a chip.
    pub(super) fn paint_root_chip(
        &self,
        pen: Pen<'_, '_>,
        column: Column,
        fonts: &Fonts,
        y: &mut f32,
    ) {
        let Pen { layer, text, ds, .. } = pen;
        let Column { x0, width } = column;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let Fonts { small, mono, .. } = fonts;
        // Editing, on the form: the saved folder ("Continue" browses from it).
        let key =
            if self.step == Step::Connection { "remote.server_folder" } else { "remote.from" };
        let from = text.shape_line(&t(key), small);
        let row_h = px(28.0);
        draw_text(layer, &from, x0, ds::optical_baseline(*y, row_h, small), p.text_muted);
        let chip_x = x0 + from.width + px(12.0);
        let path = ds::ellipsize(text, &self.root, mono, (width - from.width - px(40.0)).max(0.0));
        ds::Badge::new(&path)
            .color(p.accent)
            .fill_alpha(CHIP_FILL)
            .font(mono)
            .size(ds::BadgeSize::Custom(FOLDER_CHIP_H))
            .pad(FOLDER_CHIP_PAD)
            .radius(radius::SM)
            .paint(layer, text, ds, chip_x, *y + row_h / 2.0);
        *y += row_h + px(18.0);
    }

    /// How the folder is worked on: two cards, one chosen.
    pub(super) fn paint_modes(
        &mut self,
        pen: Pen<'_, '_>,
        column: Column,
        fonts: &Fonts,
        y: &mut f32,
    ) {
        let Pen { layer, text, ds, .. } = pen;
        let Column { x0, width } = column;
        let px = |v: f32| ds.px(v);
        let label = text.shape_line(&t("remote.mode"), &fonts.small);
        draw_text(
            layer,
            &label,
            x0,
            ds::optical_baseline(*y, px(18.0), &fonts.small),
            ds.palette.text_muted,
        );
        *y += px(26.0);
        let gap = px(12.0);
        let card_w = ((width - gap) / 2.0).floor();
        let card_h = px(92.0);
        for (i, (mode, title, detail)) in [
            (Mode::Mirror, "remote.mode_mirror", "remote.mode_mirror_detail"),
            (Mode::OnDemand, "remote.mode_on_demand", "remote.mode_on_demand_detail"),
        ]
        .into_iter()
        .enumerate()
        {
            let card = Rect::new(x0 + i as f32 * (card_w + gap), *y, card_w, card_h);
            let hit = Hit::Mode(mode);
            ds::radio_card(
                layer,
                text,
                ds,
                ds::RadioCardSpec {
                    rect: card,
                    title: &t(title),
                    detail: &t(detail),
                    selected: self.mode == mode,
                    hovered: self.hover == Some(hit),
                },
            );
            self.hits.push((card, hit));
        }
        *y += card_h + px(20.0);
    }

    /// Transfers at once: as in the settings (their value shown), or this connection's.
    pub(super) fn paint_transfers(
        &mut self,
        pen: Pen<'_, '_>,
        column: Column,
        fonts: &Fonts,
        y: &mut f32,
    ) {
        let Pen { layer, text, ds, .. } = pen;
        let x0 = column.x0;
        let px = |v: f32| ds.px(v);
        let small = &fonts.small;
        let global = crate::settings::defaults().limits;
        for (key, current, global_n, choices, hit_of) in [
            (
                "remote.parallel_uploads",
                self.uploads,
                global.uploads,
                [1, 2, 4],
                Hit::Uploads as fn(Option<usize>) -> Hit,
            ),
            (
                "remote.parallel_downloads",
                self.downloads,
                global.downloads,
                [2, 4, 8],
                Hit::Downloads,
            ),
        ] {
            let label = text.shape_line(&t(key), small);
            draw_text(
                layer,
                &label,
                x0,
                ds::optical_baseline(*y, px(18.0), small),
                ds.palette.text_muted,
            );
            *y += px(26.0);
            let mut x = x0;
            let options = std::iter::once(None).chain(choices.into_iter().map(Some));
            for option in options {
                let name = match option {
                    None => tr!("remote.parallel_global", count = global_n),
                    Some(n) => n.to_string(),
                };
                let hit = hit_of(option);
                let r = ds::choice(
                    layer,
                    text,
                    ds,
                    ds::ChoiceSpec {
                        x,
                        mid: *y + px(16.0),
                        label: &name,
                        selected: current == option,
                        hovered: self.hover == Some(hit),
                    },
                );
                self.hits.push((r, hit));
                x = r.max_x() + px(8.0);
            }
            *y += px(48.0);
        }
    }

    /// An option of the connection, on or off: a checkbox and its label (`key`), `hit` when
    /// clicked.
    pub(super) fn paint_option(
        &mut self,
        pen: Pen<'_, '_>,
        column: Column,
        fonts: &Fonts,
        y: &mut f32,
        (on, key, hit): (bool, &str, Hit),
    ) {
        let Pen { layer, text, ds, .. } = pen;
        let Column { x0, width } = column;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let font = &fonts.ui;
        let row = Rect::new(x0, *y, width, px(28.0));
        let state = if on { Check::On } else { Check::Off };
        let mid = row.min_y() + row.size.height / 2.0;
        let check = ds::checkbox(layer, text, ds, x0, mid, state, true);
        let label = text.shape_line(&t(key), font);
        let lit = self.hover == Some(hit);
        draw_text(
            layer,
            &label,
            check.max_x() + px(10.0),
            ds::optical_baseline(row.min_y(), row.size.height, font),
            if lit { p.text } else { p.text_muted },
        );
        let hit_w = check.max_x() + px(10.0) + label.width - x0;
        self.hits.push((Rect::new(x0, row.min_y(), hit_w, row.size.height), hit));
        *y += px(36.0);
    }

    /// The folder's tree under its hint (a loader while it's listed), one row per entry.
    pub(super) fn paint_tree(
        &mut self,
        mut pen: Pen<'_, '_>,
        column: Column,
        fonts: &Fonts,
        y: &mut f32,
    ) {
        let ds = pen.ds;
        let x0 = column.x0;
        let px = |v: f32| ds.px(v);
        let small = &fonts.small;
        let hint = pen.text.shape_line(&t("remote.contents_hint"), small);
        draw_text(
            pen.layer,
            &hint,
            x0,
            ds::optical_baseline(*y, px(18.0), small),
            ds.palette.text_muted,
        );
        *y += px(28.0);
        if self.tree.is_empty() && self.waiting > 0 {
            let loading = t("remote.listing");
            let loader = ds::Loader::with_text(&loading);
            ds::loader(pen.layer, pen.text, ds, x0, *y + px(14.0), &loader);
            *y += px(36.0);
        }
        let row_h = px(30.0);
        for i in 0..self.tree.len() {
            let top = *y;
            *y += row_h;
            self.paint_tree_row(pen.lend(), column, fonts, i, top);
        }
        *y += px(14.0);
    }

    /// A row of the tree: a checkbox (included), the chevron of a folder, the name.
    pub(super) fn paint_tree_row(
        &mut self,
        pen: Pen<'_, '_>,
        column: Column,
        fonts: &Fonts,
        i: usize,
        top: f32,
    ) {
        let Pen { layer, text, icons, ds } = pen;
        let Column { x0, width } = column;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let font = &fonts.ui;
        let row_h = px(30.0);
        let node = self.tree[i].clone();
        let indent = node.depth as f32 * px(18.0);
        let mid = top + row_h / 2.0;
        // The whole line: selected in the accent, under the mouse in a light tint (a
        // little apart from the next one's, so two lit ones don't touch).
        let line = Rect::new(x0, top + px(1.5), width, row_h - px(3.0));
        // The guides: a line down from each folder above it, under its checkbox (left
        // of its children's).
        let guide_x = |level: usize| x0 + level as f32 * px(18.0) + px(14.0);
        let guide_w = ds.line().max(1.0);
        tree::guides(layer, 0..node.depth, guide_x, (top, row_h), guide_w, p.indent_guide);
        let lit = matches!(self.hover, Some(Hit::Check(h) | Hit::Expand(h)) if h == i);
        if self.on_list && self.highlight == Some(i) {
            tree::highlight(layer, line, p.accent.with_alpha(0.14), px(radius::MD));
        } else if lit {
            tree::highlight(layer, line, p.text.with_alpha(0.05), px(radius::MD));
        }
        let by_parent = !self.excluded.contains(&node.path) && self.is_excluded(&node.path);
        let state = if self.is_excluded(&node.path) { Check::Off } else { Check::On };
        let check = ds::checkbox(layer, text, ds, x0 + indent + px(6.0), mid, state, !by_parent);
        self.hits.push((
            Rect::new(
                check.min_x() - px(4.0),
                check.min_y() - px(4.0),
                check.size.width + px(8.0),
                check.size.height + px(8.0),
            ),
            Hit::Check(i),
        ));
        let mut x = check.max_x() + px(8.0);
        if node.is_dir {
            let s = px(12.0);
            let at = Rect::new(x, mid - s / 2.0, s, s);
            tree::chevron(layer, icons, at, node.expanded, p.text_muted);
        }
        x += px(16.0);
        let (icon, color) =
            if node.is_dir { ("folder", p.folder_icon) } else { ("file", p.text_muted) };
        if let Some(id) = icons.id(icon) {
            let s = px(15.0);
            ui_kit::draw_icon(layer, icons, id, Rect::new(x, mid - s / 2.0, s, s), color);
        }
        x += px(22.0);
        let name = node.path.rsplit('/').next().unwrap_or(&node.path);
        let shaped = text.shape_line(name, font);
        let color =
            if self.is_excluded(&node.path) { p.text_muted.with_alpha(0.6) } else { p.text };
        draw_text(layer, &shaped, x, ds::optical_baseline(top, row_h, font), color);
        // The rest of the line: a folder opens / closes, a file is checked.
        let rest = Rect::new(
            check.max_x() + px(4.0),
            top,
            (line.max_x() - check.max_x() - px(4.0)).max(0.0),
            row_h,
        );
        self.hits.push((rest, if node.is_dir { Hit::Expand(i) } else { Hit::Check(i) }));
    }

    /// The exclusions, with their full paths (each with an × that includes it again).
    pub(super) fn paint_exclusions(
        &mut self,
        pen: Pen<'_, '_>,
        column: Column,
        fonts: &Fonts,
        y: &mut f32,
    ) {
        let Pen { layer, text, icons, ds } = pen;
        let Column { x0, width } = column;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let Fonts { small, mono, .. } = fonts;
        let excluded = self.exclusions();
        if excluded.is_empty() {
            return;
        }
        let title = text.shape_line(&tr!("remote.excluded", count = excluded.len()), small);
        draw_text(layer, &title, x0, ds::optical_baseline(*y, px(18.0), small), p.text_muted);
        *y += px(24.0);
        for (i, path) in excluded.iter().enumerate() {
            let row = Rect::new(x0, *y, width, px(26.0));
            let shaped = text.shape_line(path, mono);
            draw_text(
                layer,
                &shaped,
                x0 + px(8.0),
                ds::optical_baseline(row.min_y(), row.size.height, mono),
                p.text,
            );
            let size = px(18.0);
            let close = Rect::new(
                row.max_x() - size - px(4.0),
                row.min_y() + (row.size.height - size) / 2.0,
                size,
                size,
            );
            let spec = quiet_icon("x").round().hovered(self.hover == Some(Hit::Unexclude(i)));
            ds::button(layer, text, icons, ds, close, spec);
            self.hits.push((close, Hit::Unexclude(i)));
            *y += px(26.0);
        }
        *y += px(10.0);
    }

    /// Where the project is kept: its name and label, the local folder, where the
    /// connection and its secrets are saved.
    pub(super) fn paint_saving(&mut self, mut pen: Pen<'_, '_>, column: Column, y: &mut f32) {
        let Column { x0, width } = column;
        let px = |v: f32| pen.ds.px(v);
        let gap = px(12.0);
        let half = (width - gap) / 2.0;
        self.field(pen.lend(), Field::Name, x0, *y, half);
        self.field(pen.lend(), Field::Label, x0 + half + gap, *y, half);
        *y += px(72.0);
        self.field(pen.lend(), Field::Local, x0, *y, width);
        *y += px(72.0);
        // Editing: why the local copy stays where it is.
        if self.editing.is_some() {
            let small = pen.ds.ui_font(-1.0);
            let note = ds::ellipsize(pen.text, &t("remote.local_fixed"), &small, width);
            let shaped = pen.text.shape_line(&note, &small);
            let base = ds::optical_baseline(*y - px(6.0), px(18.0), &small);
            draw_text(pen.layer, &shaped, x0, base, pen.ds.palette.text_muted);
            *y += px(22.0);
        }
        let saved_in =
            [(SavedIn::Project, "remote.saved_project"), (SavedIn::Global, "remote.saved_global")]
                .iter()
                .map(|(k, key)| (t(key), *k == self.saved_in, Hit::SavedIn(*k)))
                .collect();
        self.choices(pen.lend(), x0, y, &t("remote.saved_in"), saved_in);
        if self.auth != AuthKind::Agent {
            let secrets = [
                (SecretsIn::Keychain, "remote.secrets_keychain"),
                (SecretsIn::ProjectEncrypted, "remote.secrets_project"),
            ]
            .iter()
            .map(|(k, key)| (t(key), *k == self.secrets_in, Hit::SecretsIn(*k)))
            .collect();
            self.choices(pen, x0, y, &t("remote.secrets_in"), secrets);
        }
    }

    /// The footer: what's happening, Back, and the step's main button.
    pub(super) fn paint_footer(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        column: Column,
        foot: Rect,
    ) {
        let p = ds.palette;
        let layer = ds::modal_footer_bar(scene, ds, foot);
        let mid = foot.min_y() + foot.size.height / 2.0;
        let busy = self.waiting > 0 || self.downloading.is_some();
        let right = self.paint_footer_buttons(layer, text, ds, column, mid, busy);
        let bar = FooterBar { x0: column.x0, right, rect: foot, mid };
        // Nothing happening: the folder chosen (on its step) or the keys.
        if self.status.is_none() {
            if self.step == Step::Folder {
                self.paint_footer_folder(layer, text, ds, &bar);
            } else {
                let mut hints = self.key_hints();
                hints.retain(|(keys, _)| keys != &vec!["↵"]);
                hints.push((vec!["esc"], t("keys.close")));
                let hints: Vec<ui_kit::KeyHint<'_>> =
                    hints.iter().map(|(k, w)| (k.as_slice(), w.as_str())).collect();
                let row =
                    Rect::new(bar.x0, foot.min_y(), (right - bar.x0).max(0.0), foot.size.height);
                ui_kit::paint_key_hints(layer, text, &hints, row, &p.key_hints(ds.scale));
            }
        }
        self.paint_footer_status(layer, text, ds, &bar, busy);
    }

    /// The footer's buttons, right to left: the main one (with the key that presses it),
    /// then Back — or Cancel while working (also while connecting, on the first step).
    /// Returns where the room left of them ends.
    pub(super) fn paint_footer_buttons(
        &mut self,
        layer: &mut render_api::Layer,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        column: Column,
        mid: f32,
        busy: bool,
    ) -> f32 {
        let editing = self.editing.is_some();
        let (label, hit) = match self.step {
            Step::Connection | Step::Contents if editing => (t("remote.save"), Hit::Save),
            Step::Connection if self.unknown.is_some() => (t("remote.trust"), Hit::Next),
            Step::Connection => (t("remote.continue"), Hit::Next),
            Step::Folder => (t("remote.choose_folder"), Hit::Choose),
            Step::Contents if self.mode == Mode::OnDemand => {
                (t("remote.connect_on_demand"), Hit::Connect)
            }
            Step::Contents => (t("remote.connect"), Hit::Connect),
        };
        // Right to left after it: Cancel while working; else Back (past the first step) —
        // editing, on the form: Continue (browse from the saved folder) and Test Connection
        // ("Trust" when the server's fingerprint is shown).
        let seconds: Vec<(Hit, String)> = if busy {
            vec![(Hit::Cancel, t("remote.review_cancel"))]
        } else if self.step != Step::Connection {
            vec![(Hit::Back, t("remote.back"))]
        } else if editing {
            let test = if self.unknown.is_some() { "remote.trust" } else { "remote.test" };
            vec![(Hit::Next, t("remote.continue")), (Hit::Test, t(test))]
        } else {
            Vec::new()
        };
        let mut hits = vec![hit];
        let mut buttons = vec![ds::FooterButton {
            label: &label,
            main: true,
            enabled: !busy,
            hovered: self.hover == Some(hit),
            ..ds::FooterButton::default()
        }];
        for (second_hit, second_label) in &seconds {
            hits.push(*second_hit);
            buttons.push(ds::FooterButton {
                label: second_label,
                main: false,
                enabled: true,
                hovered: self.hover == Some(*second_hit),
                ..ds::FooterButton::default()
            });
        }
        let (areas, left) =
            ds::footer_buttons(layer, text, ds, column.x0 + column.width, mid, &buttons);
        self.hits.extend(areas.into_iter().zip(hits));
        left
    }

    /// The folder chosen, in a chip (on the folder's step, nothing happening).
    pub(super) fn paint_footer_folder(
        &self,
        layer: &mut render_api::Layer,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        bar: &FooterBar,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let FooterBar { x0, right, rect: foot, mid } = *bar;
        let small = ds.ui_font(-1.0);
        let mono = ds.code_font(-1.0);
        let label = text.shape_line(&t("remote.selected"), &small);
        draw_text(
            layer,
            &label,
            x0,
            ds::optical_baseline(foot.min_y(), foot.size.height, &small),
            p.text_muted,
        );
        let path = self.value(Field::Path);
        let room = (right - x0 - label.width - px(32.0)).max(0.0);
        let path = ds::ellipsize(text, &path, &mono, room);
        ds::Badge::new(&path)
            .color(p.text)
            .fill(p.popup_background)
            .border(p.card_border)
            .font(&mono)
            .size(ds::BadgeSize::Custom(SELECTED_CHIP_H))
            .pad(SELECTED_CHIP_PAD)
            .radius(radius::SM)
            .paint(layer, text, ds, x0 + label.width + px(10.0), mid);
    }

    /// What's happening (a loader while working, the download's progress).
    pub(super) fn paint_footer_status(
        &self,
        layer: &mut render_api::Layer,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        bar: &FooterBar,
        busy: bool,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let FooterBar { x0, right, rect: foot, mid } = *bar;
        let room = (right - x0).max(0.0);
        let Some((message, tone)) = self.status.clone() else {
            return;
        };
        if busy {
            let (done, total) =
                (self.progress.0.load(Ordering::Relaxed), self.progress.1.load(Ordering::Relaxed));
            let shown = if self.downloading.is_some() && total > 0 {
                format!("{message} {done} / {total}")
            } else {
                message
            };
            let shown = ds::ellipsize(text, &shown, &ds.ui_font(-0.5), room - px(30.0));
            ds::loader(layer, text, ds, x0, mid, &ds::Loader::with_text(&shown));
        } else {
            let small = ds.ui_font(-1.0);
            let color = tone.color(p);
            let shown = ds::ellipsize(text, &message, &small, room);
            let shaped = text.shape_line(&shown, &small);
            draw_text(
                layer,
                &shaped,
                x0,
                ds::optical_baseline(foot.min_y(), foot.size.height, &small),
                color,
            );
        }
    }
}

impl Fonts {
    pub(super) fn of(ds: &Ctx<'_>) -> Self {
        Self { ui: ds.ui_font(-0.5), small: ds.ui_font(-1.0), mono: ds.code_font(-1.0) }
    }
}
