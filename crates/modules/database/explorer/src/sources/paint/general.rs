//! The selected source's header (name, comment, tabs) and its General tab: the fields
//! its driver asks for, the folder, the password.

use super::*;

impl SourcesView {
    fn header_tabs(&mut self, pen: &mut Pen<'_, '_>, x: f32, mid: f32) {
        let Some(tab) = self.editor.as_ref().map(|e| e.tab) else { return };
        let labels: Vec<String> = Tab::ALL.iter().map(|t_| t(t_.label_key())).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let w = ds::segmented_width(pen.text, pen.ds, &refs);
        let hovered = Tab::ALL.iter().position(|t_| self.hover == Some(Hit::Tab(*t_)));
        let selected = Tab::ALL.iter().position(|t_| *t_ == tab).unwrap_or(0);
        let rects = ds::segmented(
            pen.layer,
            pen.text,
            pen.ds,
            ds::SegmentedSpec { right: x + w, mid, labels: &refs, selected, hovered },
        );
        for (r, t_) in rects.into_iter().zip(Tab::ALL) {
            self.hits.push((r, Hit::Tab(t_)));
        }
    }

    /// Name and comment, then the tabs.
    pub(super) fn paint_header(&mut self, pen: &mut Pen<'_, '_>, r: Rect) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let half = ((r.size.width - px(16.0)) / 2.0).floor();
        let no_name = self
            .editor
            .as_ref()
            .and_then(|e| e.current())
            .is_some_and(|e| e.source.name.trim().is_empty());
        let error = no_name.then(|| t("datasources.error_required"));
        self.label(pen, r.min_x(), r.min_y(), &t("datasources.field_name"), error.as_deref(), half);
        self.input(
            pen,
            Field::Name,
            Rect::new(r.min_x(), r.min_y() + px(22.0), half, px(FIELD_H)),
            "",
        );
        let cx = r.min_x() + half + px(16.0);
        self.label(pen, cx, r.min_y(), &t("datasources.field_comment"), None, half);
        self.input(
            pen,
            Field::Comment,
            Rect::new(cx, r.min_y() + px(22.0), half, px(FIELD_H)),
            &t("datasources.comment_hint"),
        );
        let mid = r.min_y() + px(GROUP_H) + px(18.0);
        self.header_tabs(pen, r.min_x(), mid);
        // Its driver, at the tabs' end.
        let Some(editor) = &self.editor else { return };
        let Some(entry) = editor.current() else { return };
        let driver = editor
            .driver(&entry.source.driver)
            .map_or(entry.source.driver.clone(), |d| d.name.clone());
        let font = ds.ui_font(-1.0);
        let label = tr!("datasources.driver_is", driver = driver.as_str());
        let shaped = pen.text.shape_line(&label, &font);
        draw_text(
            pen.layer,
            &shaped,
            r.max_x() - shaped.width,
            ds::optical_baseline(mid - px(10.0), px(20.0), &font),
            ds.palette.text_muted,
        );
    }

    pub(super) fn paint_tab(&mut self, pen: &mut Pen<'_, '_>, x: f32, w: f32, y: &mut f32) {
        let Some(tab) = self.editor.as_ref().map(|e| e.tab) else { return };
        match tab {
            Tab::General => self.paint_general(pen, x, w, y),
            Tab::Options => self.paint_options(pen, x, w, y),
            Tab::Ssl => self.paint_ssl(pen, x, w, y),
            Tab::Schemas => self.paint_schemas(pen, x, w, y),
            Tab::Advanced => self.paint_advanced(pen, x, w, y),
        }
    }

    /// A driver's field at (`x`, `y`), `w` wide (its label over it).
    pub(super) fn spec_field(
        &mut self,
        pen: &mut Pen<'_, '_>,
        field: &FieldSpec,
        x: f32,
        y: f32,
        w: f32,
    ) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let Some(editor) = &self.editor else { return };
        let problem = editor.field_problem(&field.id).map(|e| field_error_text(&e));
        let r = Rect::new(x, y + px(22.0), w, px(FIELD_H));
        match &field.kind {
            FieldKind::Flag => {
                let on = editor.choice_of(field) == "true";
                let mid = y + px(22.0) + px(FIELD_H) / 2.0;
                let state = if on { Check::On } else { Check::Off };
                let check = ds::checkbox(pen.layer, pen.text, ds, x, mid, state, true);
                let font = ds.ui_font(-0.5);
                let shaped = pen.text.shape_line(&t(&field.label), &font);
                let lx = check.max_x() + px(8.0);
                draw_text(
                    pen.layer,
                    &shaped,
                    lx,
                    ds::optical_baseline(mid - px(10.0), px(20.0), &font),
                    ds.palette.text,
                );
                self.hits.push((
                    Rect::new(
                        x - px(4.0),
                        mid - px(12.0),
                        lx + shaped.width - x + px(8.0),
                        px(24.0),
                    ),
                    Hit::Flag(field.id.clone()),
                ));
            }
            FieldKind::Choice(values) => {
                self.label(pen, x, y, &t(&field.label), problem.as_deref(), w);
                let now = editor.choice_of(field);
                let shown =
                    values.iter().find(|(v, _)| *v == now).map_or(now.clone(), |(_, l)| t(l));
                self.dropdown(pen, Choice::Spec(field.id.clone()), r, &shown);
            }
            FieldKind::File { .. } => {
                self.label(pen, x, y, &t(&field.label), problem.as_deref(), w);
                let browse = t("datasources.browse");
                let bw = ds::primary_width(pen.text, ds, &[&browse]);
                let input =
                    Rect::new(r.min_x(), r.min_y(), r.size.width - bw - px(8.0), r.size.height);
                self.input(pen, Field::Spec(field.id.clone()), input, &field.default);
                let button = Rect::new(input.max_x() + px(8.0), r.min_y(), bw, r.size.height);
                self.secondary(pen, button, &browse, Hit::Browse(field.id.clone()), true);
            }
            FieldKind::Secret => {}
            _ => {
                self.label(pen, x, y, &t(&field.label), problem.as_deref(), w);
                self.input(pen, Field::Spec(field.id.clone()), r, &field.default);
            }
        }
    }

    fn paint_general(&mut self, pen: &mut Pen<'_, '_>, x: f32, w: f32, y: &mut f32) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let Some(editor) = &self.editor else { return };
        if editor.current().is_none() {
            return;
        }
        *y += px(4.0);
        let mut fields = editor.fields_in(FieldTab::General);
        // A user and the database side by side (the database moved up beside it).
        if let (Some(u), Some(d)) = (
            fields.iter().position(|f| f.id == "user"),
            fields.iter().position(|f| f.id == "database"),
        ) && d > u + 1
        {
            let database = fields.remove(d);
            fields.insert(u + 1, database);
        }
        let spec = editor.spec();
        let mut i = 0;
        while i < fields.len() {
            let field = &fields[i];
            if field.is_secret() {
                self.password_row(pen, x, *y, w);
                *y += px(GROUP_H);
                i += 1;
                continue;
            }
            // A host and its port side by side.
            if field.id == "host" && fields.get(i + 1).is_some_and(|f| f.id == "port") {
                let port_w = px(120.0);
                self.spec_field(pen, field, x, *y, w - port_w - px(12.0));
                self.spec_field(pen, &fields[i + 1].clone(), x + w - port_w, *y, port_w);
                *y += px(GROUP_H);
                i += 2;
                continue;
            }
            if field.id == "user" && fields.get(i + 1).is_some_and(|f| f.id == "database") {
                let half = ((w - px(12.0)) / 2.0).floor();
                self.spec_field(pen, field, x, *y, half);
                self.spec_field(
                    pen,
                    &fields[i + 1].clone(),
                    x + half + px(12.0),
                    *y,
                    w - half - px(12.0),
                );
                *y += px(GROUP_H);
                i += 2;
                continue;
            }
            let field = field.clone();
            self.spec_field(pen, &field, x, *y, w);
            *y += px(GROUP_H);
            i += 1;
        }
        if spec.url.is_some() {
            let error = self.editor.as_ref().and_then(|e| e.url_error.as_ref()).map(url_error_text);
            self.label(pen, x, *y, &t("datasources.field_url"), error.as_deref(), w);
            self.input(pen, Field::Url, Rect::new(x, *y + px(22.0), w, px(FIELD_H)), "");
            *y += px(GROUP_H);
        }
        // Its color (its folder's unless it has one).
        let (own, inherited) = {
            let Some(editor) = &self.editor else { return };
            let Some(at) = editor.selected else { return };
            let own = editor.entries[at].source.color;
            let inherited = editor.entries[at].source.folder.and_then(|f| editor.folder_color(f));
            (own, inherited)
        };
        self.label(pen, x, *y, &t("datasources.field_color"), None, w);
        let mut sx = x;
        let mid = *y + px(22.0) + px(14.0);
        let chip =
            self.inherit_chip(pen, sx, mid, "datasources.color_inherit", inherited, own.is_none());
        self.hits.push((chip, Hit::SourceColor(None)));
        sx = chip.max_x() + px(12.0);
        self.swatches(pen, sx, mid, own, false);
        *y += px(22.0 + 28.0 + 8.0);
        *y += self.note(pen, x, *y, w, &t("datasources.color_note")) + px(12.0);
    }

    /// The selected folder on the right: its name, its color, what it holds.
    pub(super) fn paint_folder(&mut self, pen: &mut Pen<'_, '_>, x: f32, y: f32, w: f32) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let Some(editor) = &self.editor else { return };
        let Some(folder) = editor.current_folder().cloned() else { return };
        let count = editor.count_in(folder.id);
        let inherited = folder.parent.and_then(|p| editor.folder_color(p));
        let no_name = folder.name.trim().is_empty();
        let error = no_name.then(|| t("datasources.error_required"));
        let half = ((w - px(16.0)) / 2.0).floor();
        self.label(pen, x, y, &t("datasources.folder_name"), error.as_deref(), half);
        self.input(pen, Field::FolderName, Rect::new(x, y + px(22.0), half, px(FIELD_H)), "");
        let font = ds.ui_font(-1.0);
        let detail = tr!("datasources.folder_count", count = count);
        let shaped = pen.text.shape_line(&detail, &font);
        draw_text(
            pen.layer,
            &shaped,
            x + half + px(16.0),
            ds::optical_baseline(y + px(22.0), px(FIELD_H), &font),
            ds.palette.text_muted,
        );
        let mut y = y + px(GROUP_H);
        self.label(pen, x, y, &t("datasources.field_color"), None, w);
        let mid = y + px(22.0) + px(14.0);
        let key = if inherited.is_some() {
            "datasources.color_inherit"
        } else {
            "datasources.color_none"
        };
        let chip = self.inherit_chip(pen, x, mid, key, inherited, folder.color.is_none());
        self.hits.push((chip, Hit::FolderColor(None)));
        self.swatches(pen, chip.max_x() + px(12.0), mid, folder.color, true);
        y += px(22.0 + 28.0 + 16.0);
        // Production: every source in it (and its subfolders) is too.
        let parent =
            folder.parent.filter(|p| self.editor.as_ref().is_some_and(|e| e.folder_production(*p)));
        let on = folder.production || parent.is_some();
        let lx = self.check_row(
            pen,
            (x, y),
            &t("datasources.folder_production"),
            on,
            parent.is_none(),
            Hit::FolderProduction,
        );
        y += px(30.0);
        let note = if parent.is_some() {
            t("datasources.folder_production_inherited")
        } else {
            t("datasources.folder_production_note")
        };
        y += self.note(pen, lx, y, w - (lx - x), &note) + px(16.0);
        y += self.note(pen, x, y, w, &t("datasources.folder_note")) + px(8.0);
        self.note(pen, x, y, w, &t("datasources.folder_drop"));
    }

    /// The user and the password's field (masked, its eye), then where it's kept.
    fn password_row(&mut self, pen: &mut Pen<'_, '_>, x: f32, y: f32, w: f32) {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let Some(editor) = &mut self.editor else { return };
        let revealed = editor.revealed;
        let Some(entry) = editor.current() else { return };
        let policy = crate::credentials::effective(&entry.source);
        let stored = entry.original.is_some() && !entry.password_typed;
        let keeps = matches!(
            policy,
            database::SecretPolicy::ProjectVault | database::SecretPolicy::Keychain
        );
        let placeholder =
            if stored && keeps { t("datasources.password_saved") } else { String::new() };
        let policy_w = px(230.0).min(w * 0.42);
        let pw = w - policy_w - px(12.0);
        self.label(pen, x, y, &t("datasources.field_password"), None, pw);
        self.label(pen, x + pw + px(12.0), y, &t("datasources.field_save"), None, policy_w);
        // Shown as typed only while its eye is on.
        if let Some(e) =
            self.editor.as_mut().and_then(|e| e.entries.get_mut(e.selected.unwrap_or(usize::MAX)))
        {
            e.password.set_masked(!revealed);
        }
        let r = Rect::new(x, y + px(22.0), pw, px(FIELD_H));
        self.input(pen, Field::Password, r, &placeholder);
        let eye =
            Rect::new(r.max_x() - px(32.0), r.min_y() + px(3.0), px(28.0), r.size.height - px(6.0));
        let spec = ButtonSpec::new(ButtonKind::Ghost)
            .icon(if revealed { "eye-off" } else { "eye" })
            .hovered(self.hover == Some(Hit::Reveal));
        ds::button(pen.layer, pen.text, pen.icons, pen.ds, eye, spec);
        self.hits.push((eye, Hit::Reveal));
        self.tips.push((
            eye,
            t(if revealed { "datasources.password_hide" } else { "datasources.password_show" }),
        ));
        let d = Rect::new(x + pw + px(12.0), y + px(22.0), policy_w, px(FIELD_H));
        self.dropdown(pen, Choice::Policy, d, &t(policy_key(policy)));
    }
}
