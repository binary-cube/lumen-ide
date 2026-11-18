//! "Configure CSV Formats…": the separated formats edited in a modal of the module's own
//! (`view-csv-formats`: closed only by ×, Esc or its buttons) — the formats listed on the
//! left (added, removed, restored to the defaults), the chosen one's name, separator (`,`
//! `;` `|` tab or any other), quote (`"`, `'` or none), header, the text `NULL` is written
//! as and the line ending on the right, with a preview of what it writes. OK saves them in
//! the settings ([`crate::settings::CSV_FORMATS`]) and hands them to the catalog; every grid's
//! "CSV ▾" popover and menus list them at once. [`FormatsEditor`] is the modal's model (pure).

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use command_model::CommandId;
use extractors::{Cell, Column, CsvFormat, Delimited, Input, Kind, LineEnd, Options, extract};
use i18n::t;
use kernel::{Point, Rect};
use ui_components::ds::{self, ChoiceSpec, Ctx};
use ui_components::{
    KeyReach, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind, ViewerPaint,
    ViewerProvider,
};
use ui_kit::{TextInput, TextInputStyle, draw_text};
use workbench_model::{CommandHandler, WorkbenchHost};

/// The modal's virtual extension (its view's kind).
pub const EXTENSION: &str = "view-csv-formats";

/// Run when the formats are given (internal).
pub const GIVEN: CommandId = CommandId("extractors.csv_formats_given");

/// The separators offered as pills (any other is typed).
pub const SEPARATORS: [&str; 4] = [",", ";", "|", "\t"];

/// The formats being edited, and the one shown (see the module's doc).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatsEditor {
    pub formats: Vec<CsvFormat>,
    pub selected: usize,
}

impl FormatsEditor {
    pub fn new(formats: Vec<CsvFormat>) -> Self {
        let formats = if formats.is_empty() { CsvFormat::defaults() } else { formats };
        Self { formats, selected: 0 }
    }

    pub fn current(&self) -> &CsvFormat {
        &self.formats[self.selected.min(self.formats.len() - 1)]
    }

    fn current_mut(&mut self) -> &mut CsvFormat {
        let at = self.selected.min(self.formats.len() - 1);
        &mut self.formats[at]
    }

    pub fn select(&mut self, at: usize) {
        if at < self.formats.len() {
            self.selected = at;
        }
    }

    /// A new format like the one shown (a new identity, a name of its own), shown.
    pub fn add(&mut self) {
        let mut format = self.current().clone();
        format.id = format!("csv-{:032x}", kernel::id::new_uuid());
        let base = t("extract.new_format");
        let mut n = 1;
        format.name = loop {
            let name = if n == 1 { base.clone() } else { format!("{base} {n}") };
            if !self.formats.iter().any(|f| f.name == name) {
                break name;
            }
            n += 1;
        };
        self.formats.push(format);
        self.selected = self.formats.len() - 1;
    }

    /// The format shown removed (never the last one).
    pub fn remove(&mut self) -> bool {
        if self.formats.len() <= 1 {
            return false;
        }
        self.formats.remove(self.selected.min(self.formats.len() - 1));
        self.selected = self.selected.min(self.formats.len() - 1);
        true
    }

    /// Back to the defaults.
    pub fn reset(&mut self) {
        self.formats = CsvFormat::defaults();
        self.selected = 0;
    }

    pub fn set_name(&mut self, name: &str) {
        self.current_mut().name = name.to_owned();
    }

    pub fn set_separator(&mut self, separator: &str) {
        self.current_mut().separator = separator.to_owned();
    }

    pub fn set_quote(&mut self, quote: Option<char>) {
        self.current_mut().quote = quote;
    }

    pub fn set_header(&mut self, header: bool) {
        self.current_mut().header = header;
    }

    pub fn set_null(&mut self, text: &str) {
        self.current_mut().null_text = text.to_owned();
    }

    pub fn set_line_end(&mut self, end: LineEnd) {
        self.current_mut().line_end = end;
    }

    /// Why the formats can't be saved (the first one that can't be used, and why), if not.
    pub fn problem(&self) -> Option<String> {
        for format in &self.formats {
            let name = if format.name.trim().is_empty() { "…" } else { format.name.as_str() };
            if format.name.trim().is_empty() {
                return Some(t("extract.format_no_name"));
            }
            if format.separator.is_empty() || format.separator.contains(['\n', '\r']) {
                return Some(i18n::tr!("extract.format_no_separator", name = name));
            }
            if !format.is_valid() {
                return Some(i18n::tr!("extract.format_quote_in_separator", name = name));
            }
            if self.formats.iter().filter(|f| f.name.trim() == format.name.trim()).count() > 1 {
                return Some(i18n::tr!("extract.format_same_name", name = name));
            }
        }
        None
    }

    /// What the format shown writes for a few sample rows.
    pub fn preview(&self) -> String {
        let columns = vec![
            Column::new("id", Kind::Integer),
            Column::new("name", Kind::Text),
            Column::new("note", Kind::Text),
        ];
        let rows = vec![
            vec![Cell::Int(1), Cell::Text("Ana"), Cell::Text("a, \"b\"")],
            vec![Cell::Int(2), Cell::Text("Ion"), Cell::Null],
        ];
        let format = self.current().clone();
        if !format.is_valid() && !format.name.trim().is_empty() {
            return String::new();
        }
        extract(&Delimited(format), Input::new("t", columns), Options::default(), &rows)
            .replace('\t', "→")
            .replace("\r\n", "␍\n")
    }
}

/// What the modal is opened with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatsRequest {
    /// A question's id: the formats are taken only for the question they were given to.
    pub id: u64,
    pub formats: Vec<CsvFormat>,
}

static NEXT: AtomicU64 = AtomicU64::new(1);
/// The formats given, waiting for [`GIVEN`].
static GIVEN_FORMATS: Mutex<Option<Vec<CsvFormat>>> = Mutex::new(None);

/// The modal opened on the formats offered now.
pub fn ask(host: &mut dyn WorkbenchHost) -> bool {
    let request = FormatsRequest {
        id: NEXT.fetch_add(1, Ordering::Relaxed),
        formats: extractors::catalog().csv_formats(),
    };
    host.open_modal(&t("extract.configure_title"), EXTENSION, &request)
}

/// [`GIVEN`]'s handler: the formats saved and offered.
#[derive(Debug, Default)]
pub struct GivenCommand;

impl CommandHandler for GivenCommand {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(formats) = GIVEN_FORMATS.lock().unwrap_or_else(PoisonError::into_inner).take()
        else {
            return false;
        };
        give(host, formats)
    }
}

/// `formats` offered from now on, saved in the settings.
pub fn give(host: &mut dyn WorkbenchHost, formats: Vec<CsvFormat>) -> bool {
    if let Some(settings) = host.user_settings() {
        crate::settings::save_formats(settings, &formats);
    }
    let defaults = formats == CsvFormat::defaults();
    extractors::catalog().set_csv_formats((!defaults).then_some(formats));
    true
}

/// The modal's provider (its virtual extension).
#[derive(Debug, Default)]
pub struct FormatsProvider;

impl ViewerProvider for FormatsProvider {
    fn id(&self) -> &'static str {
        EXTENSION
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        (path.extension().and_then(|e| e.to_str()) == Some(EXTENSION))
            .then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(FormatsView::default()))
    }
}

/// The text field having the keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    Separator,
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Format(usize),
    Add,
    Remove,
    Field(Field),
    Separator(usize),
    Quote(u8),
    Header,
    LineEnd(bool),
    Reset,
    Ok,
    Cancel,
}

/// The modal's view.
#[derive(Debug, Default)]
pub struct FormatsView {
    editor: Option<FormatsEditor>,
    name: TextInput,
    separator: TextInput,
    null: TextInput,
    field: Option<Field>,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    action: Option<ViewerAction>,
    done: bool,
}

/// The modal's content (logical pixels).
const HEIGHT: f32 = 470.0;

impl FormatsView {
    pub fn editor(&self) -> Option<&FormatsEditor> {
        self.editor.as_ref()
    }

    /// The fields show the format chosen.
    fn load_fields(&mut self) {
        let Some(editor) = &self.editor else { return };
        let format = editor.current();
        self.name = TextInput::new(&format.name);
        let custom = if SEPARATORS.contains(&format.separator.as_str()) {
            String::new()
        } else {
            format.separator.clone()
        };
        self.separator = TextInput::new(&custom);
        self.null = TextInput::new(&format.null_text);
    }

    /// What a field holds, written into the format shown.
    fn store_field(&mut self, field: Field) {
        let Some(editor) = &mut self.editor else { return };
        match field {
            Field::Name => editor.set_name(self.name.text()),
            Field::Separator => {
                if !self.separator.text().is_empty() {
                    editor.set_separator(self.separator.text());
                }
            }
            Field::Null => editor.set_null(self.null.text()),
        }
    }

    fn input(&mut self, field: Field) -> &mut TextInput {
        match field {
            Field::Name => &mut self.name,
            Field::Separator => &mut self.separator,
            Field::Null => &mut self.null,
        }
    }

    /// The formats given (when they can be): the window closes, they're saved.
    pub fn give(&mut self) -> bool {
        if let Some(field) = self.field {
            self.store_field(field);
        }
        let Some(editor) = &self.editor else { return false };
        if editor.problem().is_some() {
            return false;
        }
        *GIVEN_FORMATS.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(editor.formats.clone());
        self.done = true;
        self.action = Some(ViewerAction::RunCommand(GIVEN.0.to_owned()));
        true
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// A press on `hit`.
    fn press(&mut self, hit: Hit, p: Point) {
        if let Some(field) = self.field
            && hit != Hit::Field(field)
        {
            self.store_field(field);
            self.field = None;
        }
        let Some(editor) = &mut self.editor else { return };
        match hit {
            Hit::Format(at) => {
                editor.select(at);
                self.load_fields();
            }
            Hit::Add => {
                editor.add();
                self.load_fields();
                self.field = Some(Field::Name);
                self.name.select_all();
            }
            Hit::Remove => {
                editor.remove();
                self.load_fields();
            }
            Hit::Field(field) => {
                self.field = Some(field);
                self.input(field).click(p.x);
            }
            Hit::Separator(at) => {
                editor.set_separator(SEPARATORS[at]);
                self.separator.set_text("");
            }
            Hit::Quote(0) => editor.set_quote(Some('"')),
            Hit::Quote(1) => editor.set_quote(Some('\'')),
            Hit::Quote(_) => editor.set_quote(None),
            Hit::Header => {
                let on = editor.current().header;
                editor.set_header(!on);
            }
            Hit::LineEnd(crlf) => {
                editor.set_line_end(if crlf { LineEnd::CrLf } else { LineEnd::Lf })
            }
            Hit::Reset => {
                editor.reset();
                self.load_fields();
            }
            Hit::Ok => {
                self.give();
            }
            Hit::Cancel => self.action = Some(ViewerAction::Close),
        }
    }
}

impl Viewer for FormatsView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(request) = data.downcast_ref::<FormatsRequest>() {
            self.editor = Some(FormatsEditor::new(request.formats.clone()));
            self.load_fields();
        }
    }

    fn subtitle(&self) -> Option<String> {
        Some(t("extract.configure_subtitle"))
    }

    fn modal_height(&self) -> Option<f32> {
        Some(HEIGHT)
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some("table")
    }

    fn key_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        vec![(vec!["↵"], t("extract.configure_ok"))]
    }

    fn closed(&self) -> bool {
        self.done
    }

    fn wants_text(&self) -> bool {
        self.field.is_some()
    }

    fn text_input(&mut self, typed: &str) -> bool {
        let Some(field) = self.field else { return false };
        // A tab can be a separator; nothing else that's a control.
        let typed: String = typed
            .chars()
            .filter(|c| !c.is_control() || (*c == '\t' && field == Field::Separator))
            .collect();
        self.input(field).insert(&typed);
        self.store_field(field);
        true
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, .. } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.hits.clear();
        let Some(editor) = self.editor.clone() else { return };
        let format = editor.current().clone();
        let layer = scene.push_layer(Some(b));
        let pad = px(20.0);
        let font = ds.ui_font(-0.5);
        let small = ds.ui_font(-1.5);
        // The list, on the left.
        let list_w = px(190.0);
        let list =
            Rect::new(b.min_x() + pad, b.min_y() + px(8.0), list_w, b.size.height - px(120.0));
        layer.quad(
            render_api::Quad::filled(list, p.text.with_alpha(0.025))
                .rounded(px(ds::radius::MD))
                .bordered(ds.line(), p.border),
        );
        let row_h = px(30.0);
        for (i, f) in editor.formats.iter().enumerate() {
            let r = Rect::new(
                list.min_x() + px(4.0),
                list.min_y() + px(4.0) + i as f32 * row_h,
                list_w - px(8.0),
                row_h,
            );
            if r.max_y() > list.max_y() {
                break;
            }
            let on = i == editor.selected;
            if on {
                layer.quad(
                    render_api::Quad::filled(r, p.accent.with_alpha(0.14))
                        .rounded(px(ds::radius::SM)),
                );
            } else if self.hover == Some(Hit::Format(i)) {
                layer.quad(
                    render_api::Quad::filled(r, p.hover_background).rounded(px(ds::radius::SM)),
                );
            }
            let label = ds::ellipsize(text, &f.name, &font, r.size.width - px(20.0));
            let shaped = text.shape_line(&label, &font);
            draw_text(
                layer,
                &shaped,
                r.min_x() + px(10.0),
                ds::optical_baseline(r.min_y(), row_h, &font),
                if on { p.accent } else { p.text },
            );
            self.hits.push((r, Hit::Format(i)));
        }
        // Add / remove under the list.
        let bh = px(ds::BUTTON_H);
        let below = list.max_y() + px(10.0);
        let add = t("extract.format_add");
        let remove = t("extract.format_remove");
        let aw = ds::primary_width(text, &ds, &[&add]);
        let a = Rect::new(list.min_x(), below, aw, bh);
        ds::secondary_button(layer, text, &ds, a, &add, true, self.hover == Some(Hit::Add));
        self.hits.push((a, Hit::Add));
        let rw = ds::primary_width(text, &ds, &[&remove]);
        let r = Rect::new(a.max_x() + px(8.0), below, rw, bh);
        let can_remove = editor.formats.len() > 1;
        ds::secondary_button(
            layer,
            text,
            &ds,
            r,
            &remove,
            can_remove,
            self.hover == Some(Hit::Remove),
        );
        if can_remove {
            self.hits.push((r, Hit::Remove));
        }

        // The format, on the right.
        let x0 = list.max_x() + px(24.0);
        let label_w = px(110.0);
        let fx = x0 + label_w;
        let fw = (b.max_x() - pad - fx).max(px(120.0));
        let style = |error: bool| TextInputStyle {
            font: ds.ui_font(-0.5),
            background: p.input_background,
            border: if error { p.danger } else { p.input_border },
            border_focused: if error { p.danger } else { p.accent },
            text: p.text,
            placeholder: p.text_muted,
            selection: p.selection,
            caret: p.caret,
            radius: px(ds::radius::MD),
            border_width: ds.line(),
            padding: px(10.0),
            caret_width: px(1.5).max(1.0),
            lead: 0.0,
        };
        let line_h = px(44.0);
        let mut y = b.min_y() + px(8.0);
        let label = |layer: &mut render_api::Layer,
                     text: &mut text_layout::TextSystem,
                     key: &str,
                     y: f32| {
            let shaped = text.shape_line(&t(key), &font);
            draw_text(layer, &shaped, x0, ds::optical_baseline(y, line_h, &font), p.text_muted);
        };
        // Name.
        label(layer, text, "extract.format_name", y);
        let field = Rect::new(fx, y + px(4.0), fw, px(36.0));
        let focused = self.field == Some(Field::Name);
        self.name.paint(
            layer,
            text,
            field,
            focused,
            &t("extract.format_name"),
            &style(format.name.trim().is_empty()),
        );
        self.hits.push((field, Hit::Field(Field::Name)));
        y += line_h;
        // Separator: the usual ones, or any other typed.
        label(layer, text, "extract.format_separator", y);
        let mid = y + line_h / 2.0;
        let mut x = fx;
        for (i, sep) in SEPARATORS.iter().enumerate() {
            let shown = if *sep == "\t" { t("extract.separator_tab") } else { (*sep).to_owned() };
            let r = ds::choice(
                layer,
                text,
                &ds,
                ChoiceSpec {
                    x,
                    mid,
                    label: &shown,
                    selected: format.separator == *sep,
                    hovered: self.hover == Some(Hit::Separator(i)),
                },
            );
            self.hits.push((r, Hit::Separator(i)));
            x = r.max_x() + px(6.0);
        }
        let other =
            Rect::new(x + px(4.0), y + px(4.0), (fx + fw - x - px(4.0)).max(px(60.0)), px(36.0));
        let custom = !SEPARATORS.contains(&format.separator.as_str());
        self.separator.paint(
            layer,
            text,
            other,
            self.field == Some(Field::Separator),
            &t("extract.separator_other"),
            &style(custom && format.separator.is_empty()),
        );
        self.hits.push((other, Hit::Field(Field::Separator)));
        y += line_h;
        // Quote.
        label(layer, text, "extract.format_quote", y);
        let mid = y + line_h / 2.0;
        let mut x = fx;
        let none = t("extract.quote_none");
        for (i, (shown, quote)) in
            [("\"", Some('"')), ("'", Some('\'')), (none.as_str(), None)].into_iter().enumerate()
        {
            let r = ds::choice(
                layer,
                text,
                &ds,
                ChoiceSpec {
                    x,
                    mid,
                    label: shown,
                    selected: format.quote == quote,
                    hovered: self.hover == Some(Hit::Quote(i as u8)),
                },
            );
            self.hits.push((r, Hit::Quote(i as u8)));
            x = r.max_x() + px(6.0);
        }
        y += line_h;
        // The header.
        let mid = y + line_h / 2.0;
        let check = ds::checkbox(
            layer,
            text,
            &ds,
            fx,
            mid,
            if format.header { ds::Check::On } else { ds::Check::Off },
            true,
        );
        let shaped = text.shape_line(&t("extract.format_header"), &font);
        draw_text(
            layer,
            &shaped,
            check.max_x() + px(10.0),
            ds::optical_baseline(y, line_h, &font),
            p.text,
        );
        self.hits.push((
            Rect::new(fx, y, check.size.width + px(10.0) + shaped.width, line_h),
            Hit::Header,
        ));
        y += line_h;
        // NULL's text.
        label(layer, text, "extract.format_null", y);
        let field = Rect::new(fx, y + px(4.0), fw, px(36.0));
        self.null.paint(
            layer,
            text,
            field,
            self.field == Some(Field::Null),
            &t("extract.null_empty"),
            &style(false),
        );
        self.hits.push((field, Hit::Field(Field::Null)));
        y += line_h;
        // The line ending.
        label(layer, text, "extract.format_line_end", y);
        let mid = y + line_h / 2.0;
        let mut x = fx;
        for (crlf, shown) in [(false, "LF  \\n"), (true, "CRLF  \\r\\n")] {
            let r = ds::choice(
                layer,
                text,
                &ds,
                ChoiceSpec {
                    x,
                    mid,
                    label: shown,
                    selected: (format.line_end == LineEnd::CrLf) == crlf,
                    hovered: self.hover == Some(Hit::LineEnd(crlf)),
                },
            );
            self.hits.push((r, Hit::LineEnd(crlf)));
            x = r.max_x() + px(6.0);
        }
        y += line_h + px(4.0);
        // The preview.
        let code = ds.code_font(-1.5);
        let preview = Rect::new(x0, y, b.max_x() - pad - x0, (list.max_y() - y).max(px(60.0)));
        layer.quad(
            render_api::Quad::filled(preview, p.text.with_alpha(0.035))
                .rounded(px(ds::radius::MD))
                .bordered(ds.line(), p.border),
        );
        let lh = px(18.0);
        for (i, line) in editor.preview().lines().take(4).enumerate() {
            let shown = ds::ellipsize(text, line, &code, preview.size.width - px(20.0));
            let shaped = text.shape_line(&shown, &code);
            draw_text(
                layer,
                &shaped,
                preview.min_x() + px(10.0),
                ds::optical_baseline(preview.min_y() + px(6.0) + i as f32 * lh, lh, &code),
                p.text,
            );
        }

        // What can't be saved, and the buttons.
        let by = b.max_y() - px(56.0) + (px(56.0) - bh) / 2.0;
        let problem = editor.problem();
        if let Some(why) = &problem {
            let shown = ds::ellipsize(text, why, &small, b.size.width - 2.0 * pad);
            let shaped = text.shape_line(&shown, &small);
            draw_text(
                layer,
                &shaped,
                b.min_x() + pad,
                ds::optical_baseline(by - px(30.0), px(20.0), &small),
                p.danger,
            );
        }
        let ok = t("extract.configure_ok");
        let w = ds::primary_width(text, &ds, &[&ok]);
        let g = Rect::new(b.max_x() - pad - w, by, w, bh);
        ds::primary_button(
            layer,
            text,
            &ds,
            g,
            &ok,
            problem.is_none(),
            self.hover == Some(Hit::Ok),
        );
        self.hits.push((g, Hit::Ok));
        let cancel = t("extract.configure_cancel");
        let cw = ds::primary_width(text, &ds, &[&cancel]);
        let c = Rect::new(g.min_x() - px(10.0) - cw, by, cw, bh);
        ds::secondary_button(layer, text, &ds, c, &cancel, true, self.hover == Some(Hit::Cancel));
        self.hits.push((c, Hit::Cancel));
        let reset = t("extract.configure_reset");
        let rw = ds::primary_width(text, &ds, &[&reset]);
        let r = Rect::new(b.min_x() + pad, by, rw, bh);
        ds::secondary_button(layer, text, &ds, r, &reset, true, self.hover == Some(Hit::Reset));
        self.hits.push((r, Hit::Reset));
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        match self.hit(p) {
            Some(hit) => self.press(hit, p),
            None => {
                if let Some(field) = self.field.take() {
                    self.store_field(field);
                }
            }
        }
        self.action.take()
    }

    fn drag(&mut self, p: Point) -> bool {
        let Some(field) = self.field else { return false };
        self.input(field).extend_to(p.x);
        true
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        match self.hit(p)? {
            Hit::Field(_) => Some(ViewerCursor::Text),
            _ => Some(ViewerCursor::Pointer),
        }
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        match (key, self.field) {
            (ViewerKey::Enter, _) => {
                self.give();
            }
            (ViewerKey::Escape, Some(field)) => {
                // The field lets go of the keys first; a second Esc closes.
                self.store_field(field);
                self.field = None;
            }
            (ViewerKey::Escape, None) => self.action = Some(ViewerAction::Close),
            (ViewerKey::Up, None) => {
                if let Some(editor) = &mut self.editor {
                    editor.select(editor.selected.saturating_sub(1));
                    self.load_fields();
                }
            }
            (ViewerKey::Down, None) => {
                if let Some(editor) = &mut self.editor {
                    editor.select(editor.selected + 1);
                    self.load_fields();
                }
            }
            (_, None) => return false,
            (key, Some(field)) => {
                let input = self.input(field);
                match key {
                    ViewerKey::Left => input.move_left(reach.word, reach.extend),
                    ViewerKey::Right => input.move_right(reach.word, reach.extend),
                    ViewerKey::Home => input.home(reach.extend),
                    ViewerKey::End => input.end(reach.extend),
                    ViewerKey::Backspace => input.backspace(reach.word),
                    ViewerKey::Delete => input.delete(),
                    _ => return false,
                }
                self.store_field(field);
            }
        }
        true
    }

    fn selected_text(&self) -> Option<String> {
        let field = self.field?;
        let input = match field {
            Field::Name => &self.name,
            Field::Separator => &self.separator,
            Field::Null => &self.null,
        };
        let selected = input.selected_text();
        (!selected.is_empty()).then(|| selected.to_owned())
    }

    fn command(&mut self, id: &str) -> bool {
        match id {
            "go" => self.give(),
            "select_all" => self.field.is_some_and(|f| {
                self.input(f).select_all();
                true
            }),
            "add" => {
                self.press(Hit::Add, Point::default());
                true
            }
            id if id.starts_with("select:") => {
                let at = id["select:".len()..].parse().unwrap_or(0);
                self.press(Hit::Format(at), Point::default());
                true
            }
            _ => false,
        }
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

#[cfg(test)]
#[path = "../tests/unit/csv_formats.rs"]
mod tests;
