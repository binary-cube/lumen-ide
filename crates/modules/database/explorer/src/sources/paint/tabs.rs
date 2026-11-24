//! The source's other tabs: options, SSL, schemas, the checks, the advanced fields.

use super::*;

impl SourcesView {
    pub(super) fn paint_options(&mut self, pen: &mut Pen<'_, '_>, x: f32, w: f32, y: &mut f32) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let Some(editor) = &self.editor else { return };
        let Some(entry) = editor.current() else { return };
        let file_based = editor.driver(&entry.source.driver).is_some_and(|d| d.file_based);
        let read_only = entry.source.read_only;
        let mid = *y + px(14.0);
        let state = if read_only { Check::On } else { Check::Off };
        let check = ds::checkbox(pen.layer, pen.text, ds, x, mid, state, true);
        let font = ds.ui_font(-0.5);
        let shaped = pen.text.shape_line(&t("datasources.read_only"), &font);
        let lx = check.max_x() + px(8.0);
        draw_text(
            pen.layer,
            &shaped,
            lx,
            ds::optical_baseline(mid - px(10.0), px(20.0), &font),
            ds.palette.text,
        );
        self.hits.push((
            Rect::new(x - px(4.0), mid - px(12.0), lx + shaped.width - x + px(8.0), px(24.0)),
            Hit::ReadOnly,
        ));
        *y += px(30.0);
        *y += self.note(pen, lx, *y, w - (lx - x), &t("datasources.read_only_note")) + px(16.0);
        // Production: its own mark, or its folder's (shown on, the box disabled).
        let inherited =
            editor.selected.and_then(|i| editor.inherited_production(i)).map(|f| f.name.clone());
        let on = entry.source.production || inherited.is_some();
        let lx = self.check_row(
            pen,
            (x, *y),
            &t("datasources.production"),
            on,
            inherited.is_none(),
            Hit::Production,
        );
        *y += px(30.0);
        let note = match inherited {
            Some(folder) => tr!("datasources.production_inherited", folder = folder.as_str()),
            None => t("datasources.production_note"),
        };
        *y += self.note(pen, lx, *y, w - (lx - x), &note) + px(16.0);
        let Some(editor) = &self.editor else { return };
        let fields = editor.fields_in(FieldTab::Options);
        for field in fields {
            let h = if field.kind == FieldKind::Flag { px(52.0) } else { px(GROUP_H) };
            self.spec_field(pen, &field, x, *y, w);
            *y += h;
        }
        if !file_based {
            self.label(pen, x, *y, &t("datasources.field_startup"), None, w);
            self.input(
                pen,
                Field::Startup,
                Rect::new(x, *y + px(22.0), w, px(FIELD_H)),
                &t("datasources.startup_hint"),
            );
            *y += px(GROUP_H);
        }
        *y += self.note(pen, x, *y, w, &t("datasources.idle_note")) + px(8.0);
    }

    pub(super) fn paint_ssl(&mut self, pen: &mut Pen<'_, '_>, x: f32, w: f32, y: &mut f32) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let Some(editor) = &self.editor else { return };
        let Some(entry) = editor.current() else { return };
        if editor.driver(&entry.source.driver).is_some_and(|d| d.file_based) {
            *y += self.note(pen, x, *y, w, &t("datasources.ssl_file_note"));
            return;
        }
        self.title(pen, x, *y, &t("datasources.ssl_title"));
        *y += px(32.0);
        let fields = editor.fields_in(FieldTab::Ssl);
        if fields.is_empty() {
            *y += self.note(pen, x, *y, w, &t("datasources.ssl_none")) + px(12.0);
        }
        let mut i = 0;
        while i < fields.len() {
            // A client's certificate and its key side by side.
            let file = |f: &FieldSpec| matches!(f.kind, FieldKind::File { .. });
            if i > 0
                && file(&fields[i])
                && file(&fields[i - 1])
                && fields.get(i + 1).is_some_and(file)
            {
                let half = ((w - px(12.0)) / 2.0).floor();
                self.spec_field(pen, &fields[i], x, *y, half);
                self.spec_field(pen, &fields[i + 1], x + half + px(12.0), *y, w - half - px(12.0));
                i += 2;
            } else {
                self.spec_field(pen, &fields[i], x, *y, w);
                i += 1;
            }
            *y += px(GROUP_H);
        }
        *y += px(8.0);
        self.title(pen, x, *y, &t("datasources.ssh_title"));
        *y += px(28.0);
        *y += self.note(pen, x, *y, w, &t("datasources.ssh_note")) + px(12.0);
        let port_w = px(120.0);
        self.label(pen, x, *y, &t("datasources.field_ssh_host"), None, w);
        self.label(pen, x + w - port_w, *y, &t("datasources.field_port"), None, port_w);
        self.disabled(pen, Rect::new(x, *y + px(22.0), w - port_w - px(12.0), px(FIELD_H)), "");
        self.disabled(pen, Rect::new(x + w - port_w, *y + px(22.0), port_w, px(FIELD_H)), "22");
        *y += px(GROUP_H);
        let half = ((w - px(12.0)) / 2.0).floor();
        self.label(pen, x, *y, &t("datasources.field_user"), None, half);
        self.label(pen, x + half + px(12.0), *y, &t("datasources.field_ssh_key"), None, half);
        self.disabled(pen, Rect::new(x, *y + px(22.0), half, px(FIELD_H)), "");
        self.disabled(
            pen,
            Rect::new(x + half + px(12.0), *y + px(22.0), half, px(FIELD_H)),
            "~/.ssh/id_ed25519",
        );
        *y += px(GROUP_H);
    }

    pub(super) fn paint_schemas(&mut self, pen: &mut Pen<'_, '_>, x: f32, w: f32, y: &mut f32) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let Some(editor) = &self.editor else { return };
        let Some(entry) = editor.current() else { return };
        let driver = editor.driver(&entry.source.driver);
        let has = driver.is_some_and(|d| d.schemas);
        let has_databases = driver.is_some_and(|d| d.databases);
        if !has {
            *y += self.note(pen, x, *y, w, &t("datasources.schemas_none"));
            return;
        }
        // A server's databases first (those the tree shows), then the schemas shown in each.
        if has_databases {
            let all = editor.databases();
            let every = entry.source.databases.is_empty();
            *y += self.note(pen, x, *y, w, &t("datasources.databases_note")) + px(12.0);
            if all.is_empty() {
                *y += self.note(pen, x, *y, w, &t("datasources.databases_test")) + px(16.0);
            } else {
                let rows: Vec<(String, bool, Hit)> =
                    std::iter::once((t("datasources.databases_all"), every, Hit::AllDatabases))
                        .chain(all.iter().map(|d| {
                            (d.clone(), editor.shows_database(d), Hit::Database(d.clone()))
                        }))
                        .collect();
                self.paint_checks(pen, x, w, y, rows);
                *y += px(16.0);
            }
        }
        let Some(editor) = &self.editor else { return };
        let Some(entry) = editor.current() else { return };
        let all = editor.schemas();
        let every = entry.source.schemas.is_empty();
        *y += self.note(pen, x, *y, w, &t("datasources.schemas_note")) + px(12.0);
        if all.is_empty() {
            *y += self.note(pen, x, *y, w, &t("datasources.schemas_test"));
            return;
        }
        let rows: Vec<(String, bool, Hit)> =
            std::iter::once((t("datasources.schemas_all"), every, Hit::AllSchemas))
                .chain(
                    all.iter().map(|s| (s.clone(), editor.shows_schema(s), Hit::Schema(s.clone()))),
                )
                .collect();
        self.paint_checks(pen, x, w, y, rows);
    }

    /// A list of checkboxes, the first ("all") flush, the others under it.
    fn paint_checks(
        &mut self,
        pen: &mut Pen<'_, '_>,
        x: f32,
        w: f32,
        y: &mut f32,
        rows: Vec<(String, bool, Hit)>,
    ) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(-0.5);
        for (i, (label, on, hit)) in rows.into_iter().enumerate() {
            let h = px(32.0);
            let row = Rect::new(x - px(4.0), *y, w + px(8.0), h);
            let hovered = self.hover.as_ref() == Some(&hit);
            list_row(false, hovered).paint(pen.layer, ds, row);
            let mid = *y + h / 2.0;
            let indent = if i == 0 { 0.0 } else { px(26.0) };
            let state = if on { Check::On } else { Check::Off };
            let check = ds::Checkbox::new(state).hovered(hovered).paint(
                pen.layer,
                pen.text,
                ds,
                x + indent,
                mid,
            );
            let shaped = pen.text.shape_line(&label, &font);
            draw_text(
                pen.layer,
                &shaped,
                check.max_x() + px(8.0),
                ds::optical_baseline(mid - px(10.0), px(20.0), &font),
                ds.palette.text,
            );
            self.hits.push((row, hit));
            *y += h;
        }
    }

    pub(super) fn paint_advanced(&mut self, pen: &mut Pen<'_, '_>, x: f32, w: f32, y: &mut f32) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let Some(editor) = &self.editor else { return };
        *y += self.note(pen, x, *y, w, &t("datasources.advanced_note")) + px(12.0);
        let n = editor.options.len();
        let key_w = ((w - px(48.0)) * 0.4).floor();
        let value_w = w - key_w - px(48.0);
        if n > 0 {
            self.label(pen, x, *y, &t("datasources.option_name"), None, key_w);
            self.label(
                pen,
                x + key_w + px(10.0),
                *y,
                &t("datasources.option_value"),
                None,
                value_w,
            );
            *y += px(22.0);
        }
        for i in 0..n {
            let secret = self
                .editor
                .as_ref()
                .and_then(|e| e.options.get(i))
                .is_some_and(|(k, _)| database::source::is_secret_option(k.text().trim()));
            self.input(pen, Field::OptionKey(i), Rect::new(x, *y, key_w, px(FIELD_H)), "sslmode");
            self.input(
                pen,
                Field::OptionValue(i),
                Rect::new(x + key_w + px(10.0), *y, value_w, px(FIELD_H)),
                "",
            );
            let remove = ds::square_button(
                ds,
                (x + w - px(30.0), *y + px(FIELD_H) / 2.0),
                button_size::LARGE,
            );
            let spec = ButtonSpec::new(ButtonKind::Ghost)
                .icon("minus")
                .hovered(self.hover == Some(Hit::RemoveOption(i)));
            ds::button(pen.layer, pen.text, pen.icons, ds, remove, spec);
            self.hits.push((remove, Hit::RemoveOption(i)));
            self.tips.push((remove, t("datasources.option_remove")));
            *y += px(FIELD_H) + px(8.0);
            if secret {
                let font = ds.ui_font(-1.0);
                let shaped = pen.text.shape_line(&t("datasources.option_secret"), &font);
                draw_text(
                    pen.layer,
                    &shaped,
                    x,
                    ds::optical_baseline(*y - px(4.0), px(18.0), &font),
                    ds.palette.danger,
                );
                *y += px(18.0);
            }
        }
        let label = t("datasources.option_add");
        let bw = ds::primary_width(pen.text, ds, &[&label]);
        self.secondary(
            pen,
            Rect::new(x, *y + px(4.0), bw, px(ds::BUTTON_H)),
            &label,
            Hit::AddOption,
            true,
        );
        *y += px(ds::BUTTON_H) + px(12.0);
    }
}
