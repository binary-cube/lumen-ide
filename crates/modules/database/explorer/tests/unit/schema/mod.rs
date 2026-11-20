//! A CSV file's changes in words.

use super::*;
use database::{ColumnDef, DefaultValue, TableRef};

#[test]
fn a_file_s_changes_are_summed_up_one_line_each() {
    let t = TableRef::new("people");
    let lines = summary(&[
        SchemaChange::AddColumn { table: t.clone(), column: ColumnDef::new("city", "") },
        SchemaChange::AddColumn {
            table: t.clone(),
            column: ColumnDef::new("team", "")
                .default(DefaultValue::Value(Value::Text("red".into()))),
        },
        SchemaChange::RenameColumn { table: t.clone(), from: "a".into(), to: "b".into() },
        SchemaChange::DropColumn { table: t.clone(), column: "c".into() },
        SchemaChange::MoveColumn { table: t, column: "d".into(), to: 0 },
    ]);
    assert_eq!(lines.len(), 5);
    assert!(lines[1].contains("red"));
    assert!(lines.iter().all(|l| !l.starts_with("schema.")), "{lines:?}");
}

#[test]
fn the_column_marks_and_the_icon_buttons_say_what_they_are() {
    use std::sync::{Arc, Mutex};
    use ui_components::Viewer as _;

    let manual = crate::fake::Manual::new();
    let sessions = crate::sessions::Sessions::new(
        kernel::ProjectId::new(),
        std::path::Path::new("/projects/shop"),
        kernel::EventBus::new(),
        manual.runtime(),
        None,
    );
    let source = database::DataSource::new("shop", "fake", database::Scope::Project);
    let driver = crate::fake::FakeDriver::new(crate::fake::FakeDb::users(1), true);
    let worker = sessions.open(&source, Arc::new(driver), database::Secret::default());
    let ctx = Context {
        table: None,
        schema: None,
        structure: None,
        tables: Vec::new(),
        rows: Vec::new(),
        can: database::SchemaChanges {
            tables: true,
            columns: true,
            alter_columns: true,
            indexes: true,
            renames: true,
            foreign_keys: true,
            ..database::SchemaChanges::default()
        },
        dialect: None,
    };
    let request = SchemaRequest {
        kind: Kind::CreateTable,
        ctx,
        worker,
        answer: Arc::new(Mutex::new(None)),
        wake: Arc::new(|| {}),
    };
    let mut view = SchemaView::open(request, None);
    let form = view.form_mut().unwrap();
    form.columns[0].name.set_text("id");
    form.keys.push(form::KeyRow::default());
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    view.paint(ui_components::ViewerPaint {
        bounds: kernel::Rect::new(0.0, 0.0, 1100.0, 900.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    let center = |r: &kernel::Rect| {
        kernel::Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
    };
    // The ×s, and each of a column's boxes: a tooltip each.
    let mut checked = 0;
    for (r, hit) in &view.hits {
        let tick = matches!(
            hit,
            modal::Hit::Tick(Tick::NotNull(_) | Tick::Primary(_) | Tick::Auto(_) | Tick::Unique(_))
        );
        if tick || matches!(hit, modal::Hit::RemoveColumn(_) | modal::Hit::RemoveKey(_)) {
            let tip = view.tooltip_at(center(r)).unwrap_or_else(|| panic!("none on {hit:?}"));
            assert!(!tip.label.is_empty());
            checked += 1;
        }
    }
    assert!(checked >= 6, "{checked}");
    // The headers spelled out.
    let spelled: Vec<String> = view.tips.iter().map(|(_, l)| l.clone()).collect();
    for key in ["schema.tip_not_null", "schema.tip_primary", "schema.tip_auto", "schema.tip_unique"]
    {
        let label = i18n::t(key);
        assert!(!label.starts_with("schema."), "{key} translated");
        assert!(spelled.iter().filter(|l| **l == label).count() >= 2, "{key}: header and box");
    }
}

#[test]
fn a_statements_dangers_are_said_in_words() {
    use database::danger::{Danger, DropKind};
    use database::sql::WriteKind;
    let users = TableRef::in_schema("public", "users");
    let dangers = [
        Danger::UnfilteredWrite { kind: WriteKind::Update, table: users.clone() },
        Danger::UnfilteredWrite { kind: WriteKind::Delete, table: users.clone() },
        Danger::Truncate { table: users.clone() },
        Danger::AlterDrop { table: users, column: "age".into() },
        Danger::DropObject { kind: DropKind::Table, name: "public.users".into(), cascade: false },
        Danger::DropObject { kind: DropKind::Schema, name: "public.users".into(), cascade: true },
    ];
    let texts: Vec<String> = dangers.iter().map(super::paint::danger_text).collect();
    for (danger, text) in dangers.iter().zip(&texts) {
        assert!(!text.contains("schema.") && !text.contains('{'), "{danger:?}: {text}");
        assert!(text.contains("public.users"), "{danger:?} names its table: {text}");
    }
    assert!(texts[3].contains("age"));
    assert_ne!(texts[0], texts[1], "an update isn't a delete");
    assert_ne!(texts[4], texts[5], "CASCADE is said");
    for kind in [
        DropKind::View,
        DropKind::Index,
        DropKind::Database,
        DropKind::Function,
        DropKind::Procedure,
        DropKind::Trigger,
        DropKind::Sequence,
        DropKind::Type,
        DropKind::Other,
    ] {
        let text = super::paint::danger_text(&Danger::DropObject {
            kind,
            name: "x".into(),
            cascade: false,
        });
        assert!(!text.contains("schema."), "{kind:?}: {text}");
    }
}

/// A new table's modal over a fake source (its form, its preview not planned yet).
fn create_table_modal() -> SchemaView {
    use std::sync::{Arc, Mutex};
    let manual = crate::fake::Manual::new();
    let sessions = crate::sessions::Sessions::new(
        kernel::ProjectId::new(),
        std::path::Path::new("/projects/shop"),
        kernel::EventBus::new(),
        manual.runtime(),
        None,
    );
    let source = database::DataSource::new("shop", "fake", database::Scope::Project);
    let driver = crate::fake::FakeDriver::new(crate::fake::FakeDb::users(1), true);
    let worker = sessions.open(&source, Arc::new(driver), database::Secret::default());
    let ctx = Context {
        table: None,
        schema: None,
        structure: None,
        tables: Vec::new(),
        rows: Vec::new(),
        can: database::SchemaChanges {
            tables: true,
            columns: true,
            ..database::SchemaChanges::default()
        },
        dialect: None,
    };
    let request = SchemaRequest {
        kind: Kind::CreateTable,
        ctx,
        worker,
        answer: Arc::new(Mutex::new(None)),
        wake: Arc::new(|| {}),
    };
    SchemaView::open(request, None)
}

fn paint_schema(view: &mut SchemaView, height: f32) {
    use ui_components::Viewer as _;
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    view.paint(ui_components::ViewerPaint {
        bounds: kernel::Rect::new(0.0, 0.0, 1100.0, height),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
}

/// The topmost point of `bar` in `area`.
fn bar_in(area: kernel::Rect, bar: &ui_components::EdgeScrollbar) -> Option<kernel::Point> {
    (area.min_y() as u32..area.max_y() as u32)
        .flat_map(|y| (area.min_x() as u32..area.max_x() as u32 + 1).map(move |x| (x, y)))
        .map(|(x, y)| kernel::Point::new(x as f32, y as f32))
        .find(|p| bar.contains(*p))
}

/// The form and the preview scroll by their thumbs: pressed and dragged down, each moves
/// (no field takes the press, the other one still).
#[test]
fn the_form_s_and_the_preview_s_thumbs_are_dragged() {
    use ui_components::Viewer as _;
    let mut view = create_table_modal();
    let form = view.form_mut().unwrap();
    form.table_name.set_text("wide");
    for i in 0..30 {
        let mut row = form::ColumnRow::default();
        row.name.set_text(&format!("c{i}"));
        form.columns.push(row);
    }
    // Its plan: more statements than the preview shows.
    let changes = view.changes().unwrap();
    let script = database::transaction::Script {
        before: (0..60).map(|i| format!("SELECT {i}")).collect(),
        begin: String::new(),
        steps: Vec::new(),
        commit: String::new(),
        rollback: String::new(),
        after: Vec::new(),
        stale: Vec::new(),
    };
    let plan = database::transaction::AlterPreview { script, rewritten: Vec::new() };
    view.planned = Some((changes, Ok(Some(plan))));
    paint_schema(&mut view, 700.0);
    let focus = view.focus;
    // The form's.
    let on = bar_in(view.form_area, &view.form_bar).expect("the form's bar");
    let at = kernel::Point::new(on.x + 1.0, on.y + 2.0);
    assert_eq!(view.click(at), None);
    assert!(view.drag(kernel::Point::new(at.x, at.y + 30.0)));
    assert!(view.form_scroll > 0.0, "the form moved");
    assert!(view.release());
    assert_eq!(view.focus, focus, "no field took it");
    assert_eq!(view.preview_scroll, 0.0);
    // The preview's.
    let on = bar_in(view.preview_area, &view.preview_bar).expect("the preview's bar");
    let at = kernel::Point::new(on.x + 1.0, on.y + 2.0);
    let form_scroll = view.form_scroll;
    assert_eq!(view.click(at), None);
    assert!(view.drag(kernel::Point::new(at.x, at.y + 30.0)));
    assert!(view.preview_scroll > 0.0, "the preview moved");
    assert!(view.release());
    assert_eq!(view.form_scroll, form_scroll);
}

/// The wheel goes to the part under the mouse, through the modal's controls: the preview
/// over it, the form anywhere else.
#[test]
fn the_wheel_goes_to_the_preview_or_the_form() {
    use ui_components::Viewer as _;
    let mut view = create_table_modal();
    let form = view.form_mut().unwrap();
    form.table_name.set_text("wide");
    for i in 0..30 {
        let mut row = form::ColumnRow::default();
        row.name.set_text(&format!("c{i}"));
        form.columns.push(row);
    }
    let changes = view.changes().unwrap();
    let script = database::transaction::Script {
        before: (0..60).map(|i| format!("SELECT {i}")).collect(),
        begin: String::new(),
        steps: Vec::new(),
        commit: String::new(),
        rollback: String::new(),
        after: Vec::new(),
        stale: Vec::new(),
    };
    let plan = database::transaction::AlterPreview { script, rewritten: Vec::new() };
    view.planned = Some((changes, Ok(Some(plan))));
    paint_schema(&mut view, 700.0);
    let preview = view.preview_area;
    view.hover(kernel::Point::new(preview.min_x() + 20.0, preview.min_y() + 10.0));
    assert!(view.scroll(0.0, 30.0));
    assert!(view.preview_scroll > 0.0 && view.form_scroll == 0.0, "the preview moved");
    let form = view.form_area;
    view.hover(kernel::Point::new(form.min_x() + 20.0, form.min_y() + 10.0));
    assert!(view.scroll(0.0, 30.0));
    assert!(view.form_scroll > 0.0, "the form moved");
}

/// A field's suggestions take the keys over it, through the modal's controls: ↓ lights
/// one, the text and the other keys go on to the field (it keeps the typing), esc closes
/// them (the field's text stays); ↵ takes the lit one.
#[test]
fn a_field_s_suggestions_take_their_keys_and_the_field_keeps_the_typing() {
    use ui_components::{Viewer as _, ViewerKey};
    let mut view = create_table_modal();
    let dialect = database::console::dialect_for(database::sql::Flavor::Sqlite);
    view.form_mut().unwrap().ctx.dialect = Some(dialect);
    paint_schema(&mut view, 700.0);
    view.focus = Some(Text::Type(0));
    let typed = |view: &SchemaView| view.form().unwrap().columns[0].ty.text().to_owned();
    assert!(view.text_input("IN"));
    assert!(view.suggesting(), "typed: its suggestions shown");
    assert!(view.edit_key(ViewerKey::Down), "↓ lights a suggestion");
    assert!(view.picker.highlighted().is_some());
    assert!(view.text_input("T"), "the text goes on to the field");
    assert_eq!(typed(&view), "INT");
    assert!(view.edit_key(ViewerKey::Backspace), "so do the keys the list doesn't take");
    assert_eq!(typed(&view), "IN");
    assert!(view.suggesting());
    assert!(view.edit_key(ViewerKey::Escape), "esc closes the suggestions");
    assert!(!view.suggesting() && view.popup.is_none());
    assert_eq!(typed(&view), "IN", "the field's text stays");
    assert!(view.take_action().is_none(), "the modal stays open");
    // Typed again, the first lit: ↵ takes it into the field.
    assert!(view.text_input("T"));
    assert!(view.suggesting());
    let lit = view.picker.highlighted().expect("the first lit");
    let chosen = view.suggested[lit];
    assert!(view.edit_key(ViewerKey::Enter));
    assert_eq!(typed(&view), chosen);
    assert!(!view.suggesting());
    // The field's selection is the copy's.
    assert!(view.command("select_all"));
    assert_eq!(view.selected_text().as_deref(), Some(chosen));
}
