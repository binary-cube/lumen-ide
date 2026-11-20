//! The data views and the database window's screens.

use super::*;

impl Steps<'_> {
    /// A sample data file opened as data (see `DATA` in the module's doc): made in a
    /// temporary folder of its own (never the project's, nor anything of the user's).
    pub(super) fn data_view(&mut self, kind: &str) {
        let dir = std::env::temp_dir().join("editor-snapshot-data");
        let (db, csv) = match data_fixtures::write(&dir) {
            Ok(paths) => paths,
            Err(e) => {
                tracing::error!(error = %e, "the sample data files weren't made");
                return;
            }
        };
        let path = match kind {
            "csv" | "split" | "split-keys" => csv,
            "json" => match data_fixtures::events(&dir) {
                Ok(path) => path,
                Err(e) => {
                    tracing::error!(error = %e, "the sample events weren't made");
                    return;
                }
            },
            _ => db,
        };
        if let Err(e) = self.wb.open_file(&path) {
            tracing::error!(error = %e, "the sample data file didn't open");
            return;
        }
        self.settle_data();
        match kind {
            "structure" => {
                self.wb.viewer_command("structure");
            }
            "query" => {
                self.wb.viewer_command("table:orders");
                self.settle_data();
                self.wb.viewer_command("grid:where:total > 20 AND status <> 'cancelled'");
                self.wb.viewer_command("grid:order:customer_id, total DESC");
                self.settle_data();
                self.wb.viewer_command("grid:select-column:2");
            }
            // Find in the Rows: the field across the bar, its count and buttons at the end.
            "find" => {
                self.wb.viewer_command("grid:find");
                self.wb.type_text(self.text, "ma");
            }
            "transpose" => {
                self.wb.viewer_command("table:products");
                self.settle_data();
                self.wb.viewer_command("grid:transpose");
            }
            "menu" => {
                let at = Point::new(1700.0, 760.0);
                self.frame();
                if let Some(entries) = self.wb.viewer_menu_at(at) {
                    self.wb.open_context_menu(entries, at);
                }
                settle(220);
            }
            "split" => {
                self.wb.set_viewer_mode_split();
            }
            // Beside its text, the grid clicked: its keys move its cursor.
            "split-keys" => {
                self.wb.set_viewer_mode_split();
                self.frame();
                self.settle_data();
                if let Some(at) = self.wb.viewer_point(0.45, 0.42) {
                    self.wb.mouse_down(self.text, at, false, false, false, 1);
                    self.wb.mouse_up(self.text);
                }
                self.wb.edit_key(
                    self.text,
                    EditKey::Move { motion: document::Motion::Down, extend: false },
                );
                self.wb.edit_key(
                    self.text,
                    EditKey::Move { motion: document::Motion::Down, extend: true },
                );
                self.wb.edit_key(
                    self.text,
                    EditKey::Move { motion: document::Motion::Right, extend: true },
                );
            }
            // A JSON value in the large editor, pretty-printed.
            "json" => {
                self.wb.viewer_command("grid:select:1,1");
                self.wb.viewer_command("grid:edit");
            }
            // The `WHERE` field not understood: why, under it.
            "where-error" => {
                self.wb.viewer_command("table:orders");
                self.settle_data();
                self.wb.viewer_command("grid:where:status = 'paid' AND totl > 20");
            }
            // The grid's data extractors (plan 39o): a few cells chosen, the "CSV ▾" popover open
            // (the groups, the skips, the CSV formats, the default checked).
            "extractors" => {
                self.wb.viewer_command("table:orders");
                self.settle_data();
                self.wb.viewer_command("grid:select:1,1");
                self.wb.viewer_command("grid:extractors");
            }
            // Every row being exported (midway), and once written.
            "export" => {
                self.wb.viewer_command("table:orders");
                self.settle_data();
                self.wb.viewer_command("snapshot-export:24");
            }
            "exported" => {
                self.wb.viewer_command("table:orders");
                self.settle_data();
                self.wb.viewer_command("grid:export:csv");
                let out = dir.join("orders.csv");
                self.wb.viewer_command(&format!("picked-save:{}", out.display()));
            }
            // The structure's modals (filled as a user would, their plan come back).
            "create-table" | "create-table-types" | "alter-column" | "drop-column"
            | "create-index" => {
                self.wb.viewer_command("table:orders");
                self.wb.viewer_command("structure");
                self.settle_data();
                let command = match kind {
                    "create-table" | "create-table-types" => "schema:create-table",
                    "alter-column" => "schema:alter-column:total",
                    "drop-column" => "schema:drop-column:total",
                    _ => "schema:create-index",
                };
                self.wb.run_view_menu(self.text, command);
                self.settle_data();
                let sample = kind.strip_suffix("-types").unwrap_or(kind);
                self.wb.viewer_command(&format!("snapshot:{sample}"));
                self.settle_data();
                // Its new column's type field, its suggestions open.
                if kind.ends_with("-types") {
                    self.wb.viewer_command("snapshot:types");
                }
                settle(220);
            }
            "edit" | "preview" => {
                self.wb.viewer_command("table:orders");
                self.settle_data();
                // A status changed, a total typed, a row added, one marked for deletion.
                self.wb.viewer_command("grid:select:32,3");
                self.wb.viewer_command("grid:edit");
                self.wb.type_text(self.text, "paid");
                self.wb.edit_key(self.text, EditKey::Enter { shift: false });
                self.wb.viewer_command("grid:select:34,2");
                self.wb.viewer_command("grid:edit");
                self.wb.type_text(self.text, "99.5");
                self.wb.edit_key(self.text, EditKey::Enter { shift: false });
                self.wb.viewer_command("grid:select:36,0");
                self.wb.viewer_command("grid:delete-rows");
                self.wb.viewer_command("grid:add-row");
                self.wb.type_text(self.text, "4");
                self.wb.edit_key(self.text, EditKey::Tab { shift: false });
                self.wb.viewer_command("grid:edit");
                self.wb.type_text(self.text, "42");
                self.wb.edit_key(self.text, EditKey::Enter { shift: false });
                if kind == "preview" {
                    self.wb.run_view_menu(self.text, "submit");
                    settle(220);
                }
            }
            _ => {}
        }
        self.settle_data();
    }

    /// The "Database" window (see `DATABASE` in the module's doc): the sample files as the
    /// project's sources, kept in memory only (nothing of the project's is written).
    pub(super) fn database_window(&mut self, kind: &str) {
        use database_explorer::window::{LIST_TOP, ROW_HEIGHT, Reveal, TOOL};
        let dir = std::env::temp_dir().join("editor-snapshot-data");
        let (db, csv) = match data_fixtures::write(&dir) {
            Ok(paths) => paths,
            Err(e) => {
                tracing::error!(error = %e, "the sample data files weren't made");
                return;
            }
        };
        let id = workbench_model::WorkbenchHost::project_id(self.wb);
        let Some(sessions) = id.and_then(database_explorer::sessions) else {
            tracing::error!("no project's sessions");
            return;
        };
        let catalog = sessions.catalog();
        catalog.set_store(std::sync::Arc::new(database::NoStore));
        catalog.ensure_loaded();
        let deadline = Instant::now() + Duration::from_secs(5);
        while catalog.list(database::Scope::Project) == database_explorer::catalog::Listed::Loading
            && Instant::now() < deadline
        {
            settle(5);
        }
        let mut shop = database::DataSource::new("shop.sqlite", "sqlite", database::Scope::Project);
        shop.file = Some(db);
        shop.secrets = database::SecretPolicy::Never;
        let mut people = database::DataSource::new("people.csv", "csv", database::Scope::Project);
        people.file = Some(csv);
        people.secrets = database::SecretPolicy::Never;
        let shop_id = shop.id;
        // Folders with their colors (production in red, development in green, a subfolder
        // taking its parent's), the sources in them.
        let folders =
            matches!(kind, "folders" | "sources-folders") || kind.starts_with("folder-grid");
        let mut prod_folder = database::SourceFolder::new("prod", database::Scope::Project);
        prod_folder.color = Some(database::SourceColor::Red);
        // The data sources' modal: the folder marked as production (its sources with it).
        prod_folder.production = kind == "sources-folders";
        let mut eu_folder = database::SourceFolder::new("eu", database::Scope::Project);
        eu_folder.parent = Some(prod_folder.id);
        let mut dev_folder = database::SourceFolder::new("dev", database::Scope::Project);
        dev_folder.color = Some(database::SourceColor::Green);
        if folders {
            shop.folder = Some(prod_folder.id);
            people.folder = Some(dev_folder.id);
            for folder in [prod_folder.clone(), eu_folder.clone(), dev_folder] {
                if let Err(e) = catalog.put_folder(folder) {
                    tracing::error!(error = %e, "the sample folder wasn't added");
                }
            }
        }
        let shop_path = shop.file.clone();
        let mut sources = vec![shop, people];
        // A server with several databases (made up, in memory): its source names `postgres`.
        let mut cluster_shop = None;
        if kind == "databases" {
            database::drivers::register(cluster::factory());
            let mut pg =
                database::DataSource::new("PostgreSQL", cluster::ID, database::Scope::Project);
            pg.target.host = "10.0.0.5".into();
            pg.target.database = "postgres".into();
            pg.secrets = database::SecretPolicy::Never;
            cluster_shop = Some(catalog.database_id(pg.id, "shop"));
            sources.push(pg);
        }
        // A MySQL server (made up, in memory): its databases are its schemas, all read by
        // its one session; its source names `shop`.
        let mut mysql_source = None;
        if kind == "mysql" {
            database::drivers::register(mysql_like::factory());
            let mut my =
                database::DataSource::new("MySQL", mysql_like::ID, database::Scope::Project);
            my.target.host = "10.0.0.7".into();
            my.target.database = "shop".into();
            my.secrets = database::SecretPolicy::Never;
            mysql_source = Some(my.id);
            sources.push(my);
        }
        // The data sources' modal: servers too (nothing connects; their passwords kept in
        // memory only, never the keychain).
        let mut server = None;
        if kind.starts_with("sources") {
            sessions.set_credentials(std::sync::Arc::new(
                database_explorer::credentials::Credentials::default(),
            ));
            let mut prod =
                database::DataSource::new("shop-prod", "postgres", database::Scope::Project);
            prod.comment = "The live shop".into();
            prod.target.host = "db.example.com".into();
            prod.target.port = Some(5432);
            prod.target.user = "app".into();
            prod.target.database = "shop".into();
            prod.target.options.insert("sslmode".into(), "verify-full".into());
            prod.target.options.insert("sslrootcert".into(), "certs/root.crt".into());
            // Marked as production (its badge in the list).
            prod.production = true;
            if folders {
                prod.folder = Some(eu_folder.id);
            }
            server = Some(prod.id);
            let mut analytics =
                database::DataSource::new("analytics", "postgres", database::Scope::Global);
            analytics.target.host = "warehouse.internal".into();
            analytics.target.database = "analytics".into();
            analytics.secrets = database::SecretPolicy::Keychain;
            if folders {
                analytics.color = Some(database::SourceColor::Purple);
            }
            sources.push(prod);
            sources.push(analytics);
        }
        for source in sources {
            if let Err(e) = catalog.add(source) {
                tracing::error!(error = %e, "the sample source wasn't added");
            }
        }
        self.wb.toggle_side_panel(TOOL);
        self.frame();
        // SQLite's own schema is no schema (an attached one would be).
        let orders = database::TableRef::new("orders");
        let reveal = match (cluster_shop, mysql_source) {
            (Some(shop), _) => Reveal {
                source: shop,
                table: Some(database::TableRef::in_schema("sales", "orders")),
                columns: false,
            },
            (None, Some(my)) => Reveal {
                source: my,
                table: Some(database::TableRef::in_schema("shop", "orders")),
                columns: false,
            },
            (None, None) => Reveal { source: shop_id, table: Some(orders), columns: true },
        };
        workbench_model::ViewHost::tell_panel(self.wb, TOOL, &reveal, false);
        self.settle_side();
        match kind {
            "add" => {
                if let Some((button, (_, scale))) =
                    self.wb.side_header_button(TOOL, "add").zip(self.wb.side_content(TOOL))
                {
                    let at = Point::new(
                        button.min_x() + button.size.width / 2.0,
                        button.min_y() + button.size.height / 2.0,
                    );
                    self.wb.mouse_down(self.text, at, false, false, false, 1);
                    self.wb.mouse_up(self.text);
                    self.frame();
                    // The submenu of the drivers, opened as the mouse rests on its row.
                    let row =
                        Point::new(button.min_x() + 60.0 * scale, button.max_y() + 26.0 * scale);
                    self.hover(row);
                }
            }
            // A table of the source in the red folder opened: its tab carries the color.
            // `-more`: the grid's "…" menu open (too narrow for its toolbar); `-tip`: the
            // mouse resting on its Delete button, its tooltip shown.
            grid if grid.starts_with("folder-grid") => {
                if let Some(path) = &shop_path
                    && let Err(e) = self.wb.open_file(path)
                {
                    tracing::error!(error = %e, "the sample database didn't open");
                }
                self.settle_data();
                if grid == "folder-grid-more" {
                    self.wb.run_view_menu(self.text, "grid:more");
                    self.frame();
                    settle(220);
                } else if grid == "folder-grid-tip"
                    && self.wb.run_view_menu(self.text, "snapshot-point:delete")
                {
                    let at = *database_explorer::view::SNAPSHOT_POINT
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if let Some(at) = at {
                        self.hover(at);
                    }
                }
            }
            sample if sample.starts_with("sources") => {
                let mut request = database_explorer::sources::SourcesRequest::new(sessions.clone());
                request.select = server;
                if sample == "sources-folders" {
                    request.select = None;
                    request.select_folder = Some(prod_folder.id);
                }
                workbench_model::ViewHost::open_modal(
                    &mut *self.wb,
                    &i18n::t("datasources.modal_title"),
                    database_explorer::sources::EXTENSION,
                    &request,
                );
                self.frame();
                let shown = match sample {
                    "sources-folders" => "folders",
                    "sources-ssl" => "ssl",
                    "sources-schemas" => "schemas",
                    "sources-ok" => "test-ok",
                    "sources-failed" => "test-failed",
                    "sources-add" => "add",
                    _ => "general",
                };
                self.wb.viewer_command(&format!("snapshot:{shown}"));
                self.frame();
                settle(220);
            }
            "menu" => {
                if let Some((content, scale)) = self.wb.side_content(TOOL) {
                    // The `orders` row: the source, `tables`, `customers`, then it.
                    let y = content.min_y()
                        + ((LIST_TOP + 6.0) * scale).round()
                        + (ROW_HEIGHT * scale).round() * 3.5;
                    let at = Point::new(content.min_x() + 100.0 * scale, y);
                    if let Some(entries) = self.wb.side_menu_at(at) {
                        self.wb.open_context_menu(entries, at);
                    }
                    self.frame();
                    settle(220);
                }
            }
            _ => {}
        }
        self.frame();
    }

    /// The relations diagram of a sample store (see `RELATIONS` in the module's doc): its
    /// database made in a temporary folder of its own, opened as data, its diagram shown
    /// from the tables' list.
    pub(super) fn relations(&mut self, kind: &str) {
        // The decisions made here kept in memory only (nothing of the project's written).
        let id = workbench_model::WorkbenchHost::project_id(self.wb);
        if let Some(sessions) = id.and_then(database_explorer::sessions) {
            sessions.catalog().set_store(std::sync::Arc::new(database::NoStore));
        }
        let dir = std::env::temp_dir().join("editor-snapshot-relations");
        let path = match data_fixtures::store(&dir) {
            Ok(path) => path,
            Err(e) => {
                tracing::error!(error = %e, "the sample store wasn't made");
                return;
            }
        };
        if let Err(e) = self.wb.open_file(&path) {
            tracing::error!(error = %e, "the sample store didn't open");
            return;
        }
        self.settle_data();
        self.wb.run_view_menu(self.text, "diagram");
        self.settle_data();
        match kind {
            "focus" => {
                self.wb.viewer_command("focus-table:orders");
            }
            // The relations found from names, their list open, `reviews` selected.
            "inferred" => {
                self.wb.viewer_command("found");
                self.wb.viewer_command("select-table:reviews");
            }
            // One accepted (a virtual key), one set aside, the list showing both.
            "decided" => {
                self.wb.viewer_command("accept:1");
                self.wb.viewer_command("ignore:3");
                self.wb.viewer_command("found");
                self.wb.viewer_command("select-table:reviews");
            }
            // The popover of a line found from names.
            "edge" => {
                self.wb.viewer_command("edge:reviews.userid");
            }
            _ => {}
        }
        self.settle_data();
        settle(120);
        self.frame();
    }

    /// Draws and polls until the data view has what it asked for (its pages, counts).
    pub(super) fn settle_data(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut quiet = 0;
        while Instant::now() < deadline && quiet < 3 {
            self.frame();
            let changed = self.wb.poll_viewers();
            if changed || self.wb.loading() {
                quiet = 0;
            } else {
                quiet += 1;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.frame();
    }

    /// Draws and polls until the side panels have what they asked for.
    pub(super) fn settle_side(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut quiet = 0;
        while Instant::now() < deadline && quiet < 5 {
            self.frame();
            if self.wb.poll_side_panel() {
                quiet = 0;
            } else {
                quiet += 1;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.frame();
    }
}
