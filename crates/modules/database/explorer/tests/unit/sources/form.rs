//! The modal's model: fields and the URL in step both ways, a password typed never kept in
//! the source, the edits tracked (dirty, applied), sources added, duplicated (their
//! password copied), removed and moved between the project and every project (their
//! password following), the filter, what keeps Apply back, and the list's folders.

use super::*;
use database::fields::FieldError;

fn server_spec() -> ConnectionSpec {
    ConnectionSpec::server(5432, &["postgresql", "postgres"]).with(
        FieldSpec::new(
            "sslmode",
            "datasources.field_sslmode",
            FieldKind::Choice(vec![
                ("prefer".into(), "prefer".into()),
                ("require".into(), "require".into()),
            ]),
            Binding::Option("sslmode".into()),
            FieldTab::Ssl,
        )
        .with_default("prefer"),
    )
}

fn drivers() -> Vec<DriverInfo> {
    vec![
        DriverInfo {
            id: "postgres".into(),
            name: "PostgreSQL".into(),
            icon: "server",
            tone: database::IconTone::Neutral,
            spec: server_spec(),
            file_based: false,
            schemas: true,
            databases: true,
            external: false,
        },
        DriverInfo {
            id: "sqlite".into(),
            name: "SQLite".into(),
            icon: "database",
            tone: database::IconTone::Neutral,
            spec: ConnectionSpec::file(&["sqlite"], "sqlite"),
            file_based: true,
            schemas: false,
            databases: false,
            external: false,
        },
    ]
}

fn shop() -> DataSource {
    let mut source = DataSource::new("shop", "postgres", Scope::Project);
    source.target.host = "db".into();
    source.target.user = "app".into();
    source.target.database = "shop".into();
    source
}

fn editor() -> Editor {
    let mut global = DataSource::new("analytics", "postgres", Scope::Global);
    global.target.host = "warehouse".into();
    Editor::new(drivers(), &[shop()], &[global])
}

fn type_into(editor: &mut Editor, field: Field, text: &str) {
    let input = editor.input_mut(&field).unwrap();
    input.select_all();
    input.insert(text);
    editor.edited(&field);
}

#[test]
fn the_fields_and_the_url_follow_each_other() {
    let mut e = editor();
    assert_eq!(e.selected, Some(0), "the first source");
    assert_eq!(e.url.text(), "postgresql://app@db/shop");
    type_into(&mut e, Field::Spec("port".into()), "6543");
    e.choose("sslmode", "require");
    assert_eq!(e.url.text(), "postgresql://app@db:6543/shop?sslmode=require");
    type_into(&mut e, Field::Url, "postgres://admin:s3cret@prod.example.com/sales");
    let entry = e.current().unwrap();
    assert_eq!(entry.source.target.host, "prod.example.com");
    assert_eq!(entry.source.target.user, "admin");
    assert_eq!(entry.source.target.port, None);
    assert_eq!(entry.source.target.options.get("sslmode"), None, "the URL says none");
    assert!(entry.password_typed, "the URL's password went to its field");
    assert_eq!(entry.password.text(), "s3cret");
    assert!(entry.password.is_masked());
    assert!(!e.url.text().contains("s3cret"), "and not left in the URL: {}", e.url.text());
    assert_eq!(e.text_of("host"), "prod.example.com");
    // A URL that isn't one: said, nothing changed.
    type_into(&mut e, Field::Url, "mysql://x/y");
    assert!(e.url_error.is_some());
    assert_eq!(e.current().unwrap().source.target.host, "prod.example.com");
}

#[test]
fn edits_are_tracked_until_applied() {
    let mut e = editor();
    assert!(!e.dirty());
    type_into(&mut e, Field::Comment, "the live one");
    assert!(e.dirty());
    assert_eq!(e.plan().update.len(), 1);
    e.applied();
    assert!(!e.dirty());
    assert!(e.plan().is_empty());
    // A password typed is a change too, kept as typed (never in the source).
    type_into(&mut e, Field::Password, "hunter2");
    assert!(e.dirty());
    let plan = e.plan();
    assert!(plan.update.is_empty(), "the source itself is unchanged");
    assert!(
        matches!(&plan.secrets[..], [SecretOp::Set(s, secret)] if s.name == "shop" && secret.expose() == "hunter2")
    );
}

#[test]
fn sources_are_added_duplicated_and_removed() {
    let mut e = editor();
    let id = e.add("postgres").unwrap();
    assert_eq!(e.current().unwrap().source.id, id);
    assert_eq!(e.current().unwrap().source.name, "PostgreSQL");
    assert_eq!(e.current().unwrap().source.scope, Scope::Project);
    assert_eq!(
        e.entries.iter().filter(|x| x.source.scope == Scope::Project).count(),
        2,
        "the project's first"
    );
    // A file's: no password asked.
    e.add("sqlite");
    assert_eq!(e.current().unwrap().source.secrets, SecretPolicy::Never);
    assert!(!e.has_password());
    // A stored source duplicated: a new identity, a unique name, its password copied.
    e.select(Some(0));
    e.duplicate();
    let copy = e.current().unwrap().source.clone();
    assert_eq!(copy.name, "shop (2)");
    assert_ne!(copy.id, shop().id);
    let plan = e.plan();
    assert!(plan.secrets.iter().any(
        |op| matches!(op, SecretOp::Copy { from, to } if from.name == "shop" && to.id == copy.id)
    ));
    // Removed: from the list, then from the catalog with its password.
    let first = e.entries.iter().position(|x| x.source.name == "shop").unwrap();
    e.select(Some(first));
    e.remove();
    let plan = e.plan();
    assert_eq!(plan.remove.len(), 1);
    assert!(plan.secrets.iter().any(|op| matches!(op, SecretOp::Remove(s) if s.name == "shop")));
    assert_eq!(plan.add.len(), 3);
}

#[test]
fn moving_a_source_moves_its_password_and_says_so() {
    let mut e = editor();
    e.move_scope();
    assert_eq!(e.scope(), Some(Scope::Global));
    assert_eq!(e.current().unwrap().source.name, "shop", "still selected");
    let plan = e.plan();
    assert!(
        matches!(&plan.secrets[..], [SecretOp::Move { from, to }] if from.scope == Scope::Project && to.scope == Scope::Global)
    );
    // Its project password goes to the keychain.
    assert_eq!(crate::credentials::effective(&e.current().unwrap().source), SecretPolicy::Keychain);
    // A policy changed alone moves it too.
    e.move_scope();
    e.set_policy(SecretPolicy::Session);
    assert!(
        matches!(&e.plan().secrets[..], [SecretOp::Move { to, .. }] if to.secrets == SecretPolicy::Session)
    );
}

#[test]
fn the_filter_keeps_the_selection_among_what_it_shows() {
    let mut e = editor();
    type_into(&mut e, Field::Filter, "ANAL");
    assert_eq!(e.visible().len(), 1);
    assert_eq!(e.current().unwrap().source.name, "analytics");
    type_into(&mut e, Field::Filter, "");
    assert_eq!(e.visible().len(), 2);
}

#[test]
fn what_keeps_apply_back() {
    let mut e = editor();
    type_into(&mut e, Field::Spec("port".into()), "port");
    type_into(&mut e, Field::Name, " ");
    let problems = e.problems();
    assert!(problems.contains(&Problem::NoName(0)));
    assert!(problems.contains(&Problem::Field(0, "port".into(), FieldError::NotANumber)));
    assert_eq!(e.field_problem("port"), Some(FieldError::NotANumber));
    // An advanced option named like a secret: refused, never kept.
    let key = e.add_option();
    type_into(&mut e, key, "db_password");
    type_into(&mut e, Field::OptionValue(0), "hunter2");
    assert!(
        e.problems().iter().any(|p| matches!(p, Problem::SecretOption(0, o) if o == "db_password"))
    );
    assert!(!e.current().unwrap().source.target.options.contains_key("db_password"));
    // Another option is kept, out of the URL's fields' way.
    type_into(&mut e, Field::OptionKey(0), "statement_timeout");
    assert_eq!(
        e.current().unwrap().source.target.options.get("statement_timeout").map(String::as_str),
        Some("hunter2")
    );
}

#[test]
fn schemas_shown_or_hidden() {
    let mut e = editor();
    let id = e.current().unwrap().source.id;
    e.schemas_found = vec![(id, vec!["public".into(), "billing".into(), "staging".into()])];
    let all = e.schemas();
    assert!(e.shows_schema("staging"), "all by default");
    e.toggle_schema("staging", &all);
    assert_eq!(e.current().unwrap().source.schemas, ["public", "billing"]);
    assert!(!e.shows_schema("staging"));
    e.toggle_schema("staging", &all);
    assert!(e.current().unwrap().source.schemas.is_empty(), "every one again: all");
}

#[test]
fn databases_shown_or_hidden() {
    let mut e = editor();
    let id = e.current().unwrap().source.id;
    e.databases_found = vec![(id, vec!["postgres".into(), "shop".into(), "analytics".into()])];
    let all = e.databases();
    assert!(e.shows_database("postgres"), "all by default");
    e.toggle_database("postgres", &all);
    assert_eq!(e.current().unwrap().source.databases, ["shop", "analytics"]);
    assert!(!e.shows_database("postgres") && e.shows_database("shop"));
    // The schemas are a choice of their own.
    assert!(e.current().unwrap().source.schemas.is_empty());
    e.toggle_database("postgres", &all);
    assert!(e.current().unwrap().source.databases.is_empty(), "every one again: all");
    // One hidden that the last test didn't find (gone since) is still listed, to show again.
    e.toggle_database("shop", &all);
    e.databases_found.clear();
    assert_eq!(e.databases(), ["postgres", "analytics"]);
    e.show_all_databases();
    assert!(e.current().unwrap().source.databases.is_empty());
}

#[test]
fn folders_are_made_colored_moved_and_taken_away_then_applied() {
    let mut e = editor();
    assert_eq!(
        e.list_rows(),
        [
            ListRow::Section(Scope::Project),
            ListRow::Source { index: 0, depth: 0 },
            ListRow::Section(Scope::Global),
            ListRow::Source { index: 1, depth: 0 },
        ],
        "no folder: as before"
    );
    // A folder at the project's top (beside the selected source), named, colored.
    let prod = e.add_folder(e.new_folder_place(), "New Folder");
    assert_eq!(e.selected_folder, Some(prod));
    assert_eq!(e.selected, None, "a folder or a source selected, not both");
    type_into(&mut e, Field::FolderName, "prod");
    assert_eq!(e.folder(prod).unwrap().name, "prod");
    e.set_folder_color(prod, Some(SourceColor::Red));
    assert!(e.dirty());
    // A subfolder, and the source dragged into it: it takes the color from up the chain.
    let eu = e.add_folder(Place::Folder(prod), "New Folder");
    let shop = e.entries[0].source.id;
    assert!(e.move_source(shop, Place::Folder(eu)));
    assert_eq!(e.color_of(0), Some(SourceColor::Red));
    assert_eq!(
        e.list_rows(),
        [
            ListRow::Section(Scope::Project),
            ListRow::Folder { id: prod, depth: 0, open: true, count: 1 },
            ListRow::Folder { id: eu, depth: 1, open: true, count: 1 },
            ListRow::Source { index: 0, depth: 2 },
            ListRow::Section(Scope::Global),
            ListRow::Source { index: 1, depth: 0 },
        ]
    );
    // Its own color wins.
    e.select(Some(0));
    e.set_color(Some(SourceColor::Green));
    assert_eq!(e.color_of(0), Some(SourceColor::Green));
    // A folder never goes into one inside it; closed, its insides aren't listed.
    assert!(!e.move_folder(prod, Place::Folder(eu)));
    assert!(!e.places_for(Item::Folder(prod)).contains(&Place::Folder(eu)));
    e.toggle_folder(prod);
    assert_eq!(e.list_rows()[1], ListRow::Folder { id: prod, depth: 0, open: false, count: 1 });
    assert_eq!(e.list_rows().len(), 4);
    e.toggle_folder(prod);
    // The global source dragged into a project's folder: it's the project's now.
    let global = e.entries[1].source.id;
    assert!(e.move_source(global, Place::Folder(prod)));
    let moved = e.entries.iter().find(|x| x.source.id == global).unwrap();
    assert_eq!(moved.source.scope, Scope::Project);
    // The filter: a folder named like it shows what it holds.
    e.filter.set_text("new folder");
    e.edited(&Field::Filter);
    assert!(e.list_rows().iter().any(|r| matches!(r, ListRow::Source { .. })));
    e.filter.set_text("");
    e.edited(&Field::Filter);
    // Apply: the folders and the sources' places.
    let plan = e.plan();
    assert_eq!(plan.folders.len(), 1);
    assert_eq!(plan.folders[0].0, Scope::Project);
    assert_eq!(plan.folders[0].1.len(), 2);
    e.applied();
    assert!(!e.dirty());
    // Taken away: no source goes with it, what it held moves up.
    e.delete_folder(prod);
    assert_eq!(e.entries.len(), 2);
    assert_eq!(e.folder(eu).unwrap().parent, None);
    assert!(e.entries.iter().all(|x| x.source.folder != Some(prod)));
    assert!(e.dirty());
}

#[test]
fn a_source_moved_to_the_other_list_leaves_its_folder() {
    let mut e = editor();
    let dev = e.add_folder(Place::Top(Scope::Project), "dev");
    let shop = e.entries[0].source.id;
    e.move_source(shop, Place::Folder(dev));
    let at = e.entries.iter().position(|x| x.source.id == shop);
    e.select(at);
    e.move_scope();
    let entry = e.current().unwrap();
    assert_eq!((entry.source.scope, entry.source.folder), (Scope::Global, None));
}

/// Production: a source's own mark, or its folder's (inherited all the way down, never taken
/// away from inside); both applied with the rest.
#[test]
fn production_is_marked_on_a_source_or_inherited_from_its_folder() {
    let mut e = editor();
    e.select(Some(0));
    assert!(!e.production_of(0));
    e.set_production(true);
    assert!(e.production_of(0) && e.inherited_production(0).is_none());
    assert!(e.dirty(), "a change to apply");
    e.set_production(false);
    let prod = e.add_folder(e.new_folder_place(), "prod");
    let eu = e.add_folder(Place::Folder(prod), "eu");
    let shop = e.entries[0].source.id;
    assert!(e.move_source(shop, Place::Folder(eu)));
    assert!(!e.production_of(0));
    e.set_folder_production(prod, true);
    assert!(e.folder_production(eu), "a subfolder too");
    assert!(e.production_of(0), "from up the chain");
    assert_eq!(e.inherited_production(0).map(|f| f.id), Some(prod));
    assert!(!e.entries[0].source.production, "its own mark untouched");
    let plan = e.plan();
    assert!(plan.folders.iter().flat_map(|(_, f)| f).any(|f| f.id == prod && f.production));
    e.set_folder_production(prod, false);
    assert!(!e.production_of(0));
}
