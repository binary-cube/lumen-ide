//! The SQL consoles' screens.

use super::*;

impl Steps<'_> {
    /// A query console (see `CONSOLE` in the module's doc): the sample database kept as a
    /// project's source in memory, its consoles' list in memory too (their files in the
    /// project's own folder), their clock stopped (each reading 7 ms later).
    pub(super) fn console(&mut self, kind: &str) {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};
        let dir = std::env::temp_dir().join("editor-snapshot-console");
        let db = match data_fixtures::write(&dir) {
            Ok((db, _)) => db,
            Err(e) => {
                tracing::error!("the sample data files weren't made: {e}");
                return;
            }
        };
        let Some(id) = workbench_model::WorkbenchHost::project_id(self.wb) else { return };
        let (Some(sessions), Some(consoles)) =
            (database_explorer::sessions(id), database_console::consoles::consoles(id))
        else {
            tracing::error!("no project's sessions or consoles");
            return;
        };
        let catalog = sessions.catalog();
        catalog.set_store(Arc::new(database::NoStore));
        catalog.ensure_loaded();
        let deadline = Instant::now() + Duration::from_secs(5);
        while catalog.list(database::Scope::Project) == database_explorer::catalog::Listed::Loading
            && Instant::now() < deadline
        {
            settle(5);
        }
        let mut shop = database::DataSource::new("shop", "sqlite", database::Scope::Project);
        shop.file = Some(db);
        shop.secrets = database::SecretPolicy::Never;
        shop.color = Some(database::SourceColor::Green);
        // `prod-confirm`: the source marked as production (in red, as it would be).
        if kind == "prod-confirm" {
            shop.production = true;
            shop.color = Some(database::SourceColor::Red);
        }
        // A fixed identity: its consoles' folder (shown in the status bar) the same each run.
        shop.id = database::DataSourceId::from_u128(0x5b6f_2c7e_6a4e_4c1f_9d55_3e1f_0c2b_7a10);
        let source = shop.id;
        if let Err(e) = catalog.add(shop) {
            tracing::error!("the sample source wasn't added: {e}");
            return;
        }
        consoles.set_store(Arc::new(database::console::NoConsoles));
        let start = Instant::now();
        let ticks = Arc::new(AtomicU32::new(0));
        // What an engine measures itself (a real profile's times) frozen too.
        consoles.set_runtime(
            database_console::runtime::Runtime::new(
                |job| kernel::work::run("db-console", job),
                move || {
                    start
                        + Duration::from_millis(
                            7 * u64::from(ticks.fetch_add(1, Ordering::Relaxed)),
                        )
                },
                {
                    // Each line a little later than the one before (the same times every run).
                    let lines = Arc::new(AtomicU32::new(0));
                    move || {
                        let n = u64::from(lines.fetch_add(1, Ordering::Relaxed));
                        let ms = 23_698 + n * 137;
                        format!("2026-10-31 10:00:{:02}.{:03}", ms / 1000 % 60, ms % 1000)
                    }
                },
            )
            .with_wall(|| SNAPSHOT_WALL_MS)
            .frozen(),
        );
        // Its history in memory (nothing of a run before this one).
        consoles.history().set_store(Arc::new(database::history::MemoryHistory::default()));
        let root = consoles.root().to_path_buf();
        let _ = std::fs::remove_dir_all(database::console::folder(&root).join(source.to_string()));
        let sql = CONSOLE_SQL;
        match kind {
            // The source's consoles filtered by what's typed: the names and the texts matched.
            "jump-filter" => {
                for (text, ms, name) in [
                    (sql, Some(543), None),
                    ("SELECT * FROM customers WHERE country = 'RO';\n", Some(12), None),
                    (
                        "-- Paid orders, by day\nSELECT date(created_at), sum(total)\nFROM orders\nGROUP BY 1;\n",
                        None,
                        Some("orders_by_day"),
                    ),
                    ("DELETE FROM sessions WHERE expires_at < now();\n", Some(4), Some("cleanup")),
                    ("", None, Some("scratch")),
                ] {
                    match consoles.create(source, text) {
                        Ok(spec) => {
                            consoles.update(spec.id, |s| s.last_run_ms = ms);
                            if let Some(name) = name
                                && let Err(e) = consoles.rename(spec.id, name)
                            {
                                tracing::error!("the sample console wasn't renamed: {e:?}");
                            }
                        }
                        Err(e) => tracing::error!("the sample console wasn't made: {e}"),
                    }
                }
                database::console::ask_jump(database::console::JumpRequest {
                    project: id,
                    source,
                    schema: None,
                    table: None,
                    new: false,
                    list: false,
                    database: None,
                });
                self.wb.run_command(database_console::JUMP);
                self.frame();
                settle(120);
                self.wb.viewer_command("filter:ord");
                self.frame();
                settle(220);
            }
            // The history window of a console, searched: the statements run on its source;
            // or asking whether to clear it all.
            "history" | "history-clear" => {
                let spec = match consoles.create(source, sql) {
                    Ok(spec) => spec,
                    Err(e) => {
                        tracing::error!("the sample console wasn't made: {e}");
                        return;
                    }
                };
                let styles = database::sql::ParamStyles::of(database::sql::Flavor::Sqlite);
                let history = consoles.history();
                for (i, (text, ms, rows, outcome)) in HISTORY_SAMPLE.iter().enumerate() {
                    let at = SNAPSHOT_WALL_MS - (HISTORY_SAMPLE.len() - i) as i64 * 97_000;
                    let mut entry = database::history::HistoryEntry::new(
                        source,
                        text,
                        database::sql::Flavor::Sqlite,
                        &styles,
                        at,
                    );
                    entry.console = Some(spec.id);
                    entry.schema = Some("main".into());
                    entry.duration_ms = Some(*ms);
                    entry.rows = *rows;
                    entry.outcome = *outcome;
                    history.record(entry);
                }
                if let Err(e) = self.wb.open_file(&spec.path(&root)) {
                    tracing::error!("the sample console didn't open: {e}");
                    return;
                }
                self.frame();
                self.wb.run_command(database_console::HISTORY);
                self.frame();
                self.wb.viewer_command(if kind == "history" { "search:orders" } else { "clear" });
                self.frame();
                settle(220);
            }
            // Completion from the source's schema: the tables with their source after FROM
            // (as a list), the related tables with their condition after JOIN.
            "completion" | "join" => {
                let text = if kind == "join" { CONSOLE_JOIN } else { CONSOLE_COMPLETION };
                let spec = match consoles.create(source, text) {
                    Ok(spec) => spec,
                    Err(e) => {
                        tracing::error!("the sample console wasn't made: {e}");
                        return;
                    }
                };
                if let Err(e) = self.wb.open_file(&spec.path(&root)) {
                    tracing::error!("the sample console didn't open: {e}");
                    return;
                }
                self.frame();
                let end = text.len();
                workbench_model::EditorHost::select_active(self.wb, end..end);
                // Asked once (the schema read in the background), then again once read.
                self.wb.update_completion(self.text);
                let deadline = Instant::now() + Duration::from_secs(10);
                while consoles.indexes().busy() && Instant::now() < deadline {
                    settle(10);
                }
                self.wb.update_completion(self.text);
                self.frame();
                settle(220);
            }
            // A function's parameter help (48c): `substr(` typed, then its first argument —
            // the second parameter lit.
            "signature" => {
                let spec = match consoles.create(source, CONSOLE_SIGNATURE) {
                    Ok(spec) => spec,
                    Err(e) => {
                        tracing::error!("the sample console wasn't made: {e}");
                        return;
                    }
                };
                if let Err(e) = self.wb.open_file(&spec.path(&root)) {
                    tracing::error!("the sample console didn't open: {e}");
                    return;
                }
                self.frame();
                let end = CONSOLE_SIGNATURE.len();
                workbench_model::EditorHost::select_active(self.wb, end..end);
                for typed in ["(", "name", ","] {
                    self.wb.type_text(self.text, typed);
                }
                self.frame();
                settle(220);
            }
            // A table's hover card (its columns, types and keys), and a text checked against
            // the schema: an unknown table underlined, its message shown.
            "hover" | "unknown-table" => {
                let (text, word) = if kind == "hover" {
                    (CONSOLE_HOVER, "orders o")
                } else {
                    (CONSOLE_UNKNOWN, "order_lines")
                };
                // Checked at once: the pause typing must make is measured on the stopped
                // clock (7 ms a reading), which would hold the check past the wait below.
                consoles.checks().set_pause(Duration::ZERO);
                let spec = match consoles.create(source, text) {
                    Ok(spec) => spec,
                    Err(e) => {
                        tracing::error!("the sample console wasn't made: {e}");
                        return;
                    }
                };
                if let Err(e) = self.wb.open_file(&spec.path(&root)) {
                    tracing::error!("the sample console didn't open: {e}");
                    return;
                }
                self.frame();
                // The schema read (asked by a first look), then the text checked on it.
                let at = text.find(word).unwrap_or(0) + 2;
                let _ = self.wb.assist_hover_at(at);
                let deadline = Instant::now() + Duration::from_secs(10);
                while (consoles.busy() || consoles.indexes().is_empty())
                    && Instant::now() < deadline
                {
                    self.frame();
                    settle(10);
                }
                self.settle_console();
                // The mouse over the word: its card, or its error's tip.
                workbench_model::EditorHost::select_active(self.wb, at..at);
                self.frame();
                let Some(caret) = self.wb.caret_rect(self.text) else { return };
                let p = Point::new(caret.min_x() + 1.0, caret.min_y() + caret.size.height / 2.0);
                workbench_model::EditorHost::select_active(self.wb, 0..0);
                self.hover(p);
            }
            // A SQL file of the project (39n): attached to nothing ("No data source —
            // Attach…", `file`), then attached to the source with a statement run
            // (`file-attached`: the source beside the schema, its result in Services). The
            // file is the script's to remove after (`report.sql`, never the sample's).
            "file" | "file-attached" => {
                let path = root.join(SNAPSHOT_SQL_FILE);
                consoles.detach(&path);
                if let Err(e) = std::fs::write(&path, sql) {
                    tracing::error!("the sample SQL file wasn't written: {e}");
                    return;
                }
                if kind == "file-attached" && consoles.attach(&path, source).is_none() {
                    tracing::error!("the sample SQL file wasn't attached");
                    return;
                }
                if let Err(e) = self.wb.open_file(&path) {
                    tracing::error!("the sample SQL file didn't open: {e}");
                    return;
                }
                self.wb.set_bottom_height(400.0);
                self.frame();
                if kind == "file-attached" {
                    let at = sql.find("count(o.id)").unwrap_or(0);
                    workbench_model::EditorHost::select_active(self.wb, at..at);
                    self.wb.run_command(database_console::RUN);
                    self.settle_console();
                    workbench_model::EditorHost::select_active(self.wb, at..at);
                }
                self.frame();
                settle(220);
                self.frame();
            }
            "jump" => {
                for (text, ms) in [(sql, Some(543)), ("", None)] {
                    match consoles.create(source, text) {
                        Ok(spec) => {
                            consoles.update(spec.id, |s| s.last_run_ms = ms);
                        }
                        Err(e) => tracing::error!("the sample console wasn't made: {e}"),
                    }
                }
                database::console::ask_jump(database::console::JumpRequest {
                    project: id,
                    source,
                    schema: None,
                    table: None,
                    new: false,
                    list: false,
                    database: None,
                });
                self.wb.run_command(database_console::JUMP);
                self.frame();
                settle(220);
            }
            _ => {
                let (text, at, script) = match kind {
                    "output" | "results-many" => (CONSOLE_SCRIPT, 0, true),
                    "tx-manual" => (CONSOLE_TX, 0, true),
                    "danger" => (CONSOLE_DANGER, 0, true),
                    "prod-confirm" => {
                        (CONSOLE_PROD, CONSOLE_PROD.find("UPDATE").unwrap_or(0), false)
                    }
                    "result" | "result-edit" | "result-extractors" => (CONSOLE_CUSTOMERS, 0, false),
                    "params" => {
                        (CONSOLE_PARAMS, CONSOLE_PARAMS.find(":status").unwrap_or(0), false)
                    }
                    "plan" | "profile-sqlite" => {
                        (CONSOLE_PLAN, CONSOLE_PLAN.find("SELECT c.name").unwrap_or(0), false)
                    }
                    "profile" | "profile-narrow" | "profile-tab" | "profile-finding"
                    | "profile-graph" | "profile-flame" | "profile-compare" | "profile-tables" => {
                        let text = database_console::profile::sample::SQL;
                        (text, text.find("FROM orders").unwrap_or(0), false)
                    }
                    "error" => (CONSOLE_ERROR, CONSOLE_ERROR.find("WHER").unwrap_or(0), false),
                    _ => (sql, sql.find("count(o.id)").unwrap_or(0), false),
                };
                let spec = match consoles.create(source, text) {
                    Ok(spec) => spec,
                    Err(e) => {
                        tracing::error!("the sample console wasn't made: {e}");
                        return;
                    }
                };
                // `tx-manual`: committed by hand; `prod-confirm`: in Auto (each write asks).
                match kind {
                    "tx-manual" => {
                        consoles.update(spec.id, |s| s.tx = database::console::TxMode::Manual);
                    }
                    "prod-confirm" => {
                        consoles.update(spec.id, |s| s.tx = database::console::TxMode::Auto);
                    }
                    _ => {}
                }
                if let Err(e) = self.wb.open_file(&spec.path(&root)) {
                    tracing::error!("the sample console didn't open: {e}");
                    return;
                }
                // The Services window tall enough for its results' rows to be seen.
                self.wb.set_bottom_height(400.0);
                self.frame();
                // The cursor in the statement, then Run (or the whole script).
                workbench_model::EditorHost::select_active(self.wb, at..at);
                // `menu`: its right-click menu (its own rows first), nothing run.
                if kind == "menu" {
                    self.wb.clear_menu_target();
                    let entries = self.wb.context_menu(workbench_model::MenuPlace::Editor);
                    self.wb.open_context_menu(entries, kernel::Point::new(900.0, 330.0));
                    self.frame();
                    settle(220);
                    self.frame();
                    return;
                }
                // `params`: Run asks the statement's values first (some typed, `:status` being
                // edited, its places lit), nothing sent yet.
                if kind == "params" {
                    self.wb.run_command(database_console::RUN);
                    self.frame();
                    for command in [
                        "value:0:3",
                        "kind:2:raw",
                        "value:2:(SELECT avg(total) FROM orders)",
                        "value:3:20",
                        "remember:3",
                        "value:1:paid",
                    ] {
                        self.wb.viewer_command(command);
                    }
                    self.frame();
                    settle(220);
                    self.frame();
                    return;
                }
                // The profile's: the Services window taller, its tree narrow (the profile wide)
                // or wide (the profile narrow).
                if kind.starts_with("profile") {
                    // A finding chosen: the console's statement seen above, lit.
                    self.wb.set_bottom_height(if kind == "profile-finding" {
                        400.0
                    } else {
                        560.0
                    });
                    self.wb.show_side_panel(services_panel::TOOL);
                    let tree = if kind == "profile-narrow" { "tree=700" } else { "tree=180" };
                    let memento = ui_workbench::PanelMemento(tree.to_owned());
                    <Workbench as workbench_model::ViewHost>::tell_panel(
                        self.wb,
                        services_panel::TOOL,
                        &memento,
                        false,
                    );
                    self.frame();
                }
                // A profile made by hand (PostgreSQL's, no server): shown as the console's.
                if kind.starts_with("profile") && kind != "profile-sqlite" {
                    let runner = consoles.runner(spec.id, source);
                    let sample = &database_console::profile::sample::show;
                    let (tab, before) = match kind {
                        // The same query after an index, compared with it before.
                        "profile-compare" => {
                            let (tab, before) =
                                database_console::profile::sample::show_compare(&runner, text);
                            (tab, Some(before))
                        }
                        // Its tables, with what the server knows of them.
                        "profile-tables" => {
                            (database_console::profile::sample::show_tables(&runner, text), None)
                        }
                        _ => (sample(&runner, text), None),
                    };
                    self.settle_console();
                    match kind {
                        // Compared with the profile before the index.
                        "profile-compare" => {
                            if let Some(before) = before {
                                let command = services_panel::panel::TabCommand(format!(
                                    "profile.compare:{before}"
                                ));
                                <Workbench as workbench_model::ViewHost>::tell_panel(
                                    self.wb,
                                    services_panel::TOOL,
                                    &command,
                                    false,
                                );
                                self.frame();
                            }
                            workbench_model::EditorHost::select_active(self.wb, 0..0);
                        }
                        // As its tables, the first one chosen.
                        "profile-tables" => {
                            for id in ["view_tables", "profile.next_problem"] {
                                let command = services_panel::panel::TabCommand(id.to_owned());
                                <Workbench as workbench_model::ViewHost>::tell_panel(
                                    self.wb,
                                    services_panel::TOOL,
                                    &command,
                                    false,
                                );
                                self.frame();
                            }
                            workbench_model::EditorHost::select_active(self.wb, 0..0);
                        }
                        // Its first problem chosen: its step in the tree, its place lit in the
                        // console.
                        "profile-finding" => {
                            let command = services_panel::panel::TabCommand(
                                "profile.next_problem".to_owned(),
                            );
                            <Workbench as workbench_model::ViewHost>::tell_panel(
                                self.wb,
                                services_panel::TOOL,
                                &command,
                                false,
                            );
                            // The cursor on the first line: the lit lines stand on their own.
                            workbench_model::EditorHost::select_active(self.wb, 0..0);
                        }
                        // As a diagram (its first problem chosen) or a flame graph (its
                        // hottest step chosen).
                        "profile-graph" | "profile-flame" => {
                            let (choose, view) = if kind == "profile-graph" {
                                ("profile.next_problem", "view_graph")
                            } else {
                                ("profile.hottest", "view_flame")
                            };
                            for id in [view, choose] {
                                let command = services_panel::panel::TabCommand(id.to_owned());
                                <Workbench as workbench_model::ViewHost>::tell_panel(
                                    self.wb,
                                    services_panel::TOOL,
                                    &command,
                                    false,
                                );
                                self.frame();
                            }
                            workbench_model::EditorHost::select_active(self.wb, 0..0);
                        }
                        // Opened in an editor tab of its own, the Services window closed.
                        "profile-tab" => {
                            if let Some(set) = runner.plan(tab) {
                                workbench_model::ViewHost::open_view(
                                    &mut *self.wb,
                                    "profile [shop]",
                                    database_console::profile::VIEW,
                                    &database_console::profile::ProfileTabData { runner, set },
                                );
                                self.wb.toggle_tool(services_panel::TOOL);
                                // The editor's whole width (the project's tree hidden).
                                self.wb.toggle_project_panel();
                            }
                        }
                        _ => {}
                    }
                    self.settle_console();
                    self.frame();
                    return;
                }
                // `plan`: Explain Plan, its tree in the Services window; `profile-sqlite`:
                // Explain Analyze, measured on the sample database.
                self.wb.run_command(if kind == "profile-sqlite" {
                    database_console::EXPLAIN_ANALYZE
                } else if kind == "plan" {
                    database_console::EXPLAIN
                } else if script {
                    database_console::RUN_SCRIPT
                } else {
                    database_console::RUN
                });
                let runner = consoles.runner_of(spec.id);
                let deadline = Instant::now() + Duration::from_secs(10);
                while runner.as_ref().is_some_and(database_console::runner::Runner::busy)
                    && Instant::now() < deadline
                {
                    settle(10);
                }
                if let Some(runner) = &runner {
                    match kind {
                        // The first result pinned (kept by the next run), the Output shown.
                        "results-many" => {
                            if let Some(first) = runner.result_tabs().first() {
                                runner.pin(first.id, true);
                            }
                        }
                        "output" => runner.show_output(),
                        _ => {}
                    }
                }
                self.settle_console();
                // `result-edit`: its table's structure read (its rows editable), then a cell
                // changed, a row added and one marked for deletion, nothing submitted yet.
                if kind == "result-edit" {
                    let command = services_panel::panel::TabCommand("snapshot-edit".to_owned());
                    <Workbench as workbench_model::ViewHost>::tell_panel(
                        self.wb,
                        services_panel::TOOL,
                        &command,
                        false,
                    );
                    workbench_model::EditorHost::select_active(self.wb, 0..0);
                    self.settle_console();
                }
                // `result-extractors`: a cell of the result chosen, its grid's data extractors'
                // popover open (the groups, the skips, the CSV formats, the default checked).
                if kind == "result-extractors" {
                    for id in ["grid:select:1,1", "grid:extractors"] {
                        let command = services_panel::panel::TabCommand(id.to_owned());
                        <Workbench as workbench_model::ViewHost>::tell_panel(
                            self.wb,
                            services_panel::TOOL,
                            &command,
                            false,
                        );
                    }
                    workbench_model::EditorHost::select_active(self.wb, 0..0);
                    self.settle_console();
                }
            }
        }
        self.frame();
    }

    /// A console's run settled: its bar, the Services window (its results' grids) and the
    /// diagnostics it told, until nothing changes.
    pub(super) fn settle_console(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut quiet = 0;
        while Instant::now() < deadline && quiet < 4 {
            self.frame();
            let mut changed = self.wb.poll_viewers();
            changed |= self.wb.poll_side_panel();
            changed |= self.wb.poll_diagnostics();
            if changed || self.wb.loading() {
                quiet = 0;
            } else {
                quiet += 1;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.frame();
    }
}
