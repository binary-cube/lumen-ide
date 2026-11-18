use super::*;
use crate::sql::WriteKind;

const SCRIPT: &str =
    "SELECT 1;\n\n-- the users\nSELECT * FROM users;\nUPDATE users SET a = 1 WHERE id = 2;\n";

fn input(
    text: &str,
    cursor: usize,
    selection: Option<Range<usize>>,
    mode: RunMode,
) -> PlanInput<'_> {
    PlanInput {
        text,
        selection,
        cursor,
        mode,
        flavor: Flavor::Sqlite,
        options: SplitOptions::default(),
    }
}

fn texts(plan: &RunPlan) -> Vec<&str> {
    match plan {
        RunPlan::Run(s) | RunPlan::Choose(s) => s.iter().map(|s| s.text.as_str()).collect(),
        RunPlan::Nothing => Vec::new(),
    }
}

#[test]
fn identities_are_uuids_kept_as_text() {
    let id = ConsoleId::new();
    assert_ne!(id, ConsoleId::new());
    assert_eq!(ConsoleId::parse(&id.to_string()), Some(id));
    assert_eq!(ConsoleId::parse("console"), None);
}

#[test]
fn modes_have_stable_ids() {
    for mode in [TxMode::Auto, TxMode::Manual] {
        assert_eq!(TxMode::from_id(mode.id()), Some(mode));
    }
    for mode in [RunMode::Playground, RunMode::Script] {
        assert_eq!(RunMode::from_id(mode.id()), Some(mode));
    }
    assert_eq!(RunMode::from_id("other"), None);
}

/// A console's file is `consoles/<source>/<name>.sql` in the project's own folder: found
/// back from its path, nothing else taken for one.
#[test]
fn a_console_s_file_is_found_back_from_its_path() {
    let root = Path::new("/p");
    let source = DataSourceId::new();
    let spec = ConsoleSpec::new(source, "console_1");
    let path = spec.path(root);
    assert!(path.starts_with(folder(root)));
    assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("console_1.sql"));
    assert_eq!(identify(root, &path), Some((source, "console_1".to_owned())));
    // Not a console: elsewhere, another extension, not a source's folder, deeper.
    assert_eq!(identify(root, Path::new("/p/src/a.sql")), None);
    assert_eq!(identify(root, &path.with_extension("txt")), None);
    let other = folder(root).join("notes").join("a.sql");
    assert_eq!(identify(root, &other), None);
    let deeper = folder(root).join(source.to_string()).join("x").join("a.sql");
    assert_eq!(identify(root, &deeper), None);
    assert_eq!(identify(Path::new("/q"), &path), None, "another project's");
}

/// A project's own SQL file can be attached to a source (39n): inside the project, with a
/// SQL extension, never in its own folder (a console's file is a console).
#[test]
fn a_project_s_sql_files_are_attachable_and_nothing_else() {
    let root = Path::new("/p");
    assert_eq!(
        attachable(root, Path::new("/p/db/report.sql")),
        Some(PathBuf::from("db/report.sql"))
    );
    assert_eq!(attachable(root, Path::new("/p/a.SQL")), Some(PathBuf::from("a.SQL")));
    for x in ["ddl", "pgsql", "psql"] {
        let path = PathBuf::from(format!("/p/schema.{x}"));
        assert!(attachable(root, &path).is_some(), "{x}");
    }
    assert_eq!(attachable(root, Path::new("/p/notes.txt")), None);
    assert_eq!(attachable(root, Path::new("/p/.sql")), None, "no name");
    assert_eq!(attachable(root, Path::new("/q/a.sql")), None, "another project's");
    assert_eq!(attachable(root, Path::new("/p/../a.sql")), None);
    let console = ConsoleSpec::new(DataSourceId::new(), "console").path(root);
    assert_eq!(attachable(root, &console), None, "a console's file");
    let own = root.join(kernel::brand::PROJECT_DIR).join("x.sql");
    assert_eq!(attachable(root, &own), None, "the project's own folder");
}

/// An attached file's spec: named as the file, its path in the project (not its own folder).
#[test]
fn an_attached_file_is_kept_relative_to_the_project() {
    let root = Path::new("/p");
    let source = DataSourceId::new();
    let spec = ConsoleSpec::attached(source, PathBuf::from("db/report.sql"));
    assert!(spec.is_attached());
    assert_eq!(spec.kind, ConsoleKind::Attached);
    assert_eq!(spec.name, "report.sql");
    assert_eq!(spec.path(root), root.join("db").join("report.sql"));
    assert_eq!(identify(root, &spec.path(root)), None);
    assert!(!ConsoleSpec::new(source, "console").is_attached());
}

#[test]
fn new_consoles_are_named_after_the_taken_ones() {
    assert_eq!(next_name([]), "console");
    assert_eq!(next_name(["console"]), "console_1");
    assert_eq!(next_name(["Console", "console_1", "console_3"]), "console_2");
    assert_eq!(next_name(["mine"]), "console");
}

#[test]
fn a_table_s_console_starts_with_its_rows_the_name_selected() {
    let quote = |s: &str| if s.contains(' ') { format!("\"{s}\"") } else { s.to_owned() };
    let (text, name) = table_query(&TableRef::new("users"), quote);
    assert_eq!(text, "SELECT * FROM users;");
    assert_eq!(&text[name], "users");
    let (text, name) = table_query(&TableRef::in_schema("public", "order items"), quote);
    assert_eq!(text, "SELECT * FROM public.\"order items\";");
    assert_eq!(&text[name], "public.\"order items\"");
}

/// Playground: the statement under the cursor; none there: every one to
/// choose from.
#[test]
fn the_playground_runs_the_statement_under_the_cursor() {
    let at = |needle: &str| SCRIPT.find(needle).unwrap();
    let p = plan(&input(SCRIPT, at("users;") + 2, None, RunMode::Playground));
    assert_eq!(texts(&p), ["SELECT * FROM users"]);
    let RunPlan::Run(statements) = &p else { panic!("{p:?}") };
    assert_eq!(&SCRIPT[statements[0].range.clone()], "SELECT * FROM users");
    assert_eq!(statements[0].kind, StatementKind::Query);
    // On its comment, the statement it's about.
    let p = plan(&input(SCRIPT, at("the users"), None, RunMode::Playground));
    assert_eq!(texts(&p), ["SELECT * FROM users"]);
    // Right after a statement's `;`, on its line: that one.
    let p = plan(&input(SCRIPT, at(";\n\n") + 1, None, RunMode::Playground));
    assert_eq!(texts(&p), ["SELECT 1"]);
    // On the blank line two lines after: the choice.
    let blank = "SELECT 1;\n\n\nSELECT 2;";
    let p = plan(&input(blank, 11, None, RunMode::Playground));
    assert!(matches!(p, RunPlan::Choose(ref s) if s.len() == 2), "{p:?}");
    // The write is told apart.
    let p = plan(&input(SCRIPT, at("UPDATE"), None, RunMode::Playground));
    let RunPlan::Run(statements) = p else { panic!() };
    assert_eq!(statements[0].kind, StatementKind::Write(WriteKind::Update));
    assert!(statements[0].writes());
}

/// A selection runs its own statements (one after the other), wherever the cursor is, their
/// places those of the document.
#[test]
fn a_selection_runs_its_statements_in_either_mode() {
    let start = SCRIPT.find("-- the").unwrap();
    let end = SCRIPT.len();
    for mode in [RunMode::Playground, RunMode::Script] {
        let p = plan(&input(SCRIPT, 0, Some(start..end), mode));
        assert_eq!(texts(&p), ["SELECT * FROM users", "UPDATE users SET a = 1 WHERE id = 2"]);
        let RunPlan::Run(statements) = p else { panic!() };
        for s in &statements {
            assert_eq!(&SCRIPT[s.range.clone()], s.text);
        }
    }
    // Part of a statement: what's selected, as written.
    let p = plan(&input(SCRIPT, 0, Some(0..8), RunMode::Playground));
    assert_eq!(texts(&p), ["SELECT 1"]);
    // A selection of comments only: nothing.
    let comment = SCRIPT.find("-- the").unwrap();
    let p = plan(&input(SCRIPT, 0, Some(comment..comment + 12), RunMode::Playground));
    assert_eq!(p, RunPlan::Nothing);
    // A selection past the text or inside a character is ignored (the cursor's then).
    let text = "SELECT 'é';";
    let p = plan(&input(text, 0, Some(9..10), RunMode::Playground));
    assert_eq!(texts(&p), ["SELECT 'é'"]);
}

#[test]
fn a_script_runs_every_statement_without_asking() {
    let p = plan(&input(SCRIPT, 0, None, RunMode::Script));
    assert_eq!(texts(&p).len(), 3);
    assert_eq!(plan(&input("  -- nothing\n", 0, None, RunMode::Script)), RunPlan::Nothing);
    assert_eq!(plan(&input("", 0, None, RunMode::Playground)), RunPlan::Nothing);
    // A trigger's body stays one statement.
    let trigger = "CREATE TRIGGER t AFTER INSERT ON a BEGIN UPDATE b SET n = 1; END;\nSELECT 1;";
    assert_eq!(texts(&plan(&input(trigger, 0, None, RunMode::Script))).len(), 2);
}

#[test]
fn a_read_only_source_refuses_the_first_write() {
    let RunPlan::Run(statements) = plan(&input(SCRIPT, 0, None, RunMode::Script)) else { panic!() };
    assert_eq!(refused(&statements, false), None);
    assert_eq!(refused(&statements, true), Some(2));
    assert_eq!(refused(&statements[..2], true), None);
}

#[test]
fn summaries_are_one_line_and_cut() {
    assert_eq!(summary("SELECT *\n  FROM users", 40), "SELECT * FROM users");
    assert_eq!(summary("SELECT * FROM users", 6), "SELECT…");
}

#[derive(Debug)]
struct Fake;

impl ConsoleAccess for Fake {
    fn sources(&self) -> Vec<SourceBrief> {
        Vec::new()
    }

    fn source(&self, _id: DataSourceId) -> Option<SourceBrief> {
        None
    }

    fn connector(&self, _id: DataSourceId, _overrides: &Overrides) -> Result<Connector, DbError> {
        Err(DbError::Unsupported("nothing".into()))
    }
}

/// A project's port while it's open, never another's.
#[test]
fn the_port_is_a_project_s_while_it_s_open() {
    let (one, two) = (ProjectId::new(), ProjectId::new());
    let guard = provide(one, Arc::new(Fake));
    assert!(access(one).is_some());
    assert!(access(two).is_none());
    drop(guard);
    assert!(access(one).is_none());
}

#[test]
fn a_jump_asked_is_taken_once() {
    let request = JumpRequest {
        project: ProjectId::new(),
        source: DataSourceId::new(),
        schema: Some("public".into()),
        table: Some(TableRef::new("users")),
        new: false,
        list: false,
        database: None,
    };
    ask_jump(request.clone());
    assert_eq!(take_jump(), Some(request));
    assert_eq!(take_jump(), None);
}

// ─── 39g: the transaction's state, what's confirmed ───

fn kind_of(text: &str, flavor: Flavor) -> StatementKind {
    sql::classify(text, &sql::lex(text, flavor), flavor)
}

const PG: Flavor = Flavor::Postgres { version: Some(160_000) };

#[test]
fn production_consoles_start_manual_the_others_auto() {
    assert_eq!(TxMode::default_for(true), TxMode::Manual);
    assert_eq!(TxMode::default_for(false), TxMode::Auto);
}

#[test]
fn in_auto_statements_never_leave_a_transaction_open() {
    let mut state = TxState::Idle;
    for (i, text) in ["SELECT 1", "UPDATE t SET a = 1 WHERE id = 2", "DELETE FROM t WHERE id = 3"]
        .into_iter()
        .enumerate()
    {
        state = state.on_statement(i as u64, kind_of(text, PG), true, TxMode::Auto, PG);
    }
    assert_eq!(state, TxState::Idle);
    assert!(!state.is_open());
}

#[test]
fn in_manual_the_first_write_opens_it_and_writes_are_counted() {
    let s = TxState::Idle.on_statement(0, kind_of("SELECT 1", PG), true, TxMode::Manual, PG);
    assert_eq!(s, TxState::Idle, "a read opens nothing");
    let s =
        s.on_statement(1, kind_of("UPDATE t SET a = 1 WHERE id = 1", PG), true, TxMode::Manual, PG);
    assert_eq!(s, TxState::Open { since_statement: 1, writes: 1 });
    let s = s.on_statement(2, kind_of("SELECT * FROM t", PG), true, TxMode::Manual, PG);
    let s = s.on_statement(3, kind_of("INSERT INTO t VALUES (1)", PG), true, TxMode::Manual, PG);
    assert_eq!(s, TxState::Open { since_statement: 1, writes: 2 });
    assert_eq!(s.pending(), 2);
    // A write refused opens nothing.
    let refused = TxState::Idle.on_statement(
        0,
        kind_of("DELETE FROM t WHERE x", PG),
        false,
        TxMode::Manual,
        PG,
    );
    assert_eq!(refused, TxState::Idle);
}

#[test]
fn a_postgres_error_gives_the_transaction_up_until_it_is_rolled_back() {
    let open = TxState::Open { since_statement: 0, writes: 1 };
    let failed = open.on_statement(1, kind_of("SELECT nope", PG), false, TxMode::Manual, PG);
    assert_eq!(failed, TxState::Failed);
    assert!(failed.is_open() && failed.is_failed());
    // Everything after it stays refused, a success included (the engine refuses it anyway).
    let still = failed.on_statement(2, kind_of("SELECT 1", PG), true, TxMode::Manual, PG);
    assert_eq!(still, TxState::Failed);
    // Back to a savepoint: it goes on; ROLLBACK or COMMIT end it.
    assert!(matches!(
        failed.on_statement(3, kind_of("ROLLBACK TO SAVEPOINT a", PG), true, TxMode::Manual, PG),
        TxState::Open { .. }
    ));
    assert_eq!(
        failed.on_statement(3, kind_of("ROLLBACK", PG), true, TxMode::Manual, PG),
        TxState::Idle
    );
    assert_eq!(
        failed.on_statement(3, kind_of("COMMIT", PG), true, TxMode::Manual, PG),
        TxState::Idle
    );
    // SQLite keeps a transaction open after an error.
    let lite = open.on_statement(
        1,
        kind_of("SELECT nope", Flavor::Sqlite),
        false,
        TxMode::Manual,
        Flavor::Sqlite,
    );
    assert_eq!(lite, open);
}

#[test]
fn a_begin_and_commit_written_by_hand_are_followed_in_either_mode() {
    for mode in [TxMode::Auto, TxMode::Manual] {
        let s = TxState::Idle.on_statement(4, kind_of("BEGIN", PG), true, mode, PG);
        assert_eq!(s, TxState::Open { since_statement: 4, writes: 0 }, "{mode:?}");
        let s = s.on_statement(5, kind_of("UPDATE t SET a = 2", PG), true, mode, PG);
        assert_eq!(s.pending(), 1);
        // A second BEGIN changes nothing (the engine warns).
        assert_eq!(s.on_statement(6, kind_of("BEGIN", PG), true, mode, PG), s);
        let s = s.on_statement(6, kind_of("SAVEPOINT a", PG), true, mode, PG);
        assert_eq!(s.pending(), 1);
        assert_eq!(s.on_statement(7, kind_of("COMMIT", PG), true, mode, PG), TxState::Idle);
        // A refused COMMIT leaves it as it was.
        assert_eq!(s.on_statement(7, kind_of("COMMIT", PG), false, mode, PG), s);
    }
    assert_eq!(TxState::Open { since_statement: 0, writes: 3 }.ended(), TxState::Idle);
}

#[test]
fn the_engine_has_the_last_word() {
    let open = TxState::Open { since_statement: 2, writes: 1 };
    assert_eq!(open.follow(5, false), TxState::Idle, "ended by the engine (an implicit commit)");
    assert_eq!(TxState::Failed.follow(5, false), TxState::Idle);
    assert_eq!(TxState::Idle.follow(5, true), TxState::Open { since_statement: 5, writes: 0 });
    assert_eq!(open.follow(5, true), open);
    assert_eq!(TxState::Idle.follow(5, false), TxState::Idle);
}

fn planned_all(text: &str, flavor: Flavor) -> Vec<PlannedStatement> {
    match plan(&PlanInput {
        text,
        selection: None,
        cursor: 0,
        mode: RunMode::Script,
        flavor,
        options: SplitOptions::default(),
    }) {
        RunPlan::Run(s) => s,
        other => panic!("{other:?}"),
    }
}

#[test]
fn dangers_are_confirmed_anywhere() {
    let s = planned_all(
        "SELECT 1;\nDELETE FROM orders;\nUPDATE t SET a = 1 WHERE id = 1;\nDROP TABLE x;",
        PG,
    );
    let asked = safety(&s, PG, false, TxMode::Auto, false);
    assert!(!asked.is_clear());
    assert_eq!(asked.dangers.len(), 2);
    assert!(matches!(
        &asked.dangers[0],
        (1, Danger::UnfilteredWrite { kind: WriteKind::Delete, .. })
    ));
    assert!(matches!(&asked.dangers[1], (3, Danger::DropObject { .. })));
    assert!(asked.production_writes.is_empty(), "not production");
    assert_eq!(asked.statements(), vec![1, 3]);
    // A filtered write and a read: nothing asked.
    let s = planned_all("SELECT 1;\nUPDATE t SET a = 1 WHERE id = 1;", PG);
    assert!(safety(&s, PG, false, TxMode::Auto, false).is_clear());
}

#[test]
fn on_production_every_write_in_auto_is_confirmed_but_not_what_waits_for_commit() {
    let s =
        planned_all("SELECT 1;\nUPDATE t SET a = 1 WHERE id = 1;\nINSERT INTO t VALUES (2);", PG);
    let auto = safety(&s, PG, true, TxMode::Auto, false);
    assert_eq!(auto.production_writes, vec![1, 2]);
    assert!(auto.production);
    // Manual, or a transaction already open: nothing is permanent before Commit.
    assert!(safety(&s, PG, true, TxMode::Manual, false).is_clear());
    assert!(safety(&s, PG, true, TxMode::Auto, true).is_clear());
    // A BEGIN written by hand covers what follows it, until its COMMIT.
    let s = planned_all(
        "UPDATE a SET x = 1 WHERE id = 1;\nBEGIN;\nUPDATE b SET x = 1 WHERE id = 1;\nCOMMIT;\nUPDATE c SET x = 1 WHERE id = 1;",
        PG,
    );
    assert_eq!(safety(&s, PG, true, TxMode::Auto, false).production_writes, vec![0, 4]);
    // Reads on production: nothing.
    let s = planned_all("SELECT 1;\nSELECT * FROM t;", PG);
    assert!(safety(&s, PG, true, TxMode::Auto, false).is_clear());
}

mod parameters {
    use std::collections::HashMap;

    use super::*;
    use crate::Value;
    use crate::sql::{ParamName, ParamStyles, ParamValue, TextPosition};

    fn all(text: &str, flavor: Flavor) -> Vec<PlannedStatement> {
        match plan(&PlanInput { text, mode: RunMode::Script, flavor, ..PlanInput::default() }) {
            RunPlan::Run(s) => s,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn every_form_is_asked_once() {
        let text = "SELECT ?, ?1, $1, :id, @id, $name, :id, '?' -- :no\n;\nSELECT ? FROM t;";
        let statements = all(text, Flavor::Sqlite);
        let asked = parameters(&statements, Flavor::Sqlite, &ParamStyles::of(Flavor::Sqlite));
        let names: Vec<(&str, usize)> = asked.iter().map(|a| (a.label.as_str(), a.uses)).collect();
        // `?` is numbered within each statement; `:id` and `@id` are one; strings and
        // comments are text.
        assert_eq!(names, [("?", 2), ("?1", 2), (":id", 3), ("$name", 1)]);
        assert_eq!(asked[0].name, ParamName::Positional(1));
        // PostgreSQL's `?` is an operator.
        let pg = Flavor::Postgres { version: None };
        let asked = parameters(&all("SELECT d ? 'k', $1 FROM t", pg), pg, &ParamStyles::of(pg));
        assert_eq!(asked.iter().map(|a| a.label.as_str()).collect::<Vec<_>>(), ["$1"]);
    }

    #[test]
    fn values_are_written_in_and_errors_go_back_to_the_user_s_text() {
        let text = "-- top\nSELECT name FROM users WHERE id = :id AND name = :name AND oops;\n";
        let statements = all(text, Flavor::Sqlite);
        let values = HashMap::from([
            (ParamName::Named("id".into()), ParamValue::Literal(Value::Int(3))),
            (ParamName::Named("name".into()), ParamValue::Literal(Value::Text("it's".into()))),
        ]);
        let dialect = dialect_for(Flavor::Sqlite);
        let styles = ParamStyles::of(Flavor::Sqlite);
        let bound = bind_statements(text, &statements, Flavor::Sqlite, &styles, &values, &*dialect)
            .unwrap();
        let sent = bound[0].sent();
        assert_eq!(sent, "SELECT name FROM users WHERE id = 3 AND name = 'it''s' AND oops");
        // The template stays the document's (its marks, the history).
        assert_eq!(bound[0].text, statements[0].text);
        // An engine's error at `oops` in what was sent lands on `oops` in the document.
        let at = sent.find("oops").unwrap();
        let place =
            crate::sql::locate(text, sent, &bound[0].map(), crate::sql::EnginePosition::Byte(at))
                .unwrap();
        assert_eq!(place, TextPosition::of(text, text.find("oops").unwrap()));
        assert_eq!(
            (place.line, place.column),
            (1, text.lines().nth(1).unwrap().find("oops").unwrap())
        );
        // One left without a value: an error, never a silent NULL.
        let missing =
            bind_statements(text, &statements, Flavor::Sqlite, &styles, &HashMap::new(), &*dialect);
        assert!(matches!(missing, Err(crate::sql::BindError::Missing(_))));
        // A statement without parameters is sent as written.
        let plain = all("SELECT 1", Flavor::Sqlite);
        let same = bind_statements("SELECT 1", &plain, Flavor::Sqlite, &styles, &values, &*dialect)
            .unwrap();
        assert_eq!((same[0].bound.as_ref(), same[0].sent()), (None, "SELECT 1"));
        // A raw expression is written as typed.
        let raw = HashMap::from([(ParamName::Positional(1), ParamValue::Raw("now()".into()))]);
        let s = all("SELECT ?", Flavor::Sqlite);
        let b = bind_statements("SELECT ?", &s, Flavor::Sqlite, &styles, &raw, &*dialect).unwrap();
        assert_eq!(b[0].sent(), "SELECT now()");
    }
}

#[test]
fn a_console_s_name_is_checked_as_a_file_s() {
    let taken = ["console", "Orders"];
    assert_eq!(check_name("  daily report ", taken), Ok("daily report".to_owned()));
    assert_eq!(check_name("report.sql", taken), Ok("report".to_owned()));
    assert_eq!(check_name("report.SQL", taken), Ok("report".to_owned()));
    assert_eq!(check_name("", taken), Err(NameProblem::Empty));
    assert_eq!(check_name(".sql", taken), Err(NameProblem::Invalid('.')));
    assert_eq!(check_name("a/b", taken), Err(NameProblem::Invalid('/')));
    assert_eq!(check_name("a:b", taken), Err(NameProblem::Invalid(':')));
    assert_eq!(check_name("tab\there", taken), Err(NameProblem::Invalid('\t')));
    assert_eq!(check_name("end.", taken), Err(NameProblem::Invalid('.')));
    assert_eq!(check_name("nul", taken), Err(NameProblem::Reserved));
    assert_eq!(check_name("COM3.x", taken), Err(NameProblem::Reserved));
    assert_eq!(check_name("compass", taken), Ok("compass".to_owned()));
    assert_eq!(check_name(&"x".repeat(NAME_MAX + 1), taken), Err(NameProblem::TooLong));
    assert_eq!(check_name("orders", taken), Err(NameProblem::Taken), "any case");
    assert_eq!(check_name("ordine_ă", taken), Ok("ordine_ă".to_owned()));
}

/// A document's dialect is told by whoever said they know it (the consoles' module), by
/// project and path.
#[test]
fn a_document_s_dialect_is_told_by_its_module() {
    let project = ProjectId::new();
    let other = ProjectId::new();
    tell_flavors(Arc::new(move |p, path: &Path| {
        (p == project && path.ends_with("console.sql"))
            .then_some(Flavor::Postgres { version: None })
    }));
    assert_eq!(
        document_flavor(project, Path::new("/p/consoles/console.sql")),
        Some(Flavor::Postgres { version: None })
    );
    assert_eq!(document_flavor(project, Path::new("/p/a.sql")), None);
    assert_eq!(document_flavor(other, Path::new("/p/consoles/console.sql")), None);
}
