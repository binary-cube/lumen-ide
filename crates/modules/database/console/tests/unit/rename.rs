//! Renaming and deleting a console from the UI: the name checked as typed, the file renamed
//! and announced as any rename (the same console: its id, settings, runner), deleting asked
//! first, its file removed and announced, an open transaction rolled back.

use super::*;
use crate::fake::{FakeAccess, FakeHost, Manual, Project};
use database::console::ConsoleAccess as _;
use std::sync::Arc;
use workspace::events::{EntriesDeleted, FileRenamed};

struct Setup {
    project: Project,
    manual: Manual,
    access: Arc<FakeAccess>,
    consoles: Arc<crate::Consoles>,
    host: FakeHost,
    _registered: kernel::Subscription,
}

fn setup(name: &str) -> Setup {
    let project = Project::new(name);
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = crate::fake::consoles(&project, &manual, Arc::clone(&access));
    let host = FakeHost::new(consoles.project(), &project.root);
    let registered = crate::consoles::register(Arc::clone(&consoles));
    Setup { project, manual, access, consoles, host, _registered: registered }
}

#[test]
fn a_console_renamed_from_the_ui_stays_itself() {
    let mut s = setup("rename");
    let spec = s.consoles.create(s.access.shop, "SELECT 1;").unwrap();
    s.consoles.create(s.access.shop, "").unwrap();
    s.consoles.update(spec.id, |c| {
        c.schema = Some("main".into());
        c.tx = database::console::TxMode::Manual;
    });
    let runner = s.consoles.runner(spec.id, spec.source);
    let heard: Arc<std::sync::Mutex<Vec<FileRenamed>>> = Arc::default();
    let h = Arc::clone(&heard);
    let _sub =
        s.consoles.events().subscribe(move |e: &FileRenamed| h.lock().unwrap().push(e.clone()));
    // Its name asked: the others' names known, the field's problems said.
    assert!(ask_name(&mut s.host, spec.id));
    let request = s.host.renames.last().unwrap().clone();
    assert_eq!(request.taken, ["console_1"]);
    let mut view = RenameView::default();
    view.set_data(&request);
    assert_eq!(view.problem(), None, "its own name");
    view.set_name("Console_1");
    assert_eq!(view.problem(), Some(t("console.name_taken")));
    assert!(!view.give());
    view.set_name("a/b");
    assert_eq!(view.problem(), Some(i18n::tr!("console.name_invalid", char = "/")));
    view.set_name("  orders by day.sql ");
    assert_eq!(view.problem(), None);
    assert!(view.edit_key(ViewerKey::Enter));
    assert!(view.closed());
    let Some(ViewerAction::WithHost(work)) = view.take_action() else { panic!("its work") };
    work.run(&mut s.host);
    // The same console, its file renamed, announced.
    let now = s.consoles.spec(spec.id).unwrap();
    assert_eq!(now.name, "orders by day");
    assert_eq!((now.schema.as_deref(), now.tx), (Some("main"), database::console::TxMode::Manual));
    let file = now.path(&s.project.root);
    assert!(file.exists() && !spec.path(&s.project.root).exists());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "SELECT 1;");
    assert_eq!(heard.lock().unwrap().len(), 1);
    assert_eq!(heard.lock().unwrap()[0].to, file);
    assert_eq!(runner.name(), "orders by day", "the same runner, told");
    s.manual.run_all();
}

#[test]
fn a_rename_refused_says_why_and_changes_nothing() {
    let mut s = setup("rename-refused");
    let spec = s.consoles.create(s.access.shop, "").unwrap();
    s.consoles.create(s.access.shop, "").unwrap();
    assert!(!apply(&mut s.host, spec.id, "console_1"));
    assert_eq!(s.host.told.last().unwrap().title, t("console.name_taken"));
    assert!(!apply(&mut s.host, spec.id, "CON"));
    assert!(!apply(&mut s.host, spec.id, "  "));
    assert_eq!(s.consoles.spec(spec.id).unwrap().name, "console");
    assert!(spec.path(&s.project.root).exists());
}

#[test]
fn a_console_deleted_is_asked_first_and_its_transaction_rolled_back() {
    let mut s = setup("delete");
    let spec = s.consoles.create(s.access.shop, "BEGIN; DELETE FROM users WHERE id = 1;").unwrap();
    let heard: Arc<std::sync::Mutex<Vec<EntriesDeleted>>> = Arc::default();
    let h = Arc::clone(&heard);
    let _sub =
        s.consoles.events().subscribe(move |e: &EntriesDeleted| h.lock().unwrap().push(e.clone()));
    // A transaction open on it.
    let runner = s.consoles.runner(spec.id, spec.source);
    let connector = s.access.connector(s.access.shop, &Default::default()).unwrap();
    let text = "BEGIN; DELETE FROM users WHERE id = 1;";
    let statements = match database::console::plan(&database::console::PlanInput {
        text,
        mode: database::console::RunMode::Script,
        flavor: database::sql::Flavor::Sqlite,
        ..Default::default()
    }) {
        database::console::RunPlan::Run(s) => s,
        other => panic!("{other:?}"),
    };
    let _lease = runner.lease();
    assert!(runner.run(crate::runner::RunRequest {
        statements,
        document: text.into(),
        settings: Default::default(),
        connector,
        source_name: "shop".into(),
        detached: false,
    }));
    s.manual.run_all();
    assert!(runner.in_transaction());
    assert!(confirm_delete(&mut s.host, spec.id));
    let asked = s.host.asked.last().unwrap();
    assert!(asked.danger && asked.message.contains(&t("console.delete_tx")));
    assert!(s.consoles.spec(spec.id).is_none(), "it left the list");
    assert!(!spec.path(&s.project.root).exists());
    assert_eq!(heard.lock().unwrap()[0].paths, [spec.path(&s.project.root)]);
    assert!(s.consoles.runner_of(spec.id).is_none());
    s.manual.run_all();
    assert_eq!(s.project.look("SELECT count(*) FROM users"), [[database::Value::Int(3)]]);
    // Gone: said.
    assert!(!confirm_delete(&mut s.host, spec.id));
    assert_eq!(s.host.told.last().unwrap().title, t("console.gone"));
}

#[test]
fn the_rename_modal_draws_its_problem() {
    let mut view = RenameView::default();
    view.set_data(&RenameRequest {
        id: 1,
        project: ProjectId::new(),
        console: ConsoleId::new(),
        name: "console".into(),
        source_name: "shop".into(),
        taken: vec!["console_1".into()],
    });
    view.set_name("console_1");
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    view.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 1000.0, HEIGHT * 2.0),
        scale: 2.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    assert!(view.problem().is_some());
    assert!(view.hits.iter().any(|(_, h)| *h == Hit::Go));
    assert!(!view.command("go"), "not with a problem");
    assert!(view.command("name:orders"));
    assert!(view.command("go"));
}
