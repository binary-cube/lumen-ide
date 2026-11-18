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
